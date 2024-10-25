use std::{collections::HashSet, fs::Permissions, os::unix::fs::PermissionsExt, sync::OnceLock};

use anyhow::Context as _;
use futures::FutureExt;
use json_db::GlobalDatabase;
use pubsub::ControlFlow;
use regex::{Match, Regex};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serenity::{
    all::{Channel, Context, CreateAllowedMentions, CreateMessage, Message},
    http::CacheHttp,
    model::{
        id::{ChannelId, UserId},
        mention::Mentionable,
    },
};

use crate::in_files;

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
    GlobalDatabase::new(in_files!("music_channel_broadcast.json"));

static BANGERS: GlobalDatabase<Vec<SentBanger>> =
    GlobalDatabase::new_with_perms(in_files!("sent-bangers.json"), || {
        Permissions::from_mode(0b110_100_100)
    });

async fn broadcast_impl(
    channels: &json_db::DbGuard<'_, Channels, std::io::Error>,
    ctx: impl CacheHttp,
    author: UserId,
    source_channel_id: ChannelId,
    url: &str,
) -> anyhow::Result<()> {
    for ch in channels
        .destinations
        .iter()
        .filter(|ch| **ch != source_channel_id)
    {
        let author = async {
            let Channel::Guild(ch) = ch.to_channel(&ctx).await.ok()? else {
                return None;
            };
            ch.guild_id.member(&ctx, author).await.ok()
        }
        .await;
        tracing::info!(?ch, url, "sending banger to channel");
        let result = ch
            .send_message(
                ctx.http(),
                CreateMessage::new()
                    .content(match author {
                        Some(author) if source_channel_id == 952887798145355777 => {
                            format!("new banger from {}: {}", author.mention(), url)
                        }
                        None if source_channel_id == 952887798145355777 => {
                            format!("new banger: {}", url)
                        }
                        Some(author) => {
                            format!(
                                "new banger in {} from {}: {}",
                                source_channel_id.mention(),
                                author.mention(),
                                url
                            )
                        }
                        None => {
                            format!("new banger in {}: {}", source_channel_id.mention(), url)
                        }
                    })
                    .allowed_mentions(CreateAllowedMentions::new().empty_users()),
            )
            .await;
        if let Err(error) = result {
            tracing::error!(?error, channel = %ch, "failed to send message");
        };
    }
    if let Ok(url) = url.parse() {
        if let Err(error) = store_banger(author, url).await {
            tracing::error!(?error, "failed to store banger")
        }
    }
    Ok(())
}
pub async fn broadcast(
    ctx: impl CacheHttp,
    author: UserId,
    source_channel_id: ChannelId,
    url: &str,
) -> anyhow::Result<()> {
    let channels = tokio::time::timeout(std::time::Duration::from_secs(10), CHANNELS.load())
        .await
        .context("timed out loading db")?
        .context("loading channels database")?;
    broadcast_impl(&channels, ctx, author, source_channel_id, url).await
}

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
            broadcast_impl(
                &channels,
                &ctx.http,
                message.author.id,
                message.channel_id,
                url.as_str(),
            )
            .await?;
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

#[cfg(test)]
mod test {
    #[test]
    fn deser() {
        let s = r#"{"sources":["952887798145355777","955400996467654696","1260332705263128637","1052590945998217307"],"destinations":["1223937402368688230","955400996467654696","1052590945998217307","1260332705263128637"]}"#;

        let _: super::Channels = serde_json::from_str(s).unwrap();
    }

    #[test]
    fn parse_urls_from_message() {
        let content = "https://youtu.be/E58qLXBfLrs?si=dwgd8CizQuSde62o";
        match super::parse_urls_from_message(content)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [one] => assert_eq!(one.as_str(), content),
            e => panic!("invalid: {e:?}"),
        }
    }
}
