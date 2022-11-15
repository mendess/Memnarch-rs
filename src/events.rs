pub mod pubsub;

use crate::util;
use futures::stream::{self, StreamExt};
use pubsub::events;
use serenity::{
    model::{
        channel::Reaction,
        gateway::Ready,
        guild::Guild,
        id::{ChannelId, GuildId, MessageId},
        voice::VoiceState,
    },
    prelude::*,
};
use std::mem::take;

pub struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn voice_state_update(
        &self,
        ctx: Context,
        old: Option<VoiceState>,
        new: VoiceState,
    ) {
        // Disconnect channel of mirrodin
        if let (Some(gid @ GuildId(352399774818762759)), Some(id @ ChannelId(707561909846802462))) =
            (new.guild_id, new.channel_id)
        {
            async fn f(id: ChannelId, gid: GuildId, ctx: &Context) -> anyhow::Result<()> {
                let c = id.to_channel(ctx).await.and_then(|c| {
                    c.guild()
                        .ok_or(serenity::Error::Other("Not a guild channel"))
                })?;
                let members = c.members(ctx).await?;
                stream::iter(members)
                    .for_each(|mut m| async move {
                        let name = take(&mut m.user.name);
                        if let Err(e) = gid.disconnect_member(ctx, m).await {
                            log::error!(
                                "Failed to disconnect member {} from disconnect channel: {}",
                                name,
                                e
                            );
                        }
                    })
                    .await;
                Ok(())
            }
            if let Err(e) = f(id, gid, &ctx).await {
                log::error!("Failed to disconnect user: {}", e);
            }
        }
        pubsub::emit::<events::VoiceStateUpdate>(ctx, (new.guild_id, old, new)).await;
    }

    async fn reaction_add(&self, ctx: Context, add_reaction: Reaction) {
        if add_reaction.user_id != util::bot_id(&ctx).await {
            pubsub::emit::<events::ReactionAdd>(ctx, add_reaction).await;
        }
    }

    async fn reaction_remove(&self, ctx: Context, remove_reaction: Reaction) {
        if remove_reaction.user_id != util::bot_id(&ctx).await {
            pubsub::emit::<events::ReactionRemove>(ctx, remove_reaction).await;
        }
    }

    async fn reaction_remove_all(&self, ctx: Context, channel_id: ChannelId, msg: MessageId) {
        pubsub::emit::<events::ReactionRemoveAll>(ctx, (channel_id, msg)).await;
    }

    async fn cache_ready(&self, ctx: Context, guilds: Vec<GuildId>) {
        pubsub::emit::<events::CacheReady>(ctx, guilds).await;
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        pubsub::emit::<events::Ready>(ctx, ready).await;
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: bool) {
        log::info!("found guild {}::{}", guild.name, guild.id);
        pubsub::emit::<events::GuildCreate>(ctx, (guild, is_new)).await;
    }
}

pub struct UpdateNotify;

impl TypeMapKey for UpdateNotify {
    type Value = ChannelId;
}
