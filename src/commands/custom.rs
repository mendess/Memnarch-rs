use crate::features::custom_commands;
use itertools::Itertools;
use serenity::{
    all::{CreateEmbed, CreateMessage},
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::Message,
    prelude::*,
};

#[group]
#[prefix("custom")]
#[commands(add, remove, list)]
struct Custom;

#[command]
#[min_args(2)]
async fn add(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let mut args_it = args.raw();
    let cmd = args_it.next().unwrap().to_string();
    let output = args_it.join(" ");
    custom_commands::add(msg.guild_id.ok_or("not in a guild")?, cmd, output).await?;
    msg.channel_id.say(&ctx, "Command added!").await?;
    Ok(())
}

#[command]
#[min_args(1)]
async fn remove(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let cmd = args.raw().next().unwrap();
    let output = custom_commands::remove(msg.guild_id.ok_or("not in a guild")?, cmd).await?;
    match output {
        Some(output) => {
            msg.channel_id
                .say(&ctx, format!("Command removed: {} => '{}'!", cmd, output))
                .await?
        }
        None => {
            msg.channel_id
                .say(&ctx, format!("Command {} doesn't exist!", cmd))
                .await?
        }
    };
    Ok(())
}

#[command]
async fn list(ctx: &Context, msg: &Message) -> CommandResult {
    let cmds = custom_commands::list(msg.guild_id.ok_or("not in a guild")?).await?;
    msg.channel_id
        .send_message(
            &ctx,
            CreateMessage::new().embed({
                let mut e = CreateEmbed::new();
                if !cmds.is_empty() {
                    let count = cmds.len();
                    e = e.description(
                        cmds.into_iter()
                            .fold(String::with_capacity(count * 5), |d, (key, value)| {
                                d + &format!("{} - {}\n", key, value)
                            }),
                    );
                }
                e.title("List of custom commands")
            }),
        )
        .await?;
    Ok(())
}
