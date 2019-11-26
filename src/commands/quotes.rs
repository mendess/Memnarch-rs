use crate::consts::FILES_DIR;
use crate::permissions::IS_FRIEND_CHECK;
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
use std::{
    fs::{DirBuilder, File},
    path::PathBuf,
    sync::{Arc, Mutex},
};

const QUOTES_DIR: &str = "quotes";
const QUOTES_FILE: &str = "quotes.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuoteManager(Vec<String>);

impl QuoteManager {
    fn path() -> PathBuf {
        [FILES_DIR, QUOTES_DIR, QUOTES_FILE].iter().collect()
    }

    fn load() -> Self {
        File::open(Self::path())
            .and_then(|file| {
                serde_json::from_reader(file).map_err(|e| {
                    eprintln!("Error parsing quotes");
                    e.into()
                })
            })
            .unwrap_or_default()
    }

    fn choose(&self) -> Option<&str> {
        self.0.choose(&mut rand::thread_rng()).map(|x| x.as_str())
    }

    fn add(&mut self, quote: String) -> std::io::Result<()> {
        self.0.push(quote);
        let path = Self::path();
        println!("Quote add: {:?}", path);
        DirBuilder::new()
            .recursive(true)
            .create(path.parent().unwrap())?;
        serde_json::to_writer(File::create(path)?, self).map_err(Into::into)
    }
}

impl TypeMapKey for QuoteManager {
    type Value = Arc<Mutex<QuoteManager>>;
}

group!({
    name: "Quotes",
    options: {
        prefixes: ["quote"],
        default_command: quote,
    },
    commands: [add],
});

#[command]
#[description("Quote briliant minds")]
fn quote(ctx: &mut Context, msg: &Message) -> CommandResult {
    let quotes = fetch_quotes(ctx);
    msg.channel_id
        .say(&ctx, quotes.lock()?.choose().unwrap_or("No quotes found!"))?;
    Ok(())
}

#[command]
#[description("Add a quote")]
#[min_args(1)]
#[checks("is_friend")]
fn add(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let quotes = fetch_quotes(ctx);
    let quote = args.rest();
    quotes.lock()?.add(quote.to_owned())?;
    msg.channel_id.say(ctx, "Quote added")?;
    Ok(())
}

fn fetch_quotes(ctx: &mut Context) -> Arc<Mutex<QuoteManager>> {
    let mut share_map = ctx.data.write();
    match share_map.get::<QuoteManager>() {
        Some(quotes) => Arc::clone(quotes),
        None => {
            share_map.insert::<QuoteManager>(Arc::new(Mutex::new(QuoteManager::load())));
            Arc::clone(share_map.get_mut::<QuoteManager>().unwrap())
        }
    }
}
