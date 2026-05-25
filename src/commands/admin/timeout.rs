use poise::serenity_prelude as serenity;

use super::{
    AuditRecord, audit_reason, duration::parse_timeout_duration, moderation_api_error,
    post_audit_record, reject_self_target,
};
use crate::bot::{Context, Error};

/// Apply or clear a Discord communication timeout for a user.
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "MODERATE_MEMBERS",
    required_bot_permissions = "MODERATE_MEMBERS"
)]
pub async fn timeout(
    ctx: Context<'_>,
    #[description = "User to timeout"] user: serenity::User,
    #[description = "Duration such as 10m, 2h, 1d; use clear to remove"] duration: String,
    #[description = "Reason recorded in the audit log"] reason: String,
) -> Result<(), Error> {
    if reject_self_target(ctx, &user).await? {
        return Ok(());
    }
    let parsed_duration = match parse_timeout_duration(&duration) {
        Ok(duration) => duration,
        Err(message) => {
            ctx.send(
                poise::CreateReply::default()
                    .content(message)
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };
    let reason = audit_reason(reason);
    ctx.defer_ephemeral().await?;

    let expiry = parsed_duration
        .map(|duration| serenity::Timestamp::now().unix_timestamp() + duration.as_secs() as i64)
        .map(serenity::Timestamp::from_unix_timestamp)
        .transpose()?;
    let mut edit = serenity::EditMember::new().audit_log_reason(&reason);
    edit = match expiry {
        Some(expiry) => edit.disable_communication_until_datetime(expiry),
        None => edit.enable_communication(),
    };
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    if let Err(error) = guild_id.edit_member(ctx.http(), user.id, edit).await {
        return moderation_api_error(ctx, "timeout", error).await;
    }

    let duration_description = expiry
        .map(|_| duration.trim().to_owned())
        .unwrap_or_else(|| "Cleared".to_owned());
    let response = match expiry {
        Some(expiry) => format!(
            "Timed out <@{}> until <t:{}:F>.",
            user.id,
            expiry.unix_timestamp()
        ),
        None => format!("Cleared the timeout for <@{}>.", user.id),
    };
    ctx.send(
        poise::CreateReply::default()
            .content(response)
            .ephemeral(true),
    )
    .await?;
    post_audit_record(
        ctx,
        AuditRecord {
            action: "Timeout",
            target: &user,
            moderator: ctx.author(),
            reason: &reason,
            timeout: Some((&duration_description, expiry)),
        },
    )
    .await;
    Ok(())
}
