use crate::consts::NUMBERS;

use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::channel::Message,
    prelude::*,
};

group!({
    name: "General",
    options: {},
    commands: [ping, who_are_you, vote],
});

#[command]
#[description("Ping me maybe")]
fn ping(ctx: &mut Context, msg: &Message) -> CommandResult {
    use chrono::Local;
    let one_trip_time =
        (Local::now().timestamp_millis() - msg.timestamp.timestamp_millis()) as f32 / 1000_f32;
    if let Err(why) = msg.channel_id.say(
        &ctx.http,
        format!(
            "Pong! {} ms{}",
            one_trip_time * 2.0,
            if one_trip_time < 0.0 {
                "\n*yes it's negative, idk why either*"
            } else {
                ""
            }
        ),
    ) {
        println!("Error ponging: {:?}", why)
    }
    Ok(())
}

#[command("whoareyou")]
#[description("Find out more about me")]
fn who_are_you(ctx: &mut Context, msg: &Message) -> CommandResult {
    msg.channel_id.send_message(ctx, |m| {
        m.embed(|e| {
            e.title("I AM MEMNARCH")
                .description("Sauce code: [GitHub](https://github.com/Mendess2526/Memnarch-rs)")
                .image("https://img.scryfall.com/mci/scans/en/arc/112.jpg")
        })
    })?;
    Ok(())
}

#[command]
#[min_args(2)]
#[max_args(9)]
#[description("Create a voting of up to 9 things")]
fn vote(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let message = msg.channel_id.send_message(&ctx.http, |m| {
        m.embed(|e| {
            e.title("Vote:");
            let fs = args
                .iter::<String>()
                .filter_map(Result::ok)
                .enumerate()
                .map(|(i, a)| (a, NUMBERS[i], true));
            e.fields(fs)
        });
        m
    })?;
    args.restore();
    (0..args.iter::<String>().filter_map(Result::ok).count()).for_each(|n| {
        while let Err(_) = message.react(&ctx, NUMBERS[n]) {
            continue;
        }
    });
    Ok(())
}
