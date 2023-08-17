use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};

use daemons::{async_trait, ControlFlow, Daemon};
use futures::{
    future::{join, OptionFuture},
    FutureExt,
};
use mtg_spoilers::{Spoiler, SpoilerSource};
use pubsub::{events, subscribe};
use serenity::{
    http::CacheHttp,
    model::prelude::{
        component::ActionRowComponent, interaction::Interaction, ChannelId, GuildChannel,
    },
    prelude::{Context, Mutex},
};

use crate::util::daemons::DaemonManager;
use json_db::{Database, GlobalDatabase};

mod paths {
    pub static BASE: &str = "files/mtg-spoilers";
    pub static CACHE: &str = "files/mtg-spoilers/cache";
    pub static DB: &str = "files/mtg-spoilers/db.json";
}

#[derive(Default)]
struct SpoilerChecker {
    delay: AtomicU64,
}

impl SpoilerChecker {
    const MAX_MINUTE_INTERVAL: u64 = 1 << 5;
}

#[async_trait]
impl Daemon<true> for SpoilerChecker {
    type Data = serenity::CacheAndHttp;

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        use mtg_spoilers::{cache::file::File, mythic};
        tracing::info!(
            delay_was = self.delay.load(Ordering::Acquire),
            "checking for spoilers"
        );
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

        if new_cards.is_empty() {
            let _ = self
                .delay
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                    if d == 0 {
                        Some(1)
                    } else if d < Self::MAX_MINUTE_INTERVAL {
                        Some(d << 1)
                    } else {
                        None
                    }
                });
        } else {
            self.delay.store(1, Ordering::Release);
        }

        if let Err(e) = send_new_cards(data, new_cards).await {
            tracing::error!("failed to send new cards: {e:?}");
        }
        tracing::info!("finished checking for spoilers");
        daemons::ControlFlow::CONTINUE
    }

    async fn interval(&self) -> Duration {
        let minutes = self.delay.load(Ordering::Acquire);
        Duration::from_secs(60 * minutes)
    }

    async fn name(&self) -> String {
        stringify!(SpoilerChecker).to_string()
    }
}

const DISCUSSION_BUTTON: &str = "mtg-spoilers-discuss-button";

async fn create_thread(ctx: &Context, i: &Interaction) {
    if let Some(msg) = i.clone().message_component() {
        let Some(gid) = msg.guild_id else {
            return;
        };
        let nick = msg.user.nick_in(ctx, gid).await.unwrap_or_default();
        let guild_name = gid.name(ctx).unwrap_or_default();
        let title = msg.message.embeds.get(0).and_then(|e| {
            e.title
                .as_deref()
                .or_else(|| e.url.as_ref().and_then(|u| u.split('/').last()))
        });
        tracing::info!(
            "{} ({nick}) requested a spoilers thread in {guild_name} to discuss {}",
            msg.user.name,
            title.unwrap_or_default(),
        );
        let fetch_card = OptionFuture::from(title.map(scryfall::Card::named_fuzzy));
        let thread = msg
            .channel_id
            .create_public_thread(ctx, msg.message.id, |thread| {
                thread.name(title.unwrap_or("discussion"))
            });

        let (card, thread) = join(fetch_card, thread).await;
        let thread = match thread {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(?e);
                return;
            }
        };
        match card {
            Some(Ok(card)) => {
                if let Err(e) = thread
                    .send_message(ctx, |msg| {
                        msg.embed(|embed| {
                            if let Some(image) = card.image_uris.into_values().next() {
                                embed.thumbnail(image);
                            }
                            embed
                                .title(card.name)
                                .url(card.scryfall_uri)
                                .description(format!(
                                    "{}\n{}",
                                    card.type_line.unwrap_or_default(),
                                    card.oracle_text.unwrap_or_default(),
                                ))
                        })
                    })
                    .await
                {
                    tracing::error!(?e, "failed to send oracle text to discussion thread");
                }
            }
            Some(Err(e)) => {
                if !matches!(&e, scryfall::Error::ScryfallError(e) if e.status == 404) {
                    tracing::error!(?e, "failed to fetch oracle text for {title:?}");
                }
            }
            None => { /* there was no title so there was no way to fetch the oracle text */ }
        }
    }
}

async fn delete_discuss_button(ctx: &Context, t: &GuildChannel) {
    let msgs = match t.messages(ctx, |msgs| msgs).await {
        Err(e) => {
            tracing::error!(?e, "failed to get messages from a thread");
            return;
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
}

pub async fn initialize(d: &mut Arc<Mutex<DaemonManager>>) -> io::Result<()> {
    tokio::fs::create_dir_all(paths::BASE).await?;
    d.lock().await.add_daemon(SpoilerChecker::default()).await;
    subscribe::<events::InteractionCreate, _>(|ctx, i| {
        async move {
            create_thread(ctx, i).await;
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
    subscribe::<events::ThreadCreate, _>(|ctx, t| {
        async move {
            delete_discuss_button(ctx, t).await;
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
