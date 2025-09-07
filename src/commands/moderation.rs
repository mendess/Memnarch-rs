use futures::StreamExt;
use serenity::{
    all::{MessageId, RoleId},
    model::prelude::ReactionType,
};

use poise::{CreateReply, ReplyHandle, command};

pub fn commands() -> impl Iterator<Item = super::Command> {
    [add_reaction_role(), add_role_to_all_members()].into_iter()
}

/// Add a new role to be react added.
#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
async fn add_reaction_role(
    ctx: super::Context<'_>,
    emoji: ReactionType,
    role: RoleId,
    msg_id: MessageId,
) -> anyhow::Result<()> {
    crate::moderation::reaction_roles::reaction_role_add(
        ctx,
        ctx.guild_id().unwrap(),
        ctx.channel_id(),
        msg_id,
        emoji,
        role,
    )
    .await?;
    Ok(())
}

/// Add [role] to all users.
#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
async fn add_role_to_all_members(ctx: super::Context<'_>, role: RoleId) -> anyhow::Result<()> {
    let gid = ctx.guild_id().expect("should be used in a guild");
    let notif = ctx.say("added role to:").await?;
    let mut error = None;
    let members = gid.members_iter(ctx);
    tokio::pin!(members);
    while let Some(m) = members.next().await {
        let m = m?;
        if !m.roles.contains(&role) {
            match m.add_role(ctx, role).await {
                Ok(()) => {
                    edit_member_list_msg(ctx, m.display_name(), &notif).await;
                }
                Err(_) => {
                    let emsg = match &error {
                        Some(m) => m,
                        None => error.insert(ctx.say("failed to add role to:").await?),
                    };
                    edit_member_list_msg(ctx, m.display_name(), emsg).await;
                }
            }
        }
    }
    Ok(())
}

async fn edit_member_list_msg(ctx: super::Context<'_>, member: &str, reply: &ReplyHandle<'_>) {
    let msg = reply.message().await.unwrap();
    let mut content = msg.into_owned().content;
    if let Err(e) = reply
        .edit(
            ctx,
            CreateReply::default().content({
                if content.len() + member.len() > 2000 {
                    return;
                }
                content.push('\n');
                content.push_str(member);
                content
            }),
        )
        .await
    {
        tracing::error!("failed to edit list of added people: {e:?}");
    }
}
