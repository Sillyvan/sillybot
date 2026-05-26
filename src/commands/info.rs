use std::time::{Duration, Instant};

use crate::bot::{Context, Error};

/// Display this Sillybot instance's release and runtime information.
#[poise::command(slash_command, guild_only)]
pub async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let started = Instant::now();
    let response = ctx.say("Measuring command latency...").await?;
    response
        .edit(
            ctx,
            poise::CreateReply::default().content(info_message(
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH,
                started.elapsed(),
            )),
        )
        .await?;
    Ok(())
}

pub(crate) fn info_message(
    version: &str,
    operating_system: &str,
    architecture: &str,
    latency: Duration,
) -> String {
    format!(
        "Sillybot v{version}\nRuntime: {operating_system}/{architecture}\nCommand latency: {} ms",
        latency.as_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_release_runtime_and_command_latency() {
        assert_eq!(
            info_message("1.2.3", "linux", "x86_64", Duration::from_millis(17)),
            "Sillybot v1.2.3\nRuntime: linux/x86_64\nCommand latency: 17 ms"
        );
    }
}
