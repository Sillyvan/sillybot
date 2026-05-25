pub mod ban;
pub mod kick;
pub mod log_channel;
pub mod timeout;

use std::time::Duration;

use poise::serenity_prelude as serenity;
use tracing::warn;

use crate::bot::{Context, Error};

const MAX_TIMEOUT_SECONDS: u64 = 28 * 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuditRecord {
    action: &'static str,
    target_id: serenity::UserId,
    moderator_id: serenity::UserId,
    reason: String,
    timeout: Option<(String, Option<serenity::Timestamp>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
}

async fn post_audit_record(ctx: Context<'_>, record: AuditRecord) {
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
            format!("<@{}> (`{}`)", record.target_id, record.target_id),
            false,
        )
        .field(
            "Moderator",
            format!("<@{}> (`{}`)", record.moderator_id, record.moderator_id),
            false,
        )
        .field("Reason", &record.reason, false)
        .timestamp(serenity::Timestamp::now());
    if let Some((duration, expiry)) = &record.timeout {
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

trait ModerationBoundary {
    async fn defer_ephemeral(&self) -> Result<(), Error>;

    async fn perform(
        &self,
        action: &ModerationAction,
        target_id: serenity::UserId,
        reason: &str,
    ) -> Result<(), String>;

    async fn send_ephemeral(&self, content: String) -> Result<(), Error>;

    async fn post_audit_record(&self, record: AuditRecord);
}

struct DiscordModerationBoundary<'a> {
    ctx: Context<'a>,
}

impl ModerationBoundary for DiscordModerationBoundary<'_> {
    async fn defer_ephemeral(&self) -> Result<(), Error> {
        self.ctx.defer_ephemeral().await?;
        Ok(())
    }

    async fn perform(
        &self,
        action: &ModerationAction,
        target_id: serenity::UserId,
        reason: &str,
    ) -> Result<(), String> {
        let guild_id = self
            .ctx
            .guild_id()
            .expect("guild_only command has a guild ID");
        let result = match action {
            ModerationAction::Ban => {
                guild_id
                    .ban_with_reason(self.ctx.http(), target_id, 0, reason)
                    .await
            }
            ModerationAction::Kick => {
                guild_id
                    .kick_with_reason(self.ctx.http(), target_id, reason)
                    .await
            }
            ModerationAction::Timeout { expiry, .. } => {
                let edit = serenity::EditMember::new().audit_log_reason(reason);
                let edit = match expiry {
                    Some(expiry) => edit.disable_communication_until_datetime(*expiry),
                    None => edit.enable_communication(),
                };
                guild_id
                    .edit_member(self.ctx.http(), target_id, edit)
                    .await
                    .map(|_| ())
            }
        };
        result.map_err(|error| error.to_string())
    }

    async fn send_ephemeral(&self, content: String) -> Result<(), Error> {
        self.ctx
            .send(
                poise::CreateReply::default()
                    .content(content)
                    .ephemeral(true),
            )
            .await?;
        Ok(())
    }

    async fn post_audit_record(&self, record: AuditRecord) {
        post_audit_record(self.ctx, record).await;
    }
}

fn audit_reason(reason: String) -> String {
    reason.trim().chars().take(512).collect()
}

async fn execute_action<B: ModerationBoundary>(
    boundary: &B,
    moderator_id: serenity::UserId,
    target_id: serenity::UserId,
    reason: String,
    action: ModerationAction,
) -> Result<(), Error> {
    if moderator_id == target_id {
        boundary
            .send_ephemeral("You cannot target yourself with a moderation action.".to_owned())
            .await?;
        return Ok(());
    }
    let reason = audit_reason(reason);
    let completed = action.completed_for(target_id);
    boundary.defer_ephemeral().await?;
    if let Err(error) = boundary.perform(&action, target_id, &reason).await {
        boundary
            .send_ephemeral(format!("Failed to {} the user: {error}", completed.verb))
            .await?;
        return Ok(());
    }

    let audit_record = AuditRecord {
        action: completed.action,
        target_id,
        moderator_id,
        reason,
        timeout: completed.timeout,
    };
    boundary.send_ephemeral(completed.response).await?;
    boundary.post_audit_record(audit_record).await;
    Ok(())
}

async fn execute_ban(
    ctx: Context<'_>,
    target: serenity::User,
    reason: String,
) -> Result<(), Error> {
    execute_action(
        &DiscordModerationBoundary { ctx },
        ctx.author().id,
        target.id,
        reason,
        ModerationAction::Ban,
    )
    .await
}

async fn execute_kick(
    ctx: Context<'_>,
    target: serenity::User,
    reason: String,
) -> Result<(), Error> {
    execute_action(
        &DiscordModerationBoundary { ctx },
        ctx.author().id,
        target.id,
        reason,
        ModerationAction::Kick,
    )
    .await
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
        &DiscordModerationBoundary { ctx },
        ctx.author().id,
        target.id,
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
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum DiscordEvent {
        Deferred,
        Performed(ModerationAction, serenity::UserId, String),
        Replied(String),
        Audited(AuditRecord),
    }

    #[derive(Default)]
    struct FakeDiscord {
        events: RefCell<Vec<DiscordEvent>>,
        action_failure: Option<String>,
    }

    impl ModerationBoundary for FakeDiscord {
        async fn defer_ephemeral(&self) -> Result<(), Error> {
            self.events.borrow_mut().push(DiscordEvent::Deferred);
            Ok(())
        }

        async fn perform(
            &self,
            action: &ModerationAction,
            target_id: serenity::UserId,
            reason: &str,
        ) -> Result<(), String> {
            self.events.borrow_mut().push(DiscordEvent::Performed(
                action.clone(),
                target_id,
                reason.to_owned(),
            ));
            match &self.action_failure {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }

        async fn send_ephemeral(&self, content: String) -> Result<(), Error> {
            self.events
                .borrow_mut()
                .push(DiscordEvent::Replied(content));
            Ok(())
        }

        async fn post_audit_record(&self, record: AuditRecord) {
            self.events.borrow_mut().push(DiscordEvent::Audited(record));
        }
    }

    #[tokio::test]
    async fn successful_moderation_action_replies_and_records_audit_facts() -> Result<(), Error> {
        let discord = FakeDiscord::default();
        let moderator_id = serenity::UserId::new(7);
        let target_id = serenity::UserId::new(42);

        execute_action(
            &discord,
            moderator_id,
            target_id,
            " reason ".to_owned(),
            ModerationAction::Ban,
        )
        .await?;

        assert_eq!(
            discord.events.into_inner(),
            vec![
                DiscordEvent::Deferred,
                DiscordEvent::Performed(ModerationAction::Ban, target_id, "reason".to_owned()),
                DiscordEvent::Replied("Banned <@42>.".to_owned()),
                DiscordEvent::Audited(AuditRecord {
                    action: "Ban",
                    target_id,
                    moderator_id,
                    reason: "reason".to_owned(),
                    timeout: None,
                }),
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn self_target_is_rejected_without_calling_discord_moderation() -> Result<(), Error> {
        let discord = FakeDiscord::default();
        let member_id = serenity::UserId::new(42);

        execute_action(
            &discord,
            member_id,
            member_id,
            "reason".to_owned(),
            ModerationAction::Kick,
        )
        .await?;

        assert_eq!(
            discord.events.into_inner(),
            vec![DiscordEvent::Replied(
                "You cannot target yourself with a moderation action.".to_owned()
            )]
        );
        Ok(())
    }

    #[tokio::test]
    async fn rejected_moderation_action_reports_failure_without_an_audit_record()
    -> Result<(), Error> {
        let discord = FakeDiscord {
            action_failure: Some("missing permissions".to_owned()),
            ..Default::default()
        };
        let target_id = serenity::UserId::new(42);

        execute_action(
            &discord,
            serenity::UserId::new(7),
            target_id,
            "reason".to_owned(),
            ModerationAction::Ban,
        )
        .await?;

        assert_eq!(
            discord.events.into_inner(),
            vec![
                DiscordEvent::Deferred,
                DiscordEvent::Performed(ModerationAction::Ban, target_id, "reason".to_owned()),
                DiscordEvent::Replied("Failed to ban the user: missing permissions".to_owned()),
            ]
        );
        Ok(())
    }

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
