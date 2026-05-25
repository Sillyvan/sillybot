pub mod ban;
pub mod duration;
pub mod kick;
pub mod log_channel;
pub mod timeout;

use std::future::Future;

use poise::serenity_prelude as serenity;
use tracing::warn;

use crate::bot::Context;

pub(crate) struct AuditRecord<'a> {
    pub action: &'a str,
    pub target: &'a serenity::User,
    pub moderator: &'a serenity::User,
    pub reason: &'a str,
    pub timeout: Option<(&'a str, Option<serenity::Timestamp>)>,
}

pub(crate) struct CompletedAction {
    pub action: &'static str,
    pub verb: &'static str,
    pub response: String,
    pub timeout: Option<(String, Option<serenity::Timestamp>)>,
}

pub(crate) async fn post_audit_record(ctx: Context<'_>, record: AuditRecord<'_>) {
    let Some(guild_id) = ctx.guild_id() else {
        return;
    };
    let channel_id = match ctx.data().instance_data.admin_log_channel(guild_id).await {
        Ok(channel_id) => channel_id,
        Err(error) => {
            warn!(%guild_id, %error, "failed to read configured admin log channel");
            return;
        }
    };
    let Some(channel_id) = channel_id else {
        return;
    };

    let mut embed = serenity::CreateEmbed::new()
        .title(format!("Moderation action: {}", record.action))
        .field(
            "Target",
            format!("<@{}> (`{}`)", record.target.id, record.target.id),
            false,
        )
        .field(
            "Moderator",
            format!("<@{}> (`{}`)", record.moderator.id, record.moderator.id),
            false,
        )
        .field("Reason", record.reason, false)
        .timestamp(serenity::Timestamp::now());
    if let Some((duration, expiry)) = record.timeout {
        embed = embed.field("Duration", duration, true);
        if let Some(expiry) = expiry {
            embed = embed.field(
                "Expires",
                format!("<t:{}:F>", expiry.unix_timestamp()),
                true,
            );
        }
    }

    if let Err(error) = channel_id
        .send_message(ctx.http(), serenity::CreateMessage::new().embed(embed))
        .await
    {
        warn!(%guild_id, %channel_id, %error, "failed to send admin log record");
    }
}

pub(crate) fn audit_reason(reason: String) -> String {
    reason.trim().chars().take(512).collect()
}

async fn reject_self_target(
    ctx: Context<'_>,
    target: &serenity::User,
) -> Result<bool, crate::bot::Error> {
    if target.id != ctx.author().id {
        return Ok(false);
    }
    ctx.send(
        poise::CreateReply::default()
            .content("You cannot target yourself with a moderation action.")
            .ephemeral(true),
    )
    .await?;
    Ok(true)
}

async fn moderation_api_error(
    ctx: Context<'_>,
    action: &str,
    error: serenity::Error,
) -> Result<(), crate::bot::Error> {
    ctx.send(
        poise::CreateReply::default()
            .content(format!("Failed to {action} the user: {error}"))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

pub(crate) async fn execute_action<F, ActionFuture>(
    ctx: Context<'_>,
    target: &serenity::User,
    reason: String,
    completed: CompletedAction,
    perform: F,
) -> Result<(), crate::bot::Error>
where
    F: FnOnce(String) -> ActionFuture,
    ActionFuture: Future<Output = Result<(), serenity::Error>>,
{
    if reject_self_target(ctx, target).await? {
        return Ok(());
    }
    let reason = audit_reason(reason);
    ctx.defer_ephemeral().await?;
    if let Err(error) = perform(reason.clone()).await {
        return moderation_api_error(ctx, completed.verb, error).await;
    }

    ctx.send(
        poise::CreateReply::default()
            .content(completed.response)
            .ephemeral(true),
    )
    .await?;
    let timeout = completed
        .timeout
        .as_ref()
        .map(|(duration, expiry)| (duration.as_str(), *expiry));
    post_audit_record(
        ctx,
        AuditRecord {
            action: completed.action,
            target,
            moderator: ctx.author(),
            reason: &reason,
            timeout,
        },
    )
    .await;
    Ok(())
}
