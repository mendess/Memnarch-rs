use crate::{
    commands::Context,
    prefs::{self, user::UserPrefs},
    reminders::{self, parser::*},
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Utc};
use itertools::Itertools;
use poise::{CreateReply, command};
use serenity::{
    all::{CreateSelectMenu, CreateSelectMenuOption, UserId},
    prelude::*,
};

pub fn commands() -> impl Iterator<Item = crate::commands::Command> {
    [reminders(), remindme(), remind()].into_iter()
}

#[command(slash_command, dm_only)]
async fn reminders(ctx: Context<'_>) -> anyhow::Result<()> {
    use chrono::{Datelike, Timelike};

    let s = reminders::reminders(ctx.author().id)
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
    ctx.say(if s.is_empty() {
        "You don't have any reminders".to_string()
    } else {
        s
    })
    .await?;
    Ok(())
}

/// Set a reminder for later.
#[command(slash_command)]
async fn remindme(
    ctx: Context<'_>,
    // #[description = "
    // - (day|dia) DD/MM/YYYY (at|as|às|@) HH:MM:SS [reminder]
    // - (at|as|às|@) HH:MM:SS [reminder]
    // - X[s|m|h|d|w|month|y] [reminder]
    // "]
    reminder: TimeSpec,
    what: String,
) -> anyhow::Result<()> {
    // parse(args.rest()).map_err(|e| anyhow::anyhow!("Invalid time spec: {}", e))?;
    let when = calculate_when(ctx, reminder).await?;
    let dm = &ctx.data().daemons;
    reminders::remind(&mut *dm.lock().await, what, when, ctx.author().id).await?;
    ctx.say("You shall be reminded!").await?;
    Ok(())
}

#[command(slash_command)]
async fn remind(
    ctx: Context<'_>,
    who: UserId,
    // #[description = "
    // - (day|dia) DD/MM/YYYY (at|as|às|@) HH:MM:SS [reminder]
    // - (at|as|às|@) HH:MM:SS [reminder]
    // - X[s|m|h|d|w|month|y] [reminder]
    // "]
    when: TimeSpec,
    what: String,
) -> anyhow::Result<()> {
    let when = calculate_when(ctx, when).await?;
    let dm = &ctx.data().daemons;
    if reminders::is_blocked_by(ctx.author().id, who).await? {
        ctx.say(format!("{} blocked you", who.mention())).await?;
        return Ok(());
    }
    reminders::remind(
        &mut *dm.lock().await,
        if who == ctx.author().id {
            what
        } else {
            format!(
                r"{} asked me to remind you:
\~\~\~\~
{}
\~\~\~\~

*React with {} to block this person from reminding you. Unreact to unblock*",
                ctx.author().mention(),
                what,
                reminders::BLOCK_EMOJI,
            )
        },
        when,
        who,
    )
    .await?;
    ctx.say("It shall be ~~done~~ spammed").await?;
    Ok(())
}

async fn calculate_when(ctx: Context<'_>, when: TimeSpec) -> anyhow::Result<DateTime<Utc>> {
    let now = ctx.created_at().with_timezone(&Utc);
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
            let offset = get_user_timezone(ctx).await?;
            DateTime::from_naive_utc_and_offset(date - Duration::hours(offset as _), Utc)
        }
        TimeSpec::Time(time) => {
            let offset = get_user_timezone(ctx).await?;
            let when = DateTime::from_naive_utc_and_offset(now.date_naive().and_time(time), Utc)
                - Duration::hours(offset as _);
            if when < now {
                when + Duration::days(1)
            } else {
                when
            }
        }
    };
    Ok(when)
}

async fn get_user_timezone(ctx: Context<'_>) -> anyhow::Result<i8> {
    if let Some(UserPrefs {
        timezone_offset: Some(off),
    }) = prefs::user::get(ctx.author().id).await?
    {
        return Ok(off);
    }
    let now = Utc::now();
    let component = serenity::builder::CreateActionRow::SelectMenu(
        CreateSelectMenu::new(
            "timezone",
            serenity::all::CreateSelectMenuKind::String {
                options: (0..23)
                    .map(|i| {
                        CreateSelectMenuOption::new(
                            format!("{i:02}:{:02}", now.minute()),
                            i.to_string(),
                        )
                    })
                    .collect(),
            },
        )
        .min_values(1)
        .max_values(1),
    );

    let reply_handle = ctx
        .send(
            CreateReply::default()
                .content("I don't know what time it is over there! What time is it?")
                .components(vec![component]),
        )
        .await?;
    let response = reply_handle
        .message()
        .await?
        .await_component_interaction(ctx)
        .author_id(ctx.author().id)
        .timeout(std::time::Duration::from_secs(60))
        .await;
    let Some(response) = response else {
        anyhow::bail!("You ignored me :(");
    };
    let answer = match response.data.kind {
        serenity::all::ComponentInteractionDataKind::StringSelect { values } => {
            values[0].parse::<i8>().unwrap()
        }
        _ => unreachable!(),
    };
    let offset = answer - now.hour() as i8;
    tracing::debug!("timestamp: {} user: {}, offset: {:?}", now, answer, offset);
    prefs::user::update(ctx.author().id, |p| p.timezone_offset = Some(offset)).await?;
    Ok(offset)
}
