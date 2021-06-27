use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::{channel::Message, id::ChannelId},
    prelude::*,
};

#[group]
#[commands(create, remove)]
#[prefix("cal")]
struct Calendar;

#[command]
#[min_args(1)]
#[description("Create a calendar in a channel")]
async fn create(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let ch = args.single::<ChannelId>().map_err(|_| "Invalid channel")?;
    crate::calendar::new(ctx, ch).await?;
    msg.channel_id.say(ctx, "Created!").await?;
    Ok(())
}

#[command]
#[min_args(1)]
#[description("Remove a calendar from the channel")]
async fn remove(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let ch = args.single::<ChannelId>().map_err(|_| "Invalid channel")?;
    crate::calendar::remove(ctx, ch).await?;
    msg.channel_id.say(ctx, "Removed!").await?;
    Ok(())
}
