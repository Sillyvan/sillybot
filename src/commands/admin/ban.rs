use poise::serenity_prelude as serenity;

use super::{CompletedAction, execute_action};
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
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    execute_action(
        ctx,
        &user,
        reason,
        CompletedAction {
            action: "Ban",
            verb: "ban",
            response: format!("Banned <@{}>.", user.id),
            timeout: None,
        },
        |reason| async move {
            guild_id
                .ban_with_reason(ctx.http(), user.id, 0, &reason)
                .await
        },
    )
    .await
}
