pub mod util;

use crate::{consts::FILES_DIR, daemons::DaemonManager, get, permissions::*};
use chrono::{DateTime, Duration, Utc};
use daemons::Daemon;
use itertools::Itertools;
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::{channel::Message, id::GuildId},
    prelude::*,
};
use simsearch::SimSearch;
use songbird::input::Input;
use std::{
    collections::HashMap,
    error::Error,
    fs::{self, DirBuilder, File, OpenOptions},
    future::Future,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration as StdDuration,
};

const SFX_FILES_DIR: &str = "sfx";
const SFX_STATS_FILE: &str = "sfx_stats.json";

#[group]
#[prefix("sfx")]
#[commands(list, add, play, delete, retreive, stats, stop)]
struct SFX;

#[group]
#[commands(s)]
struct SFXAliases;

#[derive(Debug, Clone)]
pub struct SfxStats(HashMap<String, usize>);

impl TypeMapKey for SfxStats {
    type Value = Arc<Mutex<SfxStats>>;
}

impl SfxStats {
    fn path() -> io::Result<PathBuf> {
        let p = [FILES_DIR, SFX_STATS_FILE].iter().collect::<PathBuf>();
        DirBuilder::new()
            .recursive(true)
            .create(p.parent().expect("This path always has enough components"))?;
        Ok(p)
    }

    pub fn new() -> Self {
        SfxStats(
            Self::path()
                .and_then(File::open)
                .ok()
                .and_then(|f| {
                    serde_json::from_reader(f)
                        .map_err(|e| log::error!("Error loading sfx stats: '{}'", e))
                        .ok()
                })
                .unwrap_or_else(Default::default),
        )
    }

    async fn update(&mut self, sfx: &str) -> Result<(), String> {
        self.0
            .entry(sfx.to_string())
            .and_modify(|c| *c += 1)
            .or_insert(1);
        fn map_err<E: std::fmt::Debug>(sfx: &str, e: E) -> String {
            format!(
                "[SFX|{}] Failed to update {}, Error {:?}",
                Utc::now().naive_utc(),
                sfx,
                e
            )
        }
        let mf = |e| map_err(sfx, e);
        let mj = |e| map_err(sfx, e);
        Self::path()
            .and_then(File::create)
            .map_err(mf)
            .and_then(|f| serde_json::to_writer(f, &self.0).map_err(mj))
    }
}

#[derive(Debug)]
pub struct LeaveVoice {
    guild_id: GuildId,
    when: DateTime<Utc>,
    songbird: Arc<songbird::Songbird>,
}

#[serenity::async_trait]
impl Daemon for LeaveVoice {
    type Data = serenity::CacheAndHttp;

    async fn run(&mut self, _: &Self::Data) -> daemons::ControlFlow {
        log::debug!("Leaving voice");
        if let Err(e) = self.songbird.remove(self.guild_id).await {
            log::error!("Could not leave voice channel: {}", e);
        }
        daemons::ControlFlow::Break
    }

    async fn interval(&self) -> StdDuration {
        (self.when - Utc::now()).to_std().unwrap_or_default()
    }

    async fn name(&self) -> String {
        format!("LeaveVoice(id: {}, when: {})", self.guild_id, self.when)
    }
}

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
        log::info!("Playing sfx: {:?}", file);
        match songbird::ffmpeg(&file).await {
            Ok(source) => Ok(source),
            Err(e) => return Err(format!("Failed getting audio source: {:?}", e).into()),
        }
    })
    .await?;
    if let Err(e) = get!(ctx, SfxStats, lock).update(file.as_os_str().to_str().unwrap()).await {
        log::error!("Failed to update sfx stats: {}", e);
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
    call_lock.lock().await.play_source(audio);

    let data = ctx.data.read().await;
    let dm = get!(> data, DaemonManager);
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
        .set(&mut *dm, guild_id, id)
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
        .send_message(&ctx.http, |m| {
            m.embed(|e| {
                e.title("List of sfx:");
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
            })
        })
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
        .send_message(&ctx, |m| m.add_file(&file))
        .await?;
    fs::remove_file(&file)?;
    Ok(())
}

#[command]
#[aliases("get")]
#[min_args(1)]
#[description("Upload an sfx file to discord")]
#[usage("part of name")]
#[example("wow")]
async fn retreive(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let file = find_file(&args).await?;
    msg.channel_id
        .send_message(&ctx, |m| m.add_file(&file))
        .await?;
    Ok(())
}

#[command]
#[description("Show the stats of the most played sfx")]
#[usage("")]
async fn stats(ctx: &Context, msg: &Message) -> CommandResult {
    let mut stats = get!(ctx, SfxStats, lock)
        .0
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect::<Vec<(String, usize)>>();
    msg.channel_id
        .send_message(&ctx, |m| {
            m.embed(|e| {
                e.title("Stats");
                stats.sort_unstable_by_key(|(_, v)| *v);
                e.fields(stats.iter().chunks(12).into_iter().map(|x| {
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
            })
        })
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
    match search.search(search_string).get(0) {
        Some(&i) => Ok(vec[i].path()),
        None => Err(Error::new(
            NotFound,
            format!("No matches for {}", search_string),
        )),
    }
}

async fn sfx_path<S: AsRef<str>, F: Into<Option<S>>>(file: F) -> io::Result<PathBuf> {
    let p: PathBuf = match file.into() {
        Some(f) => [FILES_DIR, SFX_FILES_DIR, f.as_ref()].iter().collect(),
        None => [FILES_DIR, SFX_FILES_DIR].iter().collect(),
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
