use crate::{
    consts::NUMBERS,
    daemons::DaemonManager,
    get,
    prefs::user::{self as user_prefs, UserPrefs},
    reminders::{self, parser::*},
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Utc};
use futures::{StreamExt, TryStreamExt};
use itertools::Itertools;
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::{
        channel::{Message, ReactionType},
        guild::Member,
        id::{ChannelId, UserId},
    },
    prelude::*,
};
use std::iter::from_fn;

#[group]
#[commands(ping, who_are_you, vote, remindme, remind, version, reminders)]
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
async fn reminders(ctx: &Context, msg: &Message) -> CommandResult {
    use chrono::{Datelike, Timelike};

    let s = reminders::reminders(msg.author.id)
        .await?
        .format_with("\n", |(m, d), f| {
            f(&format_args!(
                "{:02}/{:02}/{:02} {:02}:{:02}:{:02} -> {}",
                d.day(),
                d.month(),
                d.year(),
                d.hour(),
                d.minute(),
                d.second(),
                m
            ))
        })
        .to_string();
    msg.channel_id
        .say(
            ctx,
            if s.is_empty() {
                "You don't have any reminders".to_string()
            } else {
                s
            },
        )
        .await?;
    Ok(())
}

#[command]
#[min_args(2)]
#[aliases("remindeme", "r")]
#[description("Set a reminder for later.
    Possible arguments are:
    - (day|dia) DD/MM/YYYY (at|as|às|@) HH:MM:SS [reminder]
    - (at|as|às|@) HH:MM:SS [reminder]
    - X[time parameter] [reminder]

    In the first 2 forms, any time parameter can be omited except for DD or HH.
    For example 'day 7 at 8 wake me up inside' is okay, this will remind you to wake up inside on the 7th of the current month and year at 8am.

    For the 3rd form the [time parameter] can be any of these:
    - seconds (s|sec|secs|second|seconds|segundo|segundos)
    - minutes (m|min|mins|minute|minutes|minuto|minutos)
    - hours (h|hour|hours|hora|horas)
    - days (d|day|days|dia|dias)
    - weeks (w|week|weeks|semana|semanas)
    - months (month|months|mes|meses)
    - years (y|year|years|ano|anos)"
)]
#[usage("delay message")]
#[example("day 18/7 at 8 party")]
#[example("at 9 dinner")]
#[example("3s 3 seconds have passed")]
#[example("4 m some time has passed")]
async fn remindme(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let Reminder { text, when } =
        parse(args.rest()).map_err(|e| anyhow::anyhow!("Invalid time spec: {}", e))?;
    let when = calculate_when(ctx, msg, when).await?;
    let data = ctx.data.read().await;
    let mut dm = get!(> data, DaemonManager, lock);
    reminders::remind(&mut *dm, text.into(), when, msg.author.id).await?;
    msg.channel_id.say(&ctx, "You shall be reminded!").await?;
    Ok(())
}

#[command]
async fn remind(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let user_ids = from_fn(|| args.single::<UserId>().ok()).collect::<Vec<_>>();
    if user_ids.is_empty() {
        return Err("Please mention at least one person".into());
    }
    let Reminder { text, when } =
        parse(args.rest()).map_err(|e| anyhow::anyhow!("Invalid time spec: {}", e))?;
    let when = calculate_when(ctx, msg, when).await?;
    let data = ctx.data.read().await;
    let mut dm = get!(> data, DaemonManager, lock);
    let mut got_one = false;
    for user_id in user_ids {
        if reminders::is_blocked_by(msg.author.id, user_id).await? {
            msg.channel_id
                .say(ctx, format!("{} blocked you", user_id.mention()))
                .await?;
            continue;
        }
        reminders::remind(
            &mut *dm,
            format!(
                r"{} asked me to remind you:
\~\~\~\~
{}
\~\~\~\~

*React with {} to block this person from reminding you. Unreact to unblock*",
                msg.author.mention(),
                text,
                reminders::BLOCK_EMOJI,
            ),
            when,
            user_id,
        )
        .await?;
        got_one = true;
    }
    if got_one {
        msg.channel_id
            .say(ctx, "It shall be ~~done~~ spammed")
            .await?;
    }
    Ok(())
}

async fn calculate_when(
    ctx: &Context,
    msg: &Message,
    when: TimeSpec,
) -> anyhow::Result<DateTime<Utc>> {
    let now = msg.timestamp;
    let when = match when {
        TimeSpec::Duration(dur) => now + dur,
        TimeSpec::Date((date, time)) => {
            let date = NaiveDate::from_ymd(
                date.year.unwrap_or_else(|| now.year()),
                date.month.unwrap_or_else(|| now.month()),
                date.day,
            );
            let date = NaiveDateTime::new(date, time);
            let offset = get_user_timezone(ctx, msg).await?;
            DateTime::from_utc(date - Duration::hours(offset), Utc)
        }
        TimeSpec::Time(time) => {
            let offset = get_user_timezone(ctx, msg).await?;
            let when = now
                .date()
                .and_time(time)
                .ok_or_else(|| anyhow::anyhow!("Invalid time"))?
                - Duration::hours(offset);
            if when < now {
                when + Duration::days(1)
            } else {
                when
            }
        }
    };
    Ok(when)
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
    let answer = {
        let answer = &msg
            .author
            .await_reply(&ctx)
            .await
            .ok_or_else(|| anyhow::anyhow!("no reply given"))?
            .content;
        answer
            .trim()
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("Invalid hour"))
            .and_then(|i| {
                if i < 24 {
                    Ok(i)
                } else {
                    Err(anyhow::anyhow!("Hours only go up to 23 🤔"))
                }
            })?
    };
    let offset = answer as i64 - now.hour() as i64;
    log::debug!("timestamp: {} user: {}, offset: {:?}", now, answer, offset);
    user_prefs::update(msg.author.id, |p| p.timezone_offset = Some(offset)).await?;
    Ok(offset)
}

#[group]
#[prefix("bday")]
#[commands(set_birthday_channel, next_bday, add_bday, remove_bday)]
struct BDays;

#[command("set_channel")]
#[required_permissions(ADMINISTRATOR)]
async fn set_birthday_channel(ctx: &Context, msg: &Message) -> CommandResult {
    let mut set = false;
    crate::prefs::guild::update(msg.guild_id.ok_or("must be in a guild")?, |g| {
        if g.birthday_channel == Some(msg.channel_id) {
            g.birthday_channel = None
        } else {
            g.birthday_channel = Some(msg.channel_id);
            set = true;
        }
    })
    .await?;
    if set {
        msg.channel_id
            .say(ctx, "This channel has been set as the birthday channel")
            .await?;
    } else {
        msg.channel_id
            .say(ctx, "This channel is no longer the birthday channel")
            .await?;
    }
    Ok(())
}

#[command("next")]
async fn next_bday(ctx: &Context, msg: &Message) -> CommandResult {
    macro_rules! fmt {
        ($name:expr, $nick:expr) => {
            format_args!(
                "**Name:**      {}\n**Nickname:** {}",
                $name,
                $nick.as_deref().unwrap_or("None")
            )
        };
    }

    let gid = msg.guild_id.ok_or("must be in a guild")?;
    let (date, users) = crate::birthdays::next_bday(gid)
        .await?
        .ok_or("No birthdays saved for this server :sob:")?;

    match &users[..] {
        [] => {
            log::error!("Users should never be empty. It was for date {:?}", date);
            return Err("Bot dev fucked up somehow".into());
        }
        [u] => {
            let Member { nick, user, .. } = gid.member(ctx, u.id).await?;
            msg.channel_id
                .send_message(ctx, |m| {
                    m.embed(|e| {
                        e.title("Next birthday :tada:")
                            .description(fmt!(user.name, nick).to_string())
                            .thumbnail(
                                user.avatar_url()
                                    .as_deref()
                                    .unwrap_or("https://i.imgur.com/lKmW0tc.png"),
                            )
                            .footer(|f| f.text(format!("{}/{}", date.day, date.month)))
                    })
                })
                .await?;
        }
        many => {
            let members: Vec<Member> = futures::stream::iter(many)
                .then(|u| gid.member(ctx, u.id))
                .try_collect()
                .await?;
            msg.channel_id
                .send_message(ctx, |m| {
                    m.embed(|e| {
                        e.title("Woah multiple birthdays! :tada: :tada:")
                            .description(
                                members
                                    .into_iter()
                                    .format_with("\n---\n", |m, f| f(&fmt!(m.user.name, m.nick))),
                            )
                            .footer(|f| f.text(format!("When: {}/{}", date.day, date.month)))
                    })
                })
                .await?;
        }
    }

    Ok(())
}

#[command]
#[required_permissions(ADMINISTRATOR)]
#[min_args(2)]
async fn add_bday(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let gid = msg.guild_id.ok_or("must be in a server")?;
    let uid = args.single::<UserId>()?;
    let date = NaiveDate::parse_from_str(&args.single::<String>()?, "%Y/%m/%d")?;
    let old = crate::birthdays::add_bday(gid, uid, date).await?;
    match old {
        Some(date) => {
            msg.channel_id
                .say(
                    ctx,
                    format!("Updated {} birthday, was {}", uid.mention(), date),
                )
                .await?
        }
        None => msg.channel_id.say(ctx, "Birthday added!").await?,
    };
    Ok(())
}

#[command]
#[required_permissions(ADMINISTRATOR)]
#[min_args(1)]
async fn remove_bday(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let gid = msg.guild_id.ok_or("must be in a server")?;
    let uid = args.single::<UserId>()?;
    match crate::birthdays::remove_bday(gid, uid).await? {
        Some(date) => {
            msg.channel_id
                .say(
                    ctx,
                    format!(
                        "Removed birthday for {}: was on the {}",
                        uid.mention(),
                        date
                    ),
                )
                .await?
        }
        None => {
            msg.channel_id
                .say(ctx, format!("no birthday for user {} found", uid.mention()))
                .await?
        }
    };
    Ok(())
}

#[group]
#[prefix("quiz")]
#[commands(set_quiz, unset_quiz)]
struct Quiz;

#[command("set")]
async fn set_quiz(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let channel = args.single::<ChannelId>()?;
    crate::quiz::add_quiz_guild(ctx, msg.guild_id.ok_or("not in a guild")?, channel).await?;
    msg.channel_id.say(&ctx, "done").await?;
    Ok(())
}

#[command("unset")]
async fn unset_quiz(ctx: &Context, msg: &Message) -> CommandResult {
    crate::quiz::remove_quiz_guild(ctx, msg.guild_id.ok_or("not in a guild")?).await?;
    msg.channel_id.say(ctx, "done").await?;
    Ok(())
}
