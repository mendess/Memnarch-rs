use futures::future::TryFutureExt;
use serenity::{http::Http, model::id::UserId};
use std::sync::atomic::{AtomicU64, Ordering};

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
