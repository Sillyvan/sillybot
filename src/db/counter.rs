use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use tokio::sync::Mutex;
use turso::{Builder, Connection, Value, transaction::TransactionBehavior};

use super::initialize;

#[derive(Clone, Debug)]
pub struct CounterStore {
    conn: Arc<Mutex<Connection>>,
}

impl CounterStore {
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create database directory {}", parent.display())
            })?;
        }
        let path = path
            .to_str()
            .context("DATABASE_PATH is not valid UTF-8 for Turso")?;
        let database = Builder::new_local(path)
            .build()
            .await
            .context("failed to open Turso database")?;
        let mut conn = database
            .connect()
            .context("failed to connect to Turso database")?;
        initialize(&mut conn).await?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn increment(&self) -> Result<i64> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn increments_sequentially_and_persists_after_reopen() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");

        {
            let store = CounterStore::open(&path).await?;
            assert_eq!(store.increment().await?, 1);
            assert_eq!(store.increment().await?, 2);
        }

        let reopened = CounterStore::open(&path).await?;
        assert_eq!(reopened.increment().await?, 3);
        Ok(())
    }

    #[tokio::test]
    async fn migration_is_idempotent() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        CounterStore::open(&path).await?;
        CounterStore::open(&path).await?;
        Ok(())
    }

    #[tokio::test]
    async fn repairs_an_empty_counter_table_in_an_existing_schema() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        CounterStore::open(&path).await?;

        let database = Builder::new_local(path.to_str().unwrap()).build().await?;
        let conn = database.connect()?;
        conn.execute("DELETE FROM command_counter;", ()).await?;
        drop(conn);
        drop(database);

        let reopened = CounterStore::open(&path).await?;
        assert_eq!(reopened.increment().await?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_more_than_one_global_counter_row() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        CounterStore::open(&path).await?;

        let database = Builder::new_local(path.to_str().unwrap()).build().await?;
        let conn = database.connect()?;
        conn.execute("INSERT INTO command_counter (value) VALUES (10);", ())
            .await?;
        drop(conn);
        drop(database);

        let error = CounterStore::open(&path).await.unwrap_err();
        assert!(error.to_string().contains("expected exactly one"));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_a_database_from_a_newer_schema_version() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        CounterStore::open(&path).await?;

        let database = Builder::new_local(path.to_str().unwrap()).build().await?;
        let conn = database.connect()?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (2);", ())
            .await?;
        drop(conn);
        drop(database);

        let error = CounterStore::open(&path).await.unwrap_err();
        assert!(error.to_string().contains("newer than supported"));
        Ok(())
    }
}
