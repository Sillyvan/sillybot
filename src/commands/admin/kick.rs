use poise::serenity_prelude as serenity;

use super::{
    AuditRecord, audit_reason, moderation_api_error, post_audit_record, reject_self_target,
};
use crate::bot::{Context, Error};

/// Remove a user from this guild.
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "KICK_MEMBERS",
    required_bot_permissions = "KICK_MEMBERS"
)]
pub async fn kick(
    ctx: Context<'_>,
    #[description = "User to kick"] user: serenity::User,
    #[description = "Reason recorded in the audit log"] reason: String,
) -> Result<(), Error> {
    if reject_self_target(ctx, &user).await? {
        return Ok(());
    }
    let reason = audit_reason(reason);
    ctx.defer_ephemeral().await?;
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    if let Err(error) = guild_id
        .kick_with_reason(ctx.http(), user.id, &reason)
        .await
    {
        return moderation_api_error(ctx, "kick", error).await;
    }

    ctx.send(
        poise::CreateReply::default()
            .content(format!("Kicked <@{}>.", user.id))
            .ephemeral(true),
    )
    .await?;
    post_audit_record(
        ctx,
        AuditRecord {
            action: "Kick",
            target: &user,
            moderator: ctx.author(),
            reason: &reason,
            timeout: None,
        },
    )
    .await;
    Ok(())
}
