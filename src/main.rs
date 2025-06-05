#![expect(deprecated)]

use ::daemons::ControlFlow;
use anyhow::Context as _;
use futures::{FutureExt, StreamExt, stream};
use memnarch_rs::features::{
    self, birthdays, custom_commands, mc, moderation, mtg_spoilers, music_channel_broadcast,
    reminders,
};
use memnarch_rs::{
    commands::{command_groups::*, sfx::util::LeaveVoiceDaemons},
    util::consts::FILES_DIR,
};
use pubsub::events;
use serenity::all::standard::Configuration;
use serenity::all::{CreateMessage, Http};
use serenity::{
    framework::standard::{
        Args, CommandError, CommandGroup, CommandResult, DispatchError, HelpOptions,
        StandardFramework, help_commands,
        macros::{help, hook},
    },
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
    io::{self, Read, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::time::timeout;
use tracing::Metadata;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;

use memnarch_rs::util::daemons::{DaemonManager, DaemonManagerKey};
use tracing_subscriber::EnvFilter;

fn load_config() -> std::io::Result<memnarch_rs::Config> {
    DirBuilder::new().recursive(true).create(FILES_DIR)?;
    let config_file_path = [FILES_DIR, "config.toml"].iter().collect::<PathBuf>();
    let mut config_str = String::new();
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(config_file_path)?;
    file.read_to_string(&mut config_str)?;
    Ok(toml::from_str(&config_str).unwrap_or_else(|e| {
        tracing::debug!("failed to parse config: {e}");
        file.set_len(0).expect("Couldn't truncate config file");
        let mut token = String::new();
        print!("Token: ");
        let _ = std::io::stdout().lock().flush();
        std::io::stdin()
            .read_line(&mut token)
            .expect("Couldn't read token from stdin");
        token.pop();

        let config = memnarch_rs::Config::new(token);
        if let Err(e) = toml::to_string_pretty(&config)
            .map_err(io::Error::other)
            .and_then(|config_str| file.write_all(config_str.as_bytes()))
        {
            tracing::error!("Failed to store token: {}", e);
        }
        config
    }))
}

pub struct UpdateNotify;

impl TypeMapKey for UpdateNotify {
    type Value = ChannelId;
}

fn config_logger() {
    let console = tracing_subscriber::fmt::layer()
        .pretty()
        .with_writer(io::stderr);

    let file = tracing_subscriber::fmt::layer().with_writer(|| {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open("memnarch.log")
            .expect("can't create log file")
    });

    let critical_file = tracing_subscriber::fmt::layer().with_writer(|| {
        let home = std::env::var("HOME")
            .map_err(io::Error::other)
            .expect("Can't find home directory");
        let file_path = PathBuf::from_iter([home, "memnarch_critical_error.log".into()]);
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(file_path)
            .expect("can't create critical log file")
    });

    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(console)
            .with(file)
            .with(critical_file)
            .with(EnvFilter::from_default_env())
            .with(filter_fn(|meta: &Metadata| {
                meta.target().starts_with("memnarch_rs") || meta.target().starts_with("daemons")
            })),
    )
    .unwrap();
}

macro_rules! try_init {
    ($d:expr, $m:ident) => {
        if let std::result::Result::Err(e) = $m::initialize(&mut $d).await {
            tracing::error!("Failed to initialize {}: {:?}", stringify!($m), e);
        } else {
            tracing::info!("{} initialized!", stringify!($m));
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
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{}", git_describe::git_describe!());
        return Ok(());
    }
    let config = load_config().context("loading config")?;
    let http = Http::new(&config.token);
    let (owners, bot_id) = match http.get_current_application_info().await {
        Ok(info) => {
            let mut owners = HashSet::new();
            if let Some(team) = info.team {
                owners.insert(team.owner_user_id);
            } else {
                owners.insert(info.owner.expect("cache should be enabled").id);
            }
            match http.get_current_user().await {
                Ok(bot_id) => (owners, bot_id.id),
                Err(why) => {
                    tracing::error!("Could not access current user: {why}");
                    (owners, UserId::new(352881326044741644))
                }
            }
        }
        Err(why) => {
            tracing::error!("Could not access application info: {why}");
            (
                [UserId::new(98500250540478464)].into_iter().collect(),
                UserId::new(352881326044741644),
            )
        }
    };

    let mut client = Client::builder(&config.token, GatewayIntents::all())
        .framework({
            let framework = StandardFramework::new();
            framework.configure(
                Configuration::new()
                    .prefix("|")
                    .no_dm_prefix(true)
                    .on_mention(Some(bot_id))
                    .owners(owners),
            );
            framework
                .after(after)
                .on_dispatch_error(on_dispatch_error)
                .group(&GENERAL_GROUP)
                .group(&BDAYS_GROUP)
                .group(&OWNER_GROUP)
                .group(&QUOTES_GROUP)
                .group(&CUSTOM_GROUP)
                .group(&SFX_GROUP)
                .group(&SFXALIASES_GROUP)
                .group(&TTS_GROUP)
                .group(&MODERATION_GROUP)
                .help(&MY_HELP)
        })
        .register_songbird()
        .type_map_insert::<LeaveVoiceDaemons>(Default::default())
        .event_handler(pubsub::event_handler::Handler::new(bot_id))
        .await
        .expect("Err creating client");
    let mut daemon_manager =
        DaemonManager::spawn(Arc::new((client.cache.clone(), client.http.clone())));
    reminders::load_reminders(&mut daemon_manager)
        .await
        .context("loading reminders")?;
    moderation::reaction_roles::initialize().await?;
    music_channel_broadcast::initialize().await;
    custom_commands::initialize().await;
    let mut daemon_manager = Arc::new(Mutex::new(daemon_manager));
    try_init!(daemon_manager, birthdays);
    try_init!(daemon_manager, mtg_spoilers);
    try_init!(daemon_manager, mc);
    {
        let mut data = client.data.write().await;
        if let Some(id) = std::env::args()
            .skip_while(|x| x != "-r")
            .nth(1)
            .and_then(|id| id.parse::<ChannelId>().ok())
        {
            data.insert::<UpdateNotify>(id);
        }
        data.insert::<DaemonManagerKey>(daemon_manager);
        data.insert::<memnarch_rs::Config>(config);
    }
    pubsub::subscribe::<events::Ready, _>(|ctx, ready| {
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
                "Invite me https://discord.com/oauth2/authorize?client_id={}&scope=bot\n",
                ready.user.id
            );
            if let Some(id) = ctx.data.write().await.remove::<UpdateNotify>() {
                if let Err(e) = id
                    .send_message(&ctx, CreateMessage::new().content("Updated successfully!"))
                    .await
                {
                    tracing::error!("Couldn't send update notification: {}", e);
                }
            }
            ControlFlow::BREAK
        }
        .boxed()
    })
    .await;
    pubsub::subscribe::<events::VoiceStateUpdate, _>(
        |ctx, events::VoiceStateUpdate { new, .. }| {
            async move {
                // Disconnect channel of mirrodin
                if let (Some(gid @ 352399774818762759), Some(id @ 707561909846802462)) = (
                    new.guild_id.map(|i| i.get()),
                    new.channel_id.map(|i| i.get()),
                ) {
                    async fn f(id: ChannelId, gid: GuildId, ctx: &Context) -> anyhow::Result<()> {
                        let c = id.to_channel(ctx).await.and_then(|c| {
                            c.guild()
                                .ok_or(serenity::Error::Other("Not a guild channel"))
                        })?;
                        stream::iter(c.members(ctx)?)
                            .for_each(|mut m| async move {
                                let name = std::mem::take(&mut m.user.name);
                                if let Err(e) = gid.disconnect_member(ctx, m).await {
                                    tracing::error!(
                                    "Failed to disconnect member {} from disconnect channel: {}",
                                    name,
                                    e
                                );
                                }
                            })
                            .await;
                        Ok(())
                    }
                    if let Err(e) = f(id.into(), gid.into(), ctx).await {
                        tracing::error!("Failed to disconnect user: {}", e);
                    }
                }
                ControlFlow::CONTINUE
            }
            .boxed()
        },
    )
    .await;
    pubsub::subscribe::<events::GuildCreate, _>(|_, events::GuildCreate { guild, .. }| {
        async move {
            tracing::info!("found guild {}::{}", guild.name, guild.id);
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
    let task = tokio::task::Builder::new()
        .name("bot-api")
        .spawn(features::api::start((
            client.cache.clone(),
            client.http.clone(),
        )))
        .expect("to be able to launch bot api task");
    tokio::select! {
        r = client.start() => if let Err(why) = r {
            tracing::error!("Sad face :(  {:?}", why);
        },
        _ = tokio::signal::ctrl_c() => {}
    }
    task.abort();
    tracing::info!("waiting for server to shutdown");
    if timeout(Duration::from_secs(10), task).await.is_err() {
        tracing::error!("Server didn't shutdown, forcing it");
        std::process::exit(1);
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
async fn after(ctx: &Context, msg: &Message, cmd_name: &str, error: Result<(), CommandError>) {
    match error {
        Ok(()) => {
            tracing::info!("Processed command '{}' for user '{}'", cmd_name, msg.author)
        }
        Err(why) => {
            let _ = msg.channel_id.say(ctx, why.to_string()).await;
            tracing::error!("Command '{}' failed with {:?}", cmd_name, why)
        }
    }
}

#[hook]
async fn on_dispatch_error(ctx: &Context, msg: &Message, e: DispatchError, command_name: &str) {
    msg.channel_id
        .say(ctx, format!("failed to dispatch {command_name}. {:?}", e))
        .await
        .expect("Couldn't communicate dispatch error");
}
