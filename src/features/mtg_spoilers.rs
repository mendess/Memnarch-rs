use std::{
    collections::{HashMap, HashSet},
    io,
    sync::Arc,
    time::Duration,
};

use daemons::{async_trait, Daemon};
use lazy_static::lazy_static;
use mtg_spoilers::{Spoiler, SpoilerSource};
use serenity::{
    http::CacheHttp,
    model::prelude::{ChannelId, MessageId},
    prelude::Mutex,
};

use crate::util::daemons::DaemonManager;
use json_db::{Database, GlobalDatabase};

struct SpoilerChecker;

mod paths {
    pub static BASE: &str = "files/mtg-spoilers";
    pub static CACHE: &str = "files/mtg-spoilers/cache";
    pub static DB: &str = "files/mtg-spoilers/db.json";
}

#[async_trait]
impl Daemon<true> for SpoilerChecker {
    type Data = serenity::CacheAndHttp;

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        use mtg_spoilers::{cache::file::File, mythic};
        log::info!("checking for spoilers");
        let cache = match File::new(paths::CACHE).await {
            Ok(c) => c,
            Err(e) => {
                log::error!("failed to create cache: {e:?}");
                return daemons::ControlFlow::CONTINUE;
            }
        };
        let new_cards = match mythic::new_cards(cache).await {
            Ok(n) => n,
            Err(e) => {
                log::error!("failed to fetch new cards: {e:?}");
                return daemons::ControlFlow::CONTINUE;
            }
        };

        if let Err(e) = send_new_cards(data, new_cards).await {
            log::error!("failed to send new cards: {e:?}");
        }
        daemons::ControlFlow::CONTINUE
    }

    async fn interval(&self) -> Duration {
        Duration::from_secs(60 * 20)
    }

    async fn name(&self) -> String {
        stringify!(SpoilerChecker).to_string()
    }
}

pub async fn initialize(d: &mut Arc<Mutex<DaemonManager>>) -> io::Result<()> {
    tokio::fs::create_dir_all(paths::BASE).await?;
    d.lock().await.add_daemon(SpoilerChecker).await;
    Ok(())
}

static SPOILER_CHANNEL_DB: GlobalDatabase<HashSet<ChannelId>> = Database::const_new(paths::DB);

lazy_static! {
    static ref RETRY_CACHE: Mutex<HashMap<ChannelId, Vec<(Spoiler, CardSendState)>>> =
        Mutex::default();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToggleAction {
    Added,
    Removed,
}

pub async fn toggle_channel(ch: ChannelId) -> io::Result<ToggleAction> {
    let mut db = SPOILER_CHANNEL_DB.load().await?;
    if !db.remove(&ch) {
        db.insert(ch);
        Ok(ToggleAction::Added)
    } else {
        Ok(ToggleAction::Removed)
    }
}

#[derive(Debug, Clone, Copy)]
enum CardSendState {
    SourceMsgMissing(ChannelId),
    ThreadMissing(MessageId),
    NotSent,
}

async fn send_new_cards(
    ctx: &serenity::CacheAndHttp,
    new_cards: Vec<Spoiler>,
) -> serenity::Result<()> {
    for ch in SPOILER_CHANNEL_DB.load().await?.iter() {
        let retries = RETRY_CACHE.lock().await.remove(ch).unwrap_or_default();
        for c in retries
            .iter()
            .map(|(s, c)| (s, *c))
            .chain(new_cards.iter().map(|c| (c, CardSendState::NotSent)))
        {
            if let Err((state, e)) = send_card(ctx, *ch, c).await {
                log::error!("failed to publish spoiler {c:?} to {ch}: {e:?}");
                RETRY_CACHE
                    .lock()
                    .await
                    .entry(*ch)
                    .or_default()
                    .push((c.0.clone(), state));
            }
        }
    }
    Ok(())
}

async fn send_card(
    ctx: &serenity::CacheAndHttp,
    ch: ChannelId,
    (card, state): (&Spoiler, CardSendState),
) -> Result<(), (CardSendState, serenity::Error)> {
    let msg_id = match state {
        CardSendState::ThreadMissing(msg_id) => msg_id,
        _ => {
            ch.send_message(ctx.http(), |builder| {
                builder.embed(|builder| {
                    if let Some(name) = &card.name {
                        builder.title(name);
                    }
                    builder.image(&card.image).url(&card.source_site_url)
                })
            })
            .await
            .map_err(|e| (CardSendState::NotSent, e))?
            .id
        }
    };
    let thread_id = match state {
        CardSendState::SourceMsgMissing(tid) => tid,
        _ => {
            ch.create_public_thread(ctx.http(), msg_id, |ct| {
                ct.name(
                    card.name
                        .as_deref()
                        .unwrap_or_else(|| name_from_image(&card.image)),
                )
            })
            .await
            .map_err(|e| (CardSendState::ThreadMissing(msg_id), e))?
            .id
        }
    };
    thread_id
        .send_message(ctx.http(), |m| {
            if let Some(SpoilerSource { name, url }) = &card.source {
                m.embed(|e| {
                    if let Some(url) = url {
                        e.url(url);
                    }
                    e.title("Source").description(name)
                })
            } else {
                m.content("Unkown source")
            }
        })
        .await
        .map_err(|e| (CardSendState::SourceMsgMissing(thread_id), e))?;

    Ok(())
}

fn name_from_image(s: &str) -> &str {
    s.split('/')
        .last()
        .and_then(|s| {
            let (name, _) = s.split_once('.')?;
            Some(name)
        })
        .unwrap_or("unknown card name")
}