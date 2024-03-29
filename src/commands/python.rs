use std::{sync::OnceLock, time::Duration};

use reqwest::Client;
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::Message,
    prelude::*,
};
use tokio::time::timeout;

use crate::{util::permissions::IS_FRIEND_CHECK, Config};

static HTTP: OnceLock<Client> = OnceLock::new();

#[group]
#[commands(py)]
#[checks("is_friend")]
pub struct Py;

#[command]
#[description("runs python code")]
pub async fn py(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    eval_(ctx, msg, args).await?;
    Ok(())
}

pub async fn eval_(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let code = args.rest().split("```").collect::<Vec<_>>();
    let (path, code) = match &code[..] {
        [_, code, _] => ("", code.trim_start_matches("py").trim()),
        [code] => {
            if code.bytes().filter(|b| *b == b'\n').count() == 0 {
                ("expr", code.trim().trim_matches('`').trim())
            } else {
                ("", *code)
            }
        }
        _ => return Err("write only python code or surround the code in a code block".into()),
    };
    let py_eval_address = ctx
        .data
        .read()
        .await
        .get::<Config>()
        .unwrap()
        .py_eval_address
        .clone();
    let r = timeout(
        Duration::from_secs(10),
        HTTP.get_or_init(Default::default)
            .post(format!("http://{}/{}", py_eval_address, path))
            .json(&serde_json::json! {{ "t": code }})
            .send(),
    )
    .await??;
    match r {
        r if r.status().is_success() => {
            msg.channel_id
                .say(
                    ctx,
                    format!(
                        "```\n{}\n```",
                        serde_json::to_string_pretty(&r.json::<serde_json::Value>().await?)?
                    ),
                )
                .await?;
        }
        r => {
            return Err(format!(
                "{} => {}",
                r.status().canonical_reason().unwrap_or(""),
                r.text().await?
            )
            .into())
        }
    }
    Ok(())
}
