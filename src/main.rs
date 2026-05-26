mod bot;
mod commands;
mod db;

use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use bot::AppState;
use db::InstanceData;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug)]
struct Config {
    discord_token: String,
    database_path: PathBuf,
    backup_snapshots_enabled: bool,
    dev_guild_id: Option<u64>,
}

impl Config {
    fn from_env() -> Result<Self> {
        let token_file = required_env("DISCORD_TOKEN_FILE")?;
        Self::from_values(
            PathBuf::from(token_file),
            PathBuf::from(required_env("DATABASE_PATH")?),
            env::var("BACKUP_SNAPSHOTS_ENABLED").ok(),
            env::var("DEV_GUILD_ID").ok(),
        )
    }

    fn from_values(
        token_file: PathBuf,
        database_path: PathBuf,
        backup_snapshots_enabled: Option<String>,
        dev_guild_id: Option<String>,
    ) -> Result<Self> {
        let discord_token = fs::read_to_string(&token_file)
            .with_context(|| format!("failed to read Discord token file {}", token_file.display()))?
            .trim()
            .to_owned();
        if discord_token.is_empty() {
            bail!("DISCORD_TOKEN_FILE contains an empty Discord token");
        }

        let backup_snapshots_enabled = match backup_snapshots_enabled.as_deref() {
            None | Some("false") => false,
            Some("true") => true,
            Some(value) => {
                bail!("BACKUP_SNAPSHOTS_ENABLED must be either true or false, received {value:?}")
            }
        };

        let dev_guild_id = dev_guild_id
            .map(|value| {
                value
                    .parse::<u64>()
                    .context("DEV_GUILD_ID must be a Discord guild numeric ID")
            })
            .transpose()?;

        Ok(Self {
            discord_token,
            database_path,
            backup_snapshots_enabled,
            dev_guild_id,
        })
    }
}

fn required_env(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("required environment variable {name} is not set"))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("sillybot=info")),
        )
        .init();

    let config = Config::from_env()?;
    info!(
        version = env!("CARGO_PKG_VERSION"),
        database_path = %config.database_path.display(),
        "starting Sillybot instance"
    );

    let instance_data =
        InstanceData::open(&config.database_path, config.backup_snapshots_enabled).await?;
    let state = AppState {
        instance_data,
        database_path: config.database_path,
    };

    bot::run(config.discord_token, config.dev_guild_id, state).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_file() -> Result<(tempfile::TempDir, PathBuf)> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("discord_token");
        fs::write(&path, "bot-token\n")?;
        Ok((directory, path))
    }

    #[test]
    fn accepts_disabled_snapshots_and_a_development_guild() -> Result<()> {
        let (_directory, token_file) = token_file()?;
        let config = Config::from_values(
            token_file,
            PathBuf::from("sillybot.db"),
            Some("false".to_owned()),
            Some("42".to_owned()),
        )?;

        assert_eq!(config.discord_token, "bot-token");
        assert!(!config.backup_snapshots_enabled);
        assert_eq!(config.dev_guild_id, Some(42));
        Ok(())
    }

    #[test]
    fn rejects_invalid_snapshot_setting_instead_of_silently_disabling_it() -> Result<()> {
        let (_directory, token_file) = token_file()?;
        let error = Config::from_values(
            token_file,
            PathBuf::from("sillybot.db"),
            Some("tru".to_owned()),
            None,
        )
        .unwrap_err();

        assert!(error.to_string().contains("must be either true or false"));
        Ok(())
    }

    #[test]
    fn accepts_enabled_snapshots_for_a_protected_instance() -> Result<()> {
        let (_directory, token_file) = token_file()?;
        let config = Config::from_values(
            token_file,
            PathBuf::from("sillybot.db"),
            Some("true".to_owned()),
            None,
        )?;

        assert!(config.backup_snapshots_enabled);
        Ok(())
    }
}
