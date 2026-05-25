use poise::serenity_prelude as serenity;

use super::{CompletedAction, execute_action};
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
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    execute_action(
        ctx,
        &user,
        reason,
        CompletedAction {
            action: "Kick",
            verb: "kick",
            response: format!("Kicked <@{}>.", user.id),
            timeout: None,
        },
        |reason| async move {
            guild_id
                .kick_with_reason(ctx.http(), user.id, &reason)
                .await
        },
    )
    .await
}
