use crate::{consts::FILES_DIR, cron::Task};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    http::raw::Http,
    model::{channel::Message, id::GuildId},
    prelude::*,
};
use std::{
    collections::{hash_map::Entry, HashMap},
    error::Error,
    fs::{DirBuilder, File},
    io::Error as IoError,
    io::ErrorKind as IoErrorKind,
    path::PathBuf,
    sync::Arc,
};

group!({
    name: "Custom",
    options: {
        prefixes: ["custom"],
    },
    commands: [add, remove, list],
});

//#[min_args(2)]
#[command]
fn add(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let mut args_it = args.raw();
    let fst = args_it.next().unwrap();
    let decay = if fst == "-d" || fst == "--decay" {
        true
    } else {
        args_it = args.raw();
        false
    };
    let cmd = args_it.next().unwrap().to_string();
    let output = args_it.join(" ");
    ctx.data
        .write()
        .get_mut::<CustomCommands>()
        .unwrap()
        .write()
        .add(
            msg.guild_id.ok_or_else(|| "guild_id is missing")?,
            cmd,
            output,
            decay,
        )?;
    msg.channel_id.say(&ctx, "Command added!")?;
    Ok(())
}

// #[min_args(1)]
#[command]
fn remove(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    let cmd = args.raw().next().unwrap();
    let output = ctx
        .data
        .write()
        .get_mut::<CustomCommands>()
        .unwrap()
        .write()
        .remove(msg.guild_id.ok_or_else(|| "guild_id is missing")?, cmd)?;
    match output {
        Some(output) => msg
            .channel_id
            .say(&ctx, format!("Command removed: {} => '{}'!", cmd, output))?,
        None => msg
            .channel_id
            .say(&ctx, format!("Command {} doesn't exist!", cmd))?,
    };
    Ok(())
}

#[command]
fn list(ctx: &mut Context, msg: &Message) -> CommandResult {
    let mut share_map = ctx.data.write();
    let mut cc = share_map.get_mut::<CustomCommands>().unwrap().write();
    let cmds = cc.list(msg.guild_id.ok_or_else(|| "guild_id is missing")?)?;
    dbg!(cmds.is_some());
    msg.channel_id.send_message(&ctx, |m| {
        m.embed(|e| {
            if let Some(cmds) = cmds {
                let size_hint = cmds.size_hint().0;
                e.description(cmds.fold(String::with_capacity(size_hint * 5), |d, s| {
                    d + &format!("- {}\n", s.0)
                }));
            }
            e.title("List of custom commands")
        })
    })?;
    Ok(())
}

pub type CommandPair = (String, bool);
type GuildCommands = HashMap<String, CommandPair>;

const CUSTOM_DIR: &str = "custom";

#[derive(Default, Deserialize, Serialize)]
pub struct CustomCommands {
    cmds: HashMap<GuildId, GuildCommands>,
}

impl TypeMapKey for CustomCommands {
    type Value = Arc<RwLock<CustomCommands>>;
}

impl CustomCommands {
    fn save_path(guild_id: GuildId) -> PathBuf {
        [FILES_DIR, CUSTOM_DIR, &format!("{}.json", guild_id)]
            .iter()
            .collect()
    }

    fn load(&mut self, guild_id: GuildId) -> Result<&mut GuildCommands, IoError> {
        let commands = match self.cmds.entry(guild_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let path = Self::save_path(guild_id);
                let gc = serde_json::from_reader(File::open(&path)?).map_err(|e| {
                    eprintln!("Error parsing custom commands: '{}'", e);
                    e
                })?;
                entry.insert(gc)
            }
        };
        Ok(commands)
    }

    fn save<I: Into<Option<GuildId>>>(&mut self, guild_id: I) -> Result<(), IoError> {
        DirBuilder::new()
            .recursive(true)
            .create([FILES_DIR, CUSTOM_DIR].iter().collect::<PathBuf>())?;
        match guild_id.into() {
            Some(g) => {
                serde_json::to_writer(File::create(Self::save_path(g))?, &self.cmds[&g])?;
            }
            None => self.cmds.keys().try_for_each(|k| -> Result<(), IoError> {
                serde_json::to_writer(File::create(Self::save_path(*k))?, &self.cmds[k])
                    .map_err(|e| e.into())
            })?,
        }
        Ok(())
    }

    pub fn execute(
        &mut self,
        guild_id: GuildId,
        cmd: &str,
    ) -> Result<Option<&CommandPair>, IoError> {
        match self.load(guild_id) {
            Err(e) if e.kind() == IoErrorKind::NotFound => Ok(None),
            Err(e) => Err(dbg!(e)),
            Ok(gc) => Ok(dbg!(gc.get(cmd))),
        }
    }

    pub fn add(
        &mut self,
        guild_id: GuildId,
        cmd: String,
        ret: String,
        decay: bool,
    ) -> Result<(), IoError> {
        let gc = match self.load(guild_id) {
            Ok(gc) => gc,
            Err(e) if e.kind() == IoErrorKind::NotFound => {
                self.cmds.insert(guild_id, Default::default());
                self.cmds.get_mut(&guild_id).unwrap()
            }
            Err(e) => return Err(e),
        };
        gc.insert(cmd, (ret, decay));
        self.save(guild_id)?;
        Ok(())
    }

    pub fn remove(&mut self, guild_id: GuildId, cmd: &str) -> Result<Option<String>, IoError> {
        match self.load(guild_id) {
            Err(e) if e.kind() == IoErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
            Ok(gc) => Ok(gc.remove(cmd).map(|c| c.0)),
        }
    }

    pub fn list(
        &mut self,
        guild_id: GuildId,
    ) -> Result<Option<impl Iterator<Item = &CommandPair>>, IoError> {
        match self.load(guild_id) {
            Err(e) if e.kind() == IoErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
            Ok(gc) => Ok(Some(gc.values())),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct MessageDecay {
    id: Message,
    when: DateTime<Utc>,
}

impl MessageDecay {
    pub fn new(id: Message, when: DateTime<Utc>) -> Self {
        MessageDecay { id, when }
    }
}

impl Task for MessageDecay {
    type Id = Message;
    type GlobalData = Arc<Http>;
    fn when(&self) -> DateTime<Utc> {
        self.when
    }

    fn call(&self, data: Self::GlobalData) -> Result<(), Box<dyn Error>> {
        self.id
            .delete(&*data)
            .map_err(|e| Box::new(e) as Box<dyn Error>)
    }

    fn check_id(&self, id: &Self::Id) -> bool {
        self.id.id == id.id
    }
}
