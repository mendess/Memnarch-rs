use crate::{daemons::DaemonManager, file_transaction::Database};
use chrono::{DateTime, Duration, Utc};
use daemons::ControlFlow;
use daemons::Daemon;
use lazy_static::lazy_static;
use regex::{Captures, Regex};
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

pub fn parse_duration(s: &str) -> anyhow::Result<(&str, Duration)> {
    lazy_static! {
        static ref NUMBER: Regex = Regex::new(r"\d+").unwrap();
        static ref SECONDS: Regex = Regex::new(r"^(s|sec|secs|seconds?|segundos?)( |$)").unwrap();
        static ref MINUTES: Regex = Regex::new(r"^(m|min|mins|minutes?|minutos?)( |$)").unwrap();
        static ref HOURS: Regex = Regex::new(r"^(h|hours?|horas?)( |$)").unwrap();
        static ref DAYS: Regex = Regex::new(r"^(d|days?|dias?)( |$)").unwrap();
        static ref WEEKS: Regex = Regex::new(r"^(w|weeks?|semanas?)( |$)").unwrap();
        static ref MONTHS: Regex = Regex::new(r"^(months?|mes(es)?)( |$)").unwrap();
        static ref YEARS: Regex = Regex::new(r"^(y|years?|anos?)( |$)").unwrap();
    };
    let (end, amt) = match NUMBER.captures(s).ok_or("missing number").and_then(|m| {
        let m = m.get(0).unwrap();
        Ok((m.end(), m.as_str().parse().map_err(|_| "Invalid number")?))
    }) {
        Ok(x) => x,
        Err(e) => return Err(anyhow::anyhow!(e)),
    };
    let args = &s[end..].trim();

    let end = |c: Captures<'_>| c.get(0).unwrap().end();
    let (end, dur) = if let Some(m) = SECONDS.captures(args) {
        (end(m), Duration::seconds(amt))
    } else if let Some(m) = MINUTES.captures(args) {
        (end(m), Duration::minutes(amt))
    } else if let Some(m) = HOURS.captures(args) {
        (end(m), Duration::hours(amt))
    } else if let Some(m) = DAYS.captures(args) {
        (end(m), Duration::days(amt))
    } else if let Some(m) = WEEKS.captures(args) {
        (end(m), Duration::weeks(amt))
    } else if let Some(m) = MONTHS.captures(args) {
        (end(m), Duration::days(30 * amt))
    } else if let Some(m) = YEARS.captures(args) {
        (end(m), Duration::days(365 * amt))
    } else {
        return Err(anyhow::anyhow!("Invalid time specifier"));
    };
    Ok((&args[end..].trim(), dur))
}

#[cfg(test)]
mod test {
    use super::parse_duration;
    use chrono::Duration;

    macro_rules! make_test {
        ($($time:ident => $ctor:ident$(* $mult:expr)?),* $(,)?) => {
            paste::paste! {$(
                #[test]
                fn [<$ctor _from_ $time _no_space>]() {
                    let x = concat!("2", stringify!($time), " ");
                    eprintln!("{:?}", x);
                    assert_eq!(
                        parse_duration(x).unwrap().1,
                        Duration::$ctor(2 $(* $mult)?)
                    );
                    assert_eq!(
                        parse_duration(concat!("2", stringify!($time), " cenas")).unwrap().1,
                        Duration::$ctor(2 $(* $mult)?)
                    );
                }

                #[test]
                fn [<$ctor _from_ $time _space>]() {
                    assert_eq!(
                        parse_duration(concat!("2 ", stringify!($time), " ")).unwrap().1,
                        Duration::$ctor(2 $(* $mult)?)
                    );
                    assert_eq!(
                        parse_duration(concat!("2 ", stringify!($time), " cenas")).unwrap().1,
                        Duration::$ctor(2 $(* $mult)?)
                    );
                }
            )*}
        }
    }

    make_test! {
        s => seconds,
        sec => seconds,
        secs => seconds,
        second => seconds,
        seconds => seconds,
        segundo => seconds,
        segundos => seconds,
        m => minutes,
        min => minutes,
        minute => minutes,
        minutes => minutes,
        minuto => minutes,
        minutos => minutes,
        h => hours,
        hour => hours,
        hours => hours,
        hora => hours,
        horas => hours,
        d => days,
        day => days,
        days => days,
        dia => days,
        dias => days,
        w => weeks,
        week => weeks,
        weeks => weeks,
        semana => weeks,
        semanas => weeks,
        month => days * 30,
        months => days * 30,
        mes => days * 30,
        meses => days * 30,
        year => days * 365,
        years => days * 365,
        ano => days * 365,
        anos => days * 365,
    }
}
