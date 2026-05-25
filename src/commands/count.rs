use crate::{
    bot::{Context, Error},
    db::InstanceData,
};

/// Increment and display this Sillybot instance's durable global counter.
#[poise::command(slash_command, guild_only)]
pub async fn count(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(count_message(&ctx.data().instance_data).await?)
        .await?;
    Ok(())
}

pub(crate) async fn count_message(instance_data: &InstanceData) -> Result<String, Error> {
    let value = instance_data.increment_counter().await?;
    Ok(format!("Count: {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn visibly_returns_each_new_instance_global_counter_value() -> Result<(), Error> {
        let directory = tempfile::tempdir()?;
        let data = InstanceData::open(&directory.path().join("sillybot.db"), false).await?;

        assert_eq!(count_message(&data).await?, "Count: 1");
        assert_eq!(count_message(&data).await?, "Count: 2");
        Ok(())
    }
}
