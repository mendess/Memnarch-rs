use serenity::{
    framework::standard::{
        macros::{command, group},
        CommandResult,
    },
    model::channel::Message,
    prelude::*,
};

use std::os::unix::process::CommandExt;
use std::process::Command as Fork;

group!({
    name: "Owner",
    options: {owners_only: true},
    commands: [update],
});

#[command]
#[description("Update the bot")]
fn update(ctx: &mut Context, msg: &Message) -> CommandResult {
    eprintln!("Fetching");
    Fork::new("git").arg("fetch").spawn()?.wait()?;
    eprintln!("Checking remote");
    let status = Fork::new("git")
        .args(&["rev-list", "--count", "master...master@{upstream}"])
        .output()?;
    if let 0 = String::from_utf8_lossy(&status.stdout)
        .trim()
        .parse::<i32>()?
    {
        msg.channel_id.say(&ctx, "No updates!").map(|_| ())?;
    } else if !Fork::new("git").arg("pull").output()?.status.success() {
        msg.channel_id.say(&ctx, "Error pulling!")?;
    } else if !Fork::new("cargo")
        .args(&["build", "--release"])
        .output()?
        .status
        .success()
    {
        msg.channel_id.say(&ctx, "Build Error")?;
    } else {
        Err(Fork::new("cargo").args(&["run", "--release"]).exec())?;
    }
    Ok(())
}

// fn creds(
//     _user: &str,
//     user_from_url: Option<&str>,
//     _cred: git2::CredentialType,
// ) -> Result<Cred, git2::Error> {
//     let home = dirs::home_dir().expect("No home dir");
//     let pub_key =
//         std::path::Path::new(&format!("{}/.ssh/id_rsa.pub", home.to_str().unwrap())).to_path_buf();
//     let priv_key =
//         std::path::Path::new(&format!("{}/.ssh/id_rsa", home.to_str().unwrap())).to_path_buf();
//     match user_from_url {
//         Some(user) => git2::Cred::ssh_key(user, Some(&pub_key), &priv_key, None),
//         None => Err(git2::Error::from_str("Url does not contain username")),
//     }
// }

// #[command]
// #[description("Update the bot")]
// fn update(ctx: &mut Context, msg: &Message) -> CommandResult {
//     let repo = Repository::open(".")?;
//     let mut remote = repo.find_remote("origin")?;

//     remote.connect_auth(
//         Direction::Fetch,
//         Some({
//             let mut r = RemoteCallbacks::new();
//             r.credentials(creds);
//             r
//         }),
//         None,
//     )?;
//     remote.fetch(&["master"], None, None)?;
//     for head in remote.list()? {
//         msg.channel_id.say(
//             &ctx,
//             format!(
//                 "HEAD:\n\tname: {}\n\toid: {}\n\tloid: {}\n",
//                 head.name(),
//                 head.oid(),
//                 head.loid()
//             ),
//         )?;
//     }

//     Ok(())
// }
