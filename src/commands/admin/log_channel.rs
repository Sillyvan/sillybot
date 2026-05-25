use poise::serenity_prelude as serenity;

use crate::bot::{Context, Error};

/// Configure the channel receiving moderation audit records.
#[poise::command(
    slash_command,
    guild_only,
    rename = "admin-log",
    subcommands("set", "clear", "show"),
    default_member_permissions = "MANAGE_GUILD",
    required_bot_permissions = "MANAGE_GUILD"
)]
pub async fn admin_log(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Set this guild's moderation audit channel.
#[poise::command(slash_command, guild_only)]
pub async fn set(
    ctx: Context<'_>,
    #[description = "Text channel for moderation audit records"]
    #[channel_types("Text", "News")]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    ctx.data()
        .instance_data
        .set_admin_log_channel(guild_id, channel.id)
        .await?;
    ctx.send(
        poise::CreateReply::default()
            .content(format!("Admin log channel set to <#{}>.", channel.id))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// Remove this guild's moderation audit channel.
#[poise::command(slash_command, guild_only)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    ctx.data()
        .instance_data
        .clear_admin_log_channel(guild_id)
        .await?;
    ctx.send(
        poise::CreateReply::default()
            .content("Admin log channel cleared.")
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// Show this guild's configured moderation audit channel.
#[poise::command(slash_command, guild_only)]
pub async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    let content = match ctx.data().instance_data.admin_log_channel(guild_id).await? {
        Some(channel_id) => format!("Admin log channel: <#{channel_id}>."),
        None => "No admin log channel is configured.".to_owned(),
    };
    ctx.send(
        poise::CreateReply::default()
            .content(content)
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
