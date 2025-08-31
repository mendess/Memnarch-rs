use poise::command;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tokio::sync::RwLock;

#[command(slash_command, guild_only, subcommands("say", "config", "download"))]
pub async fn tts(_: super::Context<'_>) -> anyhow::Result<()> {
    Ok(())
}

/// play a tts message over voice
#[command(slash_command, guild_only)]
pub async fn say(ctx: super::Context<'_>, text: String) -> anyhow::Result<()> {
    super::sfx::play_sfx(ctx, || async {
        let service = current_service().read().await;
        let voice = current_voice().read().await;
        let tts_link = generate_tts(Some(&*service), Some(&*voice), &text).await?;
        Ok(songbird::input::YoutubeDl::new(reqwest::Client::new(), tts_link).into())
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

/// change tts defaults
#[command(slash_command)]
pub async fn config(ctx: super::Context<'_>, service: String, voice: String) -> anyhow::Result<()> {
    ctx.say(format!(
        "Defaults change to service = {} and voice = {}",
        service, voice,
    ))
    .await?;
    *current_service().write().await = service;
    *current_voice().write().await = voice;
    Ok(())
}

async fn generate_tts(
    service: Option<&str>,
    voice: Option<&str>,
    text: &str,
) -> anyhow::Result<String> {
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
        TtsResponse::Error { error } => Err(anyhow::anyhow!("failed to generate tts: {error}")),
    }
}

/// generates a tss audio file
#[command(slash_command)]
pub async fn download(ctx: super::Context<'_>, text: String) -> anyhow::Result<()> {
    let service = current_service().read().await;
    let voice = current_voice().read().await;
    let tts_link = generate_tts(Some(&*service), Some(&*voice), &text).await?;
    ctx.say(tts_link).await?;
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
