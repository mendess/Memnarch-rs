use poise::command;

pub fn commands() -> impl Iterator<Item = super::Command> {
    [quote(), quote_add()].into_iter()
}

/// Quote briliant minds
#[command(slash_command, guild_only)]
async fn quote(ctx: super::Context<'_>) -> anyhow::Result<()> {
    ctx.say(
        ctx.data()
            .quotes
            .lock()
            .await
            .choose()
            .unwrap_or("No quotes found!"),
    )
    .await?;
    Ok(())
}

/// Add a new quote
#[command(slash_command, guild_only)]
async fn quote_add(ctx: super::Context<'_>, quote: String) -> anyhow::Result<()> {
    ctx.data().quotes.lock().await.add(quote).await?;
    ctx.say("Quote added").await?;
    Ok(())
}
