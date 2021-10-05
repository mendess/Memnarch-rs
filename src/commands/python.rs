use std::borrow::Cow;

use pyo3::{types::PyDict, Python};
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::Message,
    prelude::*,
};

use crate::permissions::IS_FRIEND_CHECK;

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
    let code: Cow<'_, str> = match &code[..] {
        &[_, code, _] => code.trim_start_matches("py").trim().into(),
        &[code] => {
            if code.bytes().filter(|b| *b == b'\n').count() == 0 {
                format!("ret = {}", code.trim().trim_matches('`').trim()).into()
            } else {
                code.into()
            }
        }
        _ => return Err("write only python code or surround the code in a code block".into()),
    };
    let locals = {
        let py = Python::acquire_gil();
        let py = py.python();
        let locals = PyDict::new(py);
        py.run(&code, None, Some(locals))?;
        format!("```\n{:#?}\n```", locals)
    };
    msg.channel_id.say(ctx, locals).await?;
    Ok(())
}
