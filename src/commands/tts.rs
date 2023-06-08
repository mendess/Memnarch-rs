use crate::commands::sfx::STOP_COMMAND;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::Message,
    prelude::*,
};
use std::{error::Error, sync::OnceLock};
use tokio::sync::RwLock;

#[group]
#[commands(say, save, config, list, stop)]
#[prefix("tts")]
#[default_command(say)]
struct Tts;

#[command]
#[min_args(1)]
#[description("play a tts message over voice")]
#[usage("text")]
#[example("pogchamp")]
pub async fn say(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    super::sfx::play_sfx(ctx, msg, || async {
        let text = args.rest();
        let service = current_service().read().await;
        let voice = current_voice().read().await;
        let tts_link = generate_tts(Some(&*service), Some(&*voice), text).await?;
        match songbird::ytdl(&tts_link).await {
            Ok(source) => Ok(source),
            Err(e) => Err(format!("Failed getting audio source: {:?}", e).into()),
        }
    })
    .await
}

fn current_service() -> &'static RwLock<String> {
    static CURRENT_SERVICE: OnceLock<RwLock<String>> = OnceLock::new();
    CURRENT_SERVICE.get_or_init(|| RwLock::new(String::from("Polly")))
}

fn current_voice() -> &'static RwLock<String> {
    static CURRENT_VOICE: OnceLock<RwLock<String>> = OnceLock::new();
    CURRENT_VOICE.get_or_init(|| RwLock::new(String::from("Brian")))
}

#[command]
#[min_args(2)]
#[description("change tts defaults")]
#[usage("Polly Brian")]
pub async fn config(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let service = args.single::<String>()?;
    let voice = args.single::<String>()?;
    msg.channel_id
        .say(
            &ctx,
            format!(
                "Defaults change to service = {} and voice = {}",
                service, voice,
            ),
        )
        .await?;
    *current_service().write().await = service;
    *current_voice().write().await = voice;
    Ok(())
}

async fn generate_tts(
    service: Option<&str>,
    voice: Option<&str>,
    text: &str,
) -> Result<String, Box<dyn Error + Send + Sync + 'static>> {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    let client = CLIENT.get_or_init(Client::new);

    let service = service.unwrap_or("Polly");
    let voice = voice.unwrap_or("Brian");
    tracing::info!("Fetching {}:{}:{:?}", service, voice, text);
    let response = client
        .post("https://lazypy.ro/tts/proxy.php")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("service={}&voice={}&text={}", service, voice, text))
        .send()
        .await?
        .json::<TtsResponse>()
        .await?;
    match response {
        TtsResponse::Success { speak_url, .. } => {
            tracing::info!("Playing {}", speak_url);
            Ok(speak_url)
        }
        TtsResponse::Error { error } => Err(error.into()),
    }
}

#[command]
#[min_args(1)]
#[description("generates a tss audio file")]
#[usage("text")]
#[example("pogchamp")]
pub async fn save(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let text = args.rest();
    let service = current_service().read().await;
    let voice = current_voice().read().await;
    let tts_link = generate_tts(Some(&*service), Some(&*voice), text).await?;
    msg.channel_id.say(&ctx, tts_link).await?;
    Ok(())
}

#[command]
pub async fn list(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id.say(&ctx, "not implemented yet").await?;
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
