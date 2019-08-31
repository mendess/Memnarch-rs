use serenity::{
    framework::standard::{
        macros::{command, group},
        CommandResult,
    },
    model::channel::Message,
    prelude::*,
};
use lazy_static::lazy_static;

use std::os::unix::process::CommandExt;
use std::process::Command as Fork;
use std::str;
use std::sync::{Mutex, TryLockError};

group!({
    name: "Owner",
    options: {owners_only: true},
    commands: [update],
});

#[command]
#[description("Update the bot")]
fn update(ctx: &mut Context, msg: &Message) -> CommandResult {
    lazy_static!{
        static ref UPDATING: Mutex<()> = Mutex::new(());
    };
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
    let out = &Fork::new("cargo").args(&["build", "--release"]).output()?;
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

    msg.channel_id.say(ctx, "Rebooting...")?;
    std::env::set_var("RUST_BACKTRACE", "1");
    let error = Fork::new("cargo")
        .args(&["run", "--release", "--", "-r", &msg.channel_id.to_string()])
        .exec();
    std::env::remove_var("RUST_BACKTRACE");
    Err(error.into())
}
