use std::{
    collections::{HashMap, HashSet},
    io,
    sync::Arc,
    time::Duration,
};

use daemons::{async_trait, Daemon};
use lazy_static::lazy_static;
use mtg_spoilers::{Spoiler, SpoilerSource};
use serenity::{http::CacheHttp, model::prelude::ChannelId, prelude::Mutex};

use crate::{daemons::DaemonManager, file_transaction::Database};

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
        Duration::from_secs(60 * 60)
    }

    async fn name(&self) -> String {
        stringify!(SpoilerChecker).to_string()
    }
}

pub async fn initialize(d: &mut Arc<Mutex<DaemonManager>>) -> io::Result<()> {
    tokio::fs::DirBuilder::new()
        .recursive(true)
        .create(paths::BASE)
        .await?;

    d.lock().await.add_daemon(SpoilerChecker).await;
    Ok(())
}

lazy_static! {
    static ref SPOILER_CHANNEL_DB: Database<HashSet<ChannelId>> = Database::new(paths::DB);
    static ref RETRY_CACHE: Mutex<HashMap<ChannelId, Vec<Spoiler>>> = Mutex::default();
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

async fn send_new_cards(
    ctx: &serenity::CacheAndHttp,
    new_cards: Vec<Spoiler>,
) -> serenity::Result<()> {
    for ch in SPOILER_CHANNEL_DB.load().await?.iter() {
        let retries = RETRY_CACHE.lock().await.remove(ch).unwrap_or_default();
        for c in retries.iter().chain(new_cards.iter()) {
            if let Err(e) = send_card(ctx, *ch, c).await {
                log::error!("failed to publish spoiler {c:?} to {ch}: {e:?}");
                RETRY_CACHE
                    .lock()
                    .await
                    .entry(*ch)
                    .or_default()
                    .push(c.clone());
            }
        }
    }
    Ok(())
}

async fn send_card(
    ctx: &serenity::CacheAndHttp,
    ch: ChannelId,
    card: &Spoiler,
) -> serenity::Result<()> {
    let msg = ch
        .send_message(ctx.http(), |builder| {
            builder.embed(|builder| {
                if let Some(name) = &card.name {
                    builder.title(name);
                }
                builder.image(&card.image).url(&card.source_site_url)
            })
        })
        .await?;
    let thread = ch
        .create_public_thread(ctx.http(), msg.id, |ct| {
            ct.name(
                card.name
                    .as_deref()
                    .unwrap_or_else(|| name_from_image(&card.image)),
            )
        })
        .await?;

    thread
        .send_message(ctx.http(), |m| {
            if let Some(SpoilerSource { name, url }) = &card.source {
                m.embed(|e| e.title("Source").description(name).url(url))
            } else {
                m.content("Unkown source")
            }
        })
        .await?;

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
