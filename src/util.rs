use chrono::{NaiveDateTime, Utc};
use dashmap::DashSet;
use futures::future::TryFutureExt;
use serde::{Deserialize, Serialize};
use serenity::{http::Http, model::id::UserId};
use std::{
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU64, Ordering},
};

static BOT_ID: AtomicU64 = AtomicU64::new(0);

pub async fn bot_id(http: impl AsRef<Http>) -> Option<UserId> {
    match BOT_ID.load(Ordering::Relaxed) {
        0 => {
            match http
                .as_ref()
                .get_current_application_info()
                .inspect_err(|e| log::error!("failed to get app info: {:?}", e))
                .map_ok(|x| x.id)
                .await
                .ok()
            {
                Some(uid) => {
                    BOT_ID.store(uid.0, Ordering::Relaxed);
                    Some(uid)
                }
                None => None,
            }
        }
        x => Some(UserId(x)),
    }
}

pub mod tuple_map {
    pub trait TupleMap<A, B> {
        fn map_first<C, F>(self, f: F) -> (C, B)
        where
            F: FnMut(A) -> C;

        fn map_snd<C, F>(self, f: F) -> (A, C)
        where
            F: FnMut(B) -> C;

        fn map_both<F, G, C, D>(self, f: F, g: G) -> (C, D)
        where
            F: FnMut(A) -> C,
            G: FnMut(B) -> D;
    }

    impl<A, B> TupleMap<A, B> for (A, B) {
        fn map_first<C, F>(self, mut f: F) -> (C, B)
        where
            F: FnMut(A) -> C,
        {
            (f(self.0), self.1)
        }

        fn map_snd<C, F>(self, mut f: F) -> (A, C)
        where
            F: FnMut(B) -> C,
        {
            (self.0, f(self.1))
        }

        fn map_both<F, G, C, D>(self, mut f: F, mut g: G) -> (C, D)
        where
            F: FnMut(A) -> C,
            G: FnMut(B) -> D,
        {
            (f(self.0), g(self.1))
        }
    }

    pub fn tuple_map_both<A, F, AL>((a, b): (A, A), mut f: F) -> (AL, AL)
    where
        F: FnMut(A) -> AL,
    {
        (f(a), f(b))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Copy, Clone)]
pub enum LockKind {
    Mutex,
    Read,
    Write,
}

impl LockKind {
    pub fn from_str(s: &str) -> Self {
        match s {
            "lock" => Self::Mutex,
            "write" => Self::Write,
            "read" => Self::Read,
            _ => panic!("cant from_str {}", s),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LockLogs {
    locks: DashSet<String>,
}

lazy_static::lazy_static! {
    static ref LOCKS: LockLogs = match std::fs::File::open("files/locks.json") {
        Ok(file) => serde_json::from_reader::<_, LockLogs>(file).unwrap_or_default(),
        Err(_) => LockLogs::default(),
    };
}

pub fn lock(kind: LockKind, file: &'static str, line: u32) -> LockCtx {
    let when = Utc::now().naive_utc();
    let ctx = LockCtx {
        kind,
        file,
        line,
        when,
    };
    log::trace!("LOCKING {:?}", ctx);
    LOCKS.locks.insert(format!("{:?}", ctx));
    serde_json::to_writer(
        std::fs::File::create("files/locks.json").unwrap(),
        &*LOCKS,
    )
    .unwrap();
    ctx
}

fn lock_drop(ctx: LockCtx) {
    log::trace!("UNLOCKING {:?}", ctx);
    LOCKS.locks.remove(&format!("{:?}", ctx));
    serde_json::to_writer(
        std::fs::File::create("files/locks.json").unwrap(),
        &*LOCKS,
    )
    .unwrap();
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct LockCtx {
    pub kind: LockKind,
    pub file: &'static str,
    pub line: u32,
    pub when: NaiveDateTime,
}

#[macro_export]
macro_rules! log_lock {
    ($lock:expr, $kind:ident) => {
        $crate::util::LogDrop {
            ctx: $crate::util::lock(
                $crate::util::LockKind::from_str(::std::stringify!($kind)),
                file!(),
                line!(),
            ),
            t: ::tokio::time::timeout(::std::time::Duration::from_secs(10), $lock.$kind())
                .await
                .expect("lock took too long to unlock"),
        }
    };
}

#[macro_export]
macro_rules! log_lock_mutex {
    ($lock:expr) => {
        $crate::log_lock!($lock, lock)
    };
}

#[macro_export]
macro_rules! log_lock_read {
    ($lock:expr) => {
        $crate::log_lock!($lock, read)
    };
}

#[macro_export]
macro_rules! log_lock_write {
    ($lock:expr) => {
        $crate::log_lock!($lock, write)
    };
}

pub struct LogDrop<T> {
    pub ctx: LockCtx,
    pub t: T,
}

impl<T> Deref for LogDrop<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.t
    }
}

impl<T> DerefMut for LogDrop<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.t
    }
}

impl<T> Drop for LogDrop<T> {
    fn drop(&mut self) {
        lock_drop(self.ctx);
    }
}
