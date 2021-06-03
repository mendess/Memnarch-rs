use tokio::sync::Mutex;
use std::sync::Arc;
use serenity::{prelude::TypeMapKey, CacheAndHttp };

daemons::monomorphise!(CacheAndHttp);

impl TypeMapKey for DaemonManager {
    type Value = Arc<Mutex<DaemonManager>>;
}
