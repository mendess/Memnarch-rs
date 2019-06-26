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

group!({
    name: "Owner",
    options: {owners_only: true},
    commands: [update],
});

#[command]
#[description("Update the bot")]
fn update(ctx: &mut Context, msg: &Message) -> CommandResult {
    eprintln!("Fetching");
    Fork::new("git").arg("fetch").spawn()?.wait()?;
    eprintln!("Checking remote");
    let status = Fork::new("git")
        .args(&["rev-list", "--count", "master...master@{upstream}"])
        .output()?;
    if let 0 = String::from_utf8_lossy(&status.stdout)
        .trim()
        .parse::<i32>()?
    {
        msg.channel_id.say(&ctx, "No updates!").map(|_| ())?;
    } else if !Fork::new("git").arg("pull").output()?.status.success() {
        msg.channel_id.say(&ctx, "Error pulling!")?;
    } else if !Fork::new("cargo")
        .args(&["build", "--release"])
        .output()?
        .status
        .success()
    {
        msg.channel_id.say(&ctx, "Build Error")?;
    } else {
        msg.channel_id
            .say(ctx, "Pulled and compiled. Rebooting...")?;
        Err(Fork::new("cargo")
            .args(&["run", "--release", "--", "-r", &msg.channel_id.to_string()])
            .exec())?;
    }
    Ok(())
}
