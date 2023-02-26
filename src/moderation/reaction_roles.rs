use ::daemons::ControlFlow;
use futures::FutureExt;
use std::{io, os::unix::prelude::OsStrExt, path::PathBuf, str::from_utf8};
use tokio::fs;

use dashmap::DashMap;
use lazy_static::lazy_static;
use serenity::{
    http::{CacheHttp, Http},
    model::prelude::{ChannelId, GuildId, MessageId, Reaction, ReactionType, Role, RoleId},
    prelude::Context,
};

use crate::{events, file_transaction::Database};

const BASE: &str = "files/moderation/reaction_roles";

lazy_static! {
    static ref REACTION_ROLES: DashMap<GuildId, Database<Vec<(ReactionType, MessageId, RoleId)>>> =
        DashMap::default();
}

pub async fn initialize() -> io::Result<()> {
    fs::DirBuilder::new().recursive(true).create(BASE).await?;
    let mut read_dir = fs::read_dir(BASE).await?;
    while let Some(d) = read_dir.next_entry().await? {
        let path = d.path();
        let gid = match path.file_stem().and_then(|n| {
            let s = from_utf8(n.as_bytes()).ok()?;
            Some(GuildId(str::parse(s).ok()?))
        }) {
            None => continue,
            Some(gid) => gid,
        };
        REACTION_ROLES.insert(gid, Database::new(path));
    }

    use events::pubsub::events::{GuildRoleDelete, ReactionAdd, ReactionRemove};
    async fn handler(ctx: &Context, reaction: &Reaction) {
        let Some(gid) = reaction.guild_id else {
            return
        };
        let Some(db) = REACTION_ROLES.get(&gid) else {
                return ;
        };
        let db = match db.load().await {
            Ok(db) => db,
            Err(e) => {
                log::error!("failed to load db: {e:?}");
                return;
            }
        };
        let Some((_, _, role)) = db.iter().find(|(e, mid, _)| e == &reaction.emoji && mid == &reaction.message_id) else {
            return;
        };
        let mut member = match gid.member(ctx, reaction.user_id.unwrap()).await {
            Ok(m) => m,
            Err(e) => {
                log::error!("failed to get member: {e:?}");
                return;
            }
        };
        if member.roles.iter().any(|r| r == role) {
            if let Err(e) = member.remove_role(ctx, role).await {
                log::error!("failed to add role: {e:?}");
            }
        } else if let Err(e) = member.add_role(ctx, role).await {
            log::error!("failed to add role: {e:?}");
        }
    }
    events::pubsub::register::<ReactionAdd, _>(|ctx: &Context, args: &Reaction| {
        async move {
            handler(ctx, args).await;
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
    events::pubsub::register::<ReactionRemove, _>(|ctx: &Context, args: &Reaction| {
        async move {
            handler(ctx, args).await;
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
    // events::pubsub::register::<GuildRoleDelete, _>(
    //     |ctx: &Context, (gid, rid, role): &(GuildId, RoleId, Option<Role>)| {
    //         async move { if let Some(g) = REACTION_ROLES.get(gid) {
    //             match g.load().await {
    //                 Ok()
    //             }
    //         } }.boxed()
    //     },
    // );

    Ok(())
}

pub(crate) async fn reaction_role_add(
    http: impl CacheHttp,
    guild_id: GuildId,
    channel_id: ChannelId,
    mid: MessageId,
    emoji: ReactionType,
    role: RoleId,
) -> anyhow::Result<()> {
    let path = [BASE, &guild_id.to_string()]
        .into_iter()
        .collect::<PathBuf>();
    let database = REACTION_ROLES
        .entry(guild_id)
        .or_insert(Database::new(path));
    let mut roles = database.load().await?;
    match roles.iter_mut().find(|(e, m, _)| e == &emoji && m == &mid) {
        Some(entry) => entry.2 = role,
        None => roles.push((emoji.clone(), mid, role)),
    }

    let message = channel_id.message(http.http(), mid).await?;
    message.react(http, emoji).await?;
    Ok(())
}
