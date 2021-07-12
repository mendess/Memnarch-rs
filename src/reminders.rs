pub mod parser;

use crate::{daemons::DaemonManager, events::pubsub, file_transaction::Database, util::bot_id};
use chrono::{DateTime, Utc};
use daemons::{ControlFlow, Daemon};
use futures::FutureExt;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serenity::{
    client::Context,
    model::{channel::Channel, id::UserId, misc::Mentionable},
};
use std::{
    collections::{HashMap, HashSet},
    io,
    time::Duration as StdDuration,
};

pub const BLOCK_EMOJI: &str = "🛡️";

lazy_static! {
    static ref DATABASE: Database<Vec<Reminder>> = Database::new("files/cron/reminders.json");
    static ref BLOCKED_USERS: Database<HashMap<UserId, HashSet<UserId>>> =
        Database::new("files/blocked_user.json");
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize, Deserialize)]
pub struct Reminder {
    message: String,
    when: DateTime<Utc>,
    id: UserId,
}

#[serenity::async_trait]
impl Daemon for Reminder {
    type Data = serenity::CacheAndHttp;

    async fn run(&mut self, data: &Self::Data) -> ControlFlow {
        match self.id.create_dm_channel(data).await {
            Ok(pch) => {
                if let Err(e) = pch.say(&data.http, &self.message).await {
                    log::error!("Failed to send reminder: {:?}", e);
                } else if let Err(e) = remove_reminder(self).await {
                    log::error!("Failed to remove reminder: {:?}", e);
                }
                ControlFlow::BREAK
            }
            Err(e) => {
                log::error!("Failed to create dm channel: {:?}", e);
                ControlFlow::CONTINUE
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
    log::info!("Loaded {} reminders", i);
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
            Some(Channel::Private(ch)) => ch.recipient.id,
            None => match ctx.http.get_channel(m.channel_id.0).await? {
                Channel::Private(ch) => ch.recipient.id,
                ch => {
                    log::trace!("Not a private channel {:?}", ch);
                    return Ok(None);
                }
            },
            Some(ch) => {
                log::trace!("Not a private channel {:?}", ch);
                return Ok(None);
            }
        };
        let blocked = match m.mentions.first() {
            Some(u) => u.id,
            None => {
                log::trace!("There were no mentions in the message");
                return Ok(None);
            }
        };
        Ok(change(
            BLOCKED_USERS.load().await?.entry(blocker).or_default(),
            blocked,
        ))
    }
    pubsub::register::<ReactionAdd, _>(|ctx, arg| {
        async move {
            match intervenients(ctx, arg, |set, blocked| {
                set.insert(blocked).then(|| blocked)
            })
            .await
            {
                Ok(Some(u)) => {
                    let blocker = arg.user_id.expect("cache");
                    log::info!("User {} blocked {}", blocker, u);
                    if let Err(e) = arg
                        .channel_id
                        .say(ctx, format!("blocked: {}", u.mention()))
                        .await
                    {
                        log::error!("failed to inform user: {}", e);
                    }
                }
                Ok(None) => (),
                Err(e) => log::error!("{:?}", e),
            }
            ControlFlow::CONTINUE
        }
        .boxed()
    });
    pubsub::register::<ReactionRemove, _>(|ctx, arg| {
        async move {
            match intervenients(ctx, arg, |set, blocked| {
                set.remove(&blocked).then(|| blocked)
            })
            .await
            {
                Ok(Some(u)) => {
                    let blocker = arg.user_id.expect("cache");
                    log::info!("User {} unblocked {}", blocker, u);
                    if let Err(e) = arg
                        .channel_id
                        .say(ctx, format!("unblocked: {}", u.mention()))
                        .await
                    {
                        log::error!("failed to inform user: {}", e);
                    }
                }
                Ok(None) => (),
                Err(e) => log::error!("{:?}", e),
            }
            ControlFlow::CONTINUE
        }
        .boxed()
    });
    Ok(())
}
