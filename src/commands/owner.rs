use chrono::DateTime;
use lazy_static::lazy_static;
use reqwest::{header, Client};
use serde::Deserialize;
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::Message,
    prelude::*,
};
use std::{
    cmp::Reverse,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::Command as StdFork,
    str,
};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    process::Command as Fork,
    sync::{Mutex, TryLockError},
};

const EXE_NAME: &str = "memnarch-rs";

#[group]
#[owners_only]
#[commands(update, pull_update, cargo_restart, restart)]
struct Owner;

#[command]
#[description("Reboots the bot")]
async fn cargo_restart(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    msg.channel_id.say(ctx, "Rebooting...").await?;
    std::env::set_var("RUST_BACKTRACE", "1");
    let error = StdFork::new("cargo")
        .args(&["run", "--release", "--", "-r", &msg.channel_id.to_string()])
        .exec();
    std::env::remove_var("RUST_BACKTRACE");
    Err(error.into())
}

#[command]
#[description("Reboots the bot")]
#[aliases("reboot")]
async fn restart(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    msg.channel_id.say(ctx, "Rebooting...").await?;
    std::env::set_var("RUST_BACKTRACE", "1");
    let error = StdFork::new("bash")
        .args(&[
            "-c",
            &format!("exec ./{} -r {}", EXE_NAME, &msg.channel_id.to_string()),
        ])
        .exec();
    std::env::remove_var("RUST_BACKTRACE");
    Err(error.into())
}

lazy_static! {
    static ref UPDATING: Mutex<()> = Mutex::new(());
}

#[command]
#[description("Update the bot")]
async fn pull_update(ctx: &Context, msg: &Message) -> CommandResult {
    let _ = match UPDATING.try_lock() {
        Err(_) => return Err("Alreading updating".into()),
        Ok(guard) => guard,
    };
    async fn check_msg(mut m: Message, ctx: &Context) -> serenity::Result<()> {
        let new_msg = format!("{} :white_check_mark:", m.content);
        m.edit(ctx, |m| m.content(new_msg)).await
    }
    let message = msg.channel_id.say(&ctx, "Fetching...").await?;
    Fork::new("git").arg("fetch").spawn()?.wait().await?;
    check_msg(message, ctx).await?;

    let message = msg.channel_id.say(&ctx, "Checking remote...").await?;
    let status = Fork::new("git")
        .args(&["rev-list", "--count", "master...master@{upstream}"])
        .output()
        .await?;
    check_msg(message, ctx).await?;

    if 0 == String::from_utf8_lossy(&status.stdout)
        .trim()
        .parse::<i32>()?
    {
        return Err("No updates!".into());
    }

    let message = msg.channel_id.say(&ctx, "Pulling from remote...").await?;
    let out = &Fork::new("git").arg("pull").output().await?;
    if !out.status.success() {
        return Err(format!(
            "Error pulling!
            ```
            ============= stdout =============
            {}
            ============= stderr =============
            {}
            ```",
            str::from_utf8(&out.stdout)?,
            str::from_utf8(&out.stderr)?
        )
        .into());
    }
    check_msg(message, ctx).await?;

    let message = msg.channel_id.say(&ctx, "Compiling...").await?;
    let out = &Fork::new("cargo")
        .args(&["build", "--release", "-j", "1"])
        .output()
        .await?;
    if !out.status.success() {
        return Err(format!(
            "Build Error!
            ```
            ============= stderr =============
            {}
            ```",
            {
                let s = str::from_utf8(&out.stderr)?;
                &s[s.len() - 1500..]
            }
        )
        .into());
    }
    check_msg(message, ctx).await?;

    cargo_restart(ctx, msg, _args).await
}

#[command]
#[description("Update the bot")]
async fn update(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let _ = match UPDATING.try_lock() {
        Err(_) => return Err("Alreading updating".into()),
        Ok(guard) => guard,
    };

    #[derive(Deserialize)]
    struct Release {
        created_at: DateTime<chrono::Utc>,
        assets_url: String,
    }
    let client = Client::new();

    println!("Getting available releases");
    let asset_url = client
        .get("https://api.github.com/repos/mendess/Memnarch-rs/releases")
        .header(header::USER_AGENT, "mendess")
        .send()
        .await?
        .json::<Vec<Release>>()
        .await?
        .into_iter()
        .min_by_key(|x| Reverse(x.created_at))
        .ok_or_else(|| "No new releases")?
        .assets_url;

    #[derive(Deserialize)]
    struct Asset {
        browser_download_url: String,
        name: String,
    }
    println!("Getting lattest release url");
    let executable_url = client
        .get(&asset_url)
        .header(header::USER_AGENT, "mendess")
        .send()
        .await?
        .json::<Vec<Asset>>()
        .await?
        .into_iter()
        .find(|x| x.name == EXE_NAME)
        .map(|x| x.browser_download_url)
        .ok_or("Release doesn't contain executable")?;

    println!("Downloading lattest release");
    let (mut temp_file, temp_path) = tempfile::NamedTempFile::new_in(".")?.into_parts();
    let bytes = client
        .get(&executable_url)
        .header(header::USER_AGENT, "mendess")
        .send()
        .await?
        .bytes()
        .await?;
    File::from_std(temp_file).write_all(&bytes).await?;
    println!("Renaming");
    fs::rename(&temp_path, EXE_NAME).await?;
    let mut perm = fs::metadata(EXE_NAME).await?.permissions();
    let mode = perm.mode() | 0o700;
    println!("Setting mode: {:o} => {:o}", perm.mode(), mode);
    perm.set_mode(mode);
    fs::set_permissions(EXE_NAME, perm).await?;

    println!("Restaring");
    restart(ctx, msg, _args).await
}
