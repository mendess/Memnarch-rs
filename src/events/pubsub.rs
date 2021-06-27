use dashmap::DashMap;
use futures::future::BoxFuture;
use lazy_static::lazy_static;
use serenity::client::Context;
use std::any::{type_name, Any, TypeId};

type Argument = dyn Any + Send + Sync;

type Callback =
    dyn for<'args> FnMut(&'args Context, &'args Argument) -> BoxFuture<'args, ()> + Sync + Send;

type Subscribers = Vec<Box<Callback>>;

pub trait Event: Any {
    type Argument: Any + Send + Sync;
}

lazy_static! {
    static ref INSTANCE: DashMap<TypeId, Subscribers> = Default::default();
}

pub fn register<T, F>(mut f: F)
where
    T: Event,
    F: for<'args> FnMut(&'args Context, &'args T::Argument) -> BoxFuture<'args, ()>
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
        f(&*ctx, any.downcast_ref::<T::Argument>().unwrap())
    });
    INSTANCE
        .entry(TypeId::of::<T>())
        .or_insert_with(Default::default)
        .push(callback);
}

pub async fn emit<T>(ctx: Context, arg: T::Argument)
where
    T: Event,
{
    tokio::spawn(async move {
        if let Some(mut subscribers) = INSTANCE.get_mut(&TypeId::of::<T>()) {
            for s in subscribers.iter_mut() {
                s(&ctx, &arg).await;
            }
        }
    });
}

pub mod events {
    use super::Event;
    use serenity::model::{
        channel::Reaction,
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
    }
}
