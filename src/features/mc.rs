use crate::{
    in_files,
    util::daemons::{DaemonManager, cache_and_http},
};
use anyhow::Context;
use daemons::{Daemon, async_trait};
use json_db::GlobalDatabase;
use mappable_rc::Marc;
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, EditChannel, Http};
use std::{
    collections::HashMap, io, net::ToSocketAddrs, ops::ControlFlow, sync::Arc, time::Duration,
};
use tokio::{sync::Mutex, time::timeout};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct Update {
    ch_name: String,
    ch_topic: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrackedServer {
    addr: String,
    #[serde(default)]
    prev_update: Option<Update>,
}

static CHANNELS: GlobalDatabase<HashMap<ChannelId, TrackedServer>> =
    GlobalDatabase::new(in_files!("mc-checker.json"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct McChecker(bool);

pub async fn initialize(manager: &Marc<Mutex<DaemonManager>>) -> io::Result<()> {
    manager.lock().await.add_daemon(McChecker(false)).await;
    Ok(())
}

#[async_trait]
impl Daemon<true> for McChecker {
    type Data = (Arc<serenity::cache::Cache>, Arc<Http>);

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        self.0 = true;
        let mut channels = match CHANNELS.load().await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = ?e, "failed to load mc-checker config");
                return ControlFlow::Continue(());
            }
        };
        for (cid, server) in channels.iter_mut() {
            if let Err(e) = run(data, *cid, server).await {
                tracing::error!(error = ?e, "failed to update server info")
            }
        }
        ControlFlow::Continue(())
    }

    async fn interval(&self) -> Duration {
        if self.0 {
            Duration::from_mins(10)
        } else {
            Duration::ZERO
        }
    }

    async fn name(&self) -> String {
        "mc-checker".into()
    }
}

#[tracing::instrument(skip(data))]
async fn run(
    data: &<McChecker as Daemon<true>>::Data,
    cid: ChannelId,
    server: &mut TrackedServer,
) -> anyhow::Result<()> {
    const ONLINE_EMOJI: &str = "-🟢";
    const OFFLINE_EMOJI: &str = "-🔴";
    const PLAYER_COUNT_EMOJIS: &[&str] = &[
        "",
        "-1️⃣",
        "-2️⃣",
        "-3️⃣",
        "-4️⃣",
        "-5️⃣",
        "-6️⃣",
        "-7️⃣",
        "-8️⃣",
        "-9️⃣",
        "-9️⃣➕",
    ];

    tracing::debug!("checking");
    let data = cache_and_http(data);
    let check_result = timeout(
        Duration::from_secs(30),
        mccli::fetch_server_info(
            server
                .addr
                .to_socket_addrs()?
                .next()
                .with_context(|| format!("no socket addresses for {}", server.addr))?,
        ),
    )
    .await
    .context("timed out pinging the server")
    .and_then(|r| r);

    let mut channel = cid
        .to_channel(data)
        .await?
        .guild()
        .context("channel is not a guild channel")?;

    let update = {
        let (name_emoji, topic) = match check_result {
            Ok(s) => (
                format!(
                    "{ONLINE_EMOJI}{}",
                    PLAYER_COUNT_EMOJIS
                        [(s.players.online as usize).clamp(0, PLAYER_COUNT_EMOJIS.len() - 1)]
                ),
                format!(
                    "server is online with {} players | https://minecraft.mendess.xyz",
                    s.players.online
                ),
            ),
            Err(e) => (
                OFFLINE_EMOJI.to_string(),
                format!("server is offline because: {e} | https://minecraft.mendess.xyz"),
            ),
        };

        let channel_name = channel.name();
        let new_name = if let Some(end) = channel_name.find(ONLINE_EMOJI) {
            format!("{}{name_emoji}", &channel_name[..end])
        } else if let Some(end) = channel_name.find(OFFLINE_EMOJI) {
            format!("{}{name_emoji}", &channel_name[..end])
        } else {
            format!("{channel_name}{name_emoji}")
        };
        Update {
            ch_name: new_name,
            ch_topic: topic,
        }
    };

    if server.prev_update.as_ref() != Some(&update) {
        tracing::debug!("changing channel to {update:?}");
        channel
            .edit(
                data,
                EditChannel::new()
                    .name(&update.ch_name)
                    .topic(&update.ch_topic),
            )
            .await?;

        server.prev_update = Some(update);
    }

    Ok(())
}
