use daemons::Daemon;
use lazy_static::lazy_static;
use log::*;
use procinfo::pid::{statm_self, Statm};
use serenity::prelude::Mutex;
use serenity::{model::id::ChannelId, CacheAndHttp};
use std::time::Duration;
use std::{
    cmp::Ordering,
    fmt::{self, Display},
    sync::atomic::{self, AtomicBool, AtomicUsize},
};

pub struct HealthMonitor {
    log_channel: ChannelId,
    first: AtomicBool,
}

impl HealthMonitor {
    pub fn new(log_channel: ChannelId) -> Self {
        Self {
            log_channel,
            first: AtomicBool::new(true),
        }
    }
}

lazy_static! {
    static ref LAST_MEASURE: Mutex<Option<Statm>> = Mutex::new(None);
}

static ALLOWED_SKIPS: AtomicUsize = AtomicUsize::new(0);

#[daemons::async_trait]
impl Daemon<true> for HealthMonitor {
    type Data = CacheAndHttp;

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        match statm_self() {
            Ok(new) => {
                debug!("Memory usage: {:?}", new);
                let diff = match &*LAST_MEASURE.lock().await {
                    Some(old) => Diff::new(old, &new),
                    None => Diff::new(&new, &new),
                };
                if diff.changed() || ALLOWED_SKIPS.load(atomic::Ordering::Relaxed) == 0 {
                    ALLOWED_SKIPS.store(5, atomic::Ordering::Relaxed);
                    let res = self
                        .log_channel
                        .send_message(&*data.http, |m| {
                            m.content(format!("**[Memory usage]** {}", diff))
                        })
                        .await;
                    if let Err(e) = res {
                        error!("Failed to send message to log channel: {}", e);
                    } else {
                        *LAST_MEASURE.lock().await = Some(new);
                    }
                } else {
                    ALLOWED_SKIPS.fetch_sub(1, atomic::Ordering::Relaxed);
                };
            }
            Err(e) => error!("Error fetching memory usage: {}", e),
        }
        daemons::ControlFlow::CONTINUE.into()
    }

    async fn name(&self) -> String {
        String::from("HealthMonitor")
    }

    async fn interval(&self) -> Duration {
        Duration::from_secs(60 * 60 * (!self.first.swap(false, atomic::Ordering::SeqCst)) as u64)
    }
}

#[derive(Clone, Copy, Debug)]
struct Changes {
    size: Ordering,
    resident: Ordering,
    share: Ordering,
    text: Ordering,
    data: Ordering,
}

#[derive(Clone, Copy, Debug)]
struct Diff<'n> {
    new: &'n Statm,
    changes: Changes,
}

impl<'n> Diff<'n> {
    fn new(old: &Statm, new: &'n Statm) -> Self {
        Self {
            new,
            changes: Changes {
                size: new.size.cmp(&old.size),
                resident: new.resident.cmp(&old.resident),
                share: new.share.cmp(&old.share),
                text: new.text.cmp(&old.text),
                data: new.text.cmp(&old.text),
            },
        }
    }

    fn changed(&self) -> bool {
        let s = &self.changes;
        [s.size, s.resident, s.share, s.text, s.data]
            .iter()
            .any(|i| !matches!(i, Ordering::Equal))
    }
}

impl<'n> Display for Diff<'n> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        macro_rules! compare {
            ($self:expr, $field:ident, $f:ident) => {
                let c = match $self.changes.$field {
                    Ordering::Equal => ":blue_square:",
                    Ordering::Less => ":green_square:",
                    Ordering::Greater => ":red_square:",
                };
                ::std::write!(
                    $f,
                    "{}: {} {} **|** ",
                    ::std::stringify!($field),
                    $self.new.$field,
                    c
                )?;
            };
        }
        compare!(self, size, f);
        compare!(self, resident, f);
        compare!(self, share, f);
        compare!(self, text, f);
        compare!(self, data, f);
        Ok(())
    }
}
