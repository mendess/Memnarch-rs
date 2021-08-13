#![warn(unused_crate_dependencies)]
#![warn(unused_features)]
#![deny(unused_must_use)]
#![warn(rust_2018_idioms)]

mod calendar;
mod commands;
mod consts;
mod daemons;
mod events;
mod file_transaction;
mod permissions;
mod reminders;
mod user_prefs;
mod util;

use self::daemons::DaemonManager;
use ::daemons::ControlFlow;
use commands::{
    command_groups::*,
    custom::CustomCommands,
    interrail::InterrailConfig,
    sfx::{util::LeaveVoiceDaemons, SfxStats},
};
use consts::FILES_DIR;
use serde::{Deserialize, Serialize};
use serenity::{
    client::bridge::gateway::GatewayIntents,
    framework::standard::{
        help_commands,
        macros::{help, hook},
        Args, CommandError, CommandGroup, CommandResult, DispatchError, HelpOptions,
        StandardFramework,
    },
    http::client::Http,
    model::{
        channel::Message,
        id::{ChannelId, GuildId, UserId},
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
use anyhow::Context as _;

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
            token.pop();

            let config = Config { token };
            if let Err(e) = serde_json::to_writer_pretty(file, &config) {
                log::error!("Failed to store token: {}", e);
            }
            config
        }))
    }
}

fn config_logger() {
    use simplelog::*;
    let config = ConfigBuilder::new()
        .add_filter_allow_str(module_path!())
        .add_filter_allow_str(stringify!(daemons))
        .set_thread_level(LevelFilter::Off)
        .set_location_level(LevelFilter::Error)
        .set_level_padding(LevelPadding::Right)
        .set_target_level(LevelFilter::Off)
        .build();
    let term = TermLogger::new(
        LevelFilter::Trace,
        config.clone(),
        TerminalMode::Stdout,
        ColorChoice::AlwaysAnsi,
    );
    let file = WriteLogger::new(
        LevelFilter::Info,
        config,
        OpenOptions::new()
            .write(true)
            .append(true)
            .create(true)
            .open("memnarch.log")
            .expect("can create log file"),
    );
    CombinedLogger::init(vec![term, file]).unwrap();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!(
        "
========================================
        ┏┳┓┏━╸┏┳┓┏┓╻┏━┓┏━┓┏━╸╻ ╻
        ┃┃┃┣╸ ┃┃┃┃┗┫┣━┫┣┳┛┃  ┣━┫
        ╹ ╹┗━╸╹ ╹╹ ╹╹ ╹╹┗╸┗━╸╹ ╹
========================================
        "
    );
    config_logger();
    let config = Config::new().context("loading config")?;
    let http = Http::new_with_token(&config.token);
    let (owners, bot_id) = match http.get_current_application_info().await {
        Ok(info) => {
            let mut owners = HashSet::new();
            owners.insert(info.owner.id);
            (owners, Some(info.id))
        }
        Err(why) => {
            log::error!("Could not access application info: {}", why);
            (
                std::array::IntoIter::new([UserId(98500250540478464)]).collect(),
                Some(UserId(352881326044741644)),
            )
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
                .group(&CALENDAR_GROUP)
                .help(&MY_HELP),
        )
        .intents(GatewayIntents::all())
        .register_songbird()
        .type_map_insert::<CustomCommands>(Arc::new(RwLock::new(CustomCommands::default())))
        .type_map_insert::<InterrailConfig>(Arc::new(RwLock::new(InterrailConfig::new())))
        .type_map_insert::<LeaveVoiceDaemons>(Default::default())
        .type_map_insert::<SfxStats>(Arc::new(Mutex::new(SfxStats::new())))
        .event_handler(events::Handler)
        .await
        .expect("Err creating client");
    let mut daemon_manager = self::daemons::DaemonManager::new(client.cache_and_http.clone());
    reminders::load_reminders(&mut daemon_manager).await.context("loading reminders")?;
    calendar::initialize(&mut daemon_manager).await;
    {
        let mut data = client.data.write().await;
        if let Some(id) = std::env::args()
            .skip_while(|x| x != "-r")
            .nth(1)
            .and_then(|id| id.parse::<ChannelId>().ok())
        {
            data.insert::<events::UpdateNotify>(id);
        }
        data.insert::<DaemonManager>(Arc::new(Mutex::new(daemon_manager)));
    }
    use events::{pubsub::events::Ready, UpdateNotify};
    events::pubsub::register::<Ready, _>(|ctx, ready| {
        use futures::prelude::*;
        async move {
            println!(
                "
░█░█░█▀█░░░█▀█░█▀█░█▀▄░░░█▀▄░█░█░█▀█░█▀█░▀█▀░█▀█░█▀▀
░█░█░█▀▀░░░█▀█░█░█░█░█░░░█▀▄░█░█░█░█░█░█░░█░░█░█░█░█
░▀▀▀░▀░░░░░▀░▀░▀░▀░▀▀░░░░▀░▀░▀▀▀░▀░▀░▀░▀░▀▀▀░▀░▀░▀▀▀
                "
            );
            println!(
                "Invite me https://discord.com/oauth2/authorize?client_id={}&scope=bot",
                ready.user.id
            );
            if let Some(id) = ctx.data.write().await.remove::<UpdateNotify>() {
                if let Err(e) = id
                    .send_message(&ctx, |m| m.content("Updated successfully!"))
                    .await
                {
                    log::error!("Couldn't send update notification: {}", e);
                }
            }
            ControlFlow::BREAK
        }
        .boxed()
    });

    if let Err(why) = client.start().await {
        log::error!("Sad face :(  {:?}", why);
    }
    Ok(())
}

#[help]
#[max_levenshtein_distance(5)]
#[lacking_permissions("hide")]
#[wrong_channel("hide")]
#[lacking_conditions("hide")]
#[lacking_ownership("hide")]
#[strikethrough_commands_tip_in_guild(" ")]
#[strikethrough_commands_tip_in_dm(" ")]
#[embed_success_colour("#71A5B0")]
#[indention_prefix("- ")]
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
    if !msg.content.starts_with('|') {
        return;
    }
    async fn f(ctx: &Context, msg: &Message, g: GuildId) -> anyhow::Result<()> {
        let cmd = match &msg.content.split_whitespace().next() {
            Some(s) if !s.is_empty() => &s[1..],
            _ => return Ok(()),
        };
        log::trace!("looking for command: {}", cmd);
        if let Some(o) = crate::get!(mut ctx, CustomCommands, write).execute(g, cmd)? {
            msg.channel_id.say(&ctx, o).await?;
        }
        Ok(())
    }
    if let Some(g) = msg.guild_id {
        if let Err(e) = f(ctx, msg, g).await {
            log::error!("Custom command failed: {:?}", e);
        }
    }
}

#[hook]
async fn after(ctx: &Context, msg: &Message, cmd_name: &str, error: Result<(), CommandError>) {
    match error {
        Ok(()) => {
            log::trace!("Processed command '{}' for user '{}'", cmd_name, msg.author)
        }
        Err(why) => {
            let _ = msg.channel_id.say(ctx, &why).await;
            log::trace!("Command '{}' failed with {:?}", cmd_name, why)
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

#[macro_export]
macro_rules! get {
    ($ctx:ident, $t:ty) => {
        $ctx.data.read().await.get::<$t>().expect(::std::concat!(
            ::std::stringify!($t),
            " was not initialized"
        ))
    };
    (mut $ctx:ident, $t:ty) => {
        $ctx.data
            .write()
            .await
            .get_mut::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
    };
    ($ctx:ident, $t:ty, $lock:ident) => {
        $ctx.data
            .read()
            .await
            .get::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
            .$lock()
            .await
    };
    (mut $ctx:ident, $t:ty, $lock:ident) => {
        $ctx.data
            .write()
            .await
            .get_mut::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
            .$lock()
            .await
    };
    (> $data:ident, $t:ty) => {
        $data.get::<$t>().expect(::std::concat!(
            ::std::stringify!($t),
            " was not initialized"
        ))
    };
    (> $data:ident, $t:ty, $lock:ident) => {
        $data
            .get::<$t>()
            .expect(::std::concat!(
                ::std::stringify!($t),
                " was not initialized"
            ))
            .$lock()
            .await
    };
}
