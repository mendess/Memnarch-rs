use chrono::{NaiveDateTime, NaiveTime, Utc};
use daemons::{ControlFlow, Daemon};
use futures::Future;

pub struct Cron<F, Fut, const H: u32, const M: u32, const S: u32>
where
    F: FnMut(&serenity::CacheAndHttp) -> Fut,
    Fut: Future<Output = ControlFlow>,
{
    name: String,
    run: F,
}

impl<F, Fut, const H: u32, const M: u32, const S: u32> Cron<F, Fut, H, M, S>
where
    F: FnMut(&serenity::CacheAndHttp) -> Fut,
    Fut: Future<Output = ControlFlow>,
{
    pub fn new(name: String, run: F) -> Self {
        Self { name, run }
    }
}

#[serenity::async_trait]
impl<F, Fut, const H: u32, const M: u32, const S: u32> Daemon for Cron<F, Fut, H, M, S>
where
    F: FnMut(&serenity::CacheAndHttp) -> Fut + Send + Sync,
    Fut: Future<Output = ControlFlow> + Send,
{
    type Data = serenity::CacheAndHttp;

    async fn interval(&self) -> std::time::Duration {
        let now = Utc::now().naive_utc();
        let midnight = NaiveDateTime::new(now.date().succ(), NaiveTime::from_hms(H, M, S));
        let dur = (midnight - now).to_std().unwrap_or_default();
        log::trace!(
            "cron task {} will happen in {}",
            self.name,
            humantime::format_duration(dur)
        );
        dur
    }

    async fn name(&self) -> String {
        self.name.clone()
    }

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        (self.run)(data).await
    }
}
