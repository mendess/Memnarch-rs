use futures::{FutureExt as _, StreamExt as _, stream};
use pubsub::events;
use serenity::all::{ChannelId, Context, GuildId};
use std::ops::ControlFlow;

pub async fn initialize() {
    pubsub::subscribe::<events::VoiceStateUpdate, _>(
        |ctx, events::VoiceStateUpdate { new, .. }| {
            async move {
                // Disconnect channel of mirrodin
                if let (Some(gid @ 352399774818762759), Some(id @ 707561909846802462)) = (
                    new.guild_id.map(|i| i.get()),
                    new.channel_id.map(|i| i.get()),
                ) {
                    async fn f(id: ChannelId, gid: GuildId, ctx: &Context) -> anyhow::Result<()> {
                        let c = id.to_channel(ctx).await.and_then(|c| {
                            c.guild()
                                .ok_or(serenity::Error::Other("Not a guild channel"))
                        })?;
                        stream::iter(c.members(ctx)?)
                            .for_each(|mut m| async move {
                                let name = std::mem::take(&mut m.user.name);
                                if let Err(e) = gid.disconnect_member(ctx, m).await {
                                    tracing::error!(
                                    "Failed to disconnect member {} from disconnect channel: {}",
                                    name,
                                    e
                                );
                                }
                            })
                            .await;
                        Ok(())
                    }
                    if let Err(e) = f(id.into(), gid.into(), ctx).await {
                        tracing::error!("Failed to disconnect user: {}", e);
                    }
                }
                ControlFlow::Continue(())
            }
            .boxed()
        },
    )
    .await;
}
