use futures::StreamExt;
use serenity::{
    framework::standard::{
        macros::{command, group},
        Args, CommandResult,
    },
    model::{
        channel::Message,
        prelude::{ReactionType, RoleId},
    },
    prelude::*,
};

#[group]
#[commands(add_reaction_role, add_base_role)]
struct Moderation;

#[command]
#[min_args(2)]
#[required_permissions(ADMINISTRATOR)]
async fn add_reaction_role(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let target = match &msg.referenced_message {
        Some(target) => target,
        None => {
            msg.channel_id
                .say(
                    ctx,
                    "must reply to the message you want the reaction roles to be added to",
                )
                .await?;
            return Ok(());
        }
    };
    let emoji = args.single::<ReactionType>()?;
    let role = args.single::<RoleId>()?;

    crate::moderation::reaction_roles::reaction_role_add(
        ctx,
        msg.guild_id.unwrap(),
        msg.channel_id,
        target.id,
        emoji,
        role,
    )
    .await?;
    Ok(())
}

#[command]
#[min_args(1)]
#[required_permissions(ADMINISTRATOR)]
async fn add_base_role(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let role = args.single::<RoleId>()?;
    let gid = msg.guild_id.expect("should be used in a guild");
    let mut notif = msg.channel_id.say(ctx, "added role to:\n").await?;
    let members = gid.members_iter(ctx);
    let mut member_names = vec![];
    tokio::pin!(members);
    while let Some(m) = members.next().await {
        let mut m = m?;
        if !m.roles.iter().any(|r| *r == role) {
            m.add_role(ctx, role).await?;
            member_names.push(m.display_name().into_owned());
            if let Err(e) = notif
                .edit(ctx, |m| {
                    m.content({
                        let mut content = String::new();
                        let mut count = 0;
                        let i = member_names
                            .iter()
                            .rev()
                            .take_while(|m| {
                                count += m.len();
                                count < 2000
                            })
                            .count();
                        for m in member_names
                            .iter()
                            .skip(member_names.len().saturating_sub(i))
                        {
                            content.push_str(m);
                            content.push('\n');
                        }
                        content.insert_str(0, "added role to:\n");
                        content
                    })
                })
                .await
            {
                tracing::error!("failed to edit list of added people: {e:?}");
            }
        }
    }
    Ok(())
}
