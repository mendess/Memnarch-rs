use crate::{get, util::consts::FILES_DIR};
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
use std::{
    collections::{hash_map::Entry, HashMap},
    fs::{DirBuilder, File},
    io::Error as IoError,
    io::ErrorKind as IoErrorKind,
    path::PathBuf,
    sync::Arc,
};

#[group]
#[prefix("custom")]
#[commands(add, remove, list)]
struct Custom;

#[command]
#[min_args(2)]
async fn add(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let mut args_it = args.raw();
    let cmd = args_it.next().unwrap().to_string();
    let output = args_it.join(" ");
    get!(ctx, CustomCommands, write).add(
        msg.guild_id.ok_or("guild_id is missing")?,
        cmd,
        output,
    )?;
    msg.channel_id.say(&ctx, "Command added!").await?;
    Ok(())
}

#[command]
#[min_args(1)]
async fn remove(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let cmd = args.raw().next().unwrap();
    let output =
        get!(ctx, CustomCommands, write).remove(msg.guild_id.ok_or("guild_id is missing")?, cmd)?;
    match output {
        Some(output) => {
            msg.channel_id
                .say(&ctx, format!("Command removed: {} => '{}'!", cmd, output))
                .await?
        }
        None => {
            msg.channel_id
                .say(&ctx, format!("Command {} doesn't exist!", cmd))
                .await?
        }
    };
    Ok(())
}

#[command]
async fn list(ctx: &Context, msg: &Message) -> CommandResult {
    let share_map = ctx.data.read().await;
    let mut cc = get!(> share_map, CustomCommands, write);
    let cmds = cc.list(msg.guild_id.ok_or("guild_id is missing")?)?;
    msg.channel_id
        .send_message(&ctx, |m| {
            m.embed(|e| {
                if let Some(cmds) = cmds {
                    let size_hint = cmds.size_hint().0;
                    e.description(
                        cmds.fold(String::with_capacity(size_hint * 5), |d, (key, value)| {
                            d + &format!("{} - {}\n", key, value)
                        }),
                    );
                }
                e.title("List of custom commands")
            })
        })
        .await?;
    Ok(())
}

type GuildCommands = HashMap<String, String>;

const CUSTOM_DIR: &str = "custom";

#[derive(Default, Deserialize, Serialize)]
pub struct CustomCommands {
    cmds: HashMap<GuildId, GuildCommands>,
}

impl TypeMapKey for CustomCommands {
    type Value = Arc<RwLock<CustomCommands>>;
}

impl CustomCommands {
    fn save_path(guild_id: GuildId) -> Result<PathBuf, IoError> {
        let p = [FILES_DIR, CUSTOM_DIR, &format!("{}.json", guild_id)]
            .iter()
            .collect::<PathBuf>();
        DirBuilder::new()
            .recursive(true)
            .create(p.parent().expect("This path always has enough components"))?;
        Ok(p)
    }

    fn load(&mut self, guild_id: GuildId) -> Result<&mut GuildCommands, IoError> {
        let commands = match self.cmds.entry(guild_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let path = Self::save_path(guild_id)?;
                let gc = serde_json::from_reader(File::open(path)?).map_err(|e| {
                    log::error!("Error parsing custom commands: '{}'", e);
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
                serde_json::to_writer(File::create(Self::save_path(g)?)?, &self.cmds[&g])?;
            }
            None => self.cmds.keys().try_for_each(|k| -> Result<(), IoError> {
                serde_json::to_writer(File::create(Self::save_path(*k)?)?, &self.cmds[k])
                    .map_err(|e| e.into())
            })?,
        }
        Ok(())
    }

    pub fn execute(&mut self, guild_id: GuildId, cmd: &str) -> Result<Option<&str>, IoError> {
        match self.load(guild_id) {
            Err(e) if e.kind() == IoErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
            Ok(gc) => Ok(gc.get(cmd).map(String::as_str)),
        }
    }

    pub fn add(&mut self, guild_id: GuildId, cmd: String, ret: String) -> Result<(), IoError> {
        let gc = match self.load(guild_id) {
            Ok(gc) => gc,
            Err(e) if e.kind() == IoErrorKind::NotFound => {
                self.cmds.insert(guild_id, Default::default());
                self.cmds.get_mut(&guild_id).unwrap()
            }
            Err(e) => return Err(e),
        };
        gc.insert(cmd, ret);
        self.save(guild_id)?;
        Ok(())
    }

    pub fn remove(&mut self, guild_id: GuildId, cmd: &str) -> Result<Option<String>, IoError> {
        match self.load(guild_id) {
            Err(e) if e.kind() == IoErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
            Ok(gc) => Ok(gc.remove(cmd)),
        }
    }

    pub fn list(
        &mut self,
        guild_id: GuildId,
    ) -> Result<Option<impl Iterator<Item = (&str, &str)>>, IoError> {
        match self.load(guild_id) {
            Err(e) if e.kind() == IoErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
            Ok(gc) => Ok(Some(gc.iter().map(|(k, v)| (k.as_str(), v.as_str())))),
        }
    }
}
