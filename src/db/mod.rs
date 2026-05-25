pub mod admin_log;
pub mod counter;

use anyhow::{Context, Result, bail};
use tracing::info;
use turso::{Connection, Value, transaction::TransactionBehavior};

const MIGRATION_1: &str = include_str!("../../migrations/0001_counter.sql");
const MIGRATION_2: &str = include_str!("../../migrations/0002_admin_log_channel.sql");
const LATEST_SCHEMA_VERSION: i64 = 2;

pub(crate) async fn initialize(conn: &mut Connection) -> Result<()> {
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
    if applied < 1 {
        conn.set_transaction_behavior(TransactionBehavior::Immediate);
        let tx = conn
            .transaction()
            .await
            .context("failed to begin database migration transaction")?;
        tx.execute(MIGRATION_1, ())
            .await
            .context("failed to apply migration 0001_counter")?;
        tx.execute("INSERT INTO schema_migrations (version) VALUES (1);", ())
            .await
            .context("failed to record migration 0001_counter")?;
        tx.commit()
            .await
            .context("failed to commit migration 0001_counter")?;
        info!(version = 1, "applied database migration");
    }
    if applied < 2 {
        conn.set_transaction_behavior(TransactionBehavior::Immediate);
        let tx = conn
            .transaction()
            .await
            .context("failed to begin database migration transaction")?;
        tx.execute(MIGRATION_2, ())
            .await
            .context("failed to apply migration 0002_admin_log_channel")?;
        tx.execute("INSERT INTO schema_migrations (version) VALUES (2);", ())
            .await
            .context("failed to record migration 0002_admin_log_channel")?;
        tx.commit()
            .await
            .context("failed to commit migration 0002_admin_log_channel")?;
        info!(version = 2, "applied database migration");
    } else {
        info!(version = applied, "database schema is current");
    }

    ensure_counter_row(conn).await?;

    let journal_mode = single_text(conn, "PRAGMA journal_mode;")
        .await
        .context("failed to inspect database journal mode")?;
    if journal_mode.eq_ignore_ascii_case("mvcc") {
        bail!("database journal mode mvcc is experimental and unsupported");
    }
    info!(journal_mode, "active database journal mode");
    Ok(())
}

async fn ensure_counter_row(conn: &mut Connection) -> Result<()> {
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
