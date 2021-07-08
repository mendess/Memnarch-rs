use crate::{
    consts::NUMBERS,
    daemons::DaemonManager,
    get, reminders,
    user_prefs::{self, UserPrefs},
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::{Message, ReactionType},
    prelude::*,
};

#[group]
#[commands(ping, who_are_you, vote, remindme, version)]
struct General;

#[command]
#[description("The bot's version")]
async fn version(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .say(ctx, git_describe::git_describe!())
        .await?;
    Ok(())
}

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
                    .description("Sauce code: [GitHub](https://github.com/Mendess2526/Memnarch-rs)")
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
#[example("4 m Remind me in 4 minutes")]
async fn remindme(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    use reminders::parser::*;
    let Reminder { text, when } =
        parse(args.rest()).map_err(|e| anyhow::anyhow!("Invalid time spec: {}", e))?;
    let now = msg.timestamp;
    let when = match when {
        TimeSpec::Duration(dur) => now + dur,
        TimeSpec::Date((date, time)) => {
            let date = NaiveDate::from_ymd(
                date.year.unwrap_or(now.year()),
                date.month.unwrap_or(now.month()),
                date.day,
            );
            let date = NaiveDateTime::new(date, time);
            let offset = get_user_timezone(ctx, msg).await?;
            DateTime::from_utc(date - Duration::hours(offset), Utc)
        }
        TimeSpec::Time(time) => {
            let offset = get_user_timezone(ctx, msg).await?;
            now.date()
                .and_time(time)
                .ok_or_else(|| anyhow::anyhow!("Invalid time"))?
                - Duration::hours(offset)
        }
    };
    let data = ctx.data.read().await;
    let mut dm = get!(> data, DaemonManager, lock);
    reminders::remind(&mut *dm, text.into(), when, msg.author.id).await?;
    msg.channel_id.say(&ctx, "You shall be reminded!").await?;
    Ok(())
}

async fn get_user_timezone(ctx: &Context, msg: &Message) -> anyhow::Result<i64> {
    if let Some(UserPrefs {
        timezone_offset: Some(off),
    }) = user_prefs::get(msg.author.id).await?
    {
        return Ok(off);
    }
    let m = msg
        .channel_id
        .say(
            ctx,
            "I don't know what time it is over there! Reply to this with the hour it is over there",
        )
        .await?;
    let now = m.timestamp;
    let answer = msg
        .author
        .await_reply(&ctx)
        .await
        .ok_or_else(|| anyhow::anyhow!("no reply given"))?
        .content
        .trim()
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("Invalid hour"))?;
    let offset = {
        let offset = NaiveDateTime::new(
            NaiveDate::from_ymd(now.year(), now.month(), now.day()),
            NaiveTime::from_hms(answer, now.minute(), now.second()),
        ) - now.naive_utc();
        log::debug!("timestamp: {} user: {}", now, answer);
        offset.num_hours()
    };
    user_prefs::update(msg.author.id, |p| p.timezone_offset = Some(offset)).await?;
    Ok(offset)
}
