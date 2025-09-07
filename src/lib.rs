#![warn(unused_features)]

pub mod commands;
pub mod features;
pub mod prefs;
pub mod util;

use toml as _;
use tracing_subscriber as _;

use features::{birthdays, moderation, mtg_spoilers, quotes, reminders};

use serde::{Deserialize, Serialize};
use serenity::{model::id::ChannelId, prelude::*};

use crate::{commands::sfx::util::LeaveVoiceDaemons, util::daemons::DaemonManager};
use anyhow::Context as _;
use mappable_rc::Marc;
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub token: String,
    pub monitor_log_channel: Option<ChannelId>,
}

impl Config {
    pub fn new(token: String) -> Self {
        Self {
            token,
            monitor_log_channel: None,
        }
    }
}

#[derive(Debug)]
pub struct Bot {
    pub daemons: Mutex<DaemonManager>,
    pub leave_voice: Mutex<LeaveVoiceDaemons>,
    pub quotes: Mutex<quotes::QuoteManager>,
}

impl TypeMapKey for Bot {
    type Value = Marc<Self>;
}

macro_rules! try_init {
    ($($m:ident)::*, $d:expr) => {
        if let std::result::Result::Err(e) = $($m)::*::initialize(&$d).await {
            tracing::error!("Failed to initialize {}: {:?}", stringify!($($m)::*), e);
        } else {
            tracing::info!("{} initialized!", stringify!($($m)::*));
        }
    };
}

impl Bot {
    pub async fn init(ctx: &serenity::all::Context) -> anyhow::Result<Marc<Self>> {
        let mut daemon_manager =
            DaemonManager::spawn(Arc::new((ctx.cache.clone(), ctx.http.clone())));
        features::reminders::load_reminders(&mut daemon_manager)
            .await
            .context("loading reminders")?;
        features::moderation::reaction_roles::initialize()
            .await
            .context("initializing reaction roles")?;
        features::music_channel_broadcast::initialize().await;
        features::disconnect_channel::initialize().await;

        let this = Marc::new(Bot {
            daemons: Mutex::new(daemon_manager),
            leave_voice: Default::default(),
            quotes: Mutex::new(
                quotes::QuoteManager::load()
                    .await
                    .context("loading quotes")?,
            ),
        });

        {
            let daemons = Marc::map(this.clone(), |b| &b.daemons);
            try_init!(features::birthdays, daemons);
            try_init!(features::mtg_spoilers, daemons);
            try_init!(features::mc, daemons);
        }

        Ok(this)
    }
}
