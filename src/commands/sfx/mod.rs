pub mod util;

use crate::util::daemons::DaemonManagerKey;
use crate::util::permissions::*;
use crate::{get, in_files};
use chrono::{DateTime, Duration, Utc};
use daemons::{ControlFlow, Daemon};
use itertools::Itertools;
use json_db::GlobalDatabase;
use serde::{Deserialize, Serialize};
use serenity::all::{CreateAttachment, CreateEmbed, CreateMessage, Http};
use serenity::{
    framework::standard::{
        Args, CommandResult,
        macros::{command, group},
    },
    model::{channel::Message, id::GuildId},
    prelude::*,
};
use simsearch::SimSearch;
use songbird::input::Input;
use std::{
    collections::HashMap,
    error::Error,
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

    async fn run(&mut self, _: &Self::Data) -> ControlFlow {
        tracing::debug!("Leaving voice. Scheduled for {}", self.when);
        if let Err(e) = self.songbird.remove(self.guild_id).await {
            tracing::error!("Could not leave voice channel: {}", e);
        }
        ControlFlow::BREAK
    }

    async fn interval(&self) -> StdDuration {
        (self.when - Utc::now()).to_std().unwrap_or_default()
    }

    async fn name(&self) -> String {
        format!("LeaveVoice(id: {}, when: {})", self.guild_id, self.when)
    }
}

#[group]
#[prefix("sfx")]
#[commands(list, add, play, delete, get, stats, stop)]
struct SFX;

#[group]
#[commands(s)]
struct SFXAliases;

#[command]
#[description("Stops everything")]
#[only_in(guilds)]
pub async fn stop(ctx: &Context, msg: &Message) -> CommandResult {
    if let Some(call) = songbird::get(ctx)
        .await
        .expect("Songbird not initialized")
        .get(msg.guild_id.unwrap())
    {
        call.lock().await.stop();
        Ok(())
    } else {
        Err("Not in a voice channel".into())
    }
}

#[command]
#[min_args(1)]
#[description("Play a saved sfx!")]
#[usage("part of name")]
#[example("wow")]
#[only_in(guilds)]
pub async fn play(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    play_impl(ctx, msg, args).await
}

#[command]
#[aliases("play")]
#[min_args(1)]
#[description("Play a saved sfx!")]
#[usage("part of name")]
#[example("wow")]
#[only_in(guilds)]
pub async fn s(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    play_impl(ctx, msg, args).await
}

async fn play_impl(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let mut file = PathBuf::new();
    play_sfx(ctx, msg, || async {
        file = find_file(&args).await?;
        msg.channel_id
            .say(&ctx, &format!("Playing {}", file.display()))
            .await?;
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

pub async fn play_sfx<F, Fut>(ctx: &Context, msg: &Message, audio_source: F) -> CommandResult
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Input, Box<dyn Error + Send + Sync>>>,
{
    let guild_id = msg.guild_id.ok_or("Not in a guild")?;

    let call_lock = util::join_or_get_call(ctx, guild_id, msg.author.id).await?;
    let audio = audio_source().await?;
    call_lock.lock().await.play_input(audio);

    let data = ctx.data.read().await;
    let dm = get!(> data, DaemonManagerKey);
    let id = dm
        .lock()
        .await
        .add_daemon(LeaveVoice {
            when: Utc::now()
                .checked_add_signed(Duration::minutes(30))
                .unwrap(),
            guild_id,
            songbird: data.get::<songbird::SongbirdKey>().unwrap().clone(),
        })
        .await;

    let mut dm = dm.lock().await;
    get!(> data, util::LeaveVoiceDaemons, lock)
        .set(&mut dm, guild_id, id)
        .await;

    Ok(())
}

#[command]
#[description("List the available sfx files")]
#[usage("")]
async fn list(ctx: &Context, msg: &Message) -> CommandResult {
    let sounds = fs::read_dir(sfx_path::<&str, _>(None).await?).map(|x| {
        let mut files = x
            .filter_map(Result::ok)
            .map(|x| String::from(x.path().as_path().file_name().unwrap().to_string_lossy()))
            .map(unicase::UniCase::new)
            .collect::<Vec<_>>();
        files.sort_unstable();
        files
    });
    msg.channel_id
        .send_message(
            &ctx.http,
            CreateMessage::new().embed({
                let mut e = CreateEmbed::new();
                e = e.title("List of sfx:");
                match &sounds {
                    Ok(files) if !files.is_empty() => {
                        e.fields(files.iter().chunks(12).into_iter().map(|x| {
                            let f = x.collect::<Vec<_>>();
                            let c1 = f[0].to_uppercase().chars().next().unwrap();
                            let c2 = f[f.len() - 1].to_uppercase().chars().next().unwrap();
                            (
                                [c1, '-', c2].iter().collect::<String>(),
                                f.iter().fold(String::new(), |acc, x| acc + "\n" + x),
                                true,
                            )
                        }))
                    }
                    Err(_) | Ok(_) => e.description("**No files :(**"),
                }
            }),
        )
        .await?;
    Ok(())
}

#[command]
#[checks("is_friend")]
#[description("Saves a new sfx file")]
#[usage("{Attatchment}")]
async fn add(ctx: &Context, msg: &Message) -> CommandResult {
    for attachment in msg.attachments.iter() {
        if attachment.size > 1024 * 1024 {
            return Err("File size too high, please keep it under 1Mb."
                .to_string()
                .into());
        }
        let bytes = attachment.download().await?;
        let path = sfx_path(&attachment.filename).await?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&bytes)?;
        msg.channel_id.say(&ctx, "File added!").await?;
    }
    Ok(())
}

#[command]
#[min_args(1)]
#[checks("is_friend")]
#[description("Remove an sfx file")]
#[usage("part of name")]
#[example("wow")]
async fn delete(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let file = find_file(&args).await?;
    msg.channel_id
        .send_message(
            &ctx,
            CreateMessage::new().add_file(
                CreateAttachment::file(&File::open(&file).await?, file.display().to_string())
                    .await?,
            ),
        )
        .await?;
    fs::remove_file(&file)?;
    Ok(())
}

#[command]
#[aliases("retreive", "retrieve")]
#[min_args(1)]
#[description("Upload an sfx file to discord")]
#[usage("part of name")]
#[example("wow")]
async fn get(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let file = find_file(&args).await?;
    msg.channel_id
        .send_message(
            &ctx,
            CreateMessage::new().add_file(
                CreateAttachment::file(&File::open(&file).await?, file.display().to_string())
                    .await?,
            ),
        )
        .await?;
    Ok(())
}

#[command]
#[description("Show the stats of the most played sfx")]
#[usage("")]
async fn stats(ctx: &Context, msg: &Message) -> CommandResult {
    let mut stats = SFX_STATS
        .load()
        .await?
        .0
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect::<Vec<(String, usize)>>();
    msg.channel_id
        .send_message(
            &ctx,
            CreateMessage::new().embed({
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
            }),
        )
        .await?;
    Ok(())
}

async fn find_file(search_string: &Args) -> io::Result<PathBuf> {
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
    let search_string = search_string.rest();
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
