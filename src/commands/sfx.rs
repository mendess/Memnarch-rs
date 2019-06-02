use crate::consts::FILES_DIR;
use crate::{VoiceAfkManager, VoiceManager};

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use itertools::Itertools;
use serenity::{
    framework::standard::{
        macros::{check, command, group},
        Args, CheckResult, CommandOptions, CommandResult, Reason,
    },
    model::channel::Message,
    prelude::*,
    voice,
};

const SFX_FILES_DIR: &str = "sfx";

group!({
    name: "SFX",
    options: {
        prefixes: ["sfx"],
    },
    commands: [list, add, play, delete, retreive],
});

#[command]
#[min_args(1)]
fn play(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
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
    {
        let share_map = ctx.data.read();
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
        let file = find_file(&args.single::<String>().unwrap())?;
        if let Some(handler) = manager.get_mut(guild_id) {
            let source = voice::ffmpeg(file)?;
            handler.play(source);
        } else {
            Err("Not in a voice channel".to_string())?;
        }
    }
    let mut share_map = ctx.data.write();
    share_map
        .get_mut::<VoiceAfkManager>()
        .expect("Expected VoiceManager in ShareMap")
        .shedule(guild_id);
    Ok(())
}

#[command]
fn list(ctx: &mut Context, msg: &Message) -> CommandResult {
    let sounds = fs::read_dir(format!("{}/{}", FILES_DIR, SFX_FILES_DIR)).map(|x| {
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
            match sounds {
                Err(_) => e.fields(vec![("**No files :(**", "", false)]),
                Ok(files) => e.fields(files.iter().chunks(12).into_iter().map(|x| {
                    let f = x.collect::<Vec<_>>();
                    let c1 = f[0].to_uppercase().chars().next().unwrap();
                    let c2 = f[f.len() - 1].to_uppercase().chars().next().unwrap();
                    (
                        [c1, '-', c2].iter().collect::<String>(),
                        f.iter().fold(String::new(), |acc, x| acc + "\n" + x),
                        true,
                    )
                })),
            }
        })
    })?;
    Ok(())
}

#[check]
#[name = "is_friend"]
fn is_friend(_: &mut Context, msg: &Message, _: &mut Args, _: &CommandOptions) -> CheckResult {
    msg.guild_id
        .and_then(|id| {
            if id.0 == 136_220_994_812_641_280 {
                Some(CheckResult::Success)
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            CheckResult::Failure(Reason::User(
                "You don't have permission to use that command!".to_string(),
            ))
        })
}

#[command]
#[checks("is_friend")]
fn add(ctx: &mut Context, msg: &Message) -> CommandResult {
    for attachment in msg.attachments.iter() {
        if attachment.size > 204_800 {
            return Err("File size too high, please keep it under 200Kb."
                .to_string()
                .into());
        }
        let bytes = attachment.download()?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(format!("{}/sfx/{}", FILES_DIR, attachment.filename))?;
        file.write_all(&bytes)?;
        msg.channel_id.say(&ctx, "File added!")?;
    }
    Ok(())
}

#[command]
#[min_args(1)]
#[owners_only]
fn delete(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let file: PathBuf = find_file(&args.single::<String>().unwrap())?;
    msg.channel_id.send_message(&ctx, |m| m.add_file(&file))?;
    fs::remove_file(&file)?;
    Ok(())
}

#[command]
#[min_args(1)]
fn retreive(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let file = find_file(&args.single::<String>().unwrap())?;
    msg.channel_id.send_message(&ctx, |m| m.add_file(&file))?;
    Ok(())
}

fn find_file(search_string: &str) -> io::Result<PathBuf> {
    use std::io::{Error, ErrorKind::NotFound};
    fs::read_dir(format!("{}/{}", FILES_DIR, SFX_FILES_DIR))?
        .filter_map(Result::ok)
        .find(|entry| match entry.file_name().to_str() {
            Some(name) => name.contains(search_string),
            None => false,
        })
        .ok_or_else(|| Error::new(NotFound, format!("No matches for {}", search_string)))
        .map(|x| x.path())
}
