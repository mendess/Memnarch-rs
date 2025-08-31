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
    pub daemons: Arc<Mutex<DaemonManager>>,
    pub leave_voice: Mutex<LeaveVoiceDaemons>,
    pub quotes: Mutex<quotes::QuoteManager>,
}

macro_rules! try_init {
    ($d:expr, $($m:ident)::*) => {
        if let std::result::Result::Err(e) = $($m)::*::initialize(&mut $d).await {
            tracing::error!("Failed to initialize {}: {:?}", stringify!($($m)::*), e);
        } else {
            tracing::info!("{} initialized!", stringify!($($m)::*));
        }
    };
}

impl Bot {
    pub async fn init(ctx: &serenity::all::Context) -> anyhow::Result<Self> {
        let mut daemon_manager =
            DaemonManager::spawn(Arc::new((ctx.cache.clone(), ctx.http.clone())));
        features::reminders::load_reminders(&mut daemon_manager)
            .await
            .context("loading reminders")?;
        features::moderation::reaction_roles::initialize().await?;
        features::music_channel_broadcast::initialize().await;
        features::disconnect_channel::initialize().await;
        let mut daemon_manager = Arc::new(Mutex::new(daemon_manager));
        try_init!(daemon_manager, features::birthdays);
        try_init!(daemon_manager, features::mtg_spoilers);
        try_init!(daemon_manager, features::mc);

        Ok(Bot {
            daemons: daemon_manager,
            leave_voice: Default::default(),
            quotes: Mutex::new(quotes::QuoteManager::load().await?),
        })
    }
}

#[macro_export]
macro_rules! get {
    ($ctx:ident, $t:ty) => {
        $ctx.data.read().await.get::<$t>().expect(::std::concat!(
            ::std::stringify!($t),
            " was not initialized"
        ))
    };
    (mut $ctx:ident, $t:ty) => {
        $ctx.data
            .write()
            .await
            .expect("lock took too long")
            .get_mut::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
    };
    ($ctx:ident, $t:ty, $lock:ident) => {
        $ctx.data
            .read()
            .await
            .get::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
            .$lock()
            .await
    };
    (mut $ctx:ident, $t:ty, $lock:ident) => {
        $ctx.data
            .write()
            .await
            .get_mut::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
            .$lock()
            .await
    };
    (> $data:ident, $t:ty) => {
        $data.get::<$t>().expect(::std::concat!(
            ::std::stringify!($t),
            " was not initialized"
        ))
    };
    (> $data:ident, $t:ty, $lock:ident) => {
        $data
            .get::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
            .$lock()
            .await
    };
}
