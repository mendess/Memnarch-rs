use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        macros::{command, group},
        CommandResult,
    },
    model::channel::Message,
    prelude::*,
};

use crate::consts::FILES_DIR;

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
}

impl TypeMapKey for QuoteManager {
    type Value = Arc<Mutex<QuoteManager>>;
}

group!({
    name: "Quotes",
    options: {},
    commands: [quote],
});

#[command]
#[description("Quote briliant minds")]
fn quote(ctx: &mut Context, msg: &Message) -> CommandResult {
    let mut share_map = ctx.data.write();
    let quotes = match share_map.get::<QuoteManager>() {
        Some(quotes) => quotes.lock()?,
        None => {
            share_map.insert::<QuoteManager>(Arc::new(Mutex::new(QuoteManager::load()?)));
            share_map.get::<QuoteManager>().unwrap().lock()?
        }
    };
    msg.channel_id
        .say(&ctx, quotes.choose().unwrap_or("No quotes found!"))?;
    Ok(())
}
