use crate::{permissions::*, daemons::DaemonManager, consts::FILES_DIR };
// use crate::VoiceManager;

use daemons::Daemon;
use chrono::{DateTime, Duration, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::{channel::Message, id::GuildId},
    prelude::*,
};
use simsearch::SimSearch;
use std::{
    collections::HashMap,
    error::Error,
    fs::{self, DirBuilder, File, OpenOptions},
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
#[commands(play)]
struct SFXAliases;

#[derive(Debug, Clone)]
pub struct SfxStats(Arc<Mutex<HashMap<String, usize>>>);

impl TypeMapKey for SfxStats {
    type Value = SfxStats;
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
        SfxStats(Arc::new(Mutex::new(
            Self::path()
                .and_then(|p| File::open(p))
                .ok()
                .and_then(|f| {
                    serde_json::from_reader(f)
                        .map_err(|e| eprintln!("Error loading sfx stats: '{}'", e))
                        .ok()
                })
                .unwrap_or_else(Default::default),
        )))
    }

    async fn update(&mut self, sfx: &str) -> Result<(), String> {
        let mut stats = self.0.lock().await;
        stats
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
            .and_then(|path| File::create(path))
            .map_err(mf)
            .and_then(|f| serde_json::to_writer(f, &*stats).map_err(mj))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LeaveVoice {
    guild_id: GuildId,
    when: DateTime<Utc>,
}

#[serenity::async_trait]
impl Daemon for LeaveVoice {
    type Data = serenity::CacheAndHttp;

    async fn run(&mut self, data: &Self::Data) -> daemons::ControlFlow {
        // TODO: Fix after reimplementing voice
        // println!(
        //     "[{:?}] Leaving guild's {} voice channel",
        //     Utc::now().naive_utc(),
        //     self.guild_id
        // );
        // manager
        //     .leave(self.guild_id)
        //     .ok_or_else(|| "Couldn't leave channel".into());
        daemons::ControlFlow::Break
    }

    async fn interval(&self) -> StdDuration {
        (self.when - Utc::now()).to_std().unwrap_or_default()
    }

    async fn name(&self) -> String {
        format!("{:?}", self)
    }
}

#[command]
#[description("Stops everything")]
pub async fn stop(ctx: &Context, msg: &Message) -> CommandResult {
    // TODO: Fix after reimplementing voice
    // let guild_id = msg.guild_id.ok_or_else(|| String::from("Not in a guild"))?;
    // let share_map = ctx.data.read().await;
    // let manager_id = share_map
    //     .get::<VoiceManager>()
    //     .expect("Expected VoiceManager in ShareMap");
    // let mut manager = manager_id.lock();
    // if let Some(handler) = manager.get_mut(guild_id) {
    //     handler.stop();
    // } else {
    //     return Err("Not in a voice channel".into());
    // }
    Ok(())
}

#[command]
#[aliases("s")]
#[min_args(1)]
#[description("Play a saved sfx!")]
#[usage("part of name")]
#[example("wow")]
pub async fn play(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    // TODO: Fix after reimplementing voice
    // let mut file = PathBuf::new();
    // play_sfx(ctx, msg, || {
    //     file = find_file(&args)?;
    //     msg.channel_id.say(
    //         &ctx,
    //         &format!(
    //             "Playing {}",
    //             file.file_name()
    //                 .unwrap_or(std::ffi::OsStr::new(""))
    //                 .to_string_lossy()
    //         ),
    //     )?;
    //     eprintln!("Playing sfx: {:?}", file);
    //     Ok(voice::ffmpeg(&file)?)
    // })?;
    // let mut share_map = ctx.data.write();
    // share_map
    //     .get_mut::<SfxStats>()
    //     .expect("Expected SfxStats in ShareMap")
    //     .update(file.as_os_str().to_str().unwrap())
    //     .err()
    //     .iter()
    //     .for_each(|e| eprintln!("{}", e));
    Ok(())
}

// TODO: Fix after reimplementing voice
// pub async fn play_sfx<F>(ctx: &Context, msg: &Message, audio_source: F) -> CommandResult
// where
//     F: FnOnce() -> Result<Box<dyn voice::AudioSource>, Box<dyn Error>>,
// {
//     let guild = msg
//         .guild(&ctx.cache)
//         .ok_or_else(|| "Groups and DMs not supported".to_string())?;
//     let guild_id = { guild.read().id };
//     let connect_to = guild
//         .read()
//         .voice_states
//         .get(&msg.author.id)
//         .and_then(|voice_state| voice_state.channel_id)
//         .ok_or_else(|| "Not in a voice channel".to_string())?;
//     let share_map = ctx.data.read();
//     let cron_sink = share_map
//         .get::<CronSink<LeaveVoice>>()
//         .expect("Expected VoiceManager in ShareMap");
//     if let Some(gid) = msg.guild_id {
//         cron_sink.cancel(gid)?;
//     }
//     let manager_id = share_map
//         .get::<VoiceManager>()
//         .expect("Expected VoiceManager in ShareMap");
//     let mut manager = manager_id.lock();
//     if let None = manager.join(guild_id, connect_to) {
//         msg.channel_id.say(&ctx, "Error joining")?;
//         return Err("Failed to join channel".into());
//     }
//     if let Some(handler) = manager.get_mut(guild_id) {
//         handler.play(audio_source()?);
//     } else {
//         return Err("Not in a voice channel".into());
//     }
//     if let Some(gid) = msg.guild_id {
//         let leave = LeaveVoice {
//             when: Utc::now()
//                 .checked_add_signed(Duration::minutes(30))
//                 .unwrap(),
//             guild_id: gid,
//         };
//         cron_sink.send(leave)?;
//     }

//     Ok(())
// }

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
    let mut stats = ctx
        .data
        .read()
        .await
        .get::<SfxStats>()
        .expect("Expected SfxStats in ShareMap")
        .0
        .lock()
        .await
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
