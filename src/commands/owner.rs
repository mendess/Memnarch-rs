use std::str::FromStr;

use serenity::{
    client::Context,
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::{channel::Message, id::ChannelId},
};

use crate::features::music_channel_broadcast;

#[group]
#[owners_only]
#[commands(music_broadcast)]
struct Owner;

#[command]
pub async fn music_broadcast(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    #[derive(Debug, Clone, Copy)]
    enum Kind {
        Source,
        Destination,
    }

    impl FromStr for Kind {
        type Err = String;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "source" => Ok(Self::Source),
                "destination" | "dest" => Ok(Self::Destination),
                _ => Err(format!("invalid kind: {s}")),
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum Mode {
        Add,
        Remove,
    }

    impl FromStr for Mode {
        type Err = String;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "add" | "insert" | "register" => Ok(Self::Add),
                "rm" | "remove" => Ok(Self::Remove),
                _ => Err(format!("invalid mode: {s}")),
            }
        }
    }

    let (kind, mode) = (args.single::<Kind>()?, args.single::<Mode>()?);
    let ch_id = args.single::<ChannelId>()?;
    let actioned = match (kind, mode) {
        (Kind::Source, Mode::Add) => music_channel_broadcast::add_source(ch_id).await,
        (Kind::Source, Mode::Remove) => music_channel_broadcast::rm_source(ch_id).await,
        (Kind::Destination, Mode::Add) => music_channel_broadcast::add_destination(ch_id).await,
        (Kind::Destination, Mode::Remove) => music_channel_broadcast::rm_destination(ch_id).await,
    }?;

    match (mode, actioned) {
        (Mode::Add, true) => msg.channel_id.say(ctx, "channel added").await?,
        (Mode::Remove, true) => msg.channel_id.say(ctx, "channel removed").await?,
        (Mode::Add, false) => msg.channel_id.say(ctx, "channel was already added").await?,
        (Mode::Remove, false) => msg.channel_id.say(ctx, "channel not found").await?,
    };

    Ok(())
}
