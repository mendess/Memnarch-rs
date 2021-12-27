use futures::future::TryFutureExt;
use serenity::{http::Http, model::id::UserId};
use std::{
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
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

static LOCK_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct LockCtx {
    id: usize,
    file: &'static str,
    line: u32,
}

impl LockCtx {
    fn new(file: &'static str, line: u32) -> LockCtx {
        Self {
            id: LOCK_ID.fetch_add(1, Ordering::Relaxed),
            file,
            line
        }
    }
}

pub struct Mutex<T> {
    ctx: LockCtx,
    l: tokio::sync::Mutex<T>,
}

impl<T> Mutex<T> {
    pub fn new(t: T, file: &'static str, line: u32) -> Self {
        let ctx = LockCtx::new(file, line);
        log::trace!("creating mutex {:?}", ctx);
        Self {
            ctx,
            l: tokio::sync::Mutex::new(t),
        }
    }

    pub async fn lock(&self) -> MutexGuard<'_, T> {
        log::warn!(
            "{:?} locking lock over type {}",
            self.ctx,
            std::any::type_name::<T>()
        );
        MutexGuard {
            ctx: self.ctx,
            g: self.l.lock().await,
        }
    }

    pub fn try_lock(&self) -> Result<MutexGuard<'_, T>, tokio::sync::TryLockError> {
        self.l.try_lock().map(|g| MutexGuard {
            ctx: self.ctx,
            g,
        })
    }
}

pub struct MutexGuard<'l, T> {
    ctx: LockCtx,
    g: tokio::sync::MutexGuard<'l, T>,
}

impl<'l, T> Deref for MutexGuard<'l, T> {
    type Target = <tokio::sync::MutexGuard<'l, T> as Deref>::Target;

    fn deref(&self) -> &Self::Target {
        self.g.deref()
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.g.deref_mut()
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        log::debug!(
            "{:?} unlocking lock over type {}",
            self.ctx,
            std::any::type_name::<T>()
        );
    }
}

pub struct RwLock<T> {
    ctx: LockCtx,
    l: tokio::sync::RwLock<T>,
}

impl<T> RwLock<T> {
    pub fn new(t: T, file: &'static str, line: u32) -> Self {
        let ctx = LockCtx::new(file, line);
        log::trace!("creating rw lock {:?}", ctx);
        Self {
            ctx,
            l: tokio::sync::RwLock::new(t),
        }
    }

    pub async fn write(&self) -> RwLockWriteGuard<'_, T> {
        log::warn!(
            "{:?} write locking over type {}",
            self.ctx,
            std::any::type_name::<T>()
        );
        RwLockWriteGuard {
            ctx: self.ctx,
            g: self.l.write().await,
        }
    }

    pub async fn read(&self) -> RwLockReadGuard<'_, T> {
        log::warn!(
            "{:?} read locking over type {}",
            self.ctx,
            std::any::type_name::<T>()
        );
        RwLockReadGuard {
            ctx: self.ctx,
            g: self.l.read().await,
        }
    }
}

pub struct RwLockWriteGuard<'l, T> {
    ctx: LockCtx,
    g: tokio::sync::RwLockWriteGuard<'l, T>,
}

impl<'l, T> Deref for RwLockWriteGuard<'l, T> {
    type Target = <tokio::sync::RwLockWriteGuard<'l, T> as Deref>::Target;

    fn deref(&self) -> &Self::Target {
        self.g.deref()
    }
}

impl<'l, T> DerefMut for RwLockWriteGuard<'l, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.g.deref_mut()
    }
}

impl<T> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        log::debug!(
            "{:?} unlocking write lock over type {}",
            self.ctx,
            std::any::type_name::<T>()
        );
    }
}

pub struct RwLockReadGuard<'l, T> {
    ctx: LockCtx,
    g: tokio::sync::RwLockReadGuard<'l, T>,
}

impl<'l, T> Deref for RwLockReadGuard<'l, T> {
    type Target = <tokio::sync::RwLockReadGuard<'l, T> as Deref>::Target;

    fn deref(&self) -> &Self::Target {
        self.g.deref()
    }
}

impl<T> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        log::debug!(
            "{:?} unlocking write lock over type {}",
            self.ctx,
            std::any::type_name::<T>()
        );
    }
}
