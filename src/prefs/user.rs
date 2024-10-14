use json_db::GlobalDatabase;
use serde::{Deserialize, Serialize};
use serenity::model::id::UserId;
use std::{collections::HashMap, io};

use crate::in_files;

static USER_PREFS: GlobalDatabase<HashMap<UserId, UserPrefs>> =
    GlobalDatabase::new(in_files!("user_prefs.json"));

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct UserPrefs {
    #[serde(default)]
    pub timezone_offset: Option<i64>,
}

pub async fn get(u: UserId) -> io::Result<Option<UserPrefs>> {
    Ok(USER_PREFS.load().await?.get(&u).cloned())
}

pub async fn update<F, R>(u: UserId, mut f: F) -> io::Result<R>
where
    F: FnMut(&mut UserPrefs) -> R,
{
    Ok(f(USER_PREFS.load().await?.entry(u).or_default()))
}
