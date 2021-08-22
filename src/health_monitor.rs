use daemons::Daemon;
use log::*;
use procinfo::pid::statm_self;
use serenity::{model::id::ChannelId, CacheAndHttp};

pub struct HealthMonitor {
    log_channel: ChannelId,
}

impl HealthMonitor {
    pub fn new(log_channel: ChannelId) -> Self {
        Self { log_channel }
    }
}

#[daemons::async_trait]
impl Daemon for HealthMonitor {
    type Data = CacheAndHttp;

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        match statm_self() {
            Ok(s) => {
                debug!("Memory usage: {:?}", s);
                let res = self
                    .log_channel
                    .send_message(&*data.http, |m| m.content(format!("Memory usage: {:?}", s)))
                    .await;
                if let Err(e) = res {
                    error!("Failed to send message to log channel: {}", e);
                }
            }
            Err(e) => error!("Error fetching memory usage: {}", e),
        }
        daemons::ControlFlow::CONTINUE.into()
    }

    async fn name(&self) -> String {
        String::from("HealthMonitor")
    }

    async fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60 * 60)
    }
}
