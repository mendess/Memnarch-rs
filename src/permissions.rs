use serenity::{
    framework::standard::{macros::check, Args, CheckResult, CommandOptions, Reason},
    model::channel::Message,
    prelude::*,
};

#[check]
#[name = "is_friend"]
pub fn is_friend(_: &mut Context, msg: &Message, _: &mut Args, _: &CommandOptions) -> CheckResult {
    msg.guild_id
        .and_then(|id| {
            if id.0 == 136_220_994_812_641_280 {
                Some(CheckResult::Success)
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            CheckResult::Failure(Reason::User(
                "You don't have permission to use that command!".to_string(),
            ))
        })
}
