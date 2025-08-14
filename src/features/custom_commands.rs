use std::{collections::HashMap, sync::LazyLock};

use futures::FutureExt;
use json_db::multifile_db::MultifileDb;
use pubsub::{ControlFlow, events};
use serenity::all::{Context, GuildId, Message};

use crate::in_files;

type GuildCommands = HashMap<String, String>;

async fn check_if_custom_command(ctx: &Context, msg: &Message) {
    if ctx.cache.current_user().id == msg.author.id {
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
        tracing::trace!("looking for command: {}", cmd);
        if let Some(o) = CUSTOM_COMMANDS.get(&g).await?
            && let Some(out) = o.load().await?.get(cmd)
        {
            msg.channel_id.say(&ctx, out).await?;
        }
        Ok(())
    }
    if let Some(g) = msg.guild_id
        && let Err(e) = f(ctx, msg, g).await
    {
        tracing::error!("Custom command failed: {:?}", e);
    }
}

static CUSTOM_COMMANDS: LazyLock<MultifileDb<GuildId, GuildCommands>> =
    LazyLock::new(|| MultifileDb::new(in_files!("custom").into()));

pub async fn add(gid: GuildId, command: String, output: String) -> anyhow::Result<()> {
    CUSTOM_COMMANDS
        .get_or_default(gid)
        .await?
        .load()
        .await?
        .insert(command, output);

    Ok(())
}

pub async fn remove(gid: GuildId, command: &str) -> anyhow::Result<Option<String>> {
    let Some(gid_commands) = CUSTOM_COMMANDS.get(&gid).await? else {
        return Ok(None);
    };
    let removed = gid_commands.load().await?.remove(command);

    Ok(removed)
}

pub async fn list(gid: GuildId) -> anyhow::Result<Vec<(String, String)>> {
    let Some(g_commands) = CUSTOM_COMMANDS.get(&gid).await? else {
        return Ok(vec![]);
    };

    let g_commands = g_commands.load().await?;

    Ok(g_commands
        .iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect())
}

pub async fn initialize() {
    pubsub::subscribe::<events::Message, _>(|ctx, msg| {
        async move {
            check_if_custom_command(ctx, msg).await;
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
}
