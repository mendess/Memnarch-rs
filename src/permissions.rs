use serenity::{
    framework::standard::{macros::check, Args, CommandOptions, Reason},
    model::channel::Message,
    prelude::*,
};

#[check]
#[name = "is_friend"]
pub async fn is_friend(
    ctx: &Context,
    msg: &Message,
    _: &mut Args,
    _: &CommandOptions,
) -> Result<(), Reason> {
    let owner = ctx
        .http
        .get_current_application_info()
        .await
        .map(|info| info.owner.id)
        .ok();

    msg.guild_id
        .and_then(|id| (id.0 == 136_220_994_812_641_280).then(|| Ok(())))
        .or_else(|| (Some(msg.author.id) == owner).then(|| Ok(())))
        .unwrap_or_else(|| {
            Err(Reason::User(
                "You don't have permission to use that command!".to_string(),
            ))
        })
}
