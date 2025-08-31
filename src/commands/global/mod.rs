mod owner;
mod reminders;

use poise::command;
use serenity::all::{CreateEmbed, CreateMessage};

pub fn commands() -> impl Iterator<Item = super::Command> {
    [ping(), who_are_you()]
        .into_iter()
        .chain(reminders::commands())
        .chain(owner::commands())
}

/// Ping me maybe
#[command(slash_command, dm_only)]
async fn ping(ctx: super::Context<'_>) -> anyhow::Result<()> {
    let sent_timestamp = ctx.created_at();
    let msg = ctx.say("Pong! calculating ms").await?;
    let rtt = msg.message().await?.timestamp.timestamp_millis() - sent_timestamp.timestamp_millis();
    msg.edit(
        ctx,
        poise::CreateReply::default().content(format!("Pong! {rtt}ms")),
    )
    .await?;
    Ok(())
}

/// Find out more about me
#[command(slash_command, dm_only, rename = "who-are-you")]
async fn who_are_you(ctx: super::Context<'_>) -> anyhow::Result<()> {
    ctx.channel_id()
        .send_message(ctx, CreateMessage::new()
            .embed(CreateEmbed::new()
                .title("I AM MEMNARCH")
                    .description("Sauce code: [GitHub](https://github.com/Mendess2526/Memnarch-rs)")
                    .image("https://cards.scryfall.io/art_crop/front/9/2/9203fde4-dbc1-449f-9618-4656f0e25e3c.jpg?1562925842")
            )
        )
        .await?;
    Ok(())
}
