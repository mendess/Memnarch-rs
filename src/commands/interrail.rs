use crate::{get, util::MentionExt};
use serde::{Deserialize, Serialize};
use serenity::{
    all::{EditMessage, Mention},
    framework::standard::{
        macros::{check, command, group},
        Args, CommandResult, Reason,
    },
    model::{channel::Message, id::ChannelId},
    prelude::*,
};
use std::{fs::File, io, sync::Arc};

#[derive(Default, Serialize, Deserialize)]
pub struct InterrailConfig {
    stories: ChannelId,
    talk: ChannelId,
}

impl InterrailConfig {
    const CONFIG: &'static str = "files/interrail_conf.json";
    pub fn new() -> Self {
        File::open(InterrailConfig::CONFIG)
            .and_then(|f| serde_json::from_reader(f).map_err(|e| e.into()))
            .unwrap_or_default()
    }

    pub fn with_ids(stories: ChannelId, talk: ChannelId) -> io::Result<Self> {
        let a = Self { stories, talk };
        serde_json::to_writer(File::create(InterrailConfig::CONFIG)?, &a)?;
        Ok(a)
    }
}

impl TypeMapKey for InterrailConfig {
    type Value = Arc<RwLock<Self>>;
}

#[group]
#[prefix("in")]
#[commands(new, edit, config)]
struct Interrail;

#[command]
#[description("Create a new story")]
#[usage("message")]
#[checks("is_interrail_channel")]
#[aliases("n")]
#[min_args(2)]
pub async fn new(ctx: &Context, _: &Message, args: Args) -> CommandResult {
    get!(ctx, InterrailConfig, read)
        .stories
        .say(&ctx.http, args.rest())
        .await?;
    Ok(())
}

#[command]
#[description("Edit an existing story")]
#[usage("#message_id message")]
#[checks("is_interrail_channel")]
#[aliases("e")]
#[min_args(2)]
pub async fn edit(ctx: &Context, _: &Message, mut args: Args) -> CommandResult {
    let msg_id = args.single::<u64>()?;
    let mut message = get!(ctx, InterrailConfig, read)
        .stories
        .message(&ctx.http, msg_id)
        .await?;
    message
        .edit(&ctx, EditMessage::new().content(args.rest()))
        .await?;
    Ok(())
}

#[command]
#[description("Edit interrail config")]
#[usage("#talk_channel #stories_channel")]
#[owners_only]
#[min_args(2)]
pub async fn config(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let talk = args.single::<Mention>()?.into_channel()?;
    let stories = args.single::<Mention>()?.into_channel()?;
    *get!(ctx, InterrailConfig, write) = InterrailConfig::with_ids(stories, talk)?;
    msg.channel_id.say(&ctx, "Configured").await?;
    Ok(())
}

#[check]
#[name = "is_interrail_channel"]
async fn is_interrail_channel(ctx: &Context, msg: &Message, _: &mut Args) -> Result<(), Reason> {
    if let Some(ic) = ctx.data.read().await.get::<InterrailConfig>() {
        if msg.channel_id == ic.read().await.talk {
            return Ok(());
        }
    }
    Err(Reason::User("You can't use this command here".into()))
}
