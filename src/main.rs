const FILES_DIR: &str = "files/";
const NUMBERS: [&str; 10] = [
    "0⃣", "1⃣", "2⃣", "3⃣", "4⃣", "5⃣", "6⃣", "7⃣", "8⃣", "9⃣",
];

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        help_commands,
        macros::{check, command, group, help},
        Args, CheckResult, CommandGroup, CommandOptions, CommandResult, DispatchError, HelpCommand,
        HelpOptions, StandardFramework,
    },
    model::{channel::Message, gateway::Ready, id::UserId, Permissions},
    prelude::*,
};

struct Handler;

impl EventHandler for Handler {
    fn ready(&self, _: Context, _ready: Ready) {
        println!("Up and running");
    }
}

#[derive(Serialize, Deserialize)]
struct Config {
    token: String,
}

impl Config {
    fn new() -> std::io::Result<Config> {
        use std::fs::{DirBuilder, OpenOptions};
        DirBuilder::new().recursive(true).create(FILES_DIR)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(format!("{}/config.json", FILES_DIR))?;
        Ok(serde_json::from_reader(file).unwrap_or_else(|_| {
            let mut file = OpenOptions::new()
                .write(true)
                .open(format!("{}/config.json", FILES_DIR))
                .expect("Couldn't open config for writing");
            let mut token = String::new();
            print!("Token: ");
            use std::io::Write;
            let _ = std::io::stdout().lock().flush();
            std::io::stdin()
                .read_line(&mut token)
                .expect("Couldn't read token from stdin");
            let config = Config { token };
            let _ = file.write_all(serde_json::to_string(&config).unwrap().as_bytes());
            config
        }))
    }
}

fn main() -> std::io::Result<()> {
    let config = Config::new()?;
    let mut client = Client::new(&config.token, Handler).expect("Err creating client");
    client.with_framework(
        StandardFramework::new()
            .configure(|c| c.prefix("|"))
            .group(&GENERAL_GROUP)
            .help(&MY_HELP_HELP_COMMAND),
    );

    if let Err(why) = client.start() {
        println!("Sad face :(  {:?}", why);
    }
    Ok(())
}

group!({
    name: "General",
    options: {},
    commands: [ping, who_are_you, vote],
});

#[help]
fn my_help(
    context: &mut Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    help_commands::with_embeds(context, msg, args, help_options, groups, owners)
}

#[command]
fn ping(ctx: &mut Context, msg: &Message) -> CommandResult {
    use chrono::Local;
    if let Err(why) = msg.channel_id.say(
        &ctx.http,
        format!(
            "Pong! {} ms",
            (Local::now().timestamp_millis() - msg.timestamp.timestamp_millis()) as f32 / 1000_f32
        ),
    ) {
        println!("Error ponging: {:?}", why)
    }
    Ok(())
}

#[command("whoareyou")]
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
fn vote(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let message = msg.channel_id.send_message(&ctx.http, |m| {
        m.embed(|e| {
            e.title("Vote:");
            let fs = args
                .iter::<String>()
                .filter_map(|x| x.ok())
                .enumerate()
                .map(|(a, i)| (a, i, true));
            e.fields(fs)
        });
        m
    })?;
    args.restore();
    (0..args.iter::<String>().filter_map(|x| x.ok()).count()).for_each(|n| {
        while let Err(_) = message.react(ctx, NUMBERS[n]) {
            continue;
        }
    });
    Ok(())
}
