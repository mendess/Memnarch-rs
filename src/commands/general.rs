use crate::consts::NUMBERS;
use crate::cron::{CronSink, Task};

use chrono::{DateTime, Duration, Utc};
use lazy_static::lazy_static;
use regex::{Match, Regex};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    http::raw::Http,
    model::{channel::Message, id::UserId},
    prelude::*,
};

use std::sync::Arc;

group!({
    name: "General",
    options: {},
    commands: [ping, who_are_you, vote, remindme],
});

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
#[max_args(9)]
#[description("Create a voting of up to 9 things")]
fn vote(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let message = msg.channel_id.send_message(&ctx.http, |m| {
        m.embed(|e| {
            e.title("Vote:");
            let fs = args
                .iter::<String>()
                .filter_map(Result::ok)
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

struct Reminder {
    message: String,
    when: DateTime<Utc>,
    id: UserId,
    http: Arc<Http>,
}

impl Task for Reminder {
    fn when(&self) -> DateTime<Utc> {
        self.when
    }

    fn call(&self) {
        // TODO: Make this pretty
        match self.id.create_dm_channel(&self.http) {
            Ok(private_channel) => {
                if let Err(e) = private_channel.say(&self.http, &self.message) {
                    eprintln!("{}", e)
                }
            }
            Err(e) => eprintln!("{}", e),
        }
    }
}

#[command]
#[min_args(2)]
fn remindme(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    lazy_static! {
        static ref SECONDS: Regex = Regex::new("(s|secs|seconds)$").unwrap();
        static ref MINUTES: Regex = Regex::new("(m|mins|minutes)$").unwrap();
        static ref HOURS: Regex = Regex::new("(h|hours)$").unwrap();
        static ref DAYS: Regex = Regex::new("(d|days)$").unwrap();
        static ref WEEKS: Regex = Regex::new("(w|weeks)$").unwrap();
    };
    let timeout = {
        let time = args.raw().next().unwrap();
        let parse = |m: Match| time[..m.start()].parse::<u32>();
        if let Some(m) = SECONDS.find(time) {
            Ok(Duration::seconds(i64::from(parse(m)?)))
        } else if let Some(m) = MINUTES.find(time) {
            Ok(Duration::minutes(i64::from(parse(m)?)))
        } else if let Some(m) = HOURS.find(time) {
            Ok(Duration::hours(i64::from(parse(m)?)))
        } else if let Some(m) = DAYS.find(time) {
            Ok(Duration::days(i64::from(parse(m)?)))
        } else if let Some(m) = WEEKS.find(time) {
            Ok(Duration::weeks(i64::from(parse(m)?)))
        } else {
            Err("Invalid time specifier")
        }
    }?;
    let reminder = Reminder {
        message: String::from(args.rest()),
        when: Utc::now() + timeout,
        id: msg.author.id,
        http: Arc::clone(&ctx.http),
    };
    let map = ctx.data.read();
    let cron = map.get::<CronSink>().unwrap();
    cron.send(Box::new(reminder))?;
    msg.channel_id.say(&ctx, "You shall be reminded!")?;
    Ok(())
}
