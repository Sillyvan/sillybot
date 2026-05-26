use std::{
    future::pending,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use poise::serenity_prelude as serenity;
use tokio::{sync::Mutex, time};
use tracing::{info, warn};
use turso::{Builder, Connection, Value, transaction::TransactionBehavior};

const MIGRATION_1: &str = include_str!("../../migrations/0001_counter.sql");
const MIGRATION_2: &str = include_str!("../../migrations/0002_admin_log_channel.sql");
const MIGRATION_3: &str = include_str!("../../migrations/0003_patch_notes_channel.sql");
const LATEST_SCHEMA_VERSION: i64 = 3;
const DAILY_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug)]
pub struct InstanceData {
    conn: Arc<Mutex<Connection>>,
    snapshots: Option<SnapshotPolicy>,
}

#[derive(Clone, Debug)]
struct SnapshotPolicy {
    snapshots_dir: PathBuf,
    markers_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchNotesSubscription {
    pub guild_id: serenity::GuildId,
    pub channel_id: serenity::ChannelId,
    pub last_article_url: Option<String>,
}

impl InstanceData {
    pub async fn open(path: &Path, snapshots_enabled: bool) -> Result<Self> {
        let data_dir = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        if let Some(data_dir) = data_dir {
            std::fs::create_dir_all(data_dir).with_context(|| {
                format!("failed to create database directory {}", data_dir.display())
            })?;
        }
        let database_existed = path.exists();
        let path_text = path
            .to_str()
            .context("DATABASE_PATH is not valid UTF-8 for Turso")?;
        let mut builder = Builder::new_local(path_text);
        if snapshots_enabled {
            builder = builder.experimental_vacuum(true);
        }
        let database = builder
            .build()
            .await
            .context("failed to open Turso database")?;
        let mut conn = database
            .connect()
            .context("failed to connect to Turso database")?;
        let snapshots = snapshots_enabled
            .then(|| SnapshotPolicy::new(data_dir.unwrap_or_else(|| Path::new("."))))
            .transpose()?;

        initialize(&mut conn, database_existed, snapshots.as_ref()).await?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            snapshots,
        })
    }

    pub async fn increment_counter(&self) -> Result<i64> {
        let mut conn = self.conn.lock().await;
        conn.set_transaction_behavior(TransactionBehavior::Immediate);
        let tx = conn
            .transaction()
            .await
            .context("failed to begin counter transaction")?;
        tx.execute("UPDATE command_counter SET value = value + 1;", ())
            .await
            .context("failed to increment global counter")?;
        let mut rows = tx
            .query("SELECT value FROM command_counter;", ())
            .await
            .context("failed to read global counter")?;
        let row = rows
            .next()
            .await
            .context("failed to retrieve global counter")?
            .context("global counter row is missing")?;
        let value = match row
            .get_value(0)
            .context("failed to decode global counter")?
        {
            Value::Integer(value) => value,
            value => bail!("unexpected global counter value: {value:?}"),
        };
        tx.commit()
            .await
            .context("failed to commit counter transaction")?;
        Ok(value)
    }

    pub async fn admin_log_channel(
        &self,
        guild_id: serenity::GuildId,
    ) -> Result<Option<serenity::ChannelId>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT channel_id FROM guild_admin_log_channel WHERE guild_id = ?1;",
                (snowflake_to_integer(guild_id.get())?,),
            )
            .await
            .context("failed to query admin log channel")?;
        let Some(row) = rows
            .next()
            .await
            .context("failed to retrieve admin log channel")?
        else {
            return Ok(None);
        };
        match row
            .get_value(0)
            .context("failed to decode admin log channel")?
        {
            Value::Integer(channel_id) => Ok(Some(serenity::ChannelId::new(
                u64::try_from(channel_id).context("admin log channel ID is negative")?,
            ))),
            value => bail!("unexpected admin log channel value: {value:?}"),
        }
    }

    pub async fn set_admin_log_channel(
        &self,
        guild_id: serenity::GuildId,
        channel_id: serenity::ChannelId,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO guild_admin_log_channel (guild_id, channel_id) VALUES (?1, ?2)
             ON CONFLICT (guild_id) DO UPDATE SET channel_id = excluded.channel_id;",
            (
                snowflake_to_integer(guild_id.get())?,
                snowflake_to_integer(channel_id.get())?,
            ),
        )
        .await
        .context("failed to set admin log channel")?;
        Ok(())
    }

    pub async fn clear_admin_log_channel(&self, guild_id: serenity::GuildId) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM guild_admin_log_channel WHERE guild_id = ?1;",
            (snowflake_to_integer(guild_id.get())?,),
        )
        .await
        .context("failed to clear admin log channel")?;
        Ok(())
    }

    pub async fn patch_notes_channel(
        &self,
        guild_id: serenity::GuildId,
    ) -> Result<Option<serenity::ChannelId>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT channel_id FROM guild_patch_notes_channel WHERE guild_id = ?1;",
                (snowflake_to_integer(guild_id.get())?,),
            )
            .await
            .context("failed to query patch notes channel")?;
        let Some(row) = rows
            .next()
            .await
            .context("failed to retrieve patch notes channel")?
        else {
            return Ok(None);
        };
        match row
            .get_value(0)
            .context("failed to decode patch notes channel")?
        {
            Value::Integer(channel_id) => Ok(Some(serenity::ChannelId::new(
                u64::try_from(channel_id).context("patch notes channel ID is negative")?,
            ))),
            value => bail!("unexpected patch notes channel value: {value:?}"),
        }
    }

    pub async fn set_patch_notes_channel(
        &self,
        guild_id: serenity::GuildId,
        channel_id: serenity::ChannelId,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO guild_patch_notes_channel (guild_id, channel_id) VALUES (?1, ?2)
             ON CONFLICT (guild_id) DO UPDATE SET channel_id = excluded.channel_id;",
            (
                snowflake_to_integer(guild_id.get())?,
                snowflake_to_integer(channel_id.get())?,
            ),
        )
        .await
        .context("failed to set patch notes channel")?;
        Ok(())
    }

    pub async fn clear_patch_notes_channel(&self, guild_id: serenity::GuildId) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM guild_patch_notes_channel WHERE guild_id = ?1;",
            (snowflake_to_integer(guild_id.get())?,),
        )
        .await
        .context("failed to clear patch notes channel")?;
        Ok(())
    }

    pub async fn patch_notes_subscriptions(&self) -> Result<Vec<PatchNotesSubscription>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT guild_id, channel_id, last_article_url FROM guild_patch_notes_channel;",
                (),
            )
            .await
            .context("failed to query patch notes subscriptions")?;
        let mut subscriptions = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .context("failed to retrieve patch notes subscription")?
        {
            let Value::Integer(guild_id) = row.get_value(0)? else {
                bail!("unexpected patch notes guild ID value");
            };
            let Value::Integer(channel_id) = row.get_value(1)? else {
                bail!("unexpected patch notes channel ID value");
            };
            let last_article_url = match row.get_value(2)? {
                Value::Text(value) => Some(value),
                Value::Null => None,
                value => bail!("unexpected patch notes article URL value: {value:?}"),
            };
            subscriptions.push(PatchNotesSubscription {
                guild_id: serenity::GuildId::new(
                    u64::try_from(guild_id).context("patch notes guild ID is negative")?,
                ),
                channel_id: serenity::ChannelId::new(
                    u64::try_from(channel_id).context("patch notes channel ID is negative")?,
                ),
                last_article_url,
            });
        }
        Ok(subscriptions)
    }

    pub async fn mark_patch_notes_article_seen(
        &self,
        guild_id: serenity::GuildId,
        article_url: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE guild_patch_notes_channel SET last_article_url = ?2 WHERE guild_id = ?1;",
            (snowflake_to_integer(guild_id.get())?, article_url),
        )
        .await
        .context("failed to record delivered patch notes article")?;
        Ok(())
    }

    pub async fn run_daily_snapshots(&self) -> Result<()> {
        self.run_snapshots_every(DAILY_SNAPSHOT_INTERVAL).await
    }

    async fn run_snapshots_every(&self, interval_duration: Duration) -> Result<()> {
        let Some(policy) = &self.snapshots else {
            return pending().await;
        };
        let mut interval = time::interval(interval_duration);
        interval.tick().await;
        loop {
            interval.tick().await;
            let conn = self.conn.lock().await;
            if let Err(error) = policy.snapshot(&conn, "daily").await {
                warn!(%error, "failed to create daily database snapshot");
            }
        }
    }
}

impl SnapshotPolicy {
    fn new(data_dir: &Path) -> Result<Self> {
        let policy = Self {
            snapshots_dir: data_dir.join("snapshots"),
            markers_dir: data_dir.join(".snapshot-events"),
        };
        std::fs::create_dir_all(&policy.snapshots_dir).with_context(|| {
            format!(
                "failed to create snapshots directory {}",
                policy.snapshots_dir.display()
            )
        })?;
        std::fs::create_dir_all(&policy.markers_dir).with_context(|| {
            format!(
                "failed to create snapshot marker directory {}",
                policy.markers_dir.display()
            )
        })?;
        Ok(policy)
    }

    async fn event_snapshot(&self, conn: &Connection, marker: &str, purpose: &str) -> Result<()> {
        let marker_path = self.markers_dir.join(format!("{marker}.complete"));
        if marker_path.exists() {
            info!(marker, "reusing completed database snapshot event");
            return Ok(());
        }
        let snapshot_path = self.snapshot(conn, purpose).await?;
        std::fs::write(&marker_path, snapshot_path.to_string_lossy().as_bytes()).with_context(
            || format!("failed to record snapshot marker {}", marker_path.display()),
        )?;
        Ok(())
    }

    fn has_current_schema_snapshot(&self, version: i64) -> bool {
        ["post-migration", "baseline"].iter().any(|purpose| {
            self.markers_dir
                .join(format!("{purpose}-{version}.complete"))
                .exists()
        })
    }

    fn mark_post_migration_required(&self, version: i64) -> Result<()> {
        let marker = self
            .markers_dir
            .join(format!("post-migration-{version}.pending"));
        std::fs::write(&marker, b"pending").with_context(|| {
            format!(
                "failed to record pending post-migration snapshot {}",
                marker.display()
            )
        })
    }

    fn post_migration_required(&self, version: i64) -> bool {
        self.markers_dir
            .join(format!("post-migration-{version}.pending"))
            .exists()
    }

    fn clear_post_migration_required(&self, version: i64) -> Result<()> {
        let marker = self
            .markers_dir
            .join(format!("post-migration-{version}.pending"));
        match std::fs::remove_file(&marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| {
                format!(
                    "failed to clear pending post-migration snapshot {}",
                    marker.display()
                )
            }),
        }
    }

    async fn snapshot(&self, conn: &Connection, purpose: &str) -> Result<PathBuf> {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        let output = self
            .snapshots_dir
            .join(format!("sillybot-{purpose}-{timestamp}.db"));
        let sql_path = output
            .to_str()
            .context("snapshot destination is not valid UTF-8 for Turso")?
            .replace('\'', "''");
        conn.execute(format!("VACUUM INTO '{sql_path}';"), ())
            .await
            .with_context(|| format!("failed to create database snapshot {}", output.display()))?;
        info!(purpose, path = %output.display(), "created database snapshot");
        Ok(output)
    }
}

async fn initialize(
    conn: &mut Connection,
    database_existed: bool,
    snapshots: Option<&SnapshotPolicy>,
) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER NOT NULL);",
        (),
    )
    .await
    .context("failed to create schema migrations table")?;

    let applied = applied_version(conn).await?;
    if applied > LATEST_SCHEMA_VERSION {
        bail!(
            "database schema version {applied} is newer than supported version {LATEST_SCHEMA_VERSION}"
        );
    }
    if let Some(snapshots) = snapshots
        && database_existed
        && applied < LATEST_SCHEMA_VERSION
    {
        snapshots
            .event_snapshot(
                conn,
                &format!("pre-migration-{applied}-to-{LATEST_SCHEMA_VERSION}"),
                &format!("pre-migration-{applied}-to-{LATEST_SCHEMA_VERSION}"),
            )
            .await?;
    }
    if let Some(snapshots) = snapshots
        && (!database_existed || applied < LATEST_SCHEMA_VERSION)
    {
        snapshots.mark_post_migration_required(LATEST_SCHEMA_VERSION)?;
    }

    let migrated = apply_migrations(conn, applied).await?;
    ensure_counter_row(conn).await?;
    verify_journal_mode(conn).await?;

    if let Some(snapshots) = snapshots {
        if !database_existed || migrated || snapshots.post_migration_required(LATEST_SCHEMA_VERSION)
        {
            snapshots
                .event_snapshot(
                    conn,
                    &format!("post-migration-{LATEST_SCHEMA_VERSION}"),
                    &format!("post-migration-{LATEST_SCHEMA_VERSION}"),
                )
                .await?;
            snapshots.clear_post_migration_required(LATEST_SCHEMA_VERSION)?;
        } else if !snapshots.has_current_schema_snapshot(LATEST_SCHEMA_VERSION) {
            snapshots
                .event_snapshot(
                    conn,
                    &format!("baseline-{LATEST_SCHEMA_VERSION}"),
                    &format!("baseline-{LATEST_SCHEMA_VERSION}"),
                )
                .await?;
        }
    }
    Ok(())
}

async fn apply_migrations(conn: &mut Connection, applied: i64) -> Result<bool> {
    let mut migrated = false;
    for (version, sql, name) in [
        (1, MIGRATION_1, "0001_counter"),
        (2, MIGRATION_2, "0002_admin_log_channel"),
        (3, MIGRATION_3, "0003_patch_notes_channel"),
    ] {
        if applied >= version {
            continue;
        }
        conn.set_transaction_behavior(TransactionBehavior::Immediate);
        let tx = conn
            .transaction()
            .await
            .context("failed to begin database migration transaction")?;
        tx.execute(sql, ())
            .await
            .with_context(|| format!("failed to apply migration {name}"))?;
        tx.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1);",
            (version,),
        )
        .await
        .with_context(|| format!("failed to record migration {name}"))?;
        tx.commit()
            .await
            .with_context(|| format!("failed to commit migration {name}"))?;
        info!(version, "applied database migration");
        migrated = true;
    }
    if !migrated {
        info!(version = applied, "database schema is current");
    }
    Ok(migrated)
}

async fn verify_journal_mode(conn: &Connection) -> Result<()> {
    let journal_mode = single_text(conn, "PRAGMA journal_mode;")
        .await
        .context("failed to inspect database journal mode")?;
    if journal_mode.eq_ignore_ascii_case("mvcc") {
        bail!("database journal mode mvcc is experimental and unsupported");
    }
    info!(journal_mode, "active database journal mode");
    Ok(())
}

async fn ensure_counter_row(conn: &Connection) -> Result<()> {
    let count = single_integer(conn, "SELECT COUNT(*) FROM command_counter;")
        .await
        .context("failed to inspect global counter row count")?;
    match count {
        0 => {
            conn.execute("INSERT INTO command_counter (value) VALUES (0);", ())
                .await
                .context("failed to initialize global counter")?;
            info!("initialized global counter");
            Ok(())
        }
        1 => Ok(()),
        count => bail!(
            "command_counter contains {count} rows; expected exactly one durable global counter row"
        ),
    }
}

async fn applied_version(conn: &Connection) -> Result<i64> {
    match single_value(conn, "SELECT MAX(version) FROM schema_migrations;")
        .await
        .context("failed to read migration state")?
    {
        Value::Integer(version) => Ok(version),
        Value::Null => Ok(0),
        value => bail!("unexpected migration version value: {value:?}"),
    }
}

async fn single_integer(conn: &Connection, sql: &str) -> Result<i64> {
    match single_value(conn, sql).await? {
        Value::Integer(value) => Ok(value),
        value => bail!("unexpected integer result: {value:?}"),
    }
}

async fn single_text(conn: &Connection, sql: &str) -> Result<String> {
    match single_value(conn, sql).await? {
        Value::Text(value) => Ok(value),
        value => bail!("unexpected text result: {value:?}"),
    }
}

async fn single_value(conn: &Connection, sql: &str) -> Result<Value> {
    let mut rows = conn.query(sql, ()).await?;
    let row = rows.next().await?.context("query returned no row")?;
    Ok(row.get_value(0)?)
}

fn snowflake_to_integer(value: u64) -> Result<i64> {
    i64::try_from(value).context("Discord ID exceeds the database integer range")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_names(directory: &Path) -> Result<Vec<String>> {
        let snapshots = directory.join("snapshots");
        if !snapshots.exists() {
            return Ok(Vec::new());
        }
        let mut names = std::fs::read_dir(snapshots)?
            .filter_map(|entry| {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => return Some(Err(error.into())),
                };
                (entry.path().extension().and_then(|ext| ext.to_str()) == Some("db"))
                    .then(|| Ok(entry.file_name().to_string_lossy().into_owned()))
            })
            .collect::<Result<Vec<_>>>()?;
        names.sort();
        Ok(names)
    }

    async fn create_version_one_database(path: &Path) -> Result<()> {
        let database = Builder::new_local(path.to_str().unwrap()).build().await?;
        let conn = database.connect()?;
        conn.execute(
            "CREATE TABLE schema_migrations (version INTEGER NOT NULL);",
            (),
        )
        .await?;
        conn.execute(MIGRATION_1, ()).await?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (1);", ())
            .await?;
        conn.execute("INSERT INTO command_counter (value) VALUES (7);", ())
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn increments_sequentially_and_persists_after_reopen() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        {
            let store = InstanceData::open(&path, false).await?;
            assert_eq!(store.increment_counter().await?, 1);
            assert_eq!(store.increment_counter().await?, 2);
        }
        let reopened = InstanceData::open(&path, false).await?;
        assert_eq!(reopened.increment_counter().await?, 3);
        Ok(())
    }

    #[tokio::test]
    async fn migration_is_idempotent() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        InstanceData::open(&path, false).await?;
        InstanceData::open(&path, false).await?;
        Ok(())
    }

    #[tokio::test]
    async fn repairs_an_empty_counter_table_in_an_existing_schema() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        InstanceData::open(&path, false).await?;
        let database = Builder::new_local(path.to_str().unwrap()).build().await?;
        let conn = database.connect()?;
        conn.execute("DELETE FROM command_counter;", ()).await?;
        drop(conn);

        let reopened = InstanceData::open(&path, false).await?;
        assert_eq!(reopened.increment_counter().await?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn stores_installed_guild_admin_log_channels() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        let data = InstanceData::open(&path, false).await?;
        let guild = serenity::GuildId::new(42);
        assert_eq!(data.admin_log_channel(guild).await?, None);
        data.set_admin_log_channel(guild, serenity::ChannelId::new(100))
            .await?;
        assert_eq!(
            data.admin_log_channel(guild).await?,
            Some(serenity::ChannelId::new(100))
        );
        data.clear_admin_log_channel(guild).await?;
        assert_eq!(data.admin_log_channel(guild).await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn stores_installed_guild_patch_notes_channels_and_delivery_markers() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let data = InstanceData::open(&directory.path().join("sillybot.db"), false).await?;
        let guild = serenity::GuildId::new(42);

        assert_eq!(data.patch_notes_channel(guild).await?, None);
        data.set_patch_notes_channel(guild, serenity::ChannelId::new(100))
            .await?;
        assert_eq!(
            data.patch_notes_subscriptions().await?,
            vec![PatchNotesSubscription {
                guild_id: guild,
                channel_id: serenity::ChannelId::new(100),
                last_article_url: None,
            }]
        );
        data.mark_patch_notes_article_seen(guild, "/patch-26-10")
            .await?;
        data.set_patch_notes_channel(guild, serenity::ChannelId::new(101))
            .await?;
        assert_eq!(
            data.patch_notes_subscriptions().await?[0]
                .last_article_url
                .as_deref(),
            Some("/patch-26-10")
        );
        data.clear_patch_notes_channel(guild).await?;
        assert_eq!(data.patch_notes_channel(guild).await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn creates_one_post_migration_snapshot_for_fresh_protected_data() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        InstanceData::open(&path, true).await?;
        InstanceData::open(&path, true).await?;
        let names = snapshot_names(directory.path())?;
        assert_eq!(names.len(), 1);
        assert!(names[0].contains("post-migration-3"));
        Ok(())
    }

    #[tokio::test]
    async fn creates_a_baseline_when_protection_is_enabled_later() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        InstanceData::open(&path, false).await?;
        InstanceData::open(&path, true).await?;
        InstanceData::open(&path, true).await?;
        let names = snapshot_names(directory.path())?;
        assert_eq!(names.len(), 1);
        assert!(names[0].contains("baseline-3"));
        Ok(())
    }

    #[tokio::test]
    async fn restores_persisted_behavior_from_a_protected_snapshot() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        let data = InstanceData::open(&path, false).await?;
        let guild = serenity::GuildId::new(42);
        assert_eq!(data.increment_counter().await?, 1);
        data.set_admin_log_channel(guild, serenity::ChannelId::new(100))
            .await?;
        drop(data);

        InstanceData::open(&path, true).await?;
        let names = snapshot_names(directory.path())?;
        let restored_path = directory.path().join("snapshots").join(&names[0]);
        let restored = InstanceData::open(&restored_path, false).await?;

        assert_eq!(restored.increment_counter().await?, 2);
        assert_eq!(
            restored.admin_log_channel(guild).await?,
            Some(serenity::ChannelId::new(100))
        );
        Ok(())
    }

    #[tokio::test]
    async fn scheduled_snapshots_capture_current_persisted_behavior() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        let data = InstanceData::open(&path, true).await?;
        assert_eq!(data.increment_counter().await?, 1);

        let scheduled_data = data.clone();
        let scheduled = tokio::spawn(async move {
            let _ = scheduled_data
                .run_snapshots_every(Duration::from_millis(10))
                .await;
        });
        let daily_path = time::timeout(Duration::from_secs(1), async {
            loop {
                let names = snapshot_names(directory.path())?;
                if let Some(name) = names.into_iter().find(|name| name.contains("daily")) {
                    return Ok::<PathBuf, anyhow::Error>(
                        directory.path().join("snapshots").join(name),
                    );
                }
                time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .context("timed out waiting for a scheduled snapshot")??;
        scheduled.abort();

        let restored = InstanceData::open(&daily_path, false).await?;
        assert_eq!(restored.increment_counter().await?, 2);
        Ok(())
    }

    #[tokio::test]
    async fn completes_a_pending_post_migration_snapshot_on_retry() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        InstanceData::open(&path, false).await?;
        let markers = directory.path().join(".snapshot-events");
        std::fs::create_dir_all(&markers)?;
        std::fs::write(markers.join("post-migration-3.pending"), b"pending")?;

        InstanceData::open(&path, true).await?;
        let names = snapshot_names(directory.path())?;
        assert_eq!(names.len(), 1);
        assert!(names[0].contains("post-migration-3"));
        assert!(!markers.join("post-migration-3.pending").exists());
        Ok(())
    }

    #[tokio::test]
    async fn snapshots_before_and_after_a_pending_migration() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        create_version_one_database(&path).await?;
        let data = InstanceData::open(&path, true).await?;
        assert_eq!(data.increment_counter().await?, 8);
        let names = snapshot_names(directory.path())?;
        assert_eq!(names.len(), 2);
        assert!(
            names
                .iter()
                .any(|name| name.contains("pre-migration-1-to-3"))
        );
        assert!(names.iter().any(|name| name.contains("post-migration-3")));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_more_than_one_global_counter_row() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        InstanceData::open(&path, false).await?;
        let database = Builder::new_local(path.to_str().unwrap()).build().await?;
        let conn = database.connect()?;
        conn.execute("INSERT INTO command_counter (value) VALUES (10);", ())
            .await?;
        drop(conn);
        let error = InstanceData::open(&path, false).await.unwrap_err();
        assert!(error.to_string().contains("expected exactly one"));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_a_database_from_a_newer_schema_version() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        InstanceData::open(&path, false).await?;
        let database = Builder::new_local(path.to_str().unwrap()).build().await?;
        let conn = database.connect()?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (4);", ())
            .await?;
        drop(conn);

        let error = InstanceData::open(&path, false).await.unwrap_err();
        assert!(error.to_string().contains("newer than supported"));
        Ok(())
    }
}
