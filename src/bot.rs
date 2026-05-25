use std::sync::Arc;

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

pub async fn run(token: String, dev_guild_id: Option<u64>, state: AppState) -> Result<()> {
    let setup_state = state.clone();
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::synchronization::declared_commands(),
            initialize_owners: false,
            event_handler: |_, event, _, _| {
                Box::pin(async move {
                    if let serenity::FullEvent::Ready { data_about_bot, .. } = event {
                        info!(user = %data_about_bot.user.name, "Discord gateway ready");
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

    let http = serenity::Http::new(&token);
    commands::synchronization::synchronize(&http, framework.options(), dev_guild_id).await?;

    let intents = gateway_intents();
    let mut client = serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await
        .context("failed to create Discord gateway client")?;
    let shard_manager = Arc::clone(&client.shard_manager);

    let client_task = client.start();
    tokio::pin!(client_task);
    let daily_snapshots = state.instance_data.run_daily_snapshots();
    tokio::pin!(daily_snapshots);
    tokio::select! {
        result = &mut client_task => result.context("Discord gateway client stopped with an error")?,
        result = &mut daily_snapshots => result?,
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
    use super::*;

    #[test]
    fn application_commands_require_no_gateway_intents() {
        assert_eq!(gateway_intents(), serenity::GatewayIntents::empty());
    }
}
