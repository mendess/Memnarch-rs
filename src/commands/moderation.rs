use futures::StreamExt;
use serenity::{
    all::{MessageId, RoleId},
    model::prelude::ReactionType,
};

use poise::{CreateReply, command};

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
    let notif = ctx.say("added role to:\n").await?;
    let members = gid.members_iter(ctx);
    let mut member_names = vec![];
    tokio::pin!(members);
    while let Some(m) = members.next().await {
        let m = m?;
        if !m.roles.contains(&role) {
            m.add_role(ctx, role).await?;
            member_names.push(m.display_name().to_owned());
            if let Err(e) = notif
                .edit(
                    ctx,
                    CreateReply::default().content({
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
                    }),
                )
                .await
            {
                tracing::error!("failed to edit list of added people: {e:?}");
            }
        }
    }
    Ok(())
}
