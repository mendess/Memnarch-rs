pub mod util;

use crate::in_files;
use anyhow::Context as _;
use chrono::{DateTime, Duration, Utc};
use daemons::Daemon;
use itertools::Itertools;
use json_db::GlobalDatabase;
use poise::{CreateReply, command};
use serde::{Deserialize, Serialize};
use serenity::all::{CreateAttachment, CreateEmbed, Http};
use serenity::model::id::GuildId;
use simsearch::SimSearch;
use songbird::input::Input;
use std::ops::ControlFlow;
use std::{
    collections::HashMap,
    fs::{self, DirBuilder, OpenOptions},
    future::Future,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration as StdDuration,
};
use tokio::fs::File;

const SFX_FILES_DIR: &str = in_files!("sfx");

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SfxStats(HashMap<String, usize>);

impl SfxStats {
    fn update(&mut self, sfx: &str) {
        self.0
            .entry(sfx.to_string())
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }
}

static SFX_STATS: GlobalDatabase<SfxStats> = GlobalDatabase::new(in_files!("sfx_stats.json"));

#[derive(Debug)]
pub struct LeaveVoice {
    guild_id: GuildId,
    when: DateTime<Utc>,
    songbird: Arc<songbird::Songbird>,
}

#[serenity::async_trait]
impl Daemon<false> for LeaveVoice {
    type Data = (Arc<serenity::cache::Cache>, Arc<Http>);

    async fn run(&mut self, _: &Self::Data) -> daemons::ControlFlow {
        tracing::debug!("Leaving voice. Scheduled for {}", self.when);
        if let Err(e) = self.songbird.remove(self.guild_id).await {
            tracing::error!("Could not leave voice channel: {}", e);
        }
        ControlFlow::Break(())
    }

    async fn interval(&self) -> StdDuration {
        (self.when - Utc::now()).to_std().unwrap_or_default()
    }

    async fn name(&self) -> String {
        format!("LeaveVoice(id: {}, when: {})", self.guild_id, self.when)
    }
}

#[command(
    slash_command,
    guild_only,
    subcommands("stop", "play", "list", "add", "delete", "stats", "download")
)]
pub async fn sfx(_: super::Context<'_>) -> anyhow::Result<()> {
    Ok(())
}

/// Play a saved sfx!
#[command(slash_command, guild_only)]
async fn play(ctx: super::Context<'_>, query: String) -> anyhow::Result<()> {
    play_impl(ctx, &query).await
}

/// Stops everything
#[command(slash_command, guild_only)]
async fn stop(ctx: super::Context<'_>) -> anyhow::Result<()> {
    if let Some(call) = songbird::get(ctx.serenity_context())
        .await
        .context("Songbird not initialized")?
        .get(ctx.guild_id().unwrap())
    {
        call.lock().await.stop();
        Ok(())
    } else {
        Err(anyhow::anyhow!("Not in a voice channel"))
    }
}

/// List the available sfx files
#[command(slash_command, guild_only)]
async fn list(ctx: super::Context<'_>) -> anyhow::Result<()> {
    let sounds = fs::read_dir(sfx_path::<&str, _>(None).await?).map(|x| {
        let mut files = x
            .filter_map(Result::ok)
            .map(|x| String::from(x.path().as_path().file_name().unwrap().to_string_lossy()))
            .map(unicase::UniCase::new)
            .collect::<Vec<_>>();
        files.sort_unstable();
        files
    });
    match sounds {
        Ok(sounds) if !sounds.is_empty() => {
            util::paginate(
                ctx,
                &sounds
                    .iter()
                    .chunks(30)
                    .into_iter()
                    .map(|x| {
                        let f = x.collect::<Vec<_>>();
                        let c1 = f[0].to_uppercase().chars().next().unwrap();
                        let c2 = f[f.len() - 1].to_uppercase().chars().next().unwrap();
                        CreateEmbed::new().title("List of sfx:").field(
                            format!("{c1}-{c2}"),
                            f.iter().fold(String::new(), |acc, x| acc + "\n" + x),
                            true,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .await?
        }
        Err(_) | Ok(_) => {
            ctx.say("No files :(").await?;
        }
    };
    Ok(())
}

/// Saves a new sfx file
#[command(slash_command, guild_only)]
async fn add(ctx: super::Context<'_>) -> anyhow::Result<()> {
    let command = match ctx {
        poise::Context::Application(application_context) => application_context.interaction,
        poise::Context::Prefix(_) => unreachable!(),
    };
    for opt in command.data.options().iter() {
        let attachment = match opt.value {
            serenity::all::ResolvedValue::Attachment(attachment) => attachment,
            _ => continue,
        };
        if attachment.size > 1024 * 1024 {
            return Err(anyhow::anyhow!(
                "File size too high, please keep it under 1Mb."
            ));
        }
        let bytes = attachment.download().await?;
        let path = sfx_path(&attachment.filename).await?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&bytes)?;
        ctx.say("File added!").await?;
    }
    Ok(())
}

/// Remove an sfx file
#[command(slash_command, guild_only)]
async fn delete(ctx: super::Context<'_>, query: String) -> anyhow::Result<()> {
    let file = find_file(&query).await?;
    ctx.send(CreateReply::default().attachment(
        CreateAttachment::file(&File::open(&file).await?, file.display().to_string()).await?,
    ))
    .await?;
    fs::remove_file(&file)?;
    Ok(())
}

/// Download an sfx file
#[command(slash_command, guild_only)]
async fn download(ctx: super::Context<'_>, query: String) -> anyhow::Result<()> {
    let file = find_file(&query).await?;
    ctx.send(CreateReply::default().attachment(
        CreateAttachment::file(&File::open(&file).await?, file.display().to_string()).await?,
    ))
    .await?;
    Ok(())
}

/// Show the stats of the most played sfx
#[command(slash_command, guild_only)]
async fn stats(ctx: super::Context<'_>) -> anyhow::Result<()> {
    let mut stats = SFX_STATS
        .load()
        .await?
        .0
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect::<Vec<(String, usize)>>();
    ctx.send(CreateReply::default().embed({
        stats.sort_unstable_by_key(|(_, v)| *v);
        CreateEmbed::new()
            .title("Stats")
            .fields(stats.iter().chunks(12).into_iter().map(|x| {
                let f = x.collect::<Vec<_>>();
                let c1 = f[0].1.to_string();
                let c2 = f[f.len() - 1].1.to_string();
                (
                    format!("{}-{}", c1, c2),
                    f.iter().fold(String::new(), |acc, x| {
                        acc + "\n"
                            + &format!(
                                "{:<5}{}",
                                x.1,
                                Path::new(&x.0).file_name().unwrap().to_string_lossy()
                            )
                    }),
                    true,
                )
            }))
    }))
    .await?;
    Ok(())
}

async fn play_impl(ctx: super::Context<'_>, search_string: &str) -> anyhow::Result<()> {
    let mut file = PathBuf::new();
    play_sfx(ctx, || async {
        file = find_file(search_string).await?;
        ctx.say(&format!("Playing {}", file.display())).await?;
        tracing::info!("Playing sfx: {:?}", file);
        Ok(songbird::input::File::new(file.clone()).into())
    })
    .await?;
    if let Err(e) = SFX_STATS
        .load()
        .await
        .map(|mut s| s.update(file.as_os_str().to_str().unwrap()))
    {
        tracing::error!("Failed to update sfx stats: {}", e);
    }
    Ok(())
}

pub(crate) async fn play_sfx<F, Fut>(ctx: super::Context<'_>, audio_source: F) -> anyhow::Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<Input>>,
{
    let guild_id = ctx.guild_id().context("Not in a guild")?;

    let call_lock = util::join_or_get_call(ctx, guild_id, ctx.author().id).await?;
    let audio = audio_source().await?;
    call_lock.lock().await.play_input(audio);

    let data = ctx.data();
    let mut dm = data.daemons.lock().await;
    let id = dm
        .add_daemon(LeaveVoice {
            when: Utc::now()
                .checked_add_signed(Duration::minutes(30))
                .unwrap(),
            guild_id,
            songbird: ctx
                .serenity_context()
                .data
                .read()
                .await
                .get::<songbird::SongbirdKey>()
                .unwrap()
                .clone(),
        })
        .await;

    data.leave_voice
        .lock()
        .await
        .set(&mut dm, guild_id, id)
        .await;

    Ok(())
}

async fn find_file(search_string: &str) -> io::Result<PathBuf> {
    use std::io::{Error, ErrorKind::NotFound};
    let (search, vec) = fs::read_dir(sfx_path::<&str, _>(None).await?)?
        .filter_map(Result::ok)
        .enumerate()
        .fold(
            (SimSearch::new(), Vec::new()),
            |(mut search, mut vec), (id, name)| {
                vec.push(name);
                search.insert(id, vec[vec.len() - 1].file_name().to_str().unwrap());
                (search, vec)
            },
        );
    match search.search(search_string).first() {
        Some(&i) => Ok(vec[i].path()),
        None => Err(Error::new(
            NotFound,
            format!("No matches for {}", search_string),
        )),
    }
}

async fn sfx_path<S: AsRef<str>, F: Into<Option<S>>>(file: F) -> io::Result<PathBuf> {
    let p: PathBuf = match file.into() {
        Some(f) => [SFX_FILES_DIR, f.as_ref()].iter().collect(),
        None => [SFX_FILES_DIR].iter().collect(),
    };
    DirBuilder::new()
        .recursive(true)
        .create(if p.components().count() > 2 {
            p.parent().unwrap()
        } else {
            p.as_path()
        })?;
    Ok(p)
}
