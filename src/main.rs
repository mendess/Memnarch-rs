#![cfg_attr(feature = "nightly", feature(drain_filter))]

mod commands;
mod consts;
mod cron;
mod permissions;

use commands::general::GENERAL_GROUP;
use commands::owner::OWNER_GROUP;
use commands::quotes::QUOTES_GROUP;
use commands::sfx::{SfxStats, SFX_ALIASES_GROUP, SFX_GROUP};
use consts::FILES_DIR;
use serde::{Deserialize, Serialize};
use serenity::{
    client::bridge::voice::ClientVoiceManager,
    framework::standard::{
        help_commands, macros::help, Args, CommandGroup, CommandResult, HelpOptions,
        StandardFramework,
    },
    model::{
        channel::{Channel, Message},
        gateway::Ready,
        id::{ChannelId, GuildId, UserId},
        voice::VoiceState,
    },
    prelude::*,
};
use std::collections::{HashSet, };
use std::fs::{DirBuilder, OpenOptions};
use std::io::Write;
use std::sync::Arc;

struct Handler;

impl EventHandler for Handler {
    fn voice_state_update(
        &self,
        ctx: Context,
        guild_id: Option<GuildId>,
        old: Option<VoiceState>,
        _new: VoiceState,
    ) {
        if old
            .and_then(|vs| vs.channel_id)
            .and_then(|id| id.to_channel(&ctx).ok())
            .and_then(Channel::guild)
            .and_then(|gc| gc.read().members(&ctx).ok().map(|m| m.len()))
            .filter(|n_members| *n_members >= 1)
            .is_some()
        {
            if let Some(guild_id) = guild_id {
                ctx.data
                    .read()
                    .get::<VoiceManager>()
                    .expect("Couldn't find VoiceManager in ShareMap")
                    .lock()
                    .leave(guild_id);
            };
        }
    }

    fn ready(&self, ctx: Context, _ready: Ready) {
        println!("Up and running");
        if let Some(id) = ctx.data.read().get::<UpdateNotify>() {
            ChannelId::from(**id)
                .send_message(&ctx, |m| m.content("Updated successfully!"))
                .expect("Couldn't send update notification");
        }
        ctx.data.write().remove::<UpdateNotify>();
    }
}

pub struct VoiceManager;

impl TypeMapKey for VoiceManager {
    type Value = Arc<Mutex<ClientVoiceManager>>;
}

struct UpdateNotify;

impl TypeMapKey for UpdateNotify {
    type Value = Arc<u64>;
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
            let mut file = OpenOptions::new()
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
            let _ = file.write_all(serde_json::to_string(&config).unwrap().as_bytes());
            config
        }))
    }
}

fn main() -> std::io::Result<()> {
    let config = Config::new()?;
    let mut client = Client::new(&config.token, Handler).expect("Err creating client");
    let cron_sink = cron::start(Arc::clone(&client.cache_and_http.http));
    {
        let mut data = client.data.write();
        data.insert::<VoiceManager>(Arc::clone(&client.voice_manager));
        data.insert::<SfxStats>(SfxStats::new());
        data.insert::<cron::CronSink>(cron_sink);
        if let Some(id) = std::env::args()
            .skip_while(|x| x != "-r")
            .nth(1)
            .and_then(|id| id.parse::<u64>().ok())
        {
            data.insert::<UpdateNotify>(Arc::new(id));
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
            .configure(|c| c.prefix("|").on_mention(Some(bot_id)).owners(owners))
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
            .group(&SFX_ALIASES_GROUP)
            .group(&OWNER_GROUP)
            .group(&QUOTES_GROUP)
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
fn my_help(
    context: &mut Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    help_commands::with_embeds(context, msg, args, help_options, groups, owners)
}
