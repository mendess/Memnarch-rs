use lazy_static::lazy_static;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::Message,
    prelude::*,
    voice,
};
use std::{error::Error, sync::RwLock};
use crate::commands::sfx::STOP_COMMAND;

#[group]
#[commands(tts, save, config, list, stop)]
#[prefix("tts")]
#[default_command(tts)]
struct Tts;

#[command]
#[min_args(1)]
#[description("play a tts message over voice")]
#[usage("text")]
#[example("pogchamp")]
pub fn tts(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    crate::commands::sfx::play_sfx(ctx, msg, || {
        let text = args.rest();
        let tts_link = generate_tts(
            Some(&*CURRENT_SERVICE.read().unwrap()),
            Some(&*CURRENT_VOICE.read().unwrap()),
            text,
        )?;
        Ok(voice::ytdl(&tts_link)?)
    })
}

lazy_static! {
    static ref CURRENT_SERVICE: RwLock<String> = RwLock::new(String::from("Polly"));
    static ref CURRENT_VOICE: RwLock<String> = RwLock::new(String::from("Brian"));
}

#[command]
#[min_args(2)]
#[description("change tts defaults")]
#[usage("Polly Brian")]
pub fn config(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let service = args.single::<String>()?;
    let voice = args.single::<String>()?;
    msg.channel_id.say(
        &ctx,
        format!(
            "Defaults change to service = {} and voice = {}",
            service, voice,
        ),
    )?;
    *CURRENT_SERVICE.write().unwrap() = service;
    *CURRENT_VOICE.write().unwrap() = voice;
    Ok(())
}

fn generate_tts(
    service: Option<&str>,
    voice: Option<&str>,
    text: &str,
) -> Result<String, Box<dyn Error>> {
    lazy_static! {
        static ref CLIENT: Client = Client::new();
    }
    let service = service.unwrap_or("Polly");
    let voice = voice.unwrap_or("Brian");
    eprintln!("[tts] Fetching {}:{}:{:?}", service, voice, text);
    let response = CLIENT
        .post("https://lazypy.ro/tts/proxy.php")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!(
            "service={}&voice={}&text={}",
            service,
            voice,
            text
        ))
        .send()?
        .json::<TtsResponse>()?;
    match response {
        TtsResponse::Success { speak_url, .. } => {
            eprintln!("[tts] Playing {}", speak_url);
            Ok(speak_url)
        },
        TtsResponse::Error { error } => Err(error.into()),
    }
}

#[command]
#[min_args(1)]
#[description("generates a tss audio file")]
#[usage("text")]
#[example("pogchamp")]
pub fn save(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let text = args.rest();
    let tts_link = generate_tts(
        Some(&*CURRENT_SERVICE.read().unwrap()),
        Some(&*CURRENT_VOICE.read().unwrap()),
        text,
    )?;
    msg.channel_id.say(&ctx, tts_link)?;
    Ok(())
}

#[command]
pub fn list(ctx: &mut Context, msg: &Message) -> CommandResult {
    msg.channel_id.say(&ctx, "not implemented yet")?;
    Ok(())
}

/*
 * {success: true, speak_url: "ola"}
 * {error: "reason"}
 */

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum TtsResponse {
    Success { success: bool, speak_url: String },
    Error { error: String },
}
