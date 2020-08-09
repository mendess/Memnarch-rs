use crate::consts::NUMBERS;
use crate::cron::{CronSink, Task};

use chrono::{DateTime, Duration, Utc};
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    http::client::Http,
    model::{channel::Message, id::UserId},
    prelude::*,
};
use std::{error::Error, sync::Arc};

#[group]
#[commands(ping, who_are_you, vote, remindme)]
struct General;

#[command]
#[description("Ping me maybe")]
fn ping(ctx: &mut Context, msg: &Message) -> CommandResult {
    use chrono::Local;
    let one_trip_time =
        (Local::now().timestamp_millis() - msg.timestamp.timestamp_millis()) as f32 / 1000_f32;
    if let Err(why) = msg.channel_id.say(
        &ctx.http,
        format!(
            "Pong! {} ms{}",
            one_trip_time * 2.0,
            if one_trip_time < 0.0 {
                "\n*yes it's negative, idk why either*"
            } else {
                ""
            }
        ),
    ) {
        println!("Error ponging: {:?}", why)
    }
    Ok(())
}

#[command("whoareyou")]
#[description("Find out more about me")]
fn who_are_you(ctx: &mut Context, msg: &Message) -> CommandResult {
    msg.channel_id.send_message(ctx, |m| {
        m.embed(|e| {
            e.title("I AM MEMNARCH")
                .description("Sauce code: [GitHub](https://github.com/Mendess2526/Memnarch-rs)")
                .image("https://img.scryfall.com/mci/scans/en/arc/112.jpg")
        })
    })?;
    Ok(())
}

#[command]
#[min_args(2)]
#[max_args(10)]
#[description("Create a voting of up to 10 things")]
#[usage("[OPTION, ...]")]
#[example("option1 \"option 2\"")]
fn vote(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let message = msg.channel_id.send_message(&ctx.http, |m| {
        m.embed(|e| {
            e.title("Vote:");
            let fs = args
                .raw_quoted()
                .enumerate()
                .map(|(i, a)| (a, NUMBERS[i], true));
            e.fields(fs)
        });
        m
    })?;
    args.restore();
    (0..args.iter::<String>().filter_map(Result::ok).count()).for_each(|n| {
        while let Err(_) = message.react(&ctx, NUMBERS[n]) {
            continue;
        }
    });
    Ok(())
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize, Deserialize)]
pub struct Reminder {
    message: String,
    when: DateTime<Utc>,
    id: UserId,
}

impl Task for Reminder {
    type Id = ();
    type GlobalData = Arc<Http>;

    fn when(&self) -> DateTime<Utc> {
        self.when
    }

    fn call(&self, http: Self::GlobalData) -> Result<(), Box<dyn Error>> {
        self.id
            .create_dm_channel(&http)
            .and_then(|private_channel| private_channel.say(&http, &self.message))
            .map(|_| ())
            .map_err(|e| Box::new(e) as Box<dyn Error>)
    }
}

#[command]
#[min_args(2)]
#[aliases("remindeme")]
#[description(
    "Set a reminder for later.
              The time parameters allowed are:
              - seconds (s|sec|secs|seconds)
              - minutes (m|min|mins|minutes)
              - hours (h|hours)
              - days (d|days)
              - weeks (w|weeks)
              - months (month|months)
              - years (year|years)
              "
)]
#[usage("delay message")]
#[example("3s Remind me in 3 seconds")]
#[example("4m Remind me in 4 minutes")]
fn remindme(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    lazy_static! {
        static ref SECONDS: Regex = Regex::new(r"\d+(s|sec|secs|seconds?|segundos?)$").unwrap();
        static ref MINUTES: Regex = Regex::new(r"\d+(m|min|mins|minutes?|minutos?)$").unwrap();
        static ref HOURS: Regex = Regex::new(r"\d+(h|hours?|horas?)$").unwrap();
        static ref DAYS: Regex = Regex::new(r"\d+(d|days?|dias?)$").unwrap();
        static ref WEEKS: Regex = Regex::new(r"\d+(w|weeks?|semanas?)$").unwrap();
        static ref MONTHS: Regex = Regex::new(r"\d+(months?|mes(es)?)$").unwrap();
        static ref YEARS: Regex = Regex::new(r"\d+(y|year|years)$").unwrap();
    };
    let timeout = {
        let time = args.raw().next().unwrap();
        let parse = |c: Captures| {
            c.get(0).unwrap().as_str();
            time[..c.get(1).unwrap().start()].parse::<u32>()
        };
        if let Some(m) = SECONDS.captures(time) {
            Ok(Duration::seconds(i64::from(parse(m)?)))
        } else if let Some(m) = MINUTES.captures(time) {
            Ok(Duration::minutes(i64::from(parse(m)?)))
        } else if let Some(m) = HOURS.captures(time) {
            Ok(Duration::hours(i64::from(parse(m)?)))
        } else if let Some(m) = DAYS.captures(time) {
            Ok(Duration::days(i64::from(parse(m)?)))
        } else if let Some(m) = WEEKS.captures(time) {
            Ok(Duration::weeks(i64::from(parse(m)?)))
        } else if let Some(m) = MONTHS.captures(time) {
            Ok(Duration::days(30 * i64::from(parse(m)?)))
        } else if let Some(m) = YEARS.captures(time) {
            Ok(Duration::days(365 * i64::from(parse(m)?)))
        } else {
            Err("Invalid time specifier")
        }
    }?;
    args.advance();
    let reminder = Reminder {
        message: format!("You asked me to remind you of this:\n{}", args.rest()),
        when: Utc::now() + timeout,
        id: msg.author.id,
    };
    ctx.data
        .read()
        .get::<CronSink<Reminder>>()
        .unwrap()
        .send(reminder)?;
    msg.channel_id.say(&ctx, "You shall be reminded!")?;
    Ok(())
}
