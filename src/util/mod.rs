pub mod consts;
pub mod daemons;
pub mod permissions;

use futures::future::TryFutureExt;
use serenity::{
    all::{ChannelId, Mention, RoleId},
    http::Http,
    model::id::UserId,
};
use std::sync::atomic::{AtomicU64, Ordering};

static BOT_ID: AtomicU64 = AtomicU64::new(0);

pub trait MentionExt {
    fn into_user(self) -> Result<UserId, &'static str>;
    fn into_role(self) -> Result<RoleId, &'static str>;
    fn into_channel(self) -> Result<ChannelId, &'static str>;
}

impl MentionExt for Mention {
    fn into_user(self) -> Result<UserId, &'static str> {
        match self {
            Self::User(uid) => Ok(uid),
            Self::Role(_) => Err("expected user mention got role mention"),
            Self::Channel(_) => Err("expected user mention got channel mention"),
        }
    }
    fn into_role(self) -> Result<RoleId, &'static str> {
        match self {
            Self::Role(rid) => Ok(rid),
            Self::User(_) => Err("expected role mention got user mention"),
            Self::Channel(_) => Err("expected role mention got channel mention"),
        }
    }
    fn into_channel(self) -> Result<ChannelId, &'static str> {
        match self {
            Self::Channel(cid) => Ok(cid),
            Self::User(_) => Err("expected channel mention got user mention"),
            Self::Role(_) => Err("expected channel mention got role mention"),
        }
    }
}

pub async fn bot_id(http: impl AsRef<Http>) -> Option<UserId> {
    match BOT_ID.load(Ordering::Relaxed) {
        0 => {
            match http
                .as_ref()
                .get_current_application_info()
                .inspect_err(|e| tracing::error!("failed to get app info: {:?}", e))
                .map_ok(|x| x.id)
                .await
                .ok()
            {
                Some(uid) => {
                    BOT_ID.store(uid.get(), Ordering::Relaxed);
                    Some(UserId::new(uid.get()))
                }
                None => None,
            }
        }
        x => Some(UserId::new(x)),
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
