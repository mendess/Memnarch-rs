#![warn(unused_crate_dependencies)]
#![warn(unused_features)]
#![expect(deprecated)]

pub mod commands;
pub mod features;
pub mod prefs;
pub mod util;

use toml as _;
use tracing_subscriber as _;

use features::{birthdays, moderation, mtg_spoilers, reminders};

use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        Args, CommandError, CommandGroup, CommandResult, DispatchError, HelpOptions, help_commands,
        macros::{help, hook},
    },
    model::{
        channel::Message,
        id::{ChannelId, UserId},
    },
    prelude::*,
};

use std::collections::HashSet;

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub token: String,
    pub monitor_log_channel: Option<ChannelId>,
}

impl Config {
    pub fn new(token: String) -> Self {
        Self {
            token,
            monitor_log_channel: None,
        }
    }
}

impl TypeMapKey for Config {
    type Value = Self;
}

#[help]
#[max_levenshtein_distance(5)]
#[lacking_permissions("hide")]
#[wrong_channel("hide")]
#[lacking_conditions("hide")]
#[lacking_ownership("hide")]
#[strikethrough_commands_tip_in_guild(" ")]
#[strikethrough_commands_tip_in_dm(" ")]
#[embed_success_colour("#71A5B0")]
#[indention_prefix("- ")]
async fn my_help(
    context: &Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    let _ = help_commands::with_embeds(context, msg, args, help_options, groups, owners).await;
    Ok(())
}

#[hook]
async fn after(ctx: &Context, msg: &Message, cmd_name: &str, error: Result<(), CommandError>) {
    match error {
        Ok(()) => {
            tracing::trace!("Processed command '{}' for user '{}'", cmd_name, msg.author)
        }
        Err(why) => {
            let _ = msg.channel_id.say(ctx, why.to_string()).await;
            tracing::trace!("Command '{}' failed with {:?}", cmd_name, why)
        }
    }
}

#[hook]
async fn on_dispatch_error(ctx: &Context, msg: &Message, e: DispatchError, command_name: &str) {
    msg.channel_id
        .say(ctx, format!("failed to dispatch {command_name}. {:?}", e))
        .await
        .expect("Couldn't communicate dispatch error");
}

#[macro_export]
macro_rules! get {
    ($ctx:ident, $t:ty) => {
        $ctx.data.read().await.get::<$t>().expect(::std::concat!(
            ::std::stringify!($t),
            " was not initialized"
        ))
    };
    (mut $ctx:ident, $t:ty) => {
        $ctx.data
            .write()
            .await
            .expect("lock took too long")
            .get_mut::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
    };
    ($ctx:ident, $t:ty, $lock:ident) => {
        $ctx.data
            .read()
            .await
            .get::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
            .$lock()
            .await
    };
    (mut $ctx:ident, $t:ty, $lock:ident) => {
        $ctx.data
            .write()
            .await
            .get_mut::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
            .$lock()
            .await
    };
    (> $data:ident, $t:ty) => {
        $data.get::<$t>().expect(::std::concat!(
            ::std::stringify!($t),
            " was not initialized"
        ))
    };
    (> $data:ident, $t:ty, $lock:ident) => {
        $data
            .get::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
            .$lock()
            .await
    };
}
