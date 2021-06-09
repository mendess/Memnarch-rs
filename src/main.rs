#![warn(unused_crate_dependencies)]
#![warn(unused_features)]
#![deny(unused_must_use)]
#![cfg_attr(feature = "nightly", feature(drain_filter))]

mod commands;
mod consts;
mod daemons;
mod file_transaction;
mod permissions;
mod reminders;

use self::daemons::DaemonManager;
use commands::{
    command_groups::*,
    custom::CustomCommands,
    interrail::InterrailConfig,
    sfx::{util::LeaveVoiceDaemons, SfxStats},
};
use consts::FILES_DIR;
use futures::prelude::*;
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        help_commands,
        macros::{help, hook},
        Args, CommandError, CommandGroup, CommandResult, DispatchError, HelpOptions,
        StandardFramework,
    },
    http::client::Http,
    model::{
        channel::Message,
        gateway::Ready,
        guild::Member,
        id::{ChannelId, GuildId, UserId},
        voice::VoiceState,
    },
    prelude::*,
};
use songbird::SerenityInit;
use std::{
    collections::HashSet,
    fs::{DirBuilder, OpenOptions},
    io::Write,
    sync::Arc,
};

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
        let current_user = match Http::get_current_user(ctx.as_ref()).await {
            Ok(user) => user,
            Err(e) => return eprintln!("Failed to get current user {:?}", e),
        };
        let has_bot = |members: Vec<Member>| {
            members
                .iter()
                .map(|m| m.user.id)
                .any(|u| current_user.id == u)
        };
        async fn members(id: ChannelId, ctx: &Context) -> Option<Vec<Member>> {
            id.to_channel(ctx)
                .await
                .ok()?
                .guild()?
                .members(ctx)
                .await
                .ok()
        }
        if let Some(id) = old.and_then(|vs| vs.channel_id) {
            if members(id, &ctx)
                .await
                .filter(|m| m.len() == 1)
                .map(has_bot)
                .unwrap_or(false)
            {
                if let Some(guild_id) = guild_id {
                    let sb = songbird::get(&ctx).await.expect("Songbird not initialized");
                    if let Err(e) = sb.remove(guild_id).await {
                        println!("Could not leave voice channel: {}", e);
                        return;
                    }
                    let data = ctx.data.read().await;
                    let dm = data.get::<DaemonManager>().expect("DaemonManager");
                    let lvc = data.get::<LeaveVoiceDaemons>().expect("LeaveVoiceDaemons");
                    lvc.lock()
                        .await
                        .remove(&mut *dm.lock().await, guild_id)
                        .await;
                };
            }
        }
        // Disconnect channel of mirrodin
        if let (
            Some(gid @ GuildId(352399774818762759)),
            Some(id @ ChannelId(707561909846802462)),
        ) = (guild_id, new.channel_id)
        {
            async fn f(id: ChannelId, gid: GuildId, ctx: &Context) -> CommandResult {
                let c = id.to_channel(ctx).await.and_then(|c| {
                    c.guild()
                        .ok_or_else(|| serenity::Error::Other("Not a guild channel"))
                })?;
                let members = c.members(ctx).await?;
                stream::iter(members)
                    .for_each(|m| async move {
                        if let Err(e) = gid.disconnect_member(ctx, m).await {
                            eprintln!(
                                "Failed to disconnect member from disconnect channel: {}",
                                e
                            );
                        }
                    })
                    .await;
                Ok(())
            }
            if let Err(e) = f(id, gid, &ctx).await {
                eprintln!("Failed to disconnect user: {}", e);
            }
        }
    }

    async fn ready(&self, ctx: Context, _ready: Ready) {
        println!("Up and running");
        if let Some(id) = ctx.data.read().await.get::<UpdateNotify>() {
            id.send_message(&ctx, |m| m.content("Updated successfully!"))
                .await
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

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = Config::new()?;
    let http = Http::new_with_token(&config.token);
    let (owners, bot_id) = match http.get_current_application_info().await {
        Ok(info) => {
            let mut owners = HashSet::new();
            owners.insert(info.owner.id);
            (owners, Some(info.id))
        }
        Err(why) => {
            eprintln!("Could not access application info: {:?}", why);
            (HashSet::new(), None)
        }
    };
    let mut client = Client::builder(&config.token)
        .framework(
            StandardFramework::new()
                .configure(|c| {
                    c.prefix("|")
                        .no_dm_prefix(true)
                        .on_mention(bot_id)
                        .owners(owners)
                })
                .normal_message(normal_message)
                .after(after)
                .on_dispatch_error(on_dispatch_error)
                .group(&GENERAL_GROUP)
                .group(&OWNER_GROUP)
                .group(&QUOTES_GROUP)
                .group(&CUSTOM_GROUP)
                .group(&INTERRAIL_GROUP)
                .group(&SFX_GROUP)
                .group(&SFXALIASES_GROUP)
                .group(&TTS_GROUP)
                .help(&MY_HELP),
        )
        .register_songbird()
        .type_map_insert::<CustomCommands>(Arc::new(RwLock::new(CustomCommands::default())))
        .type_map_insert::<InterrailConfig>(Arc::new(RwLock::new(InterrailConfig::new())))
        .type_map_insert::<LeaveVoiceDaemons>(Default::default())
        .type_map_insert::<SfxStats>(SfxStats::new())
        .event_handler(Handler)
        .await
        .expect("Err creating client");
    let mut daemon_manager = self::daemons::DaemonManager::new(client.cache_and_http.clone());
    reminders::load_reminders(&mut daemon_manager).await?;
    {
        let mut data = client.data.write().await;
        if let Some(id) = std::env::args()
            .skip_while(|x| x != "-r")
            .nth(1)
            .and_then(|id| id.parse::<ChannelId>().ok())
        {
            data.insert::<UpdateNotify>(id);
        }
        data.insert::<DaemonManager>(Arc::new(Mutex::new(daemon_manager)));
    }

    if let Err(why) = client.start().await {
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
    context: &Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    let _ = help_commands::with_embeds(context, msg, args, help_options, groups, owners).await;
    Ok(())
}

#[hook]
async fn normal_message(ctx: &Context, msg: &Message) {
    if ctx.cache.current_user_id().await == msg.author.id {
        return;
    }
    if !msg.content.starts_with("|") {
        return;
    }
    println!("looking for command: {}", msg.content);
    async fn f(ctx: &Context, msg: &Message, g: GuildId) -> CommandResult {
        let mut share_map = ctx.data.write().await;
        if let Some(o) = share_map
            .get_mut::<CustomCommands>()
            .unwrap()
            .write()
            .await
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
            msg.channel_id
                .say(&ctx, o)
                .map_err(|e| e.to_string())
                .await?;
        }
        Ok(())
    }
    match msg.guild_id {
        Some(g) => {
            if let Err(e) = f(ctx, msg, g).await {
                eprintln!("Custom command failed: {:?}", e);
            }
        }
        None => {
            eprintln!("guild_id is missing");
        }
    }
}

#[hook]
async fn after(ctx: &Context, msg: &Message, cmd_name: &str, error: Result<(), CommandError>) {
    match error {
        Ok(()) => {
            println!("Processed command '{}' for user '{}'", cmd_name, msg.author)
        }
        Err(why) => {
            let _ = msg.channel_id.say(ctx, &why).await;
            println!("Command '{}' failed with {:?}", cmd_name, why)
        }
    }
}

#[hook]
async fn on_dispatch_error(ctx: &Context, msg: &Message, e: DispatchError) {
    msg.channel_id
        .say(ctx, format!("{:?}", e))
        .await
        .expect("Couldn't communicate dispatch error");
}
