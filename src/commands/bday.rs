use crate::{commands::Context, prefs};
use anyhow::Context as _;
use chrono::{Datelike as _, Month, NaiveDate, Utc};
use futures::{StreamExt as _, TryStreamExt as _, stream};
use itertools::Itertools as _;
use num_traits::FromPrimitive as _;
use poise::{CreateReply, command};
use serenity::all::{
    ChannelId, CreateEmbed, CreateEmbedFooter, Member, Mentionable as _, RoleId, UserId,
};

#[command(
    slash_command,
    guild_only,
    subcommands("set_channel", "add", "remove", "next", "list", "month", "set_role")
)]
pub async fn bday(_ctx: Context<'_>) -> anyhow::Result<()> {
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
async fn set_channel(ctx: Context<'_>, ch: ChannelId) -> anyhow::Result<()> {
    let mut set = false;
    crate::prefs::guild::update(ctx.guild_id().context("must be in a guild")?, |g| {
        if g.birthday_channel == Some(ch) {
            g.birthday_channel = None
        } else {
            g.birthday_channel = Some(ch);
            set = true;
        }
    })
    .await?;
    if set {
        ctx.say("This channel has been set as the birthday channel")
            .await?;
    } else {
        ctx.say("This channel is no longer the birthday channel")
            .await?;
    }
    Ok(())
}

fn short_month(m: u32) -> &'static str {
    &Month::from_u32(m).unwrap().name()[..3]
}

#[command(slash_command, guild_only)]
async fn list(ctx: Context<'_>) -> anyhow::Result<()> {
    let gid = ctx.guild_id().context("must be in a server")?;
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
    ctx.send(
        CreateReply::default().embed(CreateEmbed::new().title("all the birthdays 🥳🎉🥳").fields(
            bdays.into_iter().map(|(m, nicks)| {
                (
                    format!("{} ({})", Month::from_u32(m).unwrap().name(), nicks.len()),
                    nicks
                        .into_iter()
                        .map(|(_, n)| n.unwrap_or_else(|id| format!("user with id {id} not found")))
                        .format("\n")
                        .to_string(),
                    true,
                )
            }),
        )),
    )
    .await?;
    Ok(())
}

#[command(slash_command, guild_only)]
async fn month(ctx: Context<'_>) -> anyhow::Result<()> {
    let gid = ctx.guild_id().context("must be in a guild")?;
    let month = Month::from_u32(Utc::now().month()).unwrap();
    match crate::birthdays::of_month(gid, month).await? {
        None => return Err(anyhow::anyhow!("No birthdays this month 😭")),
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
            ctx.send(
                CreateReply::default().embed(
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

#[command(slash_command, guild_only)]
async fn next(ctx: Context<'_>, who: Option<UserId>) -> anyhow::Result<()> {
    macro_rules! fmt {
        ($name:expr, $nick:expr) => {
            format!(
                "**Name:**      {}\n**Nickname:** {}",
                $name,
                $nick.as_deref().unwrap_or("None")
            )
        };
    }

    let gid = ctx.guild_id().context("must be in a guild")?;
    match who {
        Some(user) => {
            let date = crate::birthdays::of(gid, user)
                .await?
                .context("No birthdays saved for this user 😭")?;
            let member = gid.member(ctx, user).await?;
            ctx.send(poise::CreateReply::default().embed({
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
            }))
            .await?;
        }
        None => {
            let (date, users) = crate::birthdays::next_bday(gid)
                .await?
                .context("No birthdays saved for this server 😭")?;

            match &users[..] {
                [] => {
                    tracing::error!("Users should never be empty. It was for date {:?}", date);
                    return Err(anyhow::anyhow!("Bot dev fucked up somehow"));
                }
                [u] => {
                    let member = gid.member(ctx, u.id).await?;
                    ctx.send(
                        CreateReply::default().embed(
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
                    ctx.send(
                        CreateReply::default().embed(
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

#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
async fn add(ctx: Context<'_>, user: UserId, date: NaiveDate) -> anyhow::Result<()> {
    let gid = ctx.guild_id().context("must be in a server")?;
    let old = crate::birthdays::add_bday(gid, user, date).await?;
    match old {
        Some(date) => {
            ctx.say(format!("Updated {} birthday, was {}", user.mention(), date))
                .await?
        }
        None => ctx.say("Birthday added!").await?,
    };
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
async fn remove(ctx: Context<'_>, user: UserId) -> anyhow::Result<()> {
    let gid = ctx.guild_id().context("must be in a server")?;
    match crate::birthdays::remove_bday(gid, user).await? {
        Some(date) => {
            ctx.say(format!(
                "Removed birthday for {}: was on the {}",
                user.mention(),
                date
            ))
            .await?
        }
        None => {
            ctx.say(format!("no birthday for user {} found", user.mention()))
                .await?
        }
    };
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
async fn set_role(ctx: Context<'_>, role: RoleId) -> anyhow::Result<()> {
    prefs::guild::update(ctx.guild_id().context("not in a guild")?, |prefs| {
        prefs.birthday_role = Some(role)
    })
    .await?;
    ctx.say("birthday role set!").await?;
    Ok(())
}
