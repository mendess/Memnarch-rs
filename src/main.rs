#![warn(unused_crate_dependencies)]
#![warn(unused_features)]
#![deny(unused_results)]
// TODO: remove
#![allow(unused_imports)]
#![cfg_attr(feature = "nightly", feature(drain_filter))]

mod commands;
mod consts;
mod cron;
mod permissions;

use chrono::{Duration, Utc};
use commands::{
    custom::{CustomCommands, MessageDecay, CUSTOM_GROUP},
    general::{Reminder, GENERAL_GROUP},
    interrail::{InterrailConfig, INTERRAIL_GROUP},
    owner::OWNER_GROUP,
    quotes::QUOTES_GROUP,
    // sfx::{LeaveVoice, SfxStats, SFXALIASES_GROUP, SFX_GROUP},
    // tts::TTS_GROUP,
};
use consts::FILES_DIR;
use cron::{CronSink, Task};
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        help_commands, macros::help, Args, CommandGroup, CommandResult, HelpOptions,
        StandardFramework,
    },
    http::client::Http,
    model::{
        channel::{Channel, Message},
        gateway::Ready,
        guild::Member,
        id::{ChannelId, GuildId, UserId},
        voice::VoiceState,
    },
    prelude::*,
};
use std::{
    collections::HashSet,
    fs::{DirBuilder, OpenOptions},
    io::Write,
    sync::Arc,
};
use songbird::SerenityInit;

struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn voice_state_update(
        &self,
        ctx: Context,
        guild_id: Option<GuildId>,
        old: Option<VoiceState>,
        new: VoiceState,
    ) {
        let current_user = match Http::get_current_user(ctx.as_ref()) {
            Ok(user) => user,
            Err(e) => return eprintln!("Failed to get current user {:?}", e),
        };
        let has_bot = |members: &Vec<Member>| {
            members
                .iter()
                .map(|m| m.user.id)
                .any(|u| current_user.id == u)
        };
        if old
            .and_then(|vs| vs.channel_id)
            .and_then(|id| id.to_channel(&ctx).await.ok())
            .and_then(Channel::guild)
            .and_then(|gc| gc.read().members(&ctx).ok())
            .filter(has_bot)
            .map(|m| m.len() == 1)
            .unwrap_or(false)
        {
            if let Some(guild_id) = guild_id {
                ctx.data
                    .read().await
                    .get::<VoiceManager>()
                    .expect("Couldn't find VoiceManager in ShareMap")
                    .lock()
                    .leave(guild_id);
                ctx.data
                    .read().await
                    .get::<CronSink<LeaveVoice>>()
                    .unwrap()
                    .cancel(guild_id)
                    .map_err(|e| eprintln!("Failed to cancel a leave voice cron {:?}", e))
                    .ok();
            };
        }
        // Disconnect channel of mirrodin
        if let (Some(gid @ GuildId(352399774818762759)), Some(id @ ChannelId(707561909846802462))) =
            (guild_id, new.channel_id)
        {
            id.to_channel(&ctx)
                .and_then(|c| {
                    c.guild()
                        .ok_or_else(|| serenity::Error::Other("Not a guild channel"))
                })
                .and_then(|c| c.read().members(&ctx))
                .and_then(|members| {
                    members
                        .iter()
                        .try_for_each(|m| gid.disconnect_member(&ctx, m))
                })
                .map_err(|e| eprintln!("Failed to disconnect user: {}", e))
                .ok();
        }
    }

    async fn ready(&self, ctx: Context, _ready: Ready) {
        println!("Up and running");
        if let Some(id) = ctx.data.read().await.get::<UpdateNotify>() {
            id.send_message(&ctx, |m| m.content("Updated successfully!")).await
                .expect("Couldn't send update notification");
        }
        ctx.data.write().await.remove::<UpdateNotify>();
    }
}

struct UpdateNotify;

impl TypeMapKey for UpdateNotify {
    type Value = ChannelId;
}

#[derive(Serialize, Deserialize)]
struct Config {
    token: String,
}

impl Config {
    fn new() -> std::io::Result<Config> {
        DirBuilder::new().recursive(true).create(FILES_DIR)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(format!("{}/config.json", FILES_DIR))?;
        Ok(serde_json::from_reader(file).unwrap_or_else(|_| {
            let file = OpenOptions::new()
                .write(true)
                .open(format!("{}/config.json", FILES_DIR))
                .expect("Couldn't open config for writing");

            let mut token = String::new();
            print!("Token: ");
            let _ = std::io::stdout().lock().flush();
            std::io::stdin()
                .read_line(&mut token)
                .expect("Couldn't read token from stdin");

            let config = Config { token };
            let _ = serde_json::to_writer(file, &config).map_err(|e| eprintln!("{}", e));
            config
        }))
    }
}

impl<T: Task + 'static> TypeMapKey for CronSink<T> {
    type Value = Self;
}

fn main() -> std::io::Result<()> {
    let config = Config::new()?;
    let mut client = Client::new(&config.token, Handler).expect("Err creating client");
    let re_cron_sink =
        cron::start::<Reminder>("reminders.json", Arc::clone(&client.cache_and_http.http));
    let vc_cron_sink = cron::start::<LeaveVoice>("voice.json", Arc::clone(&client.voice_manager));
    let md_cron_sink = cron::start::<MessageDecay>(
        "message_decay.json",
        Arc::clone(&client.cache_and_http.http),
    );
    {
        let mut data = client.data.write();
        // data.insert::<VoiceManager>(Arc::clone(&client.voice_manager));
        data.insert::<SfxStats>(SfxStats::new());
        data.insert::<CronSink<Reminder>>(re_cron_sink);
        data.insert::<CronSink<LeaveVoice>>(vc_cron_sink);
        data.insert::<CronSink<MessageDecay>>(md_cron_sink);
        data.insert::<CustomCommands>(Arc::new(RwLock::new(Default::default())));
        data.insert::<InterrailConfig>(Arc::new(RwLock::new(InterrailConfig::new())));
        if let Some(id) = std::env::args()
            .skip_while(|x| x != "-r")
            .nth(1)
            .and_then(|id| id.parse::<ChannelId>().ok())
        {
            data.insert::<UpdateNotify>(id);
        }
    }
    let (owners, bot_id) = match client.cache_and_http.http.get_current_application_info() {
        Ok(info) => {
            let mut owners = HashSet::new();
            owners.insert(info.owner.id);

            (owners, info.id)
        }
        Err(why) => panic!("Could not access application info: {:?}", why),
    };
    client.with_framework(
        StandardFramework::new()
            .register_songbird()
            .configure(|c| c.prefix("|").on_mention(Some(bot_id)).owners(owners))
            .normal_message(move |ctx, msg| {
                if msg.author.id == bot_id {
                    return;
                }
                if !msg.content.starts_with("|") {
                    return;
                }
                println!("looking for command: {}", msg.content);
                let _ = msg
                    .guild_id
                    .ok_or_else(|| "guild_id is missing".to_string())
                    .and_then(|g| {
                        let mut share_map = ctx.data.write();
                        let decay = if let Some((o, decay)) = share_map
                            .get_mut::<CustomCommands>()
                            .unwrap()
                            .write()
                            .execute(
                                g,
                                &msg.content
                                    .split_whitespace()
                                    .next()
                                    .map(|s| &s[1..])
                                    .unwrap_or(""),
                            )
                            .map_err(|e| e.to_string())?
                        {
                            let m = msg.channel_id.say(&ctx, o).map_err(|e| e.to_string())?;
                            Some((*decay, m))
                        } else {
                            None
                        };
                        if let Some((true, m)) = decay {
                            let cron = share_map.get_mut::<CronSink<MessageDecay>>().unwrap();
                            cron.send(MessageDecay::new(m, Utc::now() + Duration::hours(1)))
                                .map_err(|_| "Couldn't decay bot message".to_string())?;
                            cron.send(MessageDecay::new(
                                msg.clone(),
                                Utc::now() + Duration::minutes(30),
                            ))
                            .map_err(|_| "Couldn't decay user message".to_string())?;
                        }
                        Ok(())
                    })
                    .map_err(|e| eprintln!("{}", e));
            })
            .after(|ctx, msg, cmd_name, error| match error {
                Ok(()) => println!("Processed command '{}' for user '{}'", cmd_name, msg.author),
                Err(why) => {
                    let _ = msg.channel_id.say(ctx, &why.0);
                    println!("Command '{}' failed with {:?}", cmd_name, why)
                }
            })
            .on_dispatch_error(|ctx, msg, error| {
                msg.channel_id
                    .say(ctx, format!("{:?}", error))
                    .expect("Couldn't communicate dispatch error");
            })
            .group(&GENERAL_GROUP)
            .group(&SFX_GROUP)
            .group(&SFXALIASES_GROUP)
            .group(&OWNER_GROUP)
            .group(&QUOTES_GROUP)
            .group(&CUSTOM_GROUP)
            .group(&INTERRAIL_GROUP)
            .group(&TTS_GROUP)
            .help(&MY_HELP),
    );

    if let Err(why) = client.start() {
        println!("Sad face :(  {:?}", why);
    }
    Ok(())
}

#[help]
#[max_levenshtein_distance(5)]
#[lacking_permissions("hide")]
#[strikethrough_commands_tip_in_guild(" ")]
#[strikethrough_commands_tip_in_dm(" ")]
async fn my_help(
    context: &mut Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    help_commands::with_embeds(context, msg, args, help_options, groups, owners)
}
