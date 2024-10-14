#![warn(unused_crate_dependencies)]
#![warn(unused_features)]
#![deny(unused_must_use)]
#![warn(rust_2018_idioms)]
#![expect(deprecated)]

pub mod commands;
pub mod features;
pub mod prefs;
pub mod util;

use toml as _;
use tracing_subscriber as _;

use commands::custom::CustomCommands;
use features::{birthdays, calendar, moderation, mtg_spoilers, reminders};

use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        help_commands,
        macros::{help, hook},
        Args, CommandError, CommandGroup, CommandResult, DispatchError, HelpOptions,
    },
    model::{
        channel::Message,
        id::{ChannelId, GuildId, UserId},
    },
    prelude::*,
};

use std::collections::HashSet;

fn default_py_eval_address() -> String {
    "localhost:31415".into()
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub token: String,
    #[serde(default = "default_py_eval_address")]
    pub py_eval_address: String,
    pub monitor_log_channel: Option<ChannelId>,
}

impl Config {
    pub fn new(token: String) -> Self {
        Self {
            token,
            py_eval_address: default_py_eval_address(),
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
async fn normal_message(ctx: &Context, msg: &Message) {
    if ctx.cache.current_user().id == msg.author.id {
        return;
    }
    if !msg.content.starts_with('|') {
        return;
    }
    async fn f(ctx: &Context, msg: &Message, g: GuildId) -> anyhow::Result<()> {
        let cmd = match &msg.content.split_whitespace().next() {
            Some(s) if !s.is_empty() => &s[1..],
            _ => return Ok(()),
        };
        tracing::trace!("looking for command: {}", cmd);
        if let Some(o) = crate::get!(mut ctx, CustomCommands, write).execute(g, cmd)? {
            msg.channel_id.say(&ctx, o).await?;
        }
        Ok(())
    }
    if let Some(g) = msg.guild_id {
        if let Err(e) = f(ctx, msg, g).await {
            tracing::error!("Custom command failed: {:?}", e);
        }
    }
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
