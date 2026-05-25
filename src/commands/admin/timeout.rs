use poise::serenity_prelude as serenity;

use crate::bot::{Context, Error};

/// Apply or clear a Discord communication timeout for a user.
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "MODERATE_MEMBERS",
    required_bot_permissions = "MODERATE_MEMBERS"
)]
pub async fn timeout(
    ctx: Context<'_>,
    #[description = "User to timeout"] user: serenity::User,
    #[description = "Duration such as 10m, 2h, 1d; use clear to remove"] duration: String,
    #[description = "Reason recorded in the audit log"] reason: String,
) -> Result<(), Error> {
    super::execute_timeout(ctx, user, duration, reason).await
}
