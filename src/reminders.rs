use crate::{daemons::DaemonManager, file_transaction::Database};
use chrono::{DateTime, Utc};
use daemons::Daemon;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serenity::model::id::UserId;
use std::{io, time::Duration as StdDuration};

lazy_static! {
    static ref DATABASE: Database<Vec<Reminder>> = Database::new("files/cron/reminders.json");
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

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        match self.id.create_dm_channel(data).await {
            Ok(pch) => {
                if let Err(e) = pch.say(&data.http, &self.message).await {
                    log::error!("Failed to send reminder: {:?}", e);
                } else if let Err(e) = remove_reminder(self).await {
                    log::error!("Failed to remove reminder: {:?}", e);
                }
                daemons::ControlFlow::Break
            }
            Err(e) => {
                log::error!("Failed to create dm channel: {:?}", e);
                daemons::ControlFlow::Continue
            }
        }
    }

    async fn interval(&self) -> StdDuration {
        (self.when - Utc::now()).to_std().unwrap_or_default()
    }

    async fn name(&self) -> String {
        format!("{:?}", self)
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

pub async fn load_reminders(daemons: &mut DaemonManager) -> io::Result<()> {
    let mut i = 0;
    for r in DATABASE.load().await?.take() {
        daemons.add_daemon(r).await;
        i += 1;
    }
    log::info!("Loaded {} reminders", i);
    Ok(())
}
