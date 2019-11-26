use crate::consts::FILES_DIR;
use crate::cron::{CronSink, Task};
use crate::permissions::IS_FRIEND_CHECK;
use crate::VoiceManager;

use chrono::{DateTime, Duration, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serenity::{
    client::bridge::voice::ClientVoiceManager,
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::{channel::Message, id::GuildId},
    prelude::*,
    voice,
};
use simsearch::SimSearch;
use std::{
    collections::HashMap,
    error::Error,
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::Arc,
};

const SFX_FILES_DIR: &str = "sfx";
const SFX_STATS_FILE: &str = "sfx_stats.json";

group!({
    name: "SFX",
    options: {
        prefixes: ["sfx"],
    },
    commands: [list, add, play, delete, retreive, stats],
});

group!({
    name: "SFX_Aliases",
    // help_available: false,
    options: {},
    commands: [play],
});

#[derive(Debug, Clone)]
pub struct SfxStats(Arc<Mutex<HashMap<String, usize>>>);

impl TypeMapKey for SfxStats {
    type Value = SfxStats;
}

impl SfxStats {
    fn path() -> PathBuf {
        [FILES_DIR, SFX_STATS_FILE].iter().collect()
    }

    pub fn new() -> Self {
        SfxStats(Arc::new(Mutex::new(
            File::open(Self::path())
                .ok()
                .and_then(|f| {
                    serde_json::from_reader(f)
                        .map_err(|e| eprintln!("Error loading sfx stats: '{}", e))
                        .ok()
                })
                .unwrap_or_else(Default::default),
        )))
    }

    fn update(&mut self, sfx: &str) -> Result<(), String> {
        let mut stats = self.0.lock();
        stats
            .entry(sfx.to_string())
            .and_modify(|c| *c += 1)
            .or_insert(0);
        fn map_err<E: std::fmt::Debug>(sfx: &str, e: E) -> String {
            format!(
                "[SFX|{}] Failed to update {}, Error {:?}",
                Utc::now().naive_utc(),
                sfx,
                e
            )
        };
        let mf = |e| map_err(sfx, e);
        let mj = |e| map_err(sfx, e);
        let path = Self::path();
        DirBuilder::new()
            .recursive(true)
            .create(path.parent().unwrap())
            .map_err(mf)?;
        File::create(path)
            .map_err(mf)
            .and_then(|f| serde_json::to_writer(f, &*stats).map_err(mj))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LeaveVoice {
    guild_id: GuildId,
    when: DateTime<Utc>,
}

impl Task for LeaveVoice {
    type Id = GuildId;
    type GlobalData = Arc<Mutex<ClientVoiceManager>>;

    fn when(&self) -> DateTime<Utc> {
        self.when
    }

    fn call(&self, data: Self::GlobalData) -> Result<(), Box<dyn Error>> {
        println!(
            "[{:?}] Leaving guild's {} voice channel",
            Utc::now().naive_utc(),
            self.guild_id
        );
        let mut manager = data.lock();
        manager
            .leave(self.guild_id)
            .ok_or_else(|| "Couldn't leave channel".into())
    }

    fn check_id(&self, id: &Self::Id) -> bool {
        self.guild_id == *id
    }
}

#[command]
#[aliases("s")]
#[min_args(1)]
#[description("Play a saved sfx!")]
#[usage("part of name")]
#[example("wow")]
fn play(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let guild = msg
        .guild(&ctx.cache)
        .ok_or_else(|| "Groups and DMs not supported".to_string())?;
    let guild_id = { guild.read().id };
    let connect_to = guild
        .read()
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id)
        .ok_or_else(|| "Not in a voice channel".to_string())?;
    let file = {
        let share_map = ctx.data.read();
        let cron_sink = share_map
            .get::<CronSink<LeaveVoice>>()
            .expect("Expected VoiceManager in ShareMap");
        if let Some(gid) = msg.guild_id {
            cron_sink.cancel(gid)?;
        }
        let manager_id = share_map
            .get::<VoiceManager>()
            .expect("Expected VoiceManager in ShareMap");
        let mut manager = manager_id.lock();
        match manager.join(guild_id, connect_to) {
            Some(_) => msg
                .channel_id
                .say(&ctx, &format!("Joined {}", connect_to.mention())),
            None => msg.channel_id.say(&ctx, "Error joining"),
        }?;
        let file = find_file(&args)?;
        eprintln!("Playing sfx: {:?}", file);
        if let Some(handler) = manager.get_mut(guild_id) {
            let source = voice::ffmpeg(&file)?;
            handler.play(source);
        } else {
            return Err("Not in a voice channel".into());
        }
        if let Some(gid) = msg.guild_id {
            let leave = LeaveVoice {
                when: Utc::now()
                    .checked_add_signed(Duration::seconds(30))
                    .unwrap(),
                guild_id: gid,
            };
            cron_sink.send(leave)?;
        }
        file
    };
    let mut share_map = ctx.data.write();
    share_map
        .get_mut::<SfxStats>()
        .expect("Expected SfxStats in ShareMap")
        .update(file.as_os_str().to_str().unwrap())
        .err()
        .iter()
        .for_each(|e| eprintln!("{}", e));

    Ok(())
}

#[command]
#[description("List the available sfx files")]
#[usage("")]
fn list(ctx: &mut Context, msg: &Message) -> CommandResult {
    let sounds = fs::read_dir(sfx_path::<&str,_>(None)).map(|x| {
        let mut files = x
            .filter_map(Result::ok)
            .map(|x| String::from(x.path().as_path().file_name().unwrap().to_string_lossy()))
            .collect::<Vec<_>>();
        files.sort_unstable();
        files
    });
    msg.channel_id.send_message(&ctx.http, |m| {
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
    })?;
    Ok(())
}

#[command]
#[checks("is_friend")]
#[description("Saves a new sfx file")]
#[usage("{Attatchment}")]
fn add(ctx: &mut Context, msg: &Message) -> CommandResult {
    for attachment in msg.attachments.iter() {
        if attachment.size > 204_800 * 2 {
            return Err("File size too high, please keep it under 400Kb."
                .to_string()
                .into());
        }
        let bytes = attachment.download()?;
        let path = sfx_path(&attachment.filename);
        DirBuilder::new()
            .recursive(true)
            .create(path.parent().unwrap())?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&bytes)?;
        msg.channel_id.say(&ctx, "File added!")?;
    }
    Ok(())
}

#[command]
#[min_args(1)]
#[owners_only]
#[description("Remove an sfx file")]
#[usage("part of name")]
#[example("wow")]
fn delete(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let file = find_file(&args)?;
    msg.channel_id.send_message(&ctx, |m| m.add_file(&file))?;
    fs::remove_file(&file)?;
    Ok(())
}

#[command]
#[aliases("get")]
#[min_args(1)]
#[description("Upload an sfx file to discord")]
#[usage("part of name")]
#[example("wow")]
fn retreive(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let file = find_file(&args)?;
    msg.channel_id.send_message(&ctx, |m| m.add_file(&file))?;
    Ok(())
}

#[command]
#[description("Show the stats of the most played sfx")]
#[usage("")]
fn stats(ctx: &mut Context, msg: &Message) -> CommandResult {
    msg.channel_id.send_message(&ctx, |m| {
        m.embed(|e| {
            e.title("Stats");
            let mut stats = ctx
                .data
                .read()
                .get::<SfxStats>()
                .expect("Expected SfxStats in ShareMap")
                .0
                .lock()
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect::<Vec<(String, usize)>>();
            stats.sort_unstable_by_key(|(_, v)| *v);
            e.fields(stats.iter().chunks(12).into_iter().map(|x| {
                let f = x.collect::<Vec<_>>();
                let c1 = f[0].1.to_string();
                let c2 = f[f.len() - 1].1.to_string();
                (
                    format!("{}-{}", c1, c2),
                    f.iter().fold(String::new(), |acc, x| {
                        acc + "\n" + &format!("{}\t{}", x.0, x.1)
                    }),
                    true,
                )
            }))
        })
    })?;
    Ok(())
}

fn find_file(search_string: &Args) -> io::Result<PathBuf> {
    use std::io::{Error, ErrorKind::NotFound};
    let (search, vec) = fs::read_dir(sfx_path::<&str,_>(None))?
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
    match &search.search(search_string) {
        v if !v.is_empty() => Ok(vec[v[0]].path()),
        _ => Err(Error::new(
            NotFound,
            format!("No matches for {}", search_string),
        )),
    }
}

fn sfx_path<S: AsRef<str>, F: Into<Option<S>>>(file: F) -> PathBuf {
    match file.into() {
        Some(f) => [FILES_DIR, SFX_FILES_DIR, f.as_ref()].iter().collect(),
        None => [FILES_DIR, SFX_FILES_DIR].iter().collect(),
    }
}
