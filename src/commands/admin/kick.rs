use poise::serenity_prelude as serenity;

use crate::bot::{Context, Error};

/// Remove a user from this guild.
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "KICK_MEMBERS",
    required_bot_permissions = "KICK_MEMBERS"
)]
pub async fn kick(
    ctx: Context<'_>,
    #[description = "User to kick"] user: serenity::User,
    #[description = "Reason recorded in the audit log"] reason: String,
) -> Result<(), Error> {
    super::execute_kick(ctx, user, reason).await
}
