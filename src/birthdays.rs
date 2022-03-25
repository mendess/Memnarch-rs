use std::{
    collections::BTreeMap, io::Write, os::unix::prelude::OsStrExt, path::PathBuf, str::from_utf8,
    sync::Arc, time::Duration,
};

use anyhow::Context;
use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use daemons::{ControlFlow, Daemon};
use dashmap::DashMap;
use futures::TryFutureExt;
use lazy_static::lazy_static;
use serenity::{
    http::Http,
    model::id::{GuildId, UserId},
    prelude::Mentionable,
};
use tokio::{fs, io, sync::Mutex};

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

pub async fn initialize(d: &mut Arc<Mutex<DaemonManager>>) -> io::Result<()> {
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
    let dm = d.clone();
    crate::log_lock_mutex!(d)
        .add_daemon(BDayChecker::new("bday checker", move |c| {
            check_bday(c.http.clone(), dm.clone())
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
    let mut map = map.load(file!(), line!()).await?;
    let tree = map.take();
    let next = match tree.range(tomorrow..).next() {
        Some((d, v)) => Some((*d, v.clone())),
        None => tree.iter().next().map(|(d, v)| (*d, v.clone())),
    };
    Ok(next)
}

pub async fn of(g: GuildId, user_id: UserId) -> anyhow::Result<Option<BDay>> {
    let map = match BDAY_MAP.get(&g) {
        None => return Ok(None),
        Some(b) => b,
    };
    let database = map.load(file!(), line!()).await?;
    Ok(database
        .iter()
        .find(|(_, users)| users.iter().any(|u| u.id == user_id))
        .map(|(date, _)| *date))
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
    let mut calendar = calendar.load(file!(), line!()).await?;
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
    let mut calendar = calendar.load(file!(), line!()).await?;
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

async fn check_bday(http: Arc<Http>, dm: Arc<Mutex<DaemonManager>>) -> ControlFlow {
    let today = BDay::from(Utc::now().naive_utc().date());
    for x in BDAY_MAP.iter() {
        let (gid, guild) = (x.key(), x.value());
        log::trace!("processing birthdays for guild {}", gid);
        let (channel, role) = match guild_prefs::get(*gid)
            .await
            .map(|p| p.and_then(|p| p.birthday_channel.map(|ch| (ch, p.birthday_role))))
        {
            Ok(Some(ch)) => ch,
            Ok(None) => {
                log::error!("birthday channel not set for guild {}", gid);
                continue;
            }
            Err(e) => {
                log::error!("Error fetching guild prefs: {:?}", e);
                continue;
            }
        };
        let guild = match guild.load(file!(), line!()).await {
            Ok(mut g) => g.take(),
            Err(e) => {
                log::error!("Error fetching guild birthdays: {:?}", e);
                continue;
            }
        };
        for (date, users) in guild.iter() {
            if *date == today {
                log::debug!("Date: {:?} / Today {:?}", date, today);
                log::debug!(
                    "There are {} users having their birthday on {:?}",
                    users.len(),
                    date
                );
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
                    if let Some(role) = role {
                        let http = &http;
                        let r = gid
                            .member(http, user.id)
                            .and_then(|mut m| async move { m.add_role(http, role).await })
                            .await;
                        if let Err(e) = r {
                            log::error!(
                                "failed to add birthday role({}) to user({}) in guild({}): {:?}",
                                role,
                                user.id,
                                gid,
                                e,
                            );
                        } else {
                            crate::log_lock_mutex!(dm)
                                .add_daemon(UnBdayBoy {
                                    user: user.id,
                                    guild: *gid,
                                })
                                .await;
                        }
                    }
                }
            } else {
                log::debug!("Date {:?} is not today {:?}", date, today);
            }
        }
    }
    ControlFlow::CONTINUE
}

struct UnBdayBoy {
    user: UserId,
    guild: GuildId,
}

#[serenity::async_trait]
impl Daemon<false> for UnBdayBoy {
    type Data = serenity::CacheAndHttp;

    async fn run(&mut self, data: &Self::Data) -> ControlFlow {
        async fn _r(this: &mut UnBdayBoy, data: &serenity::CacheAndHttp) -> anyhow::Result<()> {
            let role = match guild_prefs::get(this.guild)
                .await?
                .and_then(|p| p.birthday_role)
            {
                Some(bday_role) => bday_role,
                None => {
                    log::warn!("birthday role unconfigured");
                    return Ok(());
                }
            };
            this.guild
                .member(data, this.user)
                .await?
                .remove_role(&data.http, role)
                .await?;
            Ok(())
        }
        if let Err(e) = _r(self, data).await {
            log::error!("failed to remove birthday role: {:?}", e)
        }
        ControlFlow::BREAK
    }

    async fn name(&self) -> String {
        format!("un birthday boys {:?}", self.user)
    }

    async fn interval(&self) -> Duration {
        let now = Utc::now();
        let mid_night =
            NaiveDateTime::new(now.date().succ().naive_utc(), NaiveTime::from_hms(0, 0, 0));
        (mid_night - now.naive_utc()).to_std().unwrap_or_default()
    }
}
