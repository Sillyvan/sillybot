use crate::bot::{Context, Error};

/// Increment and display this Sillybot instance's durable global counter.
#[poise::command(slash_command, guild_only)]
pub async fn count(ctx: Context<'_>) -> Result<(), Error> {
    let value = ctx.data().instance_data.increment_counter().await?;
    ctx.say(format!("Count: {value}")).await?;
    Ok(())
}
