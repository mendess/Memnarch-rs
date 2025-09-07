use std::{fmt, str::FromStr};

use serenity::all::ChannelId;

use crate::{
    commands::{Command, Context},
    features::music_channel_broadcast,
};
use poise::command;

pub fn commands() -> impl Iterator<Item = Command> {
    [music_broadcast()].into_iter()
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Source,
    Destination,
}

#[derive(Debug, Clone)]
pub struct ParseError(String);

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Kind {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "source" => Ok(Self::Source),
            "destination" | "dest" => Ok(Self::Destination),
            _ => Err(ParseError(format!("invalid kind: {s}"))),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Add,
    Remove,
}

impl FromStr for Mode {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "add" | "insert" | "register" => Ok(Self::Add),
            "rm" | "remove" => Ok(Self::Remove),
            _ => Err(ParseError(format!("invalid mode: {s}"))),
        }
    }
}

#[command(slash_command, guild_only, owners_only)]
pub async fn music_broadcast(
    ctx: Context<'_>,
    kind: Kind,
    mode: Mode,
    channel: ChannelId,
) -> anyhow::Result<()> {
    let actioned = match (kind, mode) {
        (Kind::Source, Mode::Add) => music_channel_broadcast::add_source(channel).await,
        (Kind::Source, Mode::Remove) => music_channel_broadcast::rm_source(channel).await,
        (Kind::Destination, Mode::Add) => music_channel_broadcast::add_destination(channel).await,
        (Kind::Destination, Mode::Remove) => music_channel_broadcast::rm_destination(channel).await,
    }?;

    match (mode, actioned) {
        (Mode::Add, true) => ctx.say("channel added").await?,
        (Mode::Remove, true) => ctx.say("channel removed").await?,
        (Mode::Add, false) => ctx.say("channel was already added").await?,
        (Mode::Remove, false) => ctx.say("channel not found").await?,
    };

    Ok(())
}
