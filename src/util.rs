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
    // pub fn tuple_map<A, B, F1, F2, AL, BL>((a, b): (A, B), mut f: F1, mut g: F2) -> (AL, BL)
    // where
    //     F1: FnMut(A) -> AL,
    //     F2: FnMut(B) -> BL,
    // {
    //     (f(a), g(b))
    // }

    pub fn tuple_map_both<A, F, AL>((a, b): (A, A), mut f: F) -> (AL, AL)
    where
        F: FnMut(A) -> AL,
    {
        (f(a), f(b))
    }
}
