use serde::{Deserialize, Serialize};
use serenity::{
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
pub async fn new(ctx: &mut Context, msg: &Message, args: Args) -> CommandResult {
    if let Some(config) = ctx.data.read().get::<InterrailConfig>() {
        config.read().stories.say(&ctx.http, args.rest())?;
    } else {
        msg.channel_id.say(&ctx, "not configured")?;
    }
    Ok(())
}

#[command]
#[description("Edit an existing story")]
#[usage("#message_id message")]
#[checks("is_interrail_channel")]
#[aliases("e")]
#[min_args(2)]
pub async fn edit(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let msg_id = args.single::<u64>()?;
    if let Some(config) = ctx.data.read().get::<InterrailConfig>() {
        let mut message = config.read().stories.message(&ctx.http, msg_id)?;
        message.edit(&ctx, |c| c.content(args.rest()))?;
    } else {
        msg.channel_id.say(&ctx, "not configured")?;
    }
    Ok(())
}

#[command]
#[description("Edit interrail config")]
#[usage("#talk_channel #stories_channel")]
#[owners_only]
#[min_args(2)]
pub async fn config(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let talk = args.single::<ChannelId>()?;
    let stories = args.single::<ChannelId>()?;
    *ctx.data
        .read()
        .get::<InterrailConfig>()
        .expect("Interrail config to be loaded")
        .write() = InterrailConfig::with_ids(stories, talk)?;
    msg.channel_id.say(&ctx, "Configured")?;
    Ok(())
}

#[check]
#[name = "is_interrail_channel"]
async fn is_interrail_channel(
    ctx: &mut Context,
    msg: &Message,
    _: &mut Args,
) -> Result<(), Reason> {
    ctx.data
        .read()
        .get::<InterrailConfig>()
        .and_then(|ic| (msg.channel_id == ic.read().talk).then(|| Ok(())))
        .unwrap_or_else(|| Err(Reason::User("You can't use this command here".into())))
}
