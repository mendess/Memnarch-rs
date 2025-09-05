use anyhow::Context as _;
use futures::FutureExt;
use memnarch_rs::commands::command_groups;
use memnarch_rs::features;
use pubsub::events;
use songbird::SerenityInit;
use std::{
    fs::{DirBuilder, OpenOptions},
    io::{self, Read, Write},
    ops::ControlFlow,
    time::Duration,
};
use tokio::time::timeout;
use tracing::Metadata;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;

use mappable_rc::Marc;
use memnarch_rs::{Bot, in_files};
use serenity::{Client, all::GatewayIntents};
use tracing_subscriber::EnvFilter;

fn load_config() -> std::io::Result<memnarch_rs::Config> {
    DirBuilder::new().recursive(true).create(in_files!())?;
    let config_file_path = in_files!("config.toml");
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

fn config_logger() {
    let console = tracing_subscriber::fmt::layer()
        .pretty()
        .with_writer(io::stderr);

    let file = tracing_subscriber::fmt::layer().with_writer(|| {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open("logs/memnarch.log")
            .expect("can't create log file")
    });

    let critical_file = tracing_subscriber::fmt::layer().with_writer(|| {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open("/logs/memnarch_critical_error.log")
            .expect("can't create critical log file")
    });

    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(console)
            .with(file)
            .with(critical_file)
            .with(
                EnvFilter::builder()
                    .with_default_directive(tracing::Level::INFO.into())
                    .from_env_lossy(),
            )
            .with(filter_fn(|meta: &Metadata| {
                meta.target().starts_with("memnarch_rs") || meta.target().starts_with("daemons")
            })),
    )
    .unwrap();
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
    let mut client = Client::builder(
        &load_config().context("loading config")?.token,
        GatewayIntents::all(),
    )
    .framework(
        poise::Framework::<mappable_rc::Marc<_>, _>::builder()
            .options(poise::FrameworkOptions {
                post_command: |c| after(c).boxed(),
                on_error: |e| on_dispatch_error(e).boxed(),
                commands: command_groups::all().inspect(|c| {
                    println!(
                        "- {:<25} [guild_only: {:<5}, dm_only: {:<5}, owner_only: {:<5}, needed_permissions: {}]",
                        c.name,
                        c.guild_only,
                        c.dm_only,
                        c.owners_only,
                        c.default_member_permissions,
                    );
                    for c in &c.subcommands {
                        println!(
                            "  - {:<23} [guild_only: {:<5}, dm_only: {:<5}, owner_only: {:<5}, needed_permissions: {}]",
                            c.name,
                            c.guild_only,
                            c.dm_only,
                            c.owners_only,
                            c.default_member_permissions,
                        );
                    }
                }).collect(),
                ..Default::default()
            })
            .setup(|ctx, ready, _framework| post_init_bot(ctx, ready).boxed())
            .build(),
    )
    .register_songbird()
    .event_handler(pubsub::event_handler::Handler::new(Default::default()))
    .await
    .expect("Err creating client");
    pubsub::subscribe::<events::GuildCreate, _>(|_, events::GuildCreate { guild, .. }| {
        async move {
            tracing::info!("found guild {}::{}", guild.name, guild.id);
            ControlFlow::Continue(())
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

async fn post_init_bot(
    ctx: &serenity::all::Context,
    ready: &serenity::all::Ready,
) -> anyhow::Result<Marc<Bot>> {
    poise::builtins::register_globally(ctx, &command_groups::global().collect::<Vec<_>>()).await?;

    const MEINKRAFT: u64 = 136220994812641280;
    const TEST_SERVER: u64 = 352399774818762759;
    const MONO_BLACK: u64 = 797882422884433940;

    for g in &ready.guilds {
        let mut commands = Vec::new();
        if g.id.get() == MEINKRAFT || g.id.get() == TEST_SERVER {
            println!("instaling quotes/sfx/tts/bday in {}", g.id);
            commands.extend(command_groups::quotes().chain([
                command_groups::sfx(),
                command_groups::tts(),
                command_groups::bday(),
            ]));
        }
        if g.id.get() == MONO_BLACK || g.id.get() == TEST_SERVER {
            println!("instaling moderation/mtg_spoilers in {}", g.id);
            commands.extend(command_groups::moderation().chain(command_groups::mtg_spoilers()));
        }
        poise::builtins::register_in_guild(ctx, &commands, g.id).await?;
        tracing::info!("registered commands to {}", g.id);
    }

    let bot = Bot::init(ctx).await?;

    ctx.data.write().await.insert::<Bot>(bot.clone());

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

    Ok(bot)
}

async fn after(ctx: poise::Context<'_, mappable_rc::Marc<Bot>, anyhow::Error>) {
    tracing::info!(cmd = ?ctx.command().name, author = ?ctx.author().name, "processed command");
}

async fn on_dispatch_error(
    error: poise::FrameworkError<'_, mappable_rc::Marc<Bot>, anyhow::Error>,
) {
    match error {
        poise::FrameworkError::Command { error, ctx, .. } => {
            let _ = ctx.say(error.to_string()).await;
            tracing::error!(cmd = ?ctx.command().name, ?error, "Command failed");
        }
        error => {
            tracing::error!(?error, "framework error");
        }
    }
}
