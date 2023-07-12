use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};

use daemons::{async_trait, ControlFlow, Daemon};
use futures::FutureExt;
use mtg_spoilers::{Spoiler, SpoilerSource};
use pubsub::{events, subscribe};
use serenity::{
    http::CacheHttp,
    model::prelude::{component::ActionRowComponent, interaction::InteractionType, ChannelId},
    prelude::Mutex,
};

use crate::util::daemons::DaemonManager;
use json_db::{Database, GlobalDatabase};

#[derive(Default)]
struct SpoilerChecker(AtomicBool);

mod paths {
    pub static BASE: &str = "files/mtg-spoilers";
    pub static CACHE: &str = "files/mtg-spoilers/cache";
    pub static DB: &str = "files/mtg-spoilers/db.json";
}

#[async_trait]
impl Daemon<true> for SpoilerChecker {
    type Data = serenity::CacheAndHttp;

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        use mtg_spoilers::{cache::file::File, mythic};
        tracing::info!("checking for spoilers");
        let cache = match File::new(paths::CACHE).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("failed to create cache: {e:?}");
                return daemons::ControlFlow::CONTINUE;
            }
        };
        let new_cards = match mythic::new_cards(cache).await {
            Ok(n) => n,
            Err(e) => {
                tracing::error!("failed to fetch new cards: {e:?}");
                return daemons::ControlFlow::CONTINUE;
            }
        };

        if let Err(e) = send_new_cards(data, new_cards).await {
            tracing::error!("failed to send new cards: {e:?}");
        }
        tracing::info!("finished checking for spoilers");
        daemons::ControlFlow::CONTINUE
    }

    async fn interval(&self) -> Duration {
        match self
            .0
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => Duration::ZERO,
            Err(_) => Duration::from_secs(60 * 20),
        }
    }

    async fn name(&self) -> String {
        stringify!(SpoilerChecker).to_string()
    }
}

const DISCUSSION_BUTTON: &str = "mtg-spoilers-discuss-button";

pub async fn initialize(d: &mut Arc<Mutex<DaemonManager>>) -> io::Result<()> {
    tokio::fs::create_dir_all(paths::BASE).await?;
    d.lock().await.add_daemon(SpoilerChecker::default()).await;
    subscribe::<events::InteractionCreate, _>(|ctx, i| {
        async move {
            match i.kind() {
                InteractionType::MessageComponent => {
                    let msg = i.clone().message_component().unwrap();
                    let title = msg.message.embeds.get(0).and_then(|e| {
                        e.title
                            .as_deref()
                            .or_else(|| e.url.as_ref().and_then(|u| u.split('/').last()))
                    });
                    if let Err(e) = msg
                        .channel_id
                        .create_public_thread(ctx, msg.message.id, |thread| {
                            thread.name(title.unwrap_or("discussion"))
                        })
                        .await
                    {
                        tracing::error!(?e);
                    } else if let Err(e) = msg.create_interaction_response(ctx, |resp| {
                        resp
                            .kind(serenity::model::prelude::interaction::InteractionResponseType::UpdateMessage)
                            .interaction_response_data(|edit| {
                                edit.components(|c| c)
                            })
                    }).await
                    {
                        tracing::error!(?e)
                    }
                }
                _ => {
                    tracing::debug!("interaction ignored");
                }
            }
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
    subscribe::<events::ThreadCreate, _>(|ctx, t| {
        async move {
            let msgs = match t.messages(ctx, |msgs| msgs).await {
                Err(e) => {
                    tracing::error!(?e, "failed to get messages from a thread");
                    return ControlFlow::CONTINUE;
                }
                Ok(msgs) => msgs,
            };

            let button_message = msgs.into_iter().find_map(|m| {
                m.referenced_message.filter(|m| {
                    m.components.iter().any(|c| {
                        c.components.iter().any(|c| match c {
                            ActionRowComponent::Button(b) => {
                                b.custom_id.as_deref() == Some(DISCUSSION_BUTTON)
                            }
                            _ => false,
                        })
                    })
                })
            });
            if let Some(mut button_message) = button_message {
                if let Err(e) = button_message
                    .edit(ctx, |edit| edit.components(|c| c))
                    .await
                {
                    tracing::error!(?e, "failed to remove {DISCUSSION_BUTTON} button");
                }
            }
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
    Ok(())
}

static SPOILER_CHANNEL_DB: GlobalDatabase<HashSet<ChannelId>> = Database::const_new(paths::DB);

static RETRY_CACHE: OnceLock<Mutex<HashMap<ChannelId, Vec<Spoiler>>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToggleAction {
    Added,
    Removed,
}

pub async fn toggle_channel(ch: ChannelId) -> io::Result<ToggleAction> {
    let mut db = SPOILER_CHANNEL_DB.load().await?;
    if !db.remove(&ch) {
        db.insert(ch);
        Ok(ToggleAction::Added)
    } else {
        Ok(ToggleAction::Removed)
    }
}

async fn send_new_cards(
    ctx: &serenity::CacheAndHttp,
    new_cards: Vec<Spoiler>,
) -> serenity::Result<()> {
    for ch in SPOILER_CHANNEL_DB.load().await?.iter() {
        let retries = RETRY_CACHE
            .get_or_init(Default::default)
            .lock()
            .await
            .remove(ch)
            .unwrap_or_default();
        for c in retries.iter().chain(new_cards.iter()) {
            if let Err(e) = send_card(ctx, *ch, c).await {
                tracing::error!("failed to publish spoiler {c:#?} to {ch}: {e:?}");
                RETRY_CACHE
                    .get_or_init(Default::default)
                    .lock()
                    .await
                    .entry(*ch)
                    .or_default()
                    .push(c.clone());
            }
        }
    }
    Ok(())
}

async fn send_card(
    ctx: &serenity::CacheAndHttp,
    ch: ChannelId,
    card: &Spoiler,
) -> Result<(), serenity::Error> {
    ch.send_message(ctx.http(), |builder| {
        builder
            .embed(|builder| {
                builder.image(&card.image).url(&card.source_site_url).title(
                    card.name
                        .as_deref()
                        .unwrap_or_else(|| name_from_image(&card.image)),
                );
                if let Some(SpoilerSource { name, url }) = &card.source {
                    let mut description = format!("Source: {name}");
                    if let Some(url) = url {
                        write!(description, "\n{url}")
                            .expect("pushing to a string should never fail");
                    }
                    builder.description(description);
                }
                builder
            })
            .components(|c| {
                c.create_action_row(|row| {
                    row.create_button(|b| b.custom_id(DISCUSSION_BUTTON).label("Discuss"))
                })
            })
    })
    .await?;

    Ok(())
}

fn name_from_image(s: &str) -> &str {
    s.split('/')
        .last()
        .and_then(|s| {
            let (name, _) = s.split_once('.')?;
            Some(name)
        })
        .unwrap_or("unknown card name")
}
