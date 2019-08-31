use serenity::{
    framework::standard::{
        macros::{command, group},
        CommandResult,
    },
    model::channel::Message,
    prelude::*,
};

use std::os::unix::process::CommandExt;
use std::process::Command as Fork;
use std::str;
use std::sync::atomic::{AtomicBool, Ordering};

group!({
    name: "Owner",
    options: {owners_only: true},
    commands: [update],
});

static UPDATING: AtomicBool = AtomicBool::new(false);

#[command]
#[description("Update the bot")]
fn update(ctx: &mut Context, msg: &Message) -> CommandResult {
    if UPDATING.load(Ordering::SeqCst) {
        return Err("Alreading updating".into());
    } else {
        UPDATING.store(true, Ordering::SeqCst);
    }

    msg.channel_id.say(&ctx, "Fetching...")?;
    Fork::new("git").arg("fetch").spawn()?.wait()?;

    msg.channel_id.say(&ctx, "Checking remote...")?;
    let status = Fork::new("git")
        .args(&["rev-list", "--count", "master...master@{upstream}"])
        .output()?;

    if 0 == String::from_utf8_lossy(&status.stdout)
        .trim()
        .parse::<i32>()?
    {
        return Err("No updates!".into());
    }

    msg.channel_id.say(&ctx, "Pulling from remote...")?;
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

    msg.channel_id.say(&ctx, "Compiling...")?;
    let out = &Fork::new("cargo").args(&["build", "--release"]).output()?;
    if !out.status.success() {
        return Err(format!(
            "Build Error!
            ```
            ============= stderr =============
            {}
            ```",
            str::from_utf8(&out.stderr)?
        )
        .into());
    }

    msg.channel_id.say(ctx, "Rebooting...")?;
    Err(Fork::new("cargo")
        .args(&["run", "--release", "--", "-r", &msg.channel_id.to_string()])
        .exec()
        .into())
}
