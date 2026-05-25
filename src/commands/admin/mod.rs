pub mod ban;
pub mod kick;
pub mod log_channel;
pub mod timeout;

use std::time::Duration;

use poise::serenity_prelude as serenity;
use tracing::warn;

use crate::bot::{Context, Error};

const MAX_TIMEOUT_SECONDS: u64 = 28 * 24 * 60 * 60;

struct AuditRecord<'a> {
    action: &'a str,
    target: &'a serenity::User,
    moderator: &'a serenity::User,
    reason: &'a str,
    timeout: Option<(&'a str, Option<serenity::Timestamp>)>,
}

#[derive(Debug)]
enum ModerationAction {
    Ban,
    Kick,
    Timeout {
        duration: String,
        expiry: Option<serenity::Timestamp>,
    },
}

#[derive(Debug, PartialEq, Eq)]
enum TimeoutInput {
    Clear,
    Apply {
        description: String,
        duration: Duration,
    },
}

struct CompletedAction {
    action: &'static str,
    verb: &'static str,
    response: String,
    timeout: Option<(String, Option<serenity::Timestamp>)>,
}

impl ModerationAction {
    fn completed_for(&self, target_id: serenity::UserId) -> CompletedAction {
        match self {
            Self::Ban => CompletedAction {
                action: "Ban",
                verb: "ban",
                response: format!("Banned <@{target_id}>."),
                timeout: None,
            },
            Self::Kick => CompletedAction {
                action: "Kick",
                verb: "kick",
                response: format!("Kicked <@{target_id}>."),
                timeout: None,
            },
            Self::Timeout { duration, expiry } => CompletedAction {
                action: "Timeout",
                verb: "timeout",
                response: match expiry {
                    Some(expiry) => format!(
                        "Timed out <@{target_id}> until <t:{}:F>.",
                        expiry.unix_timestamp()
                    ),
                    None => format!("Cleared the timeout for <@{target_id}>."),
                },
                timeout: Some((duration.clone(), *expiry)),
            },
        }
    }

    async fn perform(
        &self,
        ctx: Context<'_>,
        target: &serenity::User,
        reason: &str,
    ) -> Result<(), serenity::Error> {
        let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
        match self {
            Self::Ban => {
                guild_id
                    .ban_with_reason(ctx.http(), target.id, 0, reason)
                    .await
            }
            Self::Kick => {
                guild_id
                    .kick_with_reason(ctx.http(), target.id, reason)
                    .await
            }
            Self::Timeout { expiry, .. } => {
                let edit = serenity::EditMember::new().audit_log_reason(reason);
                let edit = match expiry {
                    Some(expiry) => edit.disable_communication_until_datetime(*expiry),
                    None => edit.enable_communication(),
                };
                guild_id
                    .edit_member(ctx.http(), target.id, edit)
                    .await
                    .map(|_| ())
            }
        }
    }
}

async fn post_audit_record(ctx: Context<'_>, record: AuditRecord<'_>) {
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

fn audit_reason(reason: String) -> String {
    reason.trim().chars().take(512).collect()
}

async fn reject_self_target(ctx: Context<'_>, target: &serenity::User) -> Result<bool, Error> {
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
) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default()
            .content(format!("Failed to {action} the user: {error}"))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

async fn execute_action(
    ctx: Context<'_>,
    target: serenity::User,
    reason: String,
    action: ModerationAction,
) -> Result<(), Error> {
    if reject_self_target(ctx, &target).await? {
        return Ok(());
    }
    let reason = audit_reason(reason);
    let completed = action.completed_for(target.id);
    ctx.defer_ephemeral().await?;
    if let Err(error) = action.perform(ctx, &target, &reason).await {
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
            target: &target,
            moderator: ctx.author(),
            reason: &reason,
            timeout,
        },
    )
    .await;
    Ok(())
}

async fn execute_ban(
    ctx: Context<'_>,
    target: serenity::User,
    reason: String,
) -> Result<(), Error> {
    execute_action(ctx, target, reason, ModerationAction::Ban).await
}

async fn execute_kick(
    ctx: Context<'_>,
    target: serenity::User,
    reason: String,
) -> Result<(), Error> {
    execute_action(ctx, target, reason, ModerationAction::Kick).await
}

async fn execute_timeout(
    ctx: Context<'_>,
    target: serenity::User,
    duration: String,
    reason: String,
) -> Result<(), Error> {
    let input = match parse_timeout_input(&duration) {
        Ok(input) => input,
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
    let (duration, expiry) = match input {
        TimeoutInput::Clear => ("Cleared".to_owned(), None),
        TimeoutInput::Apply {
            description,
            duration,
        } => {
            let expiry = serenity::Timestamp::from_unix_timestamp(
                serenity::Timestamp::now().unix_timestamp() + duration.as_secs() as i64,
            )?;
            (description, Some(expiry))
        }
    };
    execute_action(
        ctx,
        target,
        reason,
        ModerationAction::Timeout { duration, expiry },
    )
    .await
}

fn parse_timeout_input(input: &str) -> Result<TimeoutInput, &'static str> {
    let description = input.trim().to_owned();
    let normalized = description.to_ascii_lowercase();
    if normalized == "clear" || normalized == "0" {
        return Ok(TimeoutInput::Clear);
    }
    let (number, multiplier) = match normalized.as_bytes().last() {
        Some(b'm') => (&normalized[..normalized.len() - 1], 60),
        Some(b'h') => (&normalized[..normalized.len() - 1], 60 * 60),
        Some(b'd') => (&normalized[..normalized.len() - 1], 24 * 60 * 60),
        _ => return Err("Duration must be `10m`, `2h`, `1d`, `0`, or `clear`."),
    };
    let amount = number
        .parse::<u64>()
        .map_err(|_| "Duration must be `10m`, `2h`, `1d`, `0`, or `clear`.")?;
    let seconds = amount
        .checked_mul(multiplier)
        .ok_or("Timeout duration cannot exceed 28 days.")?;
    if seconds == 0 {
        return Ok(TimeoutInput::Clear);
    }
    if seconds > MAX_TIMEOUT_SECONDS {
        return Err("Timeout duration cannot exceed 28 days.");
    }
    Ok(TimeoutInput::Apply {
        description,
        duration: Duration::from_secs(seconds),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_timeout_inputs() {
        assert_eq!(
            parse_timeout_input("10m"),
            Ok(TimeoutInput::Apply {
                description: "10m".to_owned(),
                duration: Duration::from_secs(600),
            })
        );
        assert_eq!(
            parse_timeout_input(" 2H "),
            Ok(TimeoutInput::Apply {
                description: "2H".to_owned(),
                duration: Duration::from_secs(7_200),
            })
        );
        assert_eq!(
            parse_timeout_input("1d"),
            Ok(TimeoutInput::Apply {
                description: "1d".to_owned(),
                duration: Duration::from_secs(86_400),
            })
        );
    }

    #[test]
    fn parses_timeout_clear_values() {
        assert_eq!(parse_timeout_input("0"), Ok(TimeoutInput::Clear));
        assert_eq!(parse_timeout_input("clear"), Ok(TimeoutInput::Clear));
    }

    #[test]
    fn rejects_invalid_or_overlong_timeout_inputs() {
        assert!(parse_timeout_input("forever").is_err());
        assert!(parse_timeout_input("29d").is_err());
        assert_eq!(
            parse_timeout_input("28d"),
            Ok(TimeoutInput::Apply {
                description: "28d".to_owned(),
                duration: Duration::from_secs(MAX_TIMEOUT_SECONDS),
            })
        );
    }

    #[test]
    fn derives_moderation_reply_and_audit_facts_from_the_action() {
        let target_id = serenity::UserId::new(42);
        let ban = ModerationAction::Ban.completed_for(target_id);
        assert_eq!(ban.action, "Ban");
        assert_eq!(ban.verb, "ban");
        assert_eq!(ban.response, "Banned <@42>.");
        assert!(ban.timeout.is_none());

        let kick = ModerationAction::Kick.completed_for(target_id);
        assert_eq!(kick.action, "Kick");
        assert_eq!(kick.verb, "kick");
        assert_eq!(kick.response, "Kicked <@42>.");
        assert!(kick.timeout.is_none());

        let expiry = serenity::Timestamp::from_unix_timestamp(1_800_000_000).unwrap();
        let timeout = ModerationAction::Timeout {
            duration: "10m".to_owned(),
            expiry: Some(expiry),
        }
        .completed_for(target_id);
        assert_eq!(timeout.action, "Timeout");
        assert_eq!(timeout.verb, "timeout");
        assert_eq!(timeout.response, "Timed out <@42> until <t:1800000000:F>.");
        assert_eq!(timeout.timeout, Some(("10m".to_owned(), Some(expiry))));

        let cleared = ModerationAction::Timeout {
            duration: "Cleared".to_owned(),
            expiry: None,
        }
        .completed_for(target_id);
        assert_eq!(cleared.response, "Cleared the timeout for <@42>.");
        assert_eq!(cleared.timeout, Some(("Cleared".to_owned(), None)));
    }

    #[test]
    fn normalizes_audit_reasons_in_the_moderation_module() {
        assert_eq!(audit_reason(" reason ".to_owned()), "reason");
        assert_eq!(audit_reason("x".repeat(513)).len(), 512);
    }
}
