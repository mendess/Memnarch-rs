#![warn(unused_crate_dependencies)]
#![warn(unused_features)]
#![deny(unused_must_use)]
#![warn(rust_2018_idioms)]

mod birthdays;
mod calendar;
mod commands;
mod consts;
mod cron;
mod curse_of_indicision;
mod daemons;
mod events;
mod file_transaction;
mod health_monitor;
mod permissions;
mod prefs;
mod quiz;
mod reminders;
mod util;

use crate::health_monitor::HealthMonitor;

use self::daemons::DaemonManager;
use ::daemons::ControlFlow;
use anyhow::Context as _;
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
    array::IntoIter as ArrayIter,
    collections::HashSet,
    env,
    fs::{DirBuilder, OpenOptions},
    io::{self, Read, Write},
    path::PathBuf,
    sync::Arc,
};

#[derive(Serialize, Deserialize)]
struct Config {
    token: String,
    monitor_log_channel: Option<ChannelId>,
}

impl Config {
    fn new() -> std::io::Result<Config> {
        DirBuilder::new().recursive(true).create(FILES_DIR)?;
        let config_file_path = [FILES_DIR, "config.toml"].iter().collect::<PathBuf>();
        let mut config_str = String::new();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&config_file_path)?;
        file.read_to_string(&mut config_str)?;
        Ok(toml::from_str(&config_str).unwrap_or_else(|_| {
            file.set_len(0).expect("Couldn't truncate config file");
            let mut token = String::new();
            print!("Token: ");
            let _ = std::io::stdout().lock().flush();
            std::io::stdin()
                .read_line(&mut token)
                .expect("Couldn't read token from stdin");
            token.pop();

            let config = Config {
                token,
                monitor_log_channel: None,
            };
            if let Err(e) = toml::to_string_pretty(&config)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                .and_then(|config_str| file.write_all(config_str.as_bytes()))
            {
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
        // .add_filter_allow_str(stringify!(daemons))
        .set_thread_level(LevelFilter::Off)
        .set_location_level(LevelFilter::Error)
        .set_level_padding(LevelPadding::Right)
        .set_target_level(LevelFilter::Off)
        .set_time_format_str("%F %T")
        .build();

    let term = TermLogger::new(
        LevelFilter::Trace,
        config.clone(),
        TerminalMode::Stdout,
        ColorChoice::AlwaysAnsi,
    );
    let file = WriteLogger::new(
        LevelFilter::Info,
        config.clone(),
        OpenOptions::new()
            .write(true)
            .append(true)
            .create(true)
            .open("memnarch.log")
            .expect("can't create log file"),
    );
    let critical_log = WriteLogger::new(LevelFilter::Error, config, {
        let home = env::var("HOME")
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            .expect("Can't find home directory");
        let file_path =
            ArrayIter::new([home, "memnarch_critical_error.log".into()]).collect::<PathBuf>();
        OpenOptions::new()
            .write(true)
            .append(true)
            .create(true)
            .open(file_path)
            .expect("can't create critical log file")
    });
    CombinedLogger::init(vec![term, file, critical_log]).unwrap();
}

macro_rules! try_init {
    ($d:expr, $m:ident) => {
        if let std::result::Result::Err(e) = $m::initialize(&mut $d).await {
            log::error!("Failed to initialize {}: {:?}", stringify!($m), e);
        } else {
            log::info!("{} initialized!", stringify!($m));
        }
    };
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
                ArrayIter::new([UserId(98500250540478464)]).collect(),
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
                .group(&BDAYS_GROUP)
                .group(&OWNER_GROUP)
                .group(&QUOTES_GROUP)
                .group(&CUSTOM_GROUP)
                .group(&INTERRAIL_GROUP)
                .group(&SFX_GROUP)
                .group(&SFXALIASES_GROUP)
                .group(&TTS_GROUP)
                .group(&CALENDAR_GROUP)
                .group(&QUIZ_GROUP)
                .group(&PY_GROUP)
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
    reminders::load_reminders(&mut daemon_manager)
        .await
        .context("loading reminders")?;
    calendar::initialize(&mut daemon_manager).await;
    try_init!(daemon_manager, quiz);
    if let Some(channel) = config.monitor_log_channel {
        daemon_manager.add_daemon(HealthMonitor::new(channel)).await;
    }
    let mut daemon_manager = Arc::new(Mutex::new(daemon_manager));
    try_init!(daemon_manager, birthdays);
    try_init!(daemon_manager, curse_of_indicision);
    {
        let mut data = client.data.write().await;
        if let Some(id) = std::env::args()
            .skip_while(|x| x != "-r")
            .nth(1)
            .and_then(|id| id.parse::<ChannelId>().ok())
        {
            data.insert::<events::UpdateNotify>(id);
        }
        data.insert::<DaemonManager>(daemon_manager);
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
