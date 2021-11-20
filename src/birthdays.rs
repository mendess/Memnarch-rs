use std::{
    collections::BTreeMap, io::Write, os::unix::prelude::OsStrExt, path::PathBuf, str::from_utf8,
    sync::Arc,
};

use anyhow::Context;
use chrono::{Datelike, NaiveDate, Utc};
use daemons::ControlFlow;
use dashmap::DashMap;
use futures::FutureExt;
use lazy_static::lazy_static;
use serenity::{
    http::Http,
    model::id::{GuildId, UserId},
    prelude::Mentionable,
};
use tokio::{fs, io};

use crate::{cron::Cron, file_transaction::Database, prefs::guild as guild_prefs, DaemonManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BDayBoy {
    pub id: UserId,
    pub year: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BDay {
    pub month: u32,
    pub day: u32,
}

impl From<NaiveDate> for BDay {
    fn from(d: NaiveDate) -> Self {
        Self {
            month: d.month(),
            day: d.day(),
        }
    }
}

const BASE: &str = "files/birthdays";

type BdayMap = DashMap<GuildId, Database<BTreeMap<BDay, Vec<BDayBoy>>, anyhow::Error>>;

lazy_static! {
    static ref BDAY_MAP: BdayMap = DashMap::new();
}

pub async fn initialize(d: &mut DaemonManager) -> io::Result<()> {
    fs::DirBuilder::new().recursive(true).create(BASE).await?;
    let mut read_dir = fs::read_dir(BASE).await?;
    while let Some(d) = read_dir.next_entry().await? {
        let path = d.path();
        let gid = match path.file_stem().and_then(|n| {
            let s = from_utf8(n.as_bytes()).ok()?;
            Some(GuildId(str::parse(s).ok()?))
        }) {
            None => continue,
            Some(gid) => gid,
        };
        BDAY_MAP.insert(gid, Database::with_ser_and_deser(path, ser, deser));
    }
    d.add_daemon(BDayChecker::new("bday checker", |c| {
        check_bday(c.http.clone()).boxed()
    }))
    .await;
    Ok(())
}

pub async fn next_bday(g: GuildId) -> anyhow::Result<Option<(BDay, Vec<BDayBoy>)>> {
    let map = match BDAY_MAP.get(&g) {
        None => return Ok(None),
        Some(b) => b,
    };
    let tomorrow = BDay::from(Utc::now().date().naive_utc().succ());
    let mut map = map.load().await?;
    let tree = map.take();
    let next = match tree.range(tomorrow..).next() {
        Some((d, v)) => Some((*d, v.clone())),
        None => tree.iter().next().map(|(d, v)| (*d, v.clone())),
    };
    Ok(next)
}

pub async fn add_bday(
    g: GuildId,
    who: UserId,
    when: NaiveDate,
) -> anyhow::Result<Option<NaiveDate>> {
    let calendar = BDAY_MAP.entry(g).or_insert_with(|| {
        let path = [BASE, &format!("{}.csv", g)]
            .into_iter()
            .collect::<PathBuf>();
        Database::with_ser_and_deser(path, ser, deser)
    });
    let mut calendar = calendar.load().await?;
    let bday = BDay::from(when);
    let removed = remove_user(&mut *calendar, who);
    calendar.entry(bday).or_default().push(BDayBoy {
        id: who,
        year: when.year(),
    });
    Ok(removed)
}

pub async fn remove_bday(g: GuildId, who: UserId) -> anyhow::Result<Option<NaiveDate>> {
    let calendar = match BDAY_MAP.get(&g) {
        Some(c) => c,
        None => return Ok(None),
    };
    let mut calendar = calendar.load().await?;
    Ok(remove_user(&mut *calendar, who))
}

fn remove_user(tree: &mut BTreeMap<BDay, Vec<BDayBoy>>, user: UserId) -> Option<NaiveDate> {
    let mut when = None;
    tree.retain(
        |date, users| match users.iter().position(|u| u.id == user) {
            Some(index) => {
                let user = users.swap_remove(index);
                when = Some(NaiveDate::from_ymd(user.year, date.month, date.day));
                !users.is_empty()
            }
            None => true,
        },
    );
    when
}

fn ser(w: &mut dyn Write, t: &BTreeMap<BDay, Vec<BDayBoy>>) -> anyhow::Result<()> {
    for (BDay { month, day }, v) in t {
        for BDayBoy { id, year } in v {
            writeln!(w, "{};{}-{}-{}", id, year, month, day)?
        }
    }
    Ok(())
}

fn deser(v: &[u8]) -> anyhow::Result<BTreeMap<BDay, Vec<BDayBoy>>> {
    v.split(|&c| c == b'\n')
        .filter(|x| !x.is_empty())
        .map(|line| {
            let mut l = line.split(|&c| c == b';');

            let uid = l
                .next()
                .map(|b| -> anyhow::Result<_> {
                    let s = from_utf8(b)
                        .with_context(|| format!("failed to deser uid {:?}", from_utf8(b)))?;
                    s.parse::<u64>()
                        .map(UserId)
                        .with_context(|| format!("failed to deser {:?}", s))
                })
                .transpose()
                .with_context(|| format!("Failed to deserialize uid {:?}", from_utf8(line)))?;

            let date = l
                .next()
                .map(|b| -> anyhow::Result<_> {
                    let s = from_utf8(b)
                        .with_context(|| format!("failed to deser date {:?}", from_utf8(b)))?;
                    NaiveDate::parse_from_str(s, "%Y-%m-%d")
                        .with_context(|| format!("failed to deser date {:?}", s))
                })
                .transpose()
                .with_context(|| format!("Failed to deserialize date {:?}", from_utf8(line)))?;

            if let (Some(uid), Some(date)) = (uid, date) {
                Ok((date, uid))
            } else {
                Err(anyhow::anyhow!(
                    "Failed to deserialize {:?}: Incomplete line. Expected 2 arguments got {}",
                    from_utf8(line),
                    [uid.is_some(), date.is_some()]
                        .into_iter()
                        .filter(|&x| x)
                        .count()
                ))
            }
        })
        .try_fold(BTreeMap::default(), |mut acc, e| {
            let e = e?;
            acc.entry(BDay::from(e.0))
                .or_insert_with(Vec::new)
                .push(BDayBoy {
                    id: e.1,
                    year: e.0.year(),
                });
            Ok(acc)
        })
}

type BDayChecker<F, Fut> = Cron<F, Fut, 0, 0, 30>;

async fn check_bday(http: Arc<Http>) -> ControlFlow {
    let today = BDay::from(Utc::now().naive_utc().date());
    for x in BDAY_MAP.iter() {
        let (gid, guild) = (x.key(), x.value());
        let channel = match guild_prefs::get(*gid)
            .await
            .map(|p| p.and_then(|p| p.birthday_channel))
        {
            Ok(Some(ch)) => ch,
            Ok(None) => {
                log::error!("birthday channel not set for guild {}", gid);
                continue;
            },
            Err(e) => {
                log::error!("Error fetching guild prefs: {:?}", e);
                continue;
            }
        };
        let guild = match guild.load().await {
            Ok(mut g) => g.take(),
            Err(e) => {
                log::error!("Error fetching guild birthdays: {:?}", e);
                continue;
            }
        };
        for (date, users) in guild.iter() {
            log::debug!("Date: {:?} / Today {:?}, is date today? {:?}", date, today, *date == today);
            if *date == today {
                for user in users {
                    log::info!("Date: {:?} - User {:?}", date, user);
                    let r = channel
                        .send_message(&http, |m| {
                            m.content(format!("Parabens! {}", user.id.mention()))
                        })
                        .await;
                    if let Err(e) = r {
                        log::error!(
                            "Failed to send happy birthday to {:?} in {}: {:?}",
                            user,
                            channel,
                            e
                        );
                    }
                }
            }
        }
    }
    ControlFlow::CONTINUE
}
