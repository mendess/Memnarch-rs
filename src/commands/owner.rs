use chrono::DateTime;
use lazy_static::lazy_static;
use reqwest::{blocking::Client, header};
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
    fs,
    io::Write,
    os::unix::process::CommandExt,
    process::Command as Fork,
    str,
    sync::{Mutex, TryLockError},
};

#[group]
#[owners_only]
#[commands(update, pull_update, cargo_restart, restart)]
struct Owner;

#[command]
#[description("Reboots the bot")]
fn cargo_restart(ctx: &mut Context, msg: &Message, _args: Args) -> CommandResult {
    msg.channel_id.say(ctx, "Rebooting...")?;
    std::env::set_var("RUST_BACKTRACE", "1");
    let error = Fork::new("cargo")
        .args(&["run", "--release", "--", "-r", &msg.channel_id.to_string()])
        .exec();
    std::env::remove_var("RUST_BACKTRACE");
    Err(error.into())
}

#[command]
#[description("Reboots the bot")]
#[aliases("reboot")]
fn restart(ctx: &mut Context, msg: &Message, _args: Args) -> CommandResult {
    msg.channel_id.say(ctx, "Rebooting...")?;
    std::env::set_var("RUST_BACKTRACE", "1");
    let error = Fork::new("./memnarch-rs")
        .args(&["-r", &msg.channel_id.to_string()])
        .exec();
    std::env::remove_var("RUST_BACKTRACE");
    Err(error.into())
}

lazy_static! {
    static ref UPDATING: Mutex<()> = Mutex::new(());
}

#[command]
#[description("Update the bot")]
fn pull_update(ctx: &mut Context, msg: &Message) -> CommandResult {
    let _ = match UPDATING.try_lock() {
        Err(TryLockError::WouldBlock) => return Err("Alreading updating".into()),
        Err(TryLockError::Poisoned(p)) => return Err(p.into()),
        Ok(guard) => guard,
    };
    let check_msg = |mut m: Message| {
        let new_msg = format!("{} :white_check_mark:", m.content);
        m.edit(&ctx, |m| m.content(new_msg))
    };
    let message = msg.channel_id.say(&ctx, "Fetching...")?;
    Fork::new("git").arg("fetch").spawn()?.wait()?;
    check_msg(message)?;

    let message = msg.channel_id.say(&ctx, "Checking remote...")?;
    let status = Fork::new("git")
        .args(&["rev-list", "--count", "master...master@{upstream}"])
        .output()?;
    check_msg(message)?;

    if 0 == String::from_utf8_lossy(&status.stdout)
        .trim()
        .parse::<i32>()?
    {
        return Err("No updates!".into());
    }

    let message = msg.channel_id.say(&ctx, "Pulling from remote...")?;
    let out = &Fork::new("git").arg("pull").output()?;
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
    check_msg(message)?;

    let message = msg.channel_id.say(&ctx, "Compiling...")?;
    let out = &Fork::new("cargo")
        .args(&["build", "--release", "-j", "1"])
        .output()?;
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
    check_msg(message)?;

    cargo_restart(ctx, msg, _args)
}

#[command]
#[description("Update the bot")]
fn update(ctx: &mut Context, msg: &Message, _args: Args) -> CommandResult {
    let _ = match UPDATING.try_lock() {
        Err(TryLockError::WouldBlock) => return Err("Alreading updating".into()),
        Err(TryLockError::Poisoned(p)) => return Err(p.into()),
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
        .send()?
        .json::<Vec<Release>>()?
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
        .send()?
        .json::<Vec<Asset>>()?
        .into_iter()
        .find(|x| x.name == "memnarch-rs")
        .map(|x| x.browser_download_url)
        .ok_or("Release doesn't contain executable")?;

    println!("Downloading lattest release");
    let mut temp = tempfile::NamedTempFile::new_in(".")?;
    let bytes = client
        .get(&executable_url)
        .header(header::USER_AGENT, "mendess")
        .send()?
        .bytes()?;
    temp.write_all(&bytes)?;
    println!("Renaming");
    fs::rename(&temp, "memnarch-rs")?;

    println!("Restaring");
    restart(ctx, msg, _args)
}
