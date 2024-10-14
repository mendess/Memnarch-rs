use json_db::GlobalDatabase;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId, RoleId};
use std::{collections::HashMap, io};

use crate::in_files;

static GUILD_PREFS: GlobalDatabase<HashMap<GuildId, GuildPrefs>> =
    GlobalDatabase::new(in_files!("guild_prefs.json"));

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct GuildPrefs {
    #[serde(default)]
    pub birthday_channel: Option<ChannelId>,
    #[serde(default)]
    pub birthday_role: Option<RoleId>,
}

pub async fn get(u: GuildId) -> io::Result<Option<GuildPrefs>> {
    Ok(GUILD_PREFS.load().await?.get(&u).cloned())
}

pub async fn update<F, R>(u: GuildId, mut f: F) -> io::Result<R>
where
    F: FnMut(&mut GuildPrefs) -> R,
{
    Ok(f(GUILD_PREFS.load().await?.entry(u).or_default()))
}
