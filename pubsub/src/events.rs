//! The events exposed by discord's api.
//!
//! See [serenity's docs](https://docs.rs/serenity/0.11.5/serenity/prelude/trait.EventHandler.html)
//! on what each event means.

use super::Event;
use serenity::model::{
    channel::Reaction,
    guild::Guild,
    id::{ChannelId, GuildId, MessageId},
    prelude::{Role, RoleId},
    voice::VoiceState,
};

macro_rules! events {
    ($($event:ident => $arg:ty),* $(,)?) => {
        $(
            pub struct $event;
            impl Event for $event {
                type Argument = $arg;
            }
        )*
    }
}

events! {
    ReactionAdd => Reaction,
    ReactionRemove => Reaction,
    ReactionRemoveAll => (ChannelId, MessageId),
    GuildRoleDelete => (GuildId, RoleId, Option<Role>),
    VoiceStateUpdate => (Option<GuildId>, Option<VoiceState>, VoiceState),
    Ready => serenity::model::gateway::Ready,
    CacheReady => Vec<GuildId>,
    GuildCreate => (Guild, bool),
}
