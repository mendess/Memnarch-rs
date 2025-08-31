use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    io,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use daemons::{ControlFlow, Daemon, async_trait};
use futures::{
    FutureExt,
    future::{OptionFuture, join},
};
use itertools::Itertools;
use mtg_spoilers::{CardText, Spoiler, SpoilerSource};
use pubsub::{events, subscribe};
use serenity::{
    all::{
        ActionRowComponent, Button, ButtonKind, CacheHttp, ChannelId, CreateActionRow,
        CreateButton, CreateEmbed, CreateMessage, CreateThread, EditMessage, GuildChannel, Http,
        Interaction,
    },
    prelude::{Context, Mutex},
};

use crate::util::daemons::{DaemonManager, cache_and_http};
use json_db::GlobalDatabase;

mod paths {
    use constcat::concat;

    use crate::in_files;

    pub const BASE: &str = in_files!("mtg-spoilers");
    pub const CACHE: &str = concat!(BASE, "/cache");
    pub const DB: &str = concat!(BASE, "/db.json");
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
    type Data = (Arc<serenity::cache::Cache>, Arc<Http>);

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
                return daemons::ControlFlow::Continue(());
            }
        };
        let new_cards = match mythic::new_cards(cache).await {
            Ok(n) => n,
            Err(e) => {
                tracing::error!("failed to fetch new cards: {e:?}");
                return daemons::ControlFlow::Continue(());
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

        if let Err(e) = send_new_cards(cache_and_http(data), new_cards).await {
            tracing::error!("failed to send new cards: {e:?}");
        }
        tracing::info!("finished checking for spoilers");
        ControlFlow::Continue(())
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
        let title = msg.message.embeds.first().and_then(|e| {
            e.title
                .as_deref()
                .or_else(|| e.url.as_ref().and_then(|u| u.split('/').next_back()))
        });

        #[derive(Default)]
        struct Card {
            thumbnail: Option<reqwest::Url>,
            scryfall_uri: Option<reqwest::Url>,
            rest: Vec<CardText>,
        }

        tracing::info!(
            "{} ({nick}) requested a spoilers thread in {guild_name} to discuss {}",
            msg.user.name,
            title.unwrap_or_default(),
        );
        let mythic_spoiler_url = msg
            .message
            .embeds
            .first()
            .and_then(|e| e.url.as_deref())
            .and_then(|url| url.parse().ok());
        let fetch_card = OptionFuture::from(title.map(|title| async move {
            match scryfall::Card::named_fuzzy(title).await {
                Ok(card) => Some(Card {
                    thumbnail: card
                        .image_uris
                        .and_then(|i| i.png.or(i.large).or(i.normal).or(i.small).or(i.border_crop)),
                    scryfall_uri: Some(card.scryfall_uri),
                    rest: match card.card_faces {
                        Some(faces) if !faces.is_empty() => faces
                            .into_iter()
                            .map(|f| CardText {
                                name: Some(f.name),
                                type_line: f.type_line,
                                text: f.oracle_text,
                            })
                            .collect(),
                        _ => {
                            vec![CardText {
                                name: Some(card.name),
                                type_line: card.type_line,
                                text: card.oracle_text,
                            }]
                        }
                    },
                }),
                Err(e) => {
                    if !matches!(&e, scryfall::Error::ScryfallError(e) if e.status == 404) {
                        tracing::error!(?e, "failed to fetch oracle text for {title:?}");
                    }
                    let scraped_card = OptionFuture::from(
                        mythic_spoiler_url.map(mtg_spoilers::mythic::get_card_text),
                    )
                    .await
                    .map(|card| {
                        card.map(|card| Card {
                            rest: card,
                            ..Default::default()
                        })
                    })
                    .transpose();

                    match scraped_card {
                        Ok(scraped_card) => scraped_card.filter(|card| !card.rest.is_empty()),
                        Err(e) => {
                            tracing::error!(?e, "failed to scrape oracle text for {title:?}");
                            None
                        }
                    }
                }
            }
        }));
        let thread = msg.channel_id.create_thread_from_message(
            ctx,
            msg.message.id,
            CreateThread::new(title.unwrap_or("discussion")),
        );

        let (card, thread) = join(fetch_card, thread).await;
        let thread = match thread {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(?e);
                return;
            }
        };
        if let Some(Card {
            thumbnail,
            scryfall_uri,
            rest,
        }) = card.flatten()
        {
            let thread = thread
                .send_message(
                    ctx,
                    CreateMessage::new().embed({
                        let mut embed = CreateEmbed::new();
                        if let Some(image) = thumbnail {
                            embed = embed.thumbnail(image);
                        }
                        if let Some(url) = scryfall_uri {
                            embed = embed.url(url);
                        }
                        let title = rest.iter().filter_map(|c| c.name.as_deref()).format(" // ");
                        let desc = match rest.as_slice() {
                            [face] => {
                                format!(
                                    "{}\n{}",
                                    face.type_line.as_deref().unwrap_or_default(),
                                    face.text.as_deref().unwrap_or_default(),
                                )
                            }
                            multi_face => multi_face
                                .iter()
                                .map(|face| {
                                    format!(
                                        "{}\n{}\n{}",
                                        face.name.as_deref().unwrap_or_default(),
                                        face.type_line.as_deref().unwrap_or_default(),
                                        face.text.as_deref().unwrap_or_default(),
                                    )
                                })
                                .format("\n")
                                .to_string(),
                        };

                        embed.title(title.to_string()).description(desc)
                    }),
                )
                .await;
            if let Err(e) = thread {
                tracing::error!(
                    %e,
                    card = ?title,
                    "failed to send oracle text to discussion thread"
                );
            }
        }
    }
}

async fn delete_discuss_button(ctx: &Context, t: &GuildChannel) {
    let msgs = match t.messages(ctx, Default::default()).await {
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
                    ActionRowComponent::Button(Button {
                        data: ButtonKind::NonLink { custom_id, .. },
                        ..
                    }) => custom_id == DISCUSSION_BUTTON,
                    _ => false,
                })
            })
        })
    });
    if let Some(mut button_message) = button_message
        && let Err(e) = button_message
            .edit(ctx, EditMessage::new().components(Default::default()))
            .await
    {
        tracing::error!(?e, "failed to remove {DISCUSSION_BUTTON} button");
    }
}

pub async fn initialize(d: &Arc<Mutex<DaemonManager>>) -> io::Result<()> {
    tokio::fs::create_dir_all(paths::BASE).await?;
    d.lock().await.add_daemon(SpoilerChecker::default()).await;
    subscribe::<events::InteractionCreate, _>(|ctx, i| {
        async move {
            create_thread(ctx, i).await;
            ControlFlow::Continue(())
        }
        .boxed()
    })
    .await;
    subscribe::<events::ThreadCreate, _>(|ctx, t| {
        async move {
            delete_discuss_button(ctx, t).await;
            ControlFlow::Continue(())
        }
        .boxed()
    })
    .await;
    Ok(())
}

static SPOILER_CHANNEL_DB: GlobalDatabase<HashSet<ChannelId>> = GlobalDatabase::new(paths::DB);

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

async fn send_new_cards(ctx: impl CacheHttp, new_cards: Vec<Spoiler>) -> serenity::Result<()> {
    for ch in SPOILER_CHANNEL_DB.load().await?.iter() {
        let retries = RETRY_CACHE
            .get_or_init(Default::default)
            .lock()
            .await
            .remove(ch)
            .unwrap_or_default();
        for c in retries.iter().chain(new_cards.iter()) {
            if let Err(e) = send_card(&ctx, *ch, c).await {
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
    ctx: impl CacheHttp,
    ch: ChannelId,
    card: &Spoiler,
) -> Result<(), serenity::Error> {
    ch.send_message(
        ctx,
        CreateMessage::new()
            .embed({
                let mut builder = CreateEmbed::new();
                builder = builder.image(&card.image).url(&card.source_site_url).title(
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
                    builder = builder.description(description);
                }
                builder
            })
            .components(vec![CreateActionRow::Buttons(vec![
                CreateButton::new(DISCUSSION_BUTTON).label("Discuss/Translate"),
            ])]),
    )
    .await?;

    Ok(())
}

fn name_from_image(s: &str) -> &str {
    s.split('/')
        .next_back()
        .and_then(|s| {
            let (name, _) = s.split_once('.')?;
            Some(name)
        })
        .unwrap_or("unknown card name")
}
