use json_db::{Database, GlobalDatabase};
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId, RoleId};
use std::{collections::HashMap, io};

static GUILD_PREFS: GlobalDatabase<HashMap<GuildId, GuildPrefs>> =
    Database::const_new("files/guild_prefs.json");

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct GuildPrefs {
    #[serde(default)]
    pub birthday_channel: Option<ChannelId>,
    #[serde(default)]
    pub birthday_role: Option<RoleId>,
    #[serde(default)]
    pub cursed: bool,
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
