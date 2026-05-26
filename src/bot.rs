use std::{future::Future, sync::Arc};

use anyhow::{Context as _, Result};
use poise::serenity_prelude as serenity;
use tracing::{error, info};

use crate::{commands, db::InstanceData};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, AppState, Error>;

#[derive(Clone, Debug)]
pub struct AppState {
    pub instance_data: InstanceData,
}

fn gateway_intents() -> serenity::GatewayIntents {
    serenity::GatewayIntents::empty()
}

async fn after_successful_synchronization<
    T,
    E,
    Synchronize,
    SynchronizeFuture,
    Gateway,
    GatewayFuture,
>(
    synchronize: Synchronize,
    gateway: Gateway,
) -> std::result::Result<T, E>
where
    Synchronize: FnOnce() -> SynchronizeFuture,
    SynchronizeFuture: Future<Output = std::result::Result<(), E>>,
    Gateway: FnOnce() -> GatewayFuture,
    GatewayFuture: Future<Output = std::result::Result<T, E>>,
{
    synchronize().await?;
    gateway().await
}

pub async fn run(token: String, dev_guild_id: Option<u64>, state: AppState) -> Result<()> {
    let http = serenity::Http::new(&token);
    let commands_to_register = commands::synchronization::declared_commands();
    let setup_state = state.clone();
    let mut client = after_successful_synchronization(
        || commands::synchronization::synchronize(&http, &commands_to_register, dev_guild_id),
        || async move {
            let framework = poise::Framework::builder()
                .options(poise::FrameworkOptions {
                    commands: commands::synchronization::declared_commands(),
                    initialize_owners: false,
                    event_handler: |serenity_ctx, event, _, data| {
                        Box::pin(async move {
                            if let serenity::FullEvent::Ready { data_about_bot, .. } = event {
                                info!(user = %data_about_bot.user.name, "Discord gateway ready");
                            }
                            if let serenity::FullEvent::InteractionCreate { interaction } = event
                                && let Some(component) = interaction.as_message_component()
                            {
                                commands::self_roles::handle_component(
                                    serenity_ctx,
                                    component,
                                    &data.instance_data,
                                )
                                .await?;
                            }
                            Ok(())
                        })
                    },
                    on_error: |error| {
                        Box::pin(async move {
                            error!(error = %error, "Discord command or framework error");
                            if let Err(response_error) = poise::builtins::on_error(error).await {
                                error!(
                                    error = %response_error,
                                    "failed to send Discord command error response"
                                );
                            }
                        })
                    },
                    ..Default::default()
                })
                .setup(move |_, _, _| {
                    let state = setup_state.clone();
                    Box::pin(async move { Ok(state) })
                })
                .build();

            serenity::ClientBuilder::new(&token, gateway_intents())
                .framework(framework)
                .await
                .context("failed to create Discord gateway client")
        },
    )
    .await?;
    let shard_manager = Arc::clone(&client.shard_manager);

    let client_task = client.start();
    tokio::pin!(client_task);
    let daily_snapshots = state.instance_data.run_daily_snapshots();
    tokio::pin!(daily_snapshots);
    let patch_notes_updates =
        commands::patch_notes::run_updates(&http, state.instance_data.clone());
    tokio::pin!(patch_notes_updates);
    tokio::select! {
        result = &mut client_task => result.context("Discord gateway client stopped with an error")?,
        result = &mut daily_snapshots => result?,
        result = &mut patch_notes_updates => result?,
        result = shutdown_signal() => {
            result?;
            info!("shutdown signal received; stopping Discord gateway");
            shard_manager.shutdown_all().await;
            client_task.await.context("Discord gateway client stopped with an error")?;
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn shutdown_signal() -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate =
        signal(SignalKind::terminate()).context("failed to install SIGTERM handler")?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.context("failed to install Ctrl-C handler")?,
        _ = terminate.recv() => {}
    }
    Ok(())
}

#[cfg(not(unix))]
async fn shutdown_signal() -> Result<()> {
    tokio::signal::ctrl_c()
        .await
        .context("failed to install Ctrl-C handler")
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, future::ready};

    use super::*;

    #[test]
    fn application_commands_require_no_gateway_intents() {
        assert_eq!(gateway_intents(), serenity::GatewayIntents::empty());
    }

    #[tokio::test]
    async fn gateway_setup_runs_only_after_command_synchronization_succeeds() {
        let events = RefCell::new(Vec::new());
        after_successful_synchronization(
            || {
                events.borrow_mut().push("synchronization");
                ready(Ok::<(), &str>(()))
            },
            || {
                events.borrow_mut().push("gateway");
                ready(Ok::<(), &str>(()))
            },
        )
        .await
        .unwrap();
        assert_eq!(events.into_inner(), vec!["synchronization", "gateway"]);

        let events = RefCell::new(Vec::new());
        let result = after_successful_synchronization(
            || {
                events.borrow_mut().push("synchronization");
                ready(Err::<(), &str>("registration rejected"))
            },
            || {
                events.borrow_mut().push("gateway");
                ready(Ok::<(), &str>(()))
            },
        )
        .await;
        assert_eq!(result, Err("registration rejected"));
        assert_eq!(events.into_inner(), vec!["synchronization"]);
    }
}
