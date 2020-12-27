use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serenity::{
    client::bridge::voice::ClientVoiceManager,
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::{channel::Message, id::GuildId},
    prelude::*,
    voice,
};
use std::error::Error;

#[group]
#[commands(tts, tts_save)]
struct Tts;

#[command]
#[min_args(1)]
#[description("play a tts message over voice")]
#[usage("tts text")]
#[example("pogchamp")]
pub fn tts(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    crate::commands::sfx::play_sfx(ctx, msg, || {
        let text = args.rest();
        let tts_link = generate_tts(text)?;
        Ok(voice::ytdl(&tts_link)?)
    })
}

fn generate_tts(text: &str) -> Result<String, Box<dyn Error>> {
    let cli = Client::new();
    let request = cli
        .post("https://lazypy.ro/tts/proxy.php")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("service=Polly&voice=Brian&text={}", text))
        .build()?;
    println!("{:?}", request.body());
    let response: String = cli.execute(dbg!(request))?.text()?;
    println!("{:?}", response);
    match serde_json::from_str::<TtsResponse>(&response)? {
        TtsResponse::Success { speak_url, .. } => Ok(speak_url),
        TtsResponse::Error { error } => Err(error.into()),
    }
}

#[command]
#[min_args(1)]
#[description("generates a tss audio file")]
#[usage("tts_save text")]
#[example("pogchamp")]
pub fn tts_save(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let text = args.rest();
    let tts_link = generate_tts(text)?;
    msg.channel_id.say(&ctx, tts_link)?;
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
