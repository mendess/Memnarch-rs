use serenity::{
    all::Mention,
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::Message,
    prelude::*,
};

use crate::util::MentionExt;

#[group]
#[commands(create, remove)]
#[prefix("cal")]
struct Calendar;

#[command]
#[min_args(1)]
#[description("Create a calendar in a channel")]
async fn create(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let ch = args.single::<Mention>()?.into_channel()?;
    crate::calendar::new(ctx, ch).await?;
    msg.channel_id.say(ctx, "Created!").await?;
    Ok(())
}

#[command]
#[min_args(1)]
#[description("Remove a calendar from the channel")]
async fn remove(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let ch = args.single::<Mention>()?.into_channel()?;
    crate::calendar::remove(ctx, ch).await?;
    msg.channel_id.say(ctx, "Removed!").await?;
    Ok(())
}
