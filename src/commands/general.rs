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
    msg.channel_id
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
        .await?;
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
    for number in NUMBERS
        .iter()
        .take(args.iter::<String>().filter_map(Result::ok).count())
    {
        while message
            .react(&ctx, number.parse::<ReactionType>().unwrap())
            .await
            .is_err()
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
              - seconds (s|sec|secs|second|seconds|segundo|segundos)
              - minutes (m|min|mins|minute|minutes|minuto|minutos)
              - hours (h|hour|hours|hora|horas)
              - days (d|day|days|dia|dias)
              - weeks (w|week|weeks|semana|semanas)
              - months (month|months|mes|meses)
              - years (y|year|years|ano|anos)"
)]
#[usage("delay message")]
#[example("3s Remind me in 3 seconds")]
#[example("4m Remind me in 4 minutes")]
async fn remindme(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    lazy_static! {
        static ref NUMBER: Regex = Regex::new(r"\d+").unwrap();
        static ref SECONDS: Regex = Regex::new(r"^(s|sec|secs|seconds?|segundos?)").unwrap();
        static ref MINUTES: Regex = Regex::new(r"^(m|min|mins|minutes?|minutos?)").unwrap();
        static ref HOURS: Regex = Regex::new(r"^(h|hours?|horas?)").unwrap();
        static ref DAYS: Regex = Regex::new(r"^(d|days?|dias?)").unwrap();
        static ref WEEKS: Regex = Regex::new(r"^(w|weeks?|semanas?)").unwrap();
        static ref MONTHS: Regex = Regex::new(r"^(months?|mes(es)?)").unwrap();
        static ref YEARS: Regex = Regex::new(r"^(y|years?|anos?)").unwrap();
    };
    let (value, dur) = {
        let args = args.rest().trim();
        let (end, amt) = match NUMBER.captures(args).ok_or("missing number").and_then(|m| {
            let m = m.get(0).unwrap();
            Ok((m.end(), m.as_str().parse().map_err(|_| "Invalid number")?))
        }) {
            Ok(x) => x,
            Err(e) => return Err(e.into()),
        };
        let args = &args[end..].trim();

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
            return Err("Invalid time specifier".into());
        };

        (&args[end..].trim(), dur)
    };
    let data = ctx.data.read().await;
    let mut dm = get!(> data, DaemonManager, lock);
    reminders::remind(
        &mut *dm,
        format!("You asked me to remind you of this:\n{}", value),
        Utc::now() + dur,
        msg.author.id,
    )
    .await?;
    msg.channel_id.say(&ctx, "You shall be reminded!").await?;
    Ok(())
}
