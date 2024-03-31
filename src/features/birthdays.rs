use std::{
    collections::BTreeMap,
    io::{self, Write},
    str::from_utf8,
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::Context;
use chrono::{Datelike, Local, Month, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use daemons::{ControlFlow, Daemon};
use futures::TryFutureExt;
use json_db::multifile_db::{FileKeySerializer, MultifileDb};
use serenity::{
    http::Http,
    model::id::{GuildId, UserId},
    prelude::Mentionable,
};
use tokio::sync::Mutex;

use crate::{
    prefs::guild as guild_prefs,
    util::daemons::{Cron, DaemonManager},
};

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

#[derive(Debug)]
struct Error {
    serializing: bool,
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    Io(io::Error),
    FileKeyParseError(String),
    Other(anyhow::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mode = if self.serializing {
            "serializing"
        } else {
            "deserializing"
        };
        match &self.kind {
            ErrorKind::Io(e) => writeln!(f, "io error while {mode}: {e:?}"),
            ErrorKind::FileKeyParseError(s) => {
                writeln!(f, "file name parse error while deserializing: {s}")
            }
            ErrorKind::Other(e) => writeln!(f, "other error while {mode}: {e:?}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self {
            serializing: false,
            kind: ErrorKind::Io(e),
        }
    }
}

impl std::error::Error for Error {}

struct GuildIdSerializer;
impl FileKeySerializer<GuildId> for GuildIdSerializer {
    type ParseError = Error;
    fn from_str(s: &str) -> Result<GuildId, Self::ParseError> {
        let Some((id, _)) = s.split_once('.') else {
            return Err(Error {
                serializing: false,
                kind: ErrorKind::FileKeyParseError(format!("key {s:?} didn't contain a ."))
            });
        };
        id.parse().map(GuildId).map_err(|e| Error {
            serializing: false,
            kind: ErrorKind::FileKeyParseError(format!("id {id:?} wasn't a valid guild id: {e:?}")),
        })
    }

    fn to_string(pk: GuildId) -> String {
        format!("{}.csv", pk.0)
    }
}

type BdayMap = MultifileDb<GuildId, GuildIdSerializer, BTreeMap<BDay, Vec<BDayBoy>>, Error>;

fn bday_map() -> &'static BdayMap {
    static BDAY_MAP: OnceLock<BdayMap> = OnceLock::new();
    BDAY_MAP.get_or_init(|| {
        MultifileDb::new_with_ser_and_deser(BASE, || Box::new(ser), || Box::new(deser))
    })
}

pub async fn initialize(d: &mut Arc<Mutex<DaemonManager>>) -> io::Result<()> {
    let dm = d.clone();
    d.lock()
        .await
        .add_daemon(BDayChecker::new("bday checker", move |c| {
            check_bday(c.http.clone(), dm.clone())
        }))
        .await;
    Ok(())
}

pub async fn next_bday(g: GuildId) -> anyhow::Result<Option<(BDay, Vec<BDayBoy>)>> {
    let tree = {
        match bday_map().get(&g).await? {
            None => return Ok(None),
            Some(b) => b.load().await?.take(),
        }
    };
    let tomorrow = BDay::from(
        Utc::now()
            .date_naive()
            .succ_opt()
            .expect("not reach the end of time"),
    );
    let next = match tree.range(tomorrow..).next() {
        Some((d, v)) => Some((*d, v.clone())),
        None => tree.iter().next().map(|(d, v)| (*d, v.clone())),
    };
    Ok(next)
}

pub async fn all(g: GuildId) -> anyhow::Result<BTreeMap<u32, Vec<(u32, BDayBoy)>>> {
    let database = match bday_map().get(&g).await? {
        None => return Ok(Default::default()),
        Some(b) => b.load().await?.take(),
    };
    Ok(database
        .into_iter()
        .fold(Default::default(), |mut acc, (d, users)| {
            acc.entry(d.month)
                .or_default()
                .extend(users.into_iter().map(|u| (d.day, u)));
            acc
        }))
}

pub async fn of(g: GuildId, user_id: UserId) -> anyhow::Result<Option<BDay>> {
    let database = match bday_map().get(&g).await? {
        None => return Ok(None),
        Some(b) => b.load().await?.take(),
    };
    Ok(database
        .iter()
        .find(|(_, users)| users.iter().any(|u| u.id == user_id))
        .map(|(date, _)| *date))
}

pub async fn of_month(
    g: GuildId,
    month: Month,
) -> anyhow::Result<Option<impl Iterator<Item = (BDay, BDayBoy)>>> {
    let database = match bday_map().get(&g).await? {
        None => return Ok(None),
        Some(b) => b.load().await?.take(),
    };
    Ok(Some(
        database
            .into_iter()
            .filter(move |(date, _)| date.month == month.number_from_month())
            .flat_map(|(date, users)| users.into_iter().map(move |u| (date, u))),
    ))
}

pub async fn add_bday(
    g: GuildId,
    who: UserId,
    when: NaiveDate,
) -> anyhow::Result<Option<NaiveDate>> {
    let calendar = bday_map().get_or_default(g).await?;
    let mut calendar = calendar.load().await?;
    let bday = BDay::from(when);
    let removed = remove_user(&mut calendar, who);
    calendar.entry(bday).or_default().push(BDayBoy {
        id: who,
        year: when.year(),
    });
    Ok(removed)
}

pub async fn remove_bday(g: GuildId, who: UserId) -> anyhow::Result<Option<NaiveDate>> {
    match bday_map().get(&g).await? {
        Some(calendar) => {
            let mut calendar = calendar.load().await?;
            Ok(remove_user(&mut calendar, who))
        }
        None => Ok(None),
    }
}

fn remove_user(tree: &mut BTreeMap<BDay, Vec<BDayBoy>>, user: UserId) -> Option<NaiveDate> {
    let mut when = None;
    tree.retain(
        |date, users| match users.iter().position(|u| u.id == user) {
            Some(index) => {
                let user = users.swap_remove(index);
                when = Some(
                    NaiveDate::from_ymd_opt(user.year, date.month, date.day)
                        .expect("formed from valid dates"),
                );
                !users.is_empty()
            }
            None => true,
        },
    );
    when
}

fn ser(w: &mut dyn Write, t: &BTreeMap<BDay, Vec<BDayBoy>>) -> Result<(), Error> {
    for (BDay { month, day }, v) in t {
        for BDayBoy { id, year } in v {
            writeln!(w, "{};{}-{}-{}", id, year, month, day).map_err(|e| Error {
                serializing: true,
                kind: ErrorKind::Io(e),
            })?
        }
    }
    Ok(())
}

fn deser(v: &[u8]) -> Result<BTreeMap<BDay, Vec<BDayBoy>>, Error> {
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
        .try_fold(BTreeMap::default(), |mut acc: BTreeMap<_, Vec<_>>, e| {
            let e = e.map_err(|e| Error {
                serializing: false,
                kind: ErrorKind::Other(e),
            })?;
            acc.entry(BDay::from(e.0))
                .or_default()
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
    let g = match bday_map().iter_guard().await {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("failed to load bdays for checking: {e:?}");
            return ControlFlow::CONTINUE;
        }
    };
    for (gid, guild) in g.iter() {
        tracing::trace!("processing birthdays for guild {}", gid);
        let (channel, role) = match guild_prefs::get(*gid)
            .await
            .map(|p| p.and_then(|p| p.birthday_channel.map(|ch| (ch, p.birthday_role))))
        {
            Ok(Some(ch)) => ch,
            Ok(None) => {
                tracing::error!("birthday channel not set for guild {}", gid);
                continue;
            }
            Err(e) => {
                tracing::error!("Error fetching guild prefs: {:?}", e);
                continue;
            }
        };
        let guild = match guild.load().await {
            Ok(mut g) => g.take(),
            Err(e) => {
                tracing::error!("Error fetching guild birthdays: {:?}", e);
                continue;
            }
        };
        for (date, users) in guild.iter() {
            if *date == today {
                tracing::debug!("Date: {:?} / Today {:?}", date, today);
                tracing::debug!(
                    "There are {} users having their birthday on {:?}",
                    users.len(),
                    date
                );
                for user in users {
                    tracing::info!("Date: {:?} - User {:?}", date, user);
                    let r = channel
                        .send_message(&http, |m| {
                            m.content(format!("Parabens! {}", user.id.mention()))
                        })
                        .await;
                    if let Err(e) = r {
                        tracing::error!(
                            "Failed to send happy birthday to {:?} in {}: {:?}",
                            user,
                            channel,
                            e
                        );
                    }
                    if let Some(role) = role {
                        let http = &http;
                        let r = gid
                            .member(&http, user.id)
                            .and_then(|mut m| async move { m.add_role(http, role).await })
                            .await;
                        if let Err(e) = r {
                            tracing::error!(
                                "failed to add birthday role({}) to user({}) in guild({}): {:?}",
                                role,
                                user.id,
                                gid,
                                e,
                            );
                        } else {
                            dm.lock()
                                .await
                                .add_daemon(UnBdayBoy {
                                    user: user.id,
                                    guild: *gid,
                                })
                                .await;
                        }
                    }
                }
            } else {
                tracing::debug!("Date {:?} is not today {:?}", date, today);
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
                    tracing::warn!("birthday role unconfigured");
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
            tracing::error!("failed to remove birthday role: {:?}", e)
        }
        ControlFlow::BREAK
    }

    async fn name(&self) -> String {
        format!("un birthday boys {:?}", self.user)
    }

    async fn interval(&self) -> Duration {
        let now = Local::now();
        let mid_night = NaiveDateTime::new(
            now.date_naive()
                .succ_opt()
                .expect("not to reach the end of time"),
            NaiveTime::from_hms_opt(0, 0, 0).expect("midnight exists"),
        );
        (mid_night - now.naive_utc()).to_std().unwrap_or_default()
    }
}
