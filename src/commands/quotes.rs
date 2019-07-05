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

use crate::consts::FILES_DIR;
use crate::permissions::IS_FRIEND_CHECK;

use std::error::Error;
use std::fs::File;
use std::sync::{Arc, Mutex};

const QUOTES_DIR: &str = "quotes/";
const QUOTES_FILE: &str = "quotes.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuoteManager(Vec<String>);

impl QuoteManager {
    fn load() -> Result<Self, Box<dyn Error>> {
        let file = File::open(format!("{}{}{}", FILES_DIR, QUOTES_DIR, QUOTES_FILE))?;
        Ok(serde_json::from_reader(file)?)
    }

    fn choose(&self) -> Option<&str> {
        self.0.choose(&mut rand::thread_rng()).map(|x| x.as_str())
    }

    fn add(&mut self, quote: String) -> std::io::Result<()> {
        self.0.push(quote);
        let file = File::create(format!("{}{}{}", FILES_DIR, QUOTES_DIR, QUOTES_FILE))?;
        serde_json::to_writer(file, self)?;
        Ok(())
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
    let quotes = fetch_quotes(ctx)?;
    msg.channel_id
        .say(&ctx, quotes.lock()?.choose().unwrap_or("No quotes found!"))?;
    Ok(())
}

#[command]
#[description("Add a quote")]
#[min_args(1)]
#[checks("is_friend")]
fn add(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let quotes = fetch_quotes(ctx)?;
    let quote = args.rest();
    quotes.lock()?.add(quote.to_owned())?;
    msg.channel_id.say(ctx, "Quote added")?;
    Ok(())
}

fn fetch_quotes(ctx: &mut Context) -> Result<Arc<Mutex<QuoteManager>>, Box<dyn Error>> {
    let mut share_map = ctx.data.write();
    Ok(match share_map.get::<QuoteManager>() {
        Some(quotes) => Arc::clone(quotes),
        None => {
            share_map.insert::<QuoteManager>(Arc::new(Mutex::new(QuoteManager::load()?)));
            Arc::clone(share_map.get_mut::<QuoteManager>().unwrap())
        }
    })
}
