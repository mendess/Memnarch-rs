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
    pub fn new(name: impl Into<String>, run: F) -> Self {
        Self {
            name: name.into(),
            run,
        }
    }
}

#[serenity::async_trait]
impl<F, Fut, const H: u32, const M: u32, const S: u32> Daemon<false> for Cron<F, Fut, H, M, S>
where
    F: FnMut(&serenity::CacheAndHttp) -> Fut + Send + Sync,
    Fut: Future<Output = ControlFlow> + Send,
{
    type Data = serenity::CacheAndHttp;

    async fn interval(&self) -> std::time::Duration {
        let now = Utc::now().naive_utc();
        let h_m_s = NaiveTime::from_hms_opt(H, M, S).expect("valid h m s");
        let mut target = NaiveDateTime::new(now.date(), h_m_s);
        if now > target {
            target = NaiveDateTime::new(
                target
                    .date()
                    .succ_opt()
                    .expect("not to reach the end of time"),
                target.time(),
            );
        }
        let dur = (target - now).to_std().unwrap_or_default();
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
