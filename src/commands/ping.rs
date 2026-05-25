use crate::bot::{Context, Error};

/// Confirm that this Sillybot instance can receive interactions.
#[poise::command(slash_command, guild_only)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Pong!").await?;
    Ok(())
}
