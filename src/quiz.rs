use crate::{
    cron::Cron,
    daemons::DaemonManager,
    events::pubsub::{
        self,
        events::{ReactionAdd, ReactionRemove},
    },
    file_transaction::Database,
};
use daemons::ControlFlow;
use futures::FutureExt;
use itertools::Itertools;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serenity::{
    client::Context,
    http::CacheHttp,
    model::{
        channel::Reaction,
        id::{ChannelId, GuildId, UserId},
        misc::Mentionable,
    },
};
use std::collections::{HashMap, HashSet};
use tokio::io;

#[derive(Serialize, Deserialize, Debug)]
struct Quizers {
    guild: GuildId,
    channel: ChannelId,
    quizers: HashSet<UserId>,
}

lazy_static! {
    static ref DATABASE: Database<HashMap<GuildId, Quizers>> = Database::new("files/quizers.json");
}

type QuizDaemon<F, Fut> = Cron<F, Fut, 20, 50, 00>;

pub async fn add_quizer(guild: GuildId, user_id: UserId) -> io::Result<()> {
    DATABASE
        .load()
        .await?
        .get_mut(&guild)
        .map(|g| g.quizers.insert(user_id));
    Ok(())
}

pub async fn rm_quizer(guild: GuildId, user_id: UserId) -> io::Result<()> {
    if let Some(q) = DATABASE.load().await?.get_mut(&guild) {
        q.quizers.remove(&user_id);
    }
    Ok(())
}

pub async fn add_quiz_guild(guild: GuildId, channel: ChannelId) -> io::Result<()> {
    DATABASE
        .load()
        .await?
        .entry(guild)
        .and_modify(|q| q.channel = channel)
        .or_insert_with(|| Quizers {
            guild,
            channel,
            quizers: Default::default(),
        });
    Ok(())
}

pub async fn initialize(daemon: &mut DaemonManager) -> io::Result<()> {
    for quizers in DATABASE.load().await?.take().into_values() {
        daemon
            .add_daemon(QuizDaemon::new(
                format!("quiz for channel {}", quizers.guild),
                move |data| {
                    let data = data.clone();
                    let msg = format!(
                        "Quiz bros: {}.\nReact with ➕ to become a quizer. React with ➖ to unbecome a quizer.",
                        quizers
                            .quizers
                            .iter()
                            .map(Mentionable::mention)
                            .format(", ")
                    );
                    let channel = quizers.channel;
                    let guild = quizers.guild;
                    async move {
                        if let Err(e) = channel.send_message(data.http(), |m| m.content(msg)).await
                        {
                            log::error!(
                                "Couldn't send quiz alert to channel {} in guild {}: {}",
                                channel,
                                guild,
                                e
                            );
                        }
                        ControlFlow::CONTINUE
                    }
                },
            ))
            .await;
    }

    async fn handle_reaction(c: &Context, r: &Reaction, emoji: &str, add: bool) {
        let m = match r.message(c).await {
            Ok(m) => m,
            Err(e) => {
                log::error!("Couldn't get message reacted to: {}", e);
                return;
            }
        };
        if r.emoji.unicode_eq(emoji) && m.content.starts_with("Quiz bros:") {
            let user = r.user_id.expect("Cache to be available");
            let guild = match r.guild_id {
                Some(g) => g,
                None => return,
            };
            if let Err(e) = if add {
                add_quizer(guild, user).await
            } else {
                rm_quizer(guild, user).await
            } {
                log::error!(
                    "Couldn't add user {} as a quizer in guild {} channel {}: {}",
                    user,
                    guild,
                    r.channel_id,
                    e
                );
            }
        }
    }
    pubsub::register::<ReactionAdd, _>(|c, a| {
        async move {
            handle_reaction(c, a, "➕", true).await;
            ControlFlow::CONTINUE
        }
        .boxed()
    });
    pubsub::register::<ReactionRemove, _>(|c, a| {
        async move {
            handle_reaction(c, a, "➖", false).await;
            ControlFlow::CONTINUE
        }
        .boxed()
    });
    Ok(())
}
