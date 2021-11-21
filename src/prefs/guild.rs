use crate::file_transaction::Database;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId, RoleId};
use std::{collections::HashMap, io};

lazy_static! {
    static ref GUILD_PREFS: Database<HashMap<GuildId, GuildPrefs>> =
        Database::new("files/guild_prefs.json");
}

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
