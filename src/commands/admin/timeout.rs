use poise::serenity_prelude as serenity;

use super::{CompletedAction, duration::parse_timeout_duration, execute_action};
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
    let expiry = parsed_duration
        .map(|duration| serenity::Timestamp::now().unix_timestamp() + duration.as_secs() as i64)
        .map(serenity::Timestamp::from_unix_timestamp)
        .transpose()?;
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
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
    execute_action(
        ctx,
        &user,
        reason,
        CompletedAction {
            action: "Timeout",
            verb: "timeout",
            response,
            timeout: Some((duration_description, expiry)),
        },
        |reason| async move {
            let edit = serenity::EditMember::new().audit_log_reason(&reason);
            let edit = match expiry {
                Some(expiry) => edit.disable_communication_until_datetime(expiry),
                None => edit.enable_communication(),
            };
            guild_id
                .edit_member(ctx.http(), user.id, edit)
                .await
                .map(|_| ())
        },
    )
    .await
}
