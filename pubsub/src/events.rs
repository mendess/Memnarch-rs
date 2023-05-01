//! The events exposed by discord's api.
//!
//! See [serenity's docs](https://docs.rs/serenity/0.11.5/serenity/client/trait.EventHandler.html)
//! on what each event means.

use std::collections::HashMap;

use super::Event;
use serenity::{
    client::bridge::gateway::event::ShardStageUpdateEvent,
    json::Value,
    model::{
        application::{command::CommandPermission, interaction::Interaction},
        channel::Reaction,
        channel::{Channel, ChannelCategory, GuildChannel, PartialGuildChannel, StageInstance},
        event::{
            ChannelPinsUpdateEvent, GuildMembersChunkEvent, GuildScheduledEventUserAddEvent,
            GuildScheduledEventUserRemoveEvent, InviteCreateEvent, InviteDeleteEvent,
            MessageUpdateEvent, ResumedEvent, ThreadListSyncEvent, ThreadMembersUpdateEvent,
            TypingStartEvent, VoiceServerUpdateEvent,
        },
        gateway::Presence,
        guild::{
            automod::{ActionExecution, Rule},
            Emoji, Guild, Integration, Member, PartialGuild, Role, ScheduledEvent, ThreadMember,
            UnavailableGuild,
        },
        id::{
            ApplicationId, ChannelId, EmojiId, GuildId, IntegrationId, MessageId, RoleId, StickerId,
        },
        sticker::Sticker,
        user::{CurrentUser, User},
        voice::VoiceState,
    },
};

macro_rules! events {
    (
        $event:ident => { $($name:ident: $member_ty:ty),* $(,)? }
        $(, $($rest:tt)* )?
    ) => (
        pub struct $event {
            $(pub $name: $member_ty),*
        }
        impl Event for $event {
            type Argument = $event;
        }

        $(events! { $($rest)* })?
    );

    (
        $event:ident => $arg:ty
        $(, $($rest:tt)* )?
    ) => (
        pub struct $event;
        impl Event for $event {
            type Argument = $arg;
        }

        $(events! { $($rest)* })?
    );

    () => ();
}

events! {
    ApplicationCommandPermissionUpdate => CommandPermission,
    AutoModerationRuleCreate => Rule,
    AutoModerationRuleUpdate => Rule,
    AutoModerationRuleDelete => Rule,
    AutoModerationActionExecution => ActionExecution,
    CacheReady => Vec<GuildId>,
    ChannelCreate => GuildChannel,
    CategoryCreate => ChannelCategory,
    CategoryDelete => ChannelCategory,
    ChannelDelete => GuildChannel,
    ChannelPinsUpdate => ChannelPinsUpdateEvent,
    ChannelUpdate => { old: Option<Channel>, new: Channel },
    GuildBanAddition => { guild_id: GuildId, banned_user: User },
    GuildBanRemoval => { guild_id: GuildId, unbanned_user: User },
    GuildCreate => { guild: Guild, is_new: bool },
    GuildDelete => { incomplete: UnavailableGuild, full: Option<Guild> },
    GuildEmojisUpdate => { guild_id: GuildId, current_state: HashMap<EmojiId, Emoji> },
    GuildIntegrationsUpdate => GuildId,
    GuildMemberAddition => Member,
    GuildMemberRemoval => {
        guild_id: GuildId,
        user: User,
        member_data_if_available: Option<Member>
    },
    GuildMemberUpdate => { old_if_available: Option<Member>, new: Member },
    GuildMembersChunk => GuildMembersChunkEvent,
    GuildRoleCreate => Role,
    GuildRoleDelete => {
        guild_id: GuildId,
        removed_role_id: RoleId,
        removed_role_data_if_available: Option<Role>,
    },
    GuildRoleUpdate => { old_data_if_available: Option<Role>, new: Role, },
    GuildStickersUpdate => { guild_id: GuildId, current_state: HashMap<StickerId, Sticker>, },
    GuildUnavailable => GuildId,
    GuildUpdate => { old_data_if_available: Option<Guild>, new_but_incomplete: PartialGuild, },
    InviteCreate => InviteCreateEvent,
    InviteDelete => InviteDeleteEvent,
    Message => serenity::model::channel::Message,
    MessageDelete => {
        channel_id: ChannelId,
        deleted_message_id: MessageId,
        guild_id: Option<GuildId>,
    },
    MessageDeleteBulk => {
        channel_id: ChannelId,
        multiple_deleted_messages_ids: Vec<MessageId>,
        guild_id: Option<GuildId>,
    },
    MessageUpdate => {
        old_if_available: Option<serenity::model::channel::Message>,
        new: Option<serenity::model::channel::Message>,
        event: MessageUpdateEvent,
    },
    ReactionAdd => Reaction,
    ReactionRemove => Reaction,
    ReactionRemoveAll => { channel_id: ChannelId, removed_from_message_id: MessageId },
    PresenceReplace => Vec<Presence>,
    PresenceUpdate => Presence,
    Ready => serenity::model::gateway::Ready,
    Resume => ResumedEvent,
    ShardStageUpdate => ShardStageUpdateEvent,
    TypingStart => TypingStartEvent,
    UserUpdate => { old_data: CurrentUser, new: CurrentUser, },
    VoiceServerUpdate => VoiceServerUpdateEvent,
    VoiceStateUpdate => { old: Option<VoiceState>, new: VoiceState, },
    WebhookUpdate => { guild_id: GuildId, belongs_to_channel_id: ChannelId },
    InteractionCreate => Interaction,
    IntegrationCreate => Integration,
    IntegrationUpdate => Integration,
    IntegrationDelete => {
        integration_id: IntegrationId,
        guild_id: GuildId,
        application_id: Option<ApplicationId>,
    },
    StageInstanceCreate => StageInstance,
    StageInstanceUpdate => StageInstance,
    StageInstanceDelete => StageInstance,
    ThreadCreate => GuildChannel,
    ThreadUpdate => GuildChannel,
    ThreadDelete => PartialGuildChannel,
    ThreadListSync => ThreadListSyncEvent,
    ThreadMemberUpdate => ThreadMember,
    ThreadMembersUpdate => ThreadMembersUpdateEvent,
    GuildScheduledEventCreate => ScheduledEvent,
    GuildScheduledEventUpdate => ScheduledEvent,
    GuildScheduledEventDelete => ScheduledEvent,
    GuildScheduledEventUserAdd => GuildScheduledEventUserAddEvent,
    GuildScheduledEventUserRemove => GuildScheduledEventUserRemoveEvent,
    Unknwon => { name: String, raw: Value },
}
