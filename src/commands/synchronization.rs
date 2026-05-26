use std::{future::Future, time::Duration};

use anyhow::{Context as _, Result};
use poise::serenity_prelude as serenity;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::bot::{AppState, Error};

pub(crate) fn declared_commands() -> Vec<poise::Command<AppState, Error>> {
    vec![
        super::ping::ping(),
        super::count::count(),
        super::info::info(),
        super::admin::ban::ban(),
        super::admin::kick::kick(),
        super::admin::timeout::timeout(),
        super::admin::log_channel::admin_log(),
        super::patch_notes::patch_notes(),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegistrationScope {
    Global,
    DevelopmentGuild(serenity::GuildId),
}

impl From<Option<u64>> for RegistrationScope {
    fn from(dev_guild_id: Option<u64>) -> Self {
        match dev_guild_id {
            Some(guild_id) => Self::DevelopmentGuild(serenity::GuildId::new(guild_id)),
            None => Self::Global,
        }
    }
}

pub(crate) async fn synchronize(
    http: &serenity::Http,
    commands: &[poise::Command<AppState, Error>],
    dev_guild_id: Option<u64>,
) -> Result<()> {
    let scope = dev_guild_id.into();
    register_in_scope(
        scope,
        |scope| async move {
            let application = http
                .get_current_application_info()
                .await
                .map_err(classify_synchronization_error)?;
            http.set_application_id(application.id);
            match scope {
                RegistrationScope::DevelopmentGuild(guild_id) => {
                    poise::builtins::register_in_guild(http, commands, guild_id).await
                }
                RegistrationScope::Global => {
                    poise::builtins::register_globally(http, commands).await
                }
            }
            .map_err(classify_synchronization_error)
        },
        sleep,
    )
    .await
    .context("failed to synchronize Discord application commands")?;

    match scope {
        RegistrationScope::DevelopmentGuild(guild_id) => {
            info!(
                guild_id = guild_id.get(),
                "synchronized development guild commands"
            )
        }
        RegistrationScope::Global => info!("synchronized global application commands"),
    }
    Ok(())
}

async fn register_in_scope<E, Registration, RegistrationFuture, Sleeper, SleepFuture>(
    scope: RegistrationScope,
    mut registration: Registration,
    sleeper: Sleeper,
) -> std::result::Result<(), E>
where
    E: std::fmt::Display,
    Registration: FnMut(RegistrationScope) -> RegistrationFuture,
    RegistrationFuture: Future<Output = std::result::Result<(), SynchronizationFailure<E>>>,
    Sleeper: FnMut(Duration) -> SleepFuture,
    SleepFuture: Future<Output = ()>,
{
    retry_synchronization(|| registration(scope), sleeper).await
}

#[derive(Debug)]
enum SynchronizationFailure<E> {
    Transient(E),
    Fatal(E),
}

fn classify_synchronization_error(
    error: serenity::Error,
) -> SynchronizationFailure<serenity::Error> {
    if is_transient(&error) {
        SynchronizationFailure::Transient(error)
    } else {
        SynchronizationFailure::Fatal(error)
    }
}

async fn retry_synchronization<E, Operation, OperationFuture, Sleeper, SleepFuture>(
    mut operation: Operation,
    mut sleeper: Sleeper,
) -> std::result::Result<(), E>
where
    E: std::fmt::Display,
    Operation: FnMut() -> OperationFuture,
    OperationFuture: Future<Output = std::result::Result<(), SynchronizationFailure<E>>>,
    Sleeper: FnMut(Duration) -> SleepFuture,
    SleepFuture: Future<Output = ()>,
{
    let mut attempt = 1_u32;
    loop {
        match operation().await {
            Ok(()) => return Ok(()),
            Err(SynchronizationFailure::Transient(error)) => {
                let delay = retry_delay(attempt);
                warn!(
                    attempt,
                    delay_seconds = delay.as_secs(),
                    error = %error,
                    "transient command synchronization failure; retrying"
                );
                sleeper(delay).await;
                attempt = attempt.saturating_add(1);
            }
            Err(SynchronizationFailure::Fatal(error)) => return Err(error),
        }
    }
}

fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs((1_u64 << attempt.saturating_sub(1).min(5)).min(30))
}

fn is_transient(error: &serenity::Error) -> bool {
    match error {
        serenity::Error::Http(http_error) => match http_error {
            serenity::HttpError::Request(_) => true,
            serenity::HttpError::UnsuccessfulRequest(response) => {
                response.status_code.as_u16() == 429 || response.status_code.is_server_error()
            }
            _ => false,
        },
        serenity::Error::Io(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque};

    use super::*;

    #[test]
    fn declares_only_guild_slash_commands_with_required_permissions() {
        let commands = declared_commands();
        assert_eq!(
            commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "ping",
                "count",
                "info",
                "ban",
                "kick",
                "timeout",
                "admin-log",
                "patch-notes"
            ]
        );
        assert!(
            commands
                .iter()
                .all(|command| command.slash_action.is_some())
        );
        assert!(commands.iter().all(|command| command.guild_only));

        let permissions = commands
            .iter()
            .map(|command| (command.name.as_str(), command.default_member_permissions))
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(permissions["ban"], serenity::Permissions::BAN_MEMBERS);
        assert_eq!(permissions["kick"], serenity::Permissions::KICK_MEMBERS);
        assert_eq!(
            permissions["timeout"],
            serenity::Permissions::MODERATE_MEMBERS
        );
        assert_eq!(
            permissions["admin-log"],
            serenity::Permissions::MANAGE_GUILD
        );
        assert_eq!(
            permissions["patch-notes"],
            serenity::Permissions::MANAGE_GUILD
        );

        let admin_log = commands
            .iter()
            .find(|command| command.name == "admin-log")
            .unwrap();
        assert_eq!(
            admin_log
                .subcommands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            vec!["set", "clear", "show"]
        );
        let patch_notes = commands
            .iter()
            .find(|command| command.name == "patch-notes")
            .unwrap();
        assert_eq!(
            patch_notes
                .subcommands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            vec!["set", "clear", "show"]
        );
    }

    #[test]
    fn command_synchronization_backoff_is_capped() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(5), Duration::from_secs(16));
        assert_eq!(retry_delay(6), Duration::from_secs(30));
        assert_eq!(retry_delay(200), Duration::from_secs(30));
    }

    #[test]
    fn defaults_to_global_registration_and_selects_dev_guild_when_configured() {
        assert_eq!(RegistrationScope::from(None), RegistrationScope::Global);
        assert_eq!(
            RegistrationScope::from(Some(42)),
            RegistrationScope::DevelopmentGuild(serenity::GuildId::new(42))
        );
    }

    #[tokio::test]
    async fn transient_synchronization_failures_retry_until_success() {
        let results = RefCell::new(VecDeque::from([
            Err(SynchronizationFailure::Transient("network unavailable")),
            Err(SynchronizationFailure::Transient("rate limited")),
            Ok(()),
        ]));
        let delays = RefCell::new(Vec::new());

        retry_synchronization(
            || std::future::ready(results.borrow_mut().pop_front().unwrap()),
            |delay| {
                delays.borrow_mut().push(delay);
                std::future::ready(())
            },
        )
        .await
        .unwrap();

        assert_eq!(
            delays.into_inner(),
            vec![Duration::from_secs(1), Duration::from_secs(2)]
        );
    }

    #[tokio::test]
    async fn non_transient_synchronization_failure_returns_without_retry() {
        let attempts = RefCell::new(0_u32);
        let result = retry_synchronization(
            || {
                *attempts.borrow_mut() += 1;
                std::future::ready(Err(SynchronizationFailure::Fatal("invalid credentials")))
            },
            |_delay| std::future::ready::<()>(()),
        )
        .await;

        assert_eq!(result, Err("invalid credentials"));
        assert_eq!(*attempts.borrow(), 1);
    }

    #[tokio::test]
    async fn registration_calls_the_selected_discord_scope() {
        for (scope, expected) in [
            (RegistrationScope::Global, RegistrationScope::Global),
            (
                RegistrationScope::DevelopmentGuild(serenity::GuildId::new(42)),
                RegistrationScope::DevelopmentGuild(serenity::GuildId::new(42)),
            ),
        ] {
            let registrations = RefCell::new(Vec::new());
            register_in_scope(
                scope,
                |registered_scope| {
                    registrations.borrow_mut().push(registered_scope);
                    std::future::ready(Ok::<(), SynchronizationFailure<&str>>(()))
                },
                |_delay| std::future::ready(()),
            )
            .await
            .unwrap();
            assert_eq!(registrations.into_inner(), vec![expected]);
        }
    }
}
