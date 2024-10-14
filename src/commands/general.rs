use crate::{
    get,
    prefs::{self, user::UserPrefs},
    reminders::{self, parser::*},
    util::{daemons::DaemonManagerKey, MentionExt},
};
use chrono::{DateTime, Datelike, Duration, Month, NaiveDate, NaiveDateTime, Timelike, Utc};
use futures::{stream, StreamExt, TryStreamExt};
use itertools::Itertools;
use num_traits::FromPrimitive;
use serenity::{
    all::{CreateEmbed, CreateEmbedFooter, CreateMessage, EditMessage, Mention},
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::{channel::Message, guild::Member},
    prelude::*,
};
use std::iter::from_fn;

#[group]
#[commands(
    ping,
    who_are_you,
    remindme,
    remind,
    version,
    reminders,
    toggle_spoilers
)]
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
    let sent_timestamp = msg.timestamp;
    let mut msg = msg
        .channel_id
        .say(&ctx.http, "Pong! calculating ms")
        .await?;
    let rtt = msg.timestamp.timestamp_millis() - sent_timestamp.timestamp_millis();
    msg.edit(&ctx, EditMessage::new().content(format!("Pong! {rtt}ms")))
        .await?;
    Ok(())
}

#[command("whoareyou")]
#[description("Find out more about me")]
async fn who_are_you(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .send_message(ctx, CreateMessage::new()
            .embed(CreateEmbed::new()
                .title("I AM MEMNARCH")
                    .description("Sauce code: [GitHub](https://github.com/Mendess2526/Memnarch-rs)")
                    .image("https://cards.scryfall.io/art_crop/front/9/2/9203fde4-dbc1-449f-9618-4656f0e25e3c.jpg?1562925842")
            )
        )
        .await?;
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
    let mut dm = get!(> data, DaemonManagerKey, lock);
    reminders::remind(&mut dm, text.into(), when, msg.author.id).await?;
    msg.channel_id.say(&ctx, "You shall be reminded!").await?;
    Ok(())
}

#[command]
async fn remind(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let user_ids = from_fn(|| args.single::<Mention>().ok()?.into_user().ok()).collect::<Vec<_>>();
    if user_ids.is_empty() {
        return Err("Please mention at least one person".into());
    }
    let Reminder { text, when } =
        parse(args.rest()).map_err(|e| anyhow::anyhow!("Invalid time spec: {}", e))?;
    let when = calculate_when(ctx, msg, when).await?;
    let data = ctx.data.read().await;
    let mut dm = get!(> data, DaemonManagerKey, lock);
    let mut got_one = false;
    for user_id in user_ids {
        if reminders::is_blocked_by(msg.author.id, user_id).await? {
            msg.channel_id
                .say(ctx, format!("{} blocked you", user_id.mention()))
                .await?;
            continue;
        }
        reminders::remind(
            &mut dm,
            if user_id == msg.author.id {
                text.into()
            } else {
                format!(
                    r"{} asked me to remind you:
\~\~\~\~
{}
\~\~\~\~

*React with {} to block this person from reminding you. Unreact to unblock*",
                    msg.author.mention(),
                    text,
                    reminders::BLOCK_EMOJI,
                )
            },
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
    let now = msg.timestamp.with_timezone(&Utc);
    let when = match when {
        TimeSpec::Duration(dur) => now + dur,
        TimeSpec::Date((date, time)) => {
            let date = NaiveDate::from_ymd_opt(
                date.year.unwrap_or_else(|| now.year()),
                date.month.unwrap_or_else(|| now.month()),
                date.day,
            )
            .expect("date is valid because it was formed from valid a date");
            let date = NaiveDateTime::new(date, time);
            let offset = get_user_timezone(ctx, msg).await?;
            DateTime::from_naive_utc_and_offset(date - Duration::hours(offset), Utc)
        }
        TimeSpec::Time(time) => {
            let offset = get_user_timezone(ctx, msg).await?;
            let when = DateTime::from_naive_utc_and_offset(now.date_naive().and_time(time), Utc)
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
    }) = prefs::user::get(msg.author.id).await?
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
            .await_reply(ctx)
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
    tracing::debug!("timestamp: {} user: {}, offset: {:?}", now, answer, offset);
    prefs::user::update(msg.author.id, |p| p.timezone_offset = Some(offset)).await?;
    Ok(offset)
}

#[group]
#[prefix("bday")]
#[default_command(next_bday)]
#[commands(
    set_birthday_channel,
    next_bday,
    bday_month,
    bday_list,
    add_bday,
    remove_bday,
    set_bday_role
)]
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

fn short_month(m: u32) -> &'static str {
    &Month::from_u32(m).unwrap().name()[..3]
}

#[command("list")]
async fn bday_list(ctx: &Context, msg: &Message) -> CommandResult {
    let gid = msg.guild_id.ok_or("must be in a server")?;
    let bdays = stream::iter(crate::birthdays::all(gid).await?)
        .then(|(m, v)| async move {
            let mut nicks = stream::iter(v)
                .then(|(d, u)| async move { (d, gid.member(ctx, u.id).await.map_err(|_| u.id)) })
                .map(|(d, m)| (d, m.map(|m| m.display_name().to_owned())))
                .collect::<Vec<_>>()
                .await;
            nicks.sort_by_key(|(d, _)| *d);
            (m, nicks)
        })
        .collect::<Vec<_>>()
        .await;
    msg.channel_id
        .send_message(
            ctx,
            CreateMessage::new().embed(
                CreateEmbed::new()
                    .title("all the birthdays 🥳🎉🥳")
                    .fields(bdays.into_iter().map(|(m, nicks)| {
                        (
                            format!("{} ({})", Month::from_u32(m).unwrap().name(), nicks.len()),
                            nicks
                                .into_iter()
                                .map(|(_, n)| {
                                    n.unwrap_or_else(|id| format!("user with id {id} not found"))
                                })
                                .format("\n")
                                .to_string(),
                            true,
                        )
                    })),
            ),
        )
        .await?;
    Ok(())
}

#[command("month")]
async fn bday_month(ctx: &Context, msg: &Message) -> CommandResult {
    let gid = msg.guild_id.ok_or("must be in a guild")?;
    let month = Month::from_u32(Utc::now().month()).unwrap();
    match crate::birthdays::of_month(gid, month).await? {
        None => return Err("No birthdays this month 😭".into()),
        Some(i) => {
            let mut bdays = stream::iter(i)
                .then(|(d, u)| async move {
                    let member = gid
                        .member(ctx, u.id)
                        .await
                        .map(|m| m.display_name().to_owned());
                    (d.day, member.map_err(|_| u.id))
                })
                .collect::<Vec<_>>()
                .await;
            bdays.sort_by_key(|x| x.0);
            msg.channel_id
                .send_message(
                    ctx,
                    CreateMessage::new().embed(
                        CreateEmbed::new()
                            .title("Birthdays this month 🥳")
                            .description(
                                bdays
                                    .into_iter()
                                    .format_with("\n", |(d, u), f| {
                                        f(&format_args!(
                                            "{:2}: {}",
                                            d,
                                            u.unwrap_or_else(|id| format!(
                                                "user with id {id} not found"
                                            ))
                                        ))
                                    })
                                    .to_string(),
                            ),
                    ),
                )
                .await?;
        }
    };
    Ok(())
}

#[command("next")]
async fn next_bday(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    macro_rules! fmt {
        ($name:expr, $nick:expr) => {
            format!(
                "**Name:**      {}\n**Nickname:** {}",
                $name,
                $nick.as_deref().unwrap_or("None")
            )
        };
    }

    let gid = msg.guild_id.ok_or("must be in a guild")?;
    match args.single::<Mention>().map(|m| m.into_user()) {
        Ok(Err(invalid_mention)) => {
            msg.channel_id.say(ctx, invalid_mention).await?;
        }
        Ok(Ok(user)) => {
            let date = crate::birthdays::of(gid, user)
                .await?
                .ok_or("No birthdays saved for this user 😭")?;
            let member = gid.member(ctx, user).await?;
            msg.channel_id
                .send_message(
                    ctx,
                    CreateMessage::new().embed({
                        let now = Utc::now().naive_utc().date();
                        let mut bday = NaiveDate::from_ymd_opt(now.year(), date.month, date.day)
                            .expect("formed from valid dates");
                        if bday < now {
                            bday = bday.with_year(now.year() + 1).unwrap();
                        }
                        CreateEmbed::new()
                            .title(format!("{}'s birthday 🎉", member.display_name()))
                            .description(format!(
                                "{}/{}.\n{} days left 👀",
                                date.day,
                                short_month(date.month),
                                (bday - now).num_days()
                            ))
                            .thumbnail(member.face())
                    }),
                )
                .await?;
        }
        Err(_) => {
            let (date, users) = crate::birthdays::next_bday(gid)
                .await?
                .ok_or("No birthdays saved for this server 😭")?;

            match &users[..] {
                [] => {
                    tracing::error!("Users should never be empty. It was for date {:?}", date);
                    return Err("Bot dev fucked up somehow".into());
                }
                [u] => {
                    let member = gid.member(ctx, u.id).await?;
                    msg.channel_id
                        .send_message(
                            ctx,
                            CreateMessage::new().embed(
                                CreateEmbed::new()
                                    .title("Next birthday :tada:")
                                    .description(fmt!(member.user.name, member.nick))
                                    .thumbnail(member.face())
                                    .footer(CreateEmbedFooter::new(format!(
                                        "{}/{}",
                                        date.day,
                                        short_month(date.month)
                                    ))),
                            ),
                        )
                        .await?;
                }
                many => {
                    let members: Vec<Member> = futures::stream::iter(many)
                        .then(|u| gid.member(ctx, u.id))
                        .try_collect()
                        .await?;
                    msg.channel_id
                        .send_message(
                            ctx,
                            CreateMessage::new().embed(
                                CreateEmbed::new()
                                    .title("Woah multiple birthdays! :tada: :tada:")
                                    .description(
                                        members
                                            .into_iter()
                                            .format_with("\n---\n", |m, f| {
                                                f(&fmt!(m.user.name, m.nick))
                                            })
                                            .to_string(),
                                    )
                                    .footer(CreateEmbedFooter::new(format!(
                                        "When: {}/{}",
                                        date.day,
                                        &Month::from_u32(date.month).unwrap().name()[..3],
                                    ))),
                            ),
                        )
                        .await?;
                }
            }
        }
    }

    Ok(())
}

#[command("add")]
#[aliases("add_bday")]
#[required_permissions(ADMINISTRATOR)]
#[min_args(2)]
#[usage("@mention YYYY/MM/DD")]
async fn add_bday(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let gid = msg.guild_id.ok_or("must be in a server")?;
    let uid = args.single::<Mention>()?.into_user()?;
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

#[command("remove")]
#[aliases("remove_bday")]
#[required_permissions(ADMINISTRATOR)]
#[min_args(1)]
#[usage("@mention")]
async fn remove_bday(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let gid = msg.guild_id.ok_or("must be in a server")?;
    let uid = args.single::<Mention>()?.into_user()?;
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

#[command]
#[required_permissions(ADMINISTRATOR)]
#[min_args(1)]
#[usage("@role")]
async fn set_bday_role(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let role_id = args.single::<Mention>()?.into_role()?;
    prefs::guild::update(msg.guild_id.ok_or("not in a guild")?, |prefs| {
        prefs.birthday_role = Some(role_id)
    })
    .await?;
    msg.channel_id.say(ctx, "birthday role set!").await?;
    Ok(())
}

#[command("toggle-spoilers")]
#[required_permissions(ADMINISTRATOR)]
async fn toggle_spoilers(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let action = crate::mtg_spoilers::toggle_channel(args.single()?).await?;
    msg.channel_id.say(ctx, format!("{action:?}")).await?;
    Ok(())
}
