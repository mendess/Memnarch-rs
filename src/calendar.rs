use crate::{daemons::DaemonManager, file_transaction::Database, util::tuple_map::tuple_map_both};
use anyhow::Context;
use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Utc, Weekday};
use daemons::{ControlFlow, Daemon};
use futures::*;
use itertools::Itertools;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serenity::{
    http::CacheHttp,
    model::{
        channel::ReactionType,
        id::{ChannelId, MessageId},
        misc::Mentionable,
    },
};
use std::iter::successors;

mod reacts {
    use serenity::model::channel::ReactionType;
    use serenity::model::id::EmojiId;

    type EmojiFallback = ((bool, EmojiId, &'static str), char);
    pub(super) const YES: EmojiFallback =
        ((true, EmojiId(723360851527991366), "perryyessign"), '✅');
    pub(super) const NO: EmojiFallback = ((true, EmojiId(723360851330859048), "perrynosign"), '❌');
    pub(super) const MAYBE: EmojiFallback =
        ((true, EmojiId(723359761382506597), "perryokaysign"), '❓');
    pub(super) const NAO_QUERO: EmojiFallback =
        ((true, EmojiId(779017270243491870), "perryguitar"), '⛔');
    pub(super) const ALL: [EmojiFallback; 4] = [YES, NO, MAYBE, NAO_QUERO];
    pub(super) fn all() -> impl Iterator<Item = (ReactionType, char)> {
        ALL.iter().map(|&((animated, id, name), f)| {
            (
                ReactionType::Custom {
                    animated,
                    id,
                    name: Some(name.into()),
                },
                f,
            )
        })
    }
}

lazy_static! {
    static ref DATABASE: Database<Vec<Calendar>> = Database::new("files/calendars.json");
}

#[derive(Debug, Serialize, Deserialize)]
struct Calendar {
    channel: ChannelId,
    messages: [MessageId; 7],
}

pub async fn new(ctx: impl CacheHttp, channel: ChannelId) -> anyhow::Result<()> {
    let bot_id = ctx
        .cache()
        .expect("Should be using cache feature")
        .current_user_id()
        .await;
    let ctx_ref = &ctx;
    channel
        .messages_iter(ctx.http())
        .filter_map(|m| future::ready(m.ok()))
        .filter(|m| future::ready(m.author.id == bot_id))
        .for_each_concurrent(Some(2), |m| async move {
            let _ = m.delete(ctx_ref).await;
        })
        .await;
    let mut messages = [MessageId(0); 7];
    for (i, d) in successors(Some(Utc::today().naive_utc()), |d| d.succ_opt())
        .take(7)
        .enumerate()
    {
        messages[i] = send_message(ctx_ref, channel, d).await?;
    }
    DATABASE.load().await?.push(Calendar { channel, messages });
    Ok(())
}

async fn send_message(
    ctx: impl CacheHttp,
    channel: ChannelId,
    d: NaiveDate,
) -> anyhow::Result<MessageId> {
    let message = channel
        .send_message(ctx.http(), |m| {
            m.embed(|e| {
                e.title(format!(
                    "{}/{} ({})",
                    d.day(),
                    d.month(),
                    translate_weekday(d.weekday())
                ))
            })
        })
        .await?;
    for (e, fallback) in reacts::all() {
        if let Err(e) = message.react(&ctx, e).await {
            log::warn!("Failed to react with custom emoji: {}", e);
            message.react(&ctx, fallback).await?;
        }
    }
    Ok(message.id)
}

pub async fn remove(ctx: impl CacheHttp, channel: ChannelId) -> anyhow::Result<()> {
    let mut calendars = DATABASE.load().await?;
    if let Some(i) = calendars.iter().position(|c| c.channel == channel) {
        let cal = &calendars[i];
        if let Err(_) = channel.delete_messages(ctx.http(), &cal.messages).await {
            for m in cal.messages {
                channel.delete_message(ctx.http(), m).await?;
            }
        }
        calendars.swap_remove(i);
        Ok(())
    } else {
        Err(anyhow::anyhow!("Channel is not a calendar"))
    }
}

async fn tick(ctx: impl CacheHttp) -> anyhow::Result<()> {
    let mut cals = DATABASE.load().await?;
    let today = Utc::today();
    for Calendar { channel, messages } in cals.iter_mut() {
        log::debug!("Ticking calendar in channel {}", channel);
        loop {
            let m_id = *messages.first().unwrap();
            let mut m = channel.message(ctx.http(), m_id).await?;
            // let mut m = match  {
            //         Ok(m) => m,
            //         Err(e) => ,
            //     };
            let (day, month) = {
                let title = m.embeds[0].title.take().unwrap();
                let (date, _) = title.split_once(' ').expect("a correct title");
                tuple_map_both(date.split_once('/').expect("a correct title"), |x| {
                    x.parse::<u32>().expect("a correct title")
                })
            };
            let old_date = NaiveDate::from_ymd(today.year(), month, day);
            if old_date >= today.naive_utc() {
                break;
            }
            let date = old_date + chrono::Duration::days(7);
            *messages.first_mut().unwrap() = send_message(&ctx, *channel, date)
                .await
                .context("sending a new message")?;
            channel
                .delete_message(ctx.http(), m_id)
                .await
                .context("deleting a message")?;
            messages.rotate_left(1);
        }
    }
    Ok(())
}

fn translate_weekday(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Segunda",
        Weekday::Tue => "Terça",
        Weekday::Wed => "Quarta",
        Weekday::Thu => "Quinta",
        Weekday::Fri => "Sexta",
        Weekday::Sat => "Sabado",
        Weekday::Sun => "Domingo",
    }
}

pub async fn initialize(dm: &mut DaemonManager) {
    use crate::events::pubsub::{
        self,
        events::{CacheReady, ReactionAdd, ReactionRemove, ReactionRemoveAll},
    };
    use serenity::{client::Context, model::channel::Message};
    use std::mem::take;

    async fn react_change(ctx: &Context, ch_id: ChannelId, msg_id: MessageId) -> ControlFlow {
        if let Err(e) = ch_id
            .message(ctx, msg_id)
            .and_then(|m| update_reacts(ctx, m))
            .await
        {
            log::error!("failed to update reacts: {}", e);
        }
        async fn update_reacts(ctx: &Context, mut message: Message) -> serenity::Result<()> {
            let bot_id = crate::util::bot_id(ctx).await;
            if matches!(bot_id, Some(id) if id != message.author.id) {
                return Ok(());
            }
            let title = match message.embeds.get_mut(0).and_then(|e| e.title.take()) {
                Some(t) => t,
                None => return Ok(()),
            };
            if DATABASE
                .load()
                .await?
                .iter()
                .all(|c| c.messages.iter().all(|m| *m != message.id))
            {
                return Ok(());
            }
            let reactions = {
                let mut reactions = Vec::with_capacity(message.reactions.len());
                for rt in take(&mut message.reactions)
                    .into_iter()
                    .map(|mr| mr.reaction_type)
                {
                    let mut users = message.reaction_users(ctx, rt.clone(), None, None).await?;
                    users.retain(|x| Some(x.id) != bot_id);
                    reactions.push((rt, users));
                }
                reactions.sort_by_cached_key(|(e, _)| match e {
                    ReactionType::Custom { id, .. } => reacts::ALL
                        .iter()
                        .position(|((_, rid, _), _)| rid == id)
                        .unwrap_or(id.0 as usize),
                    ReactionType::Unicode(s) => s.len(),
                    _ => usize::MAX,
                });
                reactions
            };
            message
                .channel_id
                .edit_message(ctx, message.id, |e| {
                    e.embed(|e| {
                        e.title(title).fields(
                            reactions
                                .into_iter()
                                .filter(|(_, v)| !v.is_empty())
                                .map(|(k, v)| {
                                    (
                                        format!("{} {}", k, v.len()),
                                        v.into_iter().map(|u| u.mention()).format("\n"),
                                        true,
                                    )
                                }),
                        )
                    })
                })
                .await?;
            Ok(())
        }
        ControlFlow::CONTINUE
    }
    pubsub::register::<ReactionAdd, _>(|c, a| {
        async move { react_change(c, a.channel_id, a.message_id).await }.boxed()
    });
    pubsub::register::<ReactionRemove, _>(|c, a| {
        async move { react_change(c, a.channel_id, a.message_id).await }.boxed()
    });
    pubsub::register::<ReactionRemoveAll, _>(|c, (ch_id, msg_id)| {
        async move { react_change(c, *ch_id, *msg_id).await }.boxed()
    });
    pubsub::register::<CacheReady, _>(|c, _| {
        async move {
            if let Err(e) = tick(c).await {
                log::error!("Failed to tick calenders after ready: {}", e);
            }
            ControlFlow::BREAK
        }
        .boxed()
    });
    dm.add_daemon(CalendarDaemon).await;
}

struct CalendarDaemon;

#[serenity::async_trait]
impl Daemon for CalendarDaemon {
    type Data = serenity::CacheAndHttp;

    async fn run(&mut self, data: &Self::Data) -> ControlFlow {
        if let Err(e) = tick(data).await {
            log::error!("Failed to tick a calendar forward: {:?}", e);
        }
        ControlFlow::CONTINUE
    }

    async fn interval(&self) -> std::time::Duration {
        let now = Utc::now().naive_utc();
        let midnight = NaiveDateTime::new(now.date().succ(), NaiveTime::from_hms(0, 10, 0));
        let dur = (midnight - now).to_std().unwrap_or_default();
        log::trace!(
            "calendars will tick forward in {}",
            humantime::format_duration(dur)
        );
        dur
    }

    async fn name(&self) -> String {
        "calendar daemon".into()
    }
}
