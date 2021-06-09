use crate::{consts::NUMBERS, daemons::DaemonManager, get, reminders};
use chrono::{Duration, Utc};
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::{Message, ReactionType},
    prelude::*,
};

#[group]
#[commands(ping, who_are_you, vote, remindme)]
struct General;

#[command]
#[description("Ping me maybe")]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    use chrono::Local;
    let one_trip_time =
        (Local::now().timestamp_millis() - msg.timestamp.timestamp_millis()) as f32 / 1000_f32;
    if let Err(why) = msg
        .channel_id
        .say(
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
        )
        .await
    {
        println!("Error ponging: {:?}", why)
    }
    Ok(())
}

#[command("whoareyou")]
#[description("Find out more about me")]
async fn who_are_you(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .send_message(ctx, |m| {
            m.embed(|e| {
                e.title("I AM MEMNARCH")
                    .description(
                        "Sauce code: [GitHub](https://github.com/Mendess2526/Memnarch-rs)",
                    )
                    .image("https://img.scryfall.com/mci/scans/en/arc/112.jpg")
            })
        })
        .await?;
    Ok(())
}

#[command]
#[min_args(2)]
#[max_args(10)]
#[description("Create a voting of up to 10 things")]
#[usage("[OPTION, ...]")]
#[example("option1 \"option 2\"")]
async fn vote(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let message = msg
        .channel_id
        .send_message(&ctx.http, |m| {
            m.embed(|e| {
                e.title("Vote:");
                let fs = args
                    .raw_quoted()
                    .enumerate()
                    .map(|(i, a)| (a, NUMBERS[i], true));
                e.fields(fs)
            });
            m
        })
        .await?;
    args.restore();
    for n in 0..args.iter::<String>().filter_map(Result::ok).count() {
        while let Err(_) = message
            .react(&ctx, NUMBERS[n].parse::<ReactionType>().unwrap())
            .await
        {
            continue;
        }
    }
    Ok(())
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
async fn remindme(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
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
    let data = ctx.data.read().await;
    let mut dm = get!(> data, DaemonManager, lock);
    reminders::remind(
        &mut *dm,
        format!("You asked me to remind you of this:\n{}", args.rest()),
        Utc::now() + timeout,
        msg.author.id,
    )
    .await?;
    msg.channel_id.say(&ctx, "You shall be reminded!").await?;
    Ok(())
}
