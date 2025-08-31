use crate::util::daemons::{DaemonManager, DaemonManagerKey};
use anyhow::Context as _;
use daemons::ControlFlow;
use futures::prelude::*;
use pubsub::{self, events::VoiceStateUpdate};
use serenity::{
    all::Context,
    model::id::{ChannelId, GuildId, UserId},
    prelude::TypeMapKey,
};
use songbird::Call;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, OnceCell};
// use crate::util::Mutex;

pub async fn join_or_get_call(
    ctx: super::super::Context<'_>,
    gid: GuildId,
    author: UserId,
) -> anyhow::Result<Arc<Mutex<Call>>> {
    let sb = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird not initialized");

    let call = match sb.get(gid) {
        Some(call) => call,
        None => {
            let (gid, voice_channel) = {
                let guild = ctx.cache().guild(gid).context("Invalid guild")?;
                let voice_channel = guild
                    .voice_states
                    .get(&author)
                    .and_then(|vs| vs.channel_id)
                    .context("Not in a voice channel")?;
                (guild.id, voice_channel)
            };

            sb.join(gid, voice_channel).await?
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
        init_voice_leave().await;
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

async fn init_voice_leave() {
    static INIT_VOICE_LEAVE: OnceCell<()> = OnceCell::const_new();
    INIT_VOICE_LEAVE
        .get_or_init(|| async {
            pubsub::subscribe::<VoiceStateUpdate, _>(|ctx, VoiceStateUpdate { old, new }| {
                async move {
                    #[derive(PartialEq, Eq)]
                    enum Alone {
                        Empty,
                        OnlyBots,
                        NotEmpty,
                    }
                    async fn alone(id: ChannelId, ctx: &Context) -> Option<Alone> {
                        let members = id.to_channel(ctx).await.ok()?.guild()?.members(ctx).ok()?;
                        Some(if members.is_empty() {
                            Alone::Empty
                        } else if members.iter().all(|m| m.user.bot) {
                            Alone::OnlyBots
                        } else {
                            Alone::NotEmpty
                        })
                    }
                    if let Some(id) = old.as_ref().and_then(|vs| vs.channel_id)
                        && alone(id, ctx).await == Some(Alone::OnlyBots)
                        && let Some(guild_id) = new.guild_id
                    {
                        let sb = songbird::get(ctx).await.expect("Songbird not initialized");
                        tracing::debug!("Leaving voice channel: {}", guild_id);
                        if let Err(e) = sb.remove(guild_id).await {
                            tracing::error!("Could not leave voice channel: {}", e);
                        } else {
                            let data = ctx.data.read().await;
                            let mut dm = crate::get!(> data, DaemonManagerKey, lock);
                            crate::get!(> data, LeaveVoiceDaemons, lock)
                                .remove(&mut dm, guild_id)
                                .await;
                        }
                    };
                    ControlFlow::CONTINUE
                }
                .boxed()
            })
            .await;
        })
        .await;
}

pub async fn paginate<U, E>(
    ctx: poise::Context<'_, U, E>,
    pages: &[serenity::builder::CreateEmbed],
) -> Result<(), serenity::Error> {
    // Define some unique identifiers for the navigation buttons
    let ctx_id = ctx.id();
    let prev_button_id = format!("{}prev", ctx_id);
    let next_button_id = format!("{}next", ctx_id);

    // Send the embed with the first page as content
    let reply = {
        let components = serenity::builder::CreateActionRow::Buttons(vec![
            serenity::builder::CreateButton::new(&prev_button_id).emoji('◀'),
            serenity::builder::CreateButton::new(&next_button_id).emoji('▶'),
        ]);

        poise::CreateReply::default()
            .embed(pages[0].clone())
            .components(vec![components])
    };

    ctx.send(reply).await?;

    // Loop through incoming interactions with the navigation buttons
    let mut current_page = 0;
    while let Some(press) = serenity::collector::ComponentInteractionCollector::new(ctx)
        // We defined our button IDs to start with `ctx_id`. If they don't, some other command's
        // button was pressed
        .filter(move |press| press.data.custom_id.starts_with(&ctx_id.to_string()))
        // Timeout when no navigation button has been pressed for 24 hours
        .timeout(std::time::Duration::from_secs(3600 * 24))
        .await
    {
        // Depending on which button was pressed, go to next or previous page
        if press.data.custom_id == next_button_id {
            current_page += 1;
            if current_page >= pages.len() {
                current_page = 0;
            }
        } else if press.data.custom_id == prev_button_id {
            current_page = current_page.checked_sub(1).unwrap_or(pages.len() - 1);
        } else {
            // This is an unrelated button interaction
            continue;
        }

        // Update the message with the new page contents
        press
            .create_response(
                ctx.serenity_context(),
                serenity::builder::CreateInteractionResponse::UpdateMessage(
                    serenity::builder::CreateInteractionResponseMessage::new()
                        .embed(pages[current_page].clone()),
                ),
            )
            .await?;
    }

    Ok(())
}
