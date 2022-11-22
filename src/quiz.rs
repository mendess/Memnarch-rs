use crate::{
    cron::Cron,
    daemons::DaemonManager,
    events::pubsub::{self, events::ReactionAdd},
    file_transaction::Database,
};
use daemons::ControlFlow;
use futures::FutureExt;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serenity::{
    client::Context,
    http::{CacheHttp, Http},
    model::{
        channel::{Reaction, ReactionType},
        id::{ChannelId, GuildId, RoleId, UserId},
        Permissions,
    },
    prelude::Mentionable,
};
use std::collections::HashMap;
use tokio::io;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
struct Quizers {
    guild: GuildId,
    channel: ChannelId,
    role: RoleId,
}

lazy_static! {
    static ref DATABASE: Database<HashMap<GuildId, Quizers>> = Database::new("files/quizers.json");
}

type QuizDaemon<F, Fut> = Cron<F, Fut, 21, 50, 00>;

pub async fn add_quizer<C: CacheHttp>(
    ctx: C,
    guild: GuildId,
    user_id: UserId,
) -> anyhow::Result<()> {
    if let Some(q) = DATABASE.load().await?.get_mut(&guild) {
        let mut member = guild.member(&ctx, user_id).await?;
        member.add_role(ctx.http(), q.role).await?;
    }
    Ok(())
}

pub async fn rm_quizer<C: CacheHttp>(
    ctx: C,
    guild: GuildId,
    user_id: UserId,
) -> anyhow::Result<()> {
    if let Some(q) = DATABASE.load().await?.get_mut(&guild) {
        let mut member = guild.member(&ctx, user_id).await?;
        member.remove_role(ctx.http(), q.role).await?;
    }
    Ok(())
}

pub async fn add_quiz_guild(
    ctx: &Context,
    guild: GuildId,
    channel: ChannelId,
) -> anyhow::Result<()> {
    let mut quizers = DATABASE.load().await?;
    match quizers.get_mut(&guild) {
        Some(q) => q.channel = channel,
        None => {
            let role = guild
                .create_role(ctx, |e| {
                    e.name("Quizzer")
                        .mentionable(true)
                        .permissions(Permissions::empty())
                })
                .await?;
            let quisers = Quizers {
                guild,
                channel,
                role: role.id,
            };
            quizers.insert(guild, quisers);
            let data = ctx.data.read().await;
            let mut daemon_mgr_lock = crate::get!(> data, DaemonManager, lock);
            add_quiz_daemon(quisers, &mut daemon_mgr_lock).await;
        }
    }
    Ok(())
}

pub async fn remove_quiz_guild(http: impl AsRef<Http>, guild: GuildId) -> anyhow::Result<()> {
    if let Some(q) = DATABASE.load().await?.remove(&guild) {
        guild.delete_role(http, q.role).await?;
    }
    Ok(())
}

async fn add_quiz_daemon(quizers: Quizers, daemon: &mut DaemonManager) {
    daemon
        .add_daemon(QuizDaemon::new(
            format!("quiz for channel {}", quizers.guild),
            move |data| {
                let msg = format!(
                    "Quiz bros: {}.\nReact with ✅ to become a quizer. React with ❌ to unbecome a quizer.",
                    quizers.role.mention()
                );
                let channel = quizers.channel;
                let guild = quizers.guild;
                let http = data.http.clone();
                async move {
                    if let Err(e) = channel.send_message(&http, |m| m.content(msg)).await
                    {
                        log::error!(
                            "Couldn't send quiz alert to channel {} in guild {}: {}",
                            channel,
                            guild,
                            e
                        );
                    }
                    ControlFlow::CONTINUE
                }
            },
        ))
        .await;
}

pub async fn initialize(daemon: &mut DaemonManager) -> io::Result<()> {
    for quizers in DATABASE.load().await?.take().into_values() {
        add_quiz_daemon(quizers, daemon).await;
    }

    async fn handle_reaction(c: &Context, reaction: &Reaction) {
        let m = match reaction.message(c).await {
            Ok(m) => m,
            Err(e) => {
                log::error!("Couldn't get message reacted to: {}", e);
                return;
            }
        };
        if m.content.starts_with("Quiz bros:") {
            let user = reaction.user_id.expect("Cache to be available");
            let guild = match reaction.guild_id {
                Some(g) => g,
                None => return,
            };
            let r = match &reaction.emoji {
                ReactionType::Unicode(e) => match e.as_str() {
                    "✅" => add_quizer(c, guild, user).await,
                    "❌" => rm_quizer(c, guild, user).await,
                    _ => return,
                },
                _ => return,
            };
            if let Err(e) = r {
                log::error!(
                    "Couldn't add user {} as a quizer in guild {} channel {}: {}",
                    user,
                    guild,
                    reaction.channel_id,
                    e
                );
            }
        }
    }
    pubsub::register::<ReactionAdd, _>(|c, a| {
        async move {
            handle_reaction(c, a).await;
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
    Ok(())
}
