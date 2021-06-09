use crate::daemons::DaemonManager;
use serenity::{
    client::Context,
    model::id::{GuildId, UserId},
    prelude::TypeMapKey,
};
use songbird::Call;
use std::{collections::HashMap, error::Error, sync::Arc};
use tokio::sync::Mutex;

pub async fn join_or_get_call(
    ctx: &Context,
    gid: GuildId,
    author: UserId,
) -> Result<Arc<Mutex<Call>>, Box<dyn Error + Send + Sync>> {
    let sb = songbird::get(ctx).await.expect("Songbird not initialized");

    let call = match sb.get(gid) {
        Some(call) => call,
        None => {
            let guild = ctx.cache.guild(gid).await.ok_or("Invalid guild")?;
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
