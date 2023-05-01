use ::daemons::ControlFlow;
use futures::FutureExt;
use lazy_static::lazy_static;
use serenity::{
    http::CacheHttp,
    model::prelude::{ChannelId, GuildId, MessageId, Reaction, ReactionType, RoleId},
    prelude::Context,
};
use std::{
    collections::{hash_map::Entry, HashMap},
    io,
    os::unix::prelude::OsStrExt,
    path::{Path, PathBuf},
    str::from_utf8,
};
use tokio::{fs, sync::Mutex};

use json_db::{json_hash_map::JsonHashMap, Database};

const BASE: &str = "files/moderation/reaction_roles";

type MapKey = (ReactionType, MessageId);

struct BorrowedKey<'s>(&'s ReactionType, MessageId);

impl<'s> PartialEq<BorrowedKey<'s>> for MapKey {
    fn eq(&self, other: &BorrowedKey<'s>) -> bool {
        &self.0 == other.0 && self.1 == other.1
    }
}

type GuildReactionMap = JsonHashMap<(ReactionType, MessageId), RoleId>;

lazy_static! {
    static ref REACTION_ROLES: Mutex<HashMap<GuildId, Database<GuildReactionMap>>> =
        Default::default();
}

async fn migrate_schema(path: &Path) -> io::Result<()> {
    let new_format = serde_json::from_reader::<_, Vec<(ReactionType, MessageId, RoleId)>>(
        std::fs::File::open(path)?,
    )?
    .into_iter()
    .map(|(r, m, rid)| ((r, m), rid))
    .collect::<Vec<_>>();

    let (file, tmp_file) = tempfile::NamedTempFile::new_in(path.parent().unwrap())?.into_parts();
    serde_json::to_writer(&file, &new_format)?;
    tokio::fs::rename(tmp_file, path).await?;
    Ok(())
}

pub async fn initialize() -> io::Result<()> {
    match fs::read_dir(BASE).await {
        Ok(mut read_dir) => {
            let mut db = REACTION_ROLES.lock().await;
            while let Some(d) = read_dir.next_entry().await? {
                let path = d.path();
                let gid = match path.file_stem().and_then(|n| {
                    let s = from_utf8(n.as_bytes()).ok()?;
                    Some(GuildId(str::parse(s).ok()?))
                }) {
                    None => continue,
                    Some(gid) => gid,
                };
                let _ = migrate_schema(&path).await;
                db.insert(gid, Database::new(path).await?);
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    use pubsub::events::{ReactionAdd, ReactionRemove};
    async fn handler<const ADD: bool>(ctx: &Context, reaction: &Reaction) {
        let Some(gid) = reaction.guild_id else {
            return
        };
        let (mut member, role) = {
            let db = REACTION_ROLES.lock().await;
            let Some(db) = db.get(&gid) else {
                return;
            };
            let db = match db.load().await {
                Ok(db) => db,
                Err(e) => {
                    log::error!("failed to load db: {e:?}");
                    return;
                }
            };
            let Some(role) = db.get(&BorrowedKey(&reaction.emoji, reaction.message_id)) else {
                return;
            };
            let member = match gid.member(ctx, reaction.user_id.unwrap()).await {
                Ok(m) => m,
                Err(e) => {
                    log::error!("failed to get member: {e:?}");
                    return;
                }
            };
            (member, *role)
        };
        if ADD {
            if let Err(e) = member.add_role(ctx, role).await {
                log::error!("failed to add role: {e:?}");
            }
        } else if let Err(e) = member.remove_role(ctx, role).await {
            log::error!("failed to add role: {e:?}");
        }
    }
    pubsub::subscribe::<ReactionAdd, _>(|ctx: &Context, args: &Reaction| {
        async move {
            handler::<true>(ctx, args).await;
            ControlFlow::CONTINUE
        }
        .boxed()
    })
    .await;
    pubsub::subscribe::<ReactionRemove, _>(|ctx: &Context, args: &Reaction| {
        async move {
            handler::<false>(ctx, args).await;
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
    let mut db = REACTION_ROLES.lock().await;
    let database = match db.entry(guild_id) {
        Entry::Occupied(o) => o.into_mut(),
        Entry::Vacant(v) => v.insert(Database::new(path).await?),
    };
    let mut roles = database.load().await?;
    roles
        .entry((emoji.clone(), mid))
        .and_modify(|v| *v = role)
        .or_insert_with(|| role);

    let message = channel_id.message(http.http(), mid).await?;
    message.react(http, emoji).await?;
    Ok(())
}