use crate::{
    daemons::DaemonManager,
    events::pubsub::{self, events::VoiceStateUpdate},
};
use daemons::ControlFlow;
use futures::prelude::*;
use serenity::{
    client::Context,
    model::id::{ChannelId, GuildId, UserId},
    prelude::TypeMapKey,
};
use songbird::Call;
use std::{collections::HashMap, error::Error, sync::Arc, sync::Once};
use tokio::sync::Mutex;
// use crate::util::Mutex;

pub async fn join_or_get_call(
    ctx: &Context,
    gid: GuildId,
    author: UserId,
) -> Result<Arc<Mutex<Call>>, Box<dyn Error + Send + Sync>> {
    let sb = songbird::get(ctx).await.expect("Songbird not initialized");

    let call = match sb.get(gid) {
        Some(call) => call,
        None => {
            let guild = ctx.cache.guild(gid).ok_or("Invalid guild")?;
            let voice_channel = guild
                .voice_states
                .get(&author)
                .and_then(|vs| vs.channel_id)
                .ok_or("Not in a voice channel")?;

            let (call, result) = sb.join(guild.id, voice_channel).await;

            result?;
            call
        }
    };

    Ok(call)
}

#[derive(Debug, Default)]
pub struct LeaveVoiceDaemons(HashMap<GuildId, usize>);

impl TypeMapKey for LeaveVoiceDaemons {
    type Value = Arc<Mutex<Self>>;
}

impl LeaveVoiceDaemons {
    pub async fn set(&mut self, daemons: &mut DaemonManager, guild_id: GuildId, index: usize) {
        init_voice_leave();
        if let Some(prev) = self.0.insert(guild_id, index) {
            let _ = daemons.cancel(prev).await;
        }
    }

    pub async fn remove(&mut self, daemons: &mut DaemonManager, guild_id: GuildId) {
        if let Some(prev) = self.0.remove(&guild_id) {
            let _ = daemons.cancel(prev).await;
        }
    }
}

fn init_voice_leave() {
    static INIT_VOICE_LEAVE: Once = Once::new();
    INIT_VOICE_LEAVE.call_once(|| {
        pubsub::register::<VoiceStateUpdate, _>(|ctx, (guild_id, old, _)| {
            async move {
                #[derive(PartialEq, Eq)]
                enum Alone {
                    Empty,
                    OnlyBots,
                    NotEmpty,
                }
                async fn alone(id: ChannelId, ctx: &Context) -> Option<Alone> {
                    let members = id
                        .to_channel(ctx)
                        .await
                        .ok()?
                        .guild()?
                        .members(ctx)
                        .await
                        .ok()?;
                    Some(if members.is_empty() {
                        Alone::Empty
                    } else if members.iter().all(|m| m.user.bot) {
                        Alone::OnlyBots
                    } else {
                        Alone::NotEmpty
                    })
                }
                if let Some(id) = old.as_ref().and_then(|vs| vs.channel_id) {
                    if alone(id, ctx).await == Some(Alone::OnlyBots) {
                        if let Some(guild_id) = *guild_id {
                            let sb = songbird::get(ctx).await.expect("Songbird not initialized");
                            log::debug!("Leaving voice channel: {}", guild_id);
                            if let Err(e) = sb.remove(guild_id).await {
                                log::error!("Could not leave voice channel: {}", e);
                            } else {
                                let data = ctx.data.read().await;
                                let mut dm = crate::get!(> data, DaemonManager, lock);
                                crate::get!(> data, LeaveVoiceDaemons, lock)
                                    .remove(&mut dm, guild_id)
                                    .await;
                            }
                        };
                    }
                }
                ControlFlow::CONTINUE
            }
            .boxed()
        });
    });
}
