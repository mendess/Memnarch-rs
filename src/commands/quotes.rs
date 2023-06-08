use crate::get;
use crate::util::permissions::*;
use crate::util::consts::FILES_DIR;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::Message,
    prelude::*,
};
use std::{path::PathBuf, sync::Arc};
use tokio::{
    fs::{DirBuilder, File},
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

const QUOTES_DIR: &str = "quotes";
const QUOTES_FILE: &str = "quotes.json";

#[group]
#[prefix("quote")]
#[default_command(quote)]
#[commands(add)]
#[checks("is_friend")]
struct Quotes;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuoteManager(Vec<String>);

impl QuoteManager {
    async fn path() -> std::io::Result<PathBuf> {
        let p = [FILES_DIR, QUOTES_DIR, QUOTES_FILE]
            .iter()
            .collect::<PathBuf>();
        DirBuilder::new()
            .recursive(true)
            .create(p.parent().expect("This path always has enough components"))
            .await?;
        Ok(p)
    }

    async fn load() -> std::io::Result<Self> {
        let path = Self::path().await?;
        let mut file = File::open(path).await?;
        let mut s = String::new();
        // TODO: don't read to a string
        file.read_to_string(&mut s).await.and_then(|_| {
            serde_json::from_str(&s).map_err(|e| {
                tracing::error!("Error parsing quotes");
                e.into()
            })
        })
    }

    fn choose(&self) -> Option<&str> {
        self.0.choose(&mut rand::thread_rng()).map(|x| x.as_str())
    }

    async fn add(&mut self, quote: String) -> std::io::Result<()> {
        self.0.push(quote);
        let path = Self::path().await?;
        tracing::trace!("Quote add: {:?}", path);
        // TODO: don't write to a string
        let content = serde_json::to_string(self)?;
        File::create(path)
            .await?
            .write_all(content.as_bytes())
            .await
    }
}

impl TypeMapKey for QuoteManager {
    type Value = Arc<Mutex<QuoteManager>>;
}

#[command]
#[description("Quote briliant minds")]
async fn quote(ctx: &Context, msg: &Message) -> CommandResult {
    let quotes = fetch_quotes(ctx).await;
    msg.channel_id
        .say(
            &ctx,
            quotes.lock().await.choose().unwrap_or("No quotes found!"),
        )
        .await?;
    Ok(())
}

#[command]
#[description("Add a quote")]
#[min_args(1)]
#[checks("is_friend")]
async fn add(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let quotes = fetch_quotes(ctx).await;
    let quote = args.rest();
    quotes.lock().await.add(quote.to_owned()).await?;
    msg.channel_id.say(ctx, "Quote added").await?;
    Ok(())
}

async fn fetch_quotes(ctx: &Context) -> Arc<Mutex<QuoteManager>> {
    let mut share_map = ctx.data.write().await;
    match share_map.get::<QuoteManager>() {
        Some(quotes) => Arc::clone(quotes),
        None => {
            share_map.insert::<QuoteManager>(Arc::new(Mutex::new(
                QuoteManager::load().await.unwrap_or_default(),
            )));
            Arc::clone(get!(> share_map, QuoteManager))
        }
    }
}
