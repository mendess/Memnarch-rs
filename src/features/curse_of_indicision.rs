use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::Duration,
};

use daemons::{ControlFlow, Daemon};
use futures::{stream::StreamExt, FutureExt};
use json_db::GlobalDatabase;
use pubsub::{self, events};
use rand::seq::SliceRandom;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serenity::model::{
    channel::{ChannelType, Message, ReactionType},
    id::{ChannelId, GuildId, MessageId},
};

use crate::util::daemons::DaemonManager;

use tokio::sync::Mutex;

static DATABASE: GlobalDatabase<HashMap<GuildId, Curse>> = GlobalDatabase::new("files/curses.json");

fn curse_regex() -> &'static Regex {
    static CURSE_REGEX: OnceLock<Regex> = OnceLock::new();
    CURSE_REGEX.get_or_init(|| Regex::new("Curse\\(([0-9]+)\\)").unwrap())
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
struct Curse {
    guild: GuildId,
    last_msg: Option<(ChannelId, MessageId)>,
    sim: bool,
}

const EMOJIS: [[&str; 3]; 2] = [["🇳", "🇦", "🇴"], ["🇸", "🇮", "🇲"]];

#[serenity::async_trait]
impl Daemon<false> for Curse {
    type Data = serenity::CacheAndHttp;

    async fn run(&mut self, d: &Self::Data) -> ControlFlow {
        async fn _r(this: &mut Curse, d: &serenity::CacheAndHttp) -> anyhow::Result<()> {
            let channels = this.guild.channels(&d.http).await?;
            let msgs = || {
                futures::stream::iter(channels.values().filter(|c| c.kind == ChannelType::Text))
                    .map(|ch| ch.messages(&d.http, |g| g.limit(10)))
                    .filter_map(|ch| async { ch.await.ok() })
                    .flat_map(futures::stream::iter)
                    .collect::<Vec<Message>>()
            };
            if let Some((ch, msg)) = this.last_msg.take() {
                for e in EMOJIS.iter().flatten() {
                    ch.delete_reaction(&d.http, msg, None, ReactionType::Unicode(e.to_string()))
                        .await?;
                }
            } else if let Some(m) = msgs().await.choose(&mut rand::rngs::OsRng) {
                for e in EMOJIS[this.sim as usize] {
                    m.react(d, ReactionType::Unicode(e.to_string())).await?;
                }
                this.sim = !this.sim;
                this.last_msg = Some((m.channel_id, m.id))
            } else {
                tracing::error!("no messages found in the cursed server: {}", this.guild);
            }
            save(*this).await
        }
        if let Err(e) = _r(self, d).await {
            tracing::error!("failed to haunt server {}: {:?}", self.guild, e)
        }
        ControlFlow::CONTINUE
    }

    async fn name(&self) -> String {
        format!("Curse({})", self.guild)
    }

    async fn interval(&self) -> Duration {
        Duration::from_secs(if self.last_msg.is_some() { 5 } else { 35 })
    }
}

pub async fn initialize(d: &mut Arc<Mutex<DaemonManager>>) -> anyhow::Result<()> {
    {
        let mut d = d.lock().await;
        for (g, c) in DATABASE.load().await?.take() {
            if is_cursed(g).await {
                tracing::info!("cursing {}", g);
                d.add_daemon(c).await;
            }
        }
    }
    let d = d.clone();
    pubsub::subscribe::<events::GuildCreate, _>(move |_, events::GuildCreate { guild, .. }| {
        curse(guild.id, d.clone()).boxed()
    })
    .await;
    Ok(())
}

async fn curse(guild: GuildId, d: Arc<Mutex<DaemonManager>>) -> ControlFlow {
    if is_cursed(guild).await {
        let mut mng = d.lock().await;
        let is_registered = mng
            .daemon_names()
            .filter_map(|(_, h)| curse_regex().captures(h.name()))
            .filter_map(|c| c.get(1))
            .filter_map(|c| c.as_str().parse::<u64>().ok().map(GuildId))
            .inspect(|c| tracing::trace!("{:?}", c))
            .any(|id| id == guild);

        if is_registered {
            tracing::info!("guild {} already registered", guild);
        } else {
            let curse = Curse {
                guild,
                last_msg: None,
                sim: false,
            };
            tracing::info!("cursing {}", guild);
            match DATABASE.load().await {
                Ok(mut v) => {
                    v.insert(guild, curse);
                    mng.add_daemon(curse).await;
                }
                Err(e) => {
                    tracing::error!("failed to serialize curse {}: {:?}", guild, e)
                }
            }
        }
    }
    ControlFlow::CONTINUE
}

async fn is_cursed(guild: GuildId) -> bool {
    Some(true)
        == crate::prefs::guild::get(guild)
            .await
            .ok()
            .flatten()
            .map(|p| p.cursed)
}

async fn save(curse: Curse) -> anyhow::Result<()> {
    DATABASE.load().await?.insert(curse.guild, curse);
    Ok(())
}
