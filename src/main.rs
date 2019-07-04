mod commands;
mod consts;

use commands::general::GENERAL_GROUP;
use commands::owner::OWNER_GROUP;
use commands::quotes::QUOTES_GROUP;
use commands::sfx::{SfxStats, SFX_ALIASES_GROUP, SFX_GROUP};
use consts::FILES_DIR;

use std::collections::{HashSet, VecDeque};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use chrono::Duration as CDuration;
use chrono::{DateTime, Utc};
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

type VoiceChannelQueue = VecDeque<(DateTime<Utc>, GuildId)>;
#[derive(Clone)]
pub struct VoiceAfkManager {
    channels: Arc<Mutex<VoiceChannelQueue>>,
    voice_manager: Arc<Mutex<ClientVoiceManager>>,
}

impl VoiceAfkManager {
    fn new(voice_manager: Arc<Mutex<ClientVoiceManager>>) -> Self {
        VoiceAfkManager {
            channels: Default::default(),
            voice_manager,
        }
    }

    fn update(&mut self) {
        let now = Utc::now();
        let mut channels = self.channels.lock();
        while let Some(_) = channels.front().filter(|(date, _)| *date < now) {
            let (_, guild_id) = channels.pop_front().unwrap();
            println!(
                "[{:?}] Leaving guild's {} voice channel",
                Utc::now().naive_utc(),
                guild_id
            );
            let mut manager = self.voice_manager.lock();
            manager.leave(guild_id);
        }
    }

    pub fn shedule(&mut self, guild_id: GuildId) {
        let mut channels = self.channels.lock();
        channels.retain(|(_, gid)| guild_id != *gid);
        channels.push_back((
            Utc::now()
                .checked_add_signed(CDuration::minutes(30))
                .unwrap(),
            guild_id,
        ));
        println!(
            "[{:?}] Sheduling for guild: {}",
            Utc::now().naive_utc(),
            guild_id
        );
    }
}

impl TypeMapKey for VoiceAfkManager {
    type Value = VoiceAfkManager;
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
        use std::fs::{DirBuilder, OpenOptions};
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
            use std::io::Write;
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
    {
        let mut data = client.data.write();
        data.insert::<VoiceManager>(Arc::clone(&client.voice_manager));
        data.insert::<VoiceAfkManager>(VoiceAfkManager::new(Arc::clone(&client.voice_manager)));
        data.insert::<SfxStats>(SfxStats::new());
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
                Ok(()) => println!("Processed command {}", cmd_name),
                Err(why) => {
                    let _ = msg.channel_id.say(ctx, &why.0);
                    println!("Command {} failed with {:?}", cmd_name, why)
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
    let mut voice_afk_manager = {
        let v = client.data.read();
        v.get::<VoiceAfkManager>()
            .expect("Couldn't find Voice Afk Manager in ShareMap")
            .clone()
    };

    // AFK Monitoring
    let continue_ruining = Arc::new(AtomicBool::new(true));
    let continue_ruining_clone = Arc::clone(&continue_ruining);
    let thread_handle = thread::spawn(move || {
        while continue_ruining_clone.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(1));
            voice_afk_manager.update();
        }
    });

    if let Err(why) = client.start() {
        println!("Sad face :(  {:?}", why);
    }

    continue_ruining.store(false, Ordering::SeqCst);
    thread_handle.join().unwrap();
    Ok(())
}

#[help]
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
