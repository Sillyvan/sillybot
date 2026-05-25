use std::sync::Arc;

use anyhow::{Context, Result, bail};
use poise::serenity_prelude as serenity;
use tokio::sync::Mutex;
use turso::{Connection, Value};

#[derive(Clone, Debug)]
pub struct AdminLogStore {
    conn: Arc<Mutex<Connection>>,
}

impl AdminLogStore {
    pub(super) fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    pub async fn get(&self, guild_id: serenity::GuildId) -> Result<Option<serenity::ChannelId>> {
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

    pub async fn set(
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

    pub async fn clear(&self, guild_id: serenity::GuildId) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM guild_admin_log_channel WHERE guild_id = ?1;",
            (snowflake_to_integer(guild_id.get())?,),
        )
        .await
        .context("failed to clear admin log channel")?;
        Ok(())
    }
}

fn snowflake_to_integer(value: u64) -> Result<i64> {
    i64::try_from(value).context("Discord ID exceeds the database integer range")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::counter::CounterStore;

    #[tokio::test]
    async fn sets_gets_replaces_and_clears_per_guild_channel() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sillybot.db");
        let counter_store = CounterStore::open(&path).await?;
        let store = counter_store.admin_log_store();
        let guild = serenity::GuildId::new(42);

        assert_eq!(store.get(guild).await?, None);
        store.set(guild, serenity::ChannelId::new(100)).await?;
        assert_eq!(store.get(guild).await?, Some(serenity::ChannelId::new(100)));
        store.set(guild, serenity::ChannelId::new(101)).await?;
        assert_eq!(store.get(guild).await?, Some(serenity::ChannelId::new(101)));
        store.clear(guild).await?;
        assert_eq!(store.get(guild).await?, None);
        Ok(())
    }
}
