use daemons::ControlFlow;
use futures::future::BoxFuture;
use lazy_static::lazy_static;
use serenity::{
    client::Context,
    prelude::{Mutex, RwLock},
};
use std::{
    any::{type_name, Any, TypeId},
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};

type Argument = dyn Any + Send + Sync;

type Callback = dyn for<'args> FnMut(&'args Context, &'args Argument) -> BoxFuture<'args, ControlFlow>
    + Sync
    + Send;

type Subscribers = Vec<Box<Callback>>;

pub trait Event: Any {
    type Argument: Any + Send + Sync;
}

lazy_static! {
    static ref EVENT_HANDLERS: RwLock<HashMap<TypeId, Arc<Mutex<Subscribers>>>> =
        Default::default();
}

pub async fn register<T, F>(mut f: F)
where
    T: Event,
    F: for<'args> FnMut(&'args Context, &'args T::Argument) -> BoxFuture<'args, ControlFlow>
        + Sync
        + Send
        + 'static,
{
    log::info!(
        "Registered a callback for {}: {}",
        type_name::<T>().split("::").last().unwrap(),
        type_name::<F>()
    );
    let callback: Box<Callback> = Box::new(move |ctx: &Context, any: &Argument| {
        f(ctx, any.downcast_ref::<T::Argument>().unwrap())
    });
    match EVENT_HANDLERS.write().await.entry(TypeId::of::<T>()) {
        Entry::Occupied(subs) => subs.get().lock().await.push(callback),
        Entry::Vacant(subs) => {
            subs.insert(Arc::new(Mutex::new(vec![callback])));
        }
    };
}

pub async fn emit<T>(ctx: Context, arg: T::Argument)
where
    T: Event,
{
    tokio::spawn(async move {
        let mut to_remove = vec![];
        let subscribers = EVENT_HANDLERS.read().await.get(&TypeId::of::<T>()).cloned();
        if let Some(subscribers) = subscribers {
            let mut subscribers = subscribers.lock().await;
            for (i, s) in subscribers.iter_mut().enumerate() {
                if s(&ctx, &arg).await.is_break() {
                    to_remove.push(i);
                }
            }
            for i in to_remove {
                let _ = subscribers.remove(i);
                log::trace!("Removed a callback for {}, index: {}", type_name::<T>(), i);
            }
        }
    });
}

pub mod events {
    use super::Event;
    use serenity::model::{
        channel::Reaction,
        guild::Guild,
        id::{ChannelId, GuildId, MessageId},
        voice::VoiceState,
    };
    macro_rules! events {
        ($($event:ident => $arg:ty),* $(,)?) => {
            $(
                pub struct $event;
                impl Event for $event {
                    type Argument = $arg;
                }
            )*
        }
    }
    events! {
        ReactionAdd => Reaction,
        ReactionRemove => Reaction,
        ReactionRemoveAll => (ChannelId, MessageId),
        VoiceStateUpdate => (Option<GuildId>, Option<VoiceState>, VoiceState),
        Ready => serenity::model::gateway::Ready,
        CacheReady => Vec<GuildId>,
        GuildCreate => (Guild, bool),
    }
}
