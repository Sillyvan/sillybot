use poise::serenity_prelude as serenity;

use crate::bot::{Context, Error};

/// Ban a user from this guild.
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "BAN_MEMBERS",
    required_bot_permissions = "BAN_MEMBERS"
)]
pub async fn ban(
    ctx: Context<'_>,
    #[description = "User to ban"] user: serenity::User,
    #[description = "Reason recorded in the audit log"] reason: String,
) -> Result<(), Error> {
    super::execute_ban(ctx, user, reason).await
}
