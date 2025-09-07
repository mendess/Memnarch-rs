pub mod parser;

use crate::{
    in_files,
    util::{
        bot_id,
        daemons::{DaemonManager, cache_and_http},
    },
};
use chrono::{DateTime, Utc};
use daemons::Daemon;
use futures::FutureExt;
use json_db::GlobalDatabase;
use serde::{Deserialize, Serialize};
use serenity::{
    all::Http,
    client::Context,
    model::{channel::Channel, id::UserId},
    prelude::Mentionable,
};
use std::{
    collections::{HashMap, HashSet},
    io,
    ops::ControlFlow,
    sync::Arc,
    time::Duration as StdDuration,
};

pub const BLOCK_EMOJI: &str = "🛡️";

static DATABASE: GlobalDatabase<Vec<Reminder>> =
    GlobalDatabase::new(in_files!("cron/reminders.json"));
static BLOCKED_USERS: GlobalDatabase<HashMap<UserId, HashSet<UserId>>> =
    GlobalDatabase::new(in_files!("blocked_user.json"));

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize, Deserialize)]
pub struct Reminder {
    message: String,
    when: DateTime<Utc>,
    id: UserId,
}

#[serenity::async_trait]
impl Daemon<false> for Reminder {
    type Data = (Arc<serenity::cache::Cache>, Arc<Http>);

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        let data = cache_and_http(data);
        match self.id.create_dm_channel(data).await {
            Ok(pch) => {
                if let Err(e) = pch.say(&data, &self.message).await {
                    tracing::error!("Failed to send reminder: {:?}", e);
                } else if let Err(e) = remove_reminder(self).await {
                    tracing::error!("Failed to remove reminder: {:?}", e);
                }
                ControlFlow::Break(())
            }
            Err(e) => {
                tracing::error!("Failed to create dm channel: {:?}", e);
                ControlFlow::Continue(())
            }
        }
    }

    async fn interval(&self) -> StdDuration {
        (self.when - Utc::now()).to_std().unwrap_or_default()
    }

    async fn name(&self) -> String {
        format!("Remind {} on {}", self.id, self.when)
    }
}

pub async fn is_blocked_by(from: UserId, to: UserId) -> io::Result<bool> {
    if let Some(blocked) = BLOCKED_USERS.load().await?.get(&to) {
        Ok(blocked.contains(&from))
    } else {
        Ok(false)
    }
}

async fn remove_reminder(reminder: &Reminder) -> io::Result<()> {
    let mut reminders = DATABASE.load().await?;
    reminders.retain(|r| r != reminder);
    Ok(())
}

pub async fn remind(
    daemons: &mut DaemonManager,
    message: String,
    when: DateTime<Utc>,
    id: UserId,
) -> io::Result<()> {
    let reminder = Reminder { message, when, id };
    let mut reminders = DATABASE.load().await?;
    reminders.push(reminder.clone());
    daemons.add_daemon(reminder).await;
    Ok(())
}

pub async fn reminders(u: UserId) -> io::Result<impl Iterator<Item = (String, DateTime<Utc>)>> {
    Ok(DATABASE
        .load()
        .await?
        .take()
        .into_iter()
        .filter(move |r| r.id == u)
        .map(|r| (r.message, r.when)))
}

pub async fn load_reminders(daemons: &mut DaemonManager) -> io::Result<()> {
    use pubsub::events::{ReactionAdd, ReactionRemove};
    use serenity::model::channel::Reaction;

    let mut i = 0usize;
    for r in DATABASE.load().await?.take() {
        daemons.add_daemon(r).await;
        i += 1;
    }
    tracing::info!("Loaded {} reminders", i);
    async fn intervenients<F, R>(
        ctx: &Context,
        arg: &Reaction,
        change: F,
    ) -> anyhow::Result<Option<R>>
    where
        F: FnOnce(&mut HashSet<UserId>, UserId) -> Option<R>,
    {
        if !arg.emoji.unicode_eq(BLOCK_EMOJI) {
            return Ok(None);
        }
        let m = arg.message(&ctx.http).await?;
        if !m.content.contains(BLOCK_EMOJI) {
            return Ok(None);
        }
        if Some(m.author.id) != bot_id(ctx).await {
            return Ok(None);
        }
        let blocker = match m.channel(ctx).await {
            Ok(Channel::Private(ch)) => ch.recipient.id,
            Err(_) => match ctx.http.get_channel(m.channel_id).await? {
                Channel::Private(ch) => ch.recipient.id,
                ch => {
                    tracing::trace!("Not a private channel {:?}", ch);
                    return Ok(None);
                }
            },
            Ok(ch) => {
                tracing::trace!("Not a private channel {:?}", ch);
                return Ok(None);
            }
        };
        let blocked = match m.mentions.first() {
            Some(u) => u.id,
            None => {
                tracing::trace!("There were no mentions in the message");
                return Ok(None);
            }
        };
        Ok(change(
            BLOCKED_USERS.load().await?.entry(blocker).or_default(),
            blocked,
        ))
    }
    pubsub::subscribe::<ReactionAdd, _>(|ctx, arg| {
        async move {
            match intervenients(ctx, arg, |set, blocked| {
                set.insert(blocked).then_some(blocked)
            })
            .await
            {
                Ok(Some(u)) => {
                    let blocker = arg.user_id.expect("cache");
                    tracing::info!("User {} blocked {}", blocker, u);
                    if let Err(e) = arg
                        .channel_id
                        .say(ctx, format!("blocked: {}", u.mention()))
                        .await
                    {
                        tracing::error!("failed to inform user: {}", e);
                    }
                }
                Ok(None) => (),
                Err(e) => tracing::error!("{:?}", e),
            }
            ControlFlow::Continue(())
        }
        .boxed()
    })
    .await;
    pubsub::subscribe::<ReactionRemove, _>(|ctx, arg| {
        async move {
            match intervenients(ctx, arg, |set, blocked| {
                set.remove(&blocked).then_some(blocked)
            })
            .await
            {
                Ok(Some(u)) => {
                    let blocker = arg.user_id.expect("cache");
                    tracing::info!("User {} unblocked {}", blocker, u);
                    if let Err(e) = arg
                        .channel_id
                        .say(ctx, format!("unblocked: {}", u.mention()))
                        .await
                    {
                        tracing::error!("failed to inform user: {}", e);
                    }
                }
                Ok(None) => (),
                Err(e) => tracing::error!("{:?}", e),
            }
            ControlFlow::Continue(())
        }
        .boxed()
    })
    .await;
    Ok(())
}
