use poise::command;
use serenity::all::ChannelId;

pub fn commands() -> impl Iterator<Item = super::Command> {
    [toggle_spoilers()].into_iter()
}

/// Toggles on or off the posting of new magic the gathering cards as they are revealed.
#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
async fn toggle_spoilers(ctx: super::Context<'_>, ch: ChannelId) -> anyhow::Result<()> {
    let action = crate::mtg_spoilers::toggle_channel(ch).await?;
    ctx.say(format!("{action:?}")).await?;
    Ok(())
}
