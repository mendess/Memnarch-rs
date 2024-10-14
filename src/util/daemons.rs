use serenity::{all::Http, cache::Cache, prelude::TypeMapKey};
use std::sync::Arc;
use tokio::sync::Mutex;

daemons::monomorphise!((Arc<Cache>, Arc<Http>));

pub fn cache_and_http(pair: &(Arc<Cache>, Arc<Http>)) -> (&Arc<Cache>, &Http) {
    (&pair.0, &*pair.1)
}

pub struct DaemonManagerKey;

impl TypeMapKey for DaemonManagerKey {
    type Value = Arc<Mutex<DaemonManager>>;
}
