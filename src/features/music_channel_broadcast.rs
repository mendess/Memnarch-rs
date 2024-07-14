use std::{collections::HashSet, sync::OnceLock};

use anyhow::Context as _;
use futures::FutureExt;
use json_db::GlobalDatabase;
use pubsub::ControlFlow;
use regex::{Match, Regex};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serenity::{
    client::Context,
    model::{
        channel::Message,
        id::{ChannelId, UserId},
        mention::Mentionable,
    },
};

#[derive(Debug, Default, Serialize, Deserialize)]
struct Channels {
    sources: HashSet<ChannelId>,
    destinations: HashSet<ChannelId>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SentBanger {
    sender: UserId,
    url: Url,
}

static CHANNELS: GlobalDatabase<Channels> =
    GlobalDatabase::new("files/music_channel_broadcast.json");

static BANGERS: GlobalDatabase<Vec<SentBanger>> = GlobalDatabase::new("files/sent-bangers.json");

pub async fn initialize() {
    use pubsub::events;

    async fn handler(ctx: &Context, message: &Message) -> anyhow::Result<()> {
        if message.author.bot {
            return Ok(());
        }
        let channels = CHANNELS.load().await.context("loading channels database")?;
        if !channels.sources.contains(&message.channel_id) {
            return Ok(());
        }
        for url in parse_urls_from_message(&message.content) {
            for ch in channels
                .destinations
                .iter()
                .filter(|ch| **ch != message.channel_id)
            {
                if let Err(error) = ch
                    .say(
                        ctx,
                        format!(
                            "new banger from {}: {}",
                            message.channel_id.mention(),
                            url.as_str()
                        ),
                    )
                    .await
                {
                    tracing::error!(?error, channel = %ch, "failed to send message")
                }
            }
            if let Ok(url) = url.as_str().parse() {
                if let Err(error) = store_banger(message.author.id, url).await {
                    tracing::error!(?error, "failed to store banger")
                }
            }
        }
        Ok(())
    }

    pubsub::subscribe::<events::Message, _>(|ctx: &Context, message: &Message| {
        async move {
            if let Err(error) = handler(ctx, message).await {
                tracing::error!(
                    ?error,
                    "failed to handle message that might have come from a music channel"
                );
            }
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
}

fn parse_urls_from_message(content: &str) -> impl Iterator<Item = Match<'_>> {
    static IS_URL: OnceLock<Regex> = OnceLock::new();
    let is_url = IS_URL.get_or_init(|| Regex::new(r"https?://[^\s]+").unwrap());
    fn is_valid(s: &Match<'_>) -> bool {
        static INVALID_URLS: OnceLock<[Regex; 1]> = OnceLock::new();
        let invalid_urls = INVALID_URLS.get_or_init(|| [Regex::new(r"tenor\.com").unwrap()]);
        invalid_urls.iter().all(|m| !m.is_match(s.as_str()))
    }
    is_url.find_iter(content).filter(is_valid)
}

async fn store_banger(author: UserId, url: Url) -> anyhow::Result<()> {
    BANGERS.load().await?.push(SentBanger {
        sender: author,
        url,
    });

    Ok(())
}

pub async fn add_source(ch: ChannelId) -> anyhow::Result<bool> {
    let inserted = CHANNELS
        .load()
        .await
        .context("loading channels database")?
        .sources
        .insert(ch);
    Ok(inserted)
}

pub async fn rm_source(ch: ChannelId) -> anyhow::Result<bool> {
    let removed = CHANNELS
        .load()
        .await
        .context("loading channels database")?
        .sources
        .remove(&ch);
    Ok(removed)
}

pub async fn add_destination(ch: ChannelId) -> anyhow::Result<bool> {
    let inserted = CHANNELS
        .load()
        .await
        .context("loading channels database")?
        .destinations
        .insert(ch);
    Ok(inserted)
}

pub async fn rm_destination(ch: ChannelId) -> anyhow::Result<bool> {
    let removed = CHANNELS
        .load()
        .await
        .context("loading channels database")?
        .destinations
        .remove(&ch);
    Ok(removed)
}
