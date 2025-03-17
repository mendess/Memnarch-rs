use crate::{
    in_files,
    util::daemons::{DaemonManager, cache_and_http},
};
use anyhow::Context;
use daemons::{ControlFlow, Daemon, async_trait};
use json_db::GlobalDatabase;
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, Colour, CreateEmbed, CreateMessage, Http, MessageId};
use std::{collections::HashMap, io, net::ToSocketAddrs, sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::timeout};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
enum LastKnownState {
    Online,
    #[default]
    Offline,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrackedServer {
    addr: String,
    #[serde(default)]
    last_state: LastKnownState,
    #[serde(default)]
    last_message: Option<MessageId>,
}

static CHANNELS: GlobalDatabase<HashMap<ChannelId, TrackedServer>> =
    GlobalDatabase::new(in_files!("mc-checker.json"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct McChecker(bool);

pub async fn initialize(manager: &mut Arc<Mutex<DaemonManager>>) -> io::Result<()> {
    manager.lock().await.add_daemon(McChecker(false)).await;
    Ok(())
}

#[async_trait]
impl Daemon<true> for McChecker {
    type Data = (Arc<serenity::cache::Cache>, Arc<Http>);

    async fn run(&mut self, data: &Self::Data) -> ControlFlow {
        self.0 = true;
        if let Err(e) = run(data).await {
            tracing::error!(error = ?e, "failed to update server info")
        }
        ControlFlow::CONTINUE
    }

    async fn interval(&self) -> Duration {
        if self.0 {
            Duration::from_secs(60 * 10)
        } else {
            Duration::ZERO
        }
    }

    async fn name(&self) -> String {
        "mc-checker".into()
    }
}

async fn run(data: &<McChecker as Daemon<true>>::Data) -> anyhow::Result<()> {
    for (c, server) in CHANNELS.load().await?.iter_mut() {
        let TrackedServer {
            addr,
            last_state,
            last_message,
        } = server;
        let msg = match timeout(
            Duration::from_secs(30),
            mccli::fetch_server_info(
                addr.to_socket_addrs()?
                    .next()
                    .with_context(|| format!("no socket addresses for {addr}"))?,
            ),
        )
        .await
        .context("timed out pinging the server")
        .and_then(|r| r)
        {
            Ok(_) if *last_state != LastKnownState::Online => {
                *last_state = LastKnownState::Online;
                CreateMessage::new().embed(
                    CreateEmbed::new()
                        .color(Colour::DARK_GREEN)
                        .title("server is online ✅"),
                )
            }
            Err(e) if *last_state != LastKnownState::Offline => {
                *last_state = LastKnownState::Offline;
                let mut reason = e.to_string();
                reason.truncate(1024);
                CreateMessage::new().embed(
                    CreateEmbed::new()
                        .color(Colour::RED)
                        .title("server is offline ❌")
                        .field("why?", reason, true),
                )
            }
            _ => return Ok(()),
        };

        if let Some(last_message) = last_message.take() {
            if let Err(e) = c.delete_message(&data.1, last_message).await {
                tracing::error!(error = ?e, "failed to delete last message");
            }
        }
        *last_message = Some(c.send_message(cache_and_http(data), msg).await?.id);
    }

    Ok(())
}
