use crate::bot::{Context, Error};

/// Confirm that this Sillybot instance can receive interactions.
#[poise::command(slash_command, guild_only)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(ping_message()).await?;
    Ok(())
}

pub(crate) fn ping_message() -> &'static str {
    "Pong!"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirms_that_the_instance_can_receive_interactions() {
        assert_eq!(ping_message(), "Pong!");
    }
}
