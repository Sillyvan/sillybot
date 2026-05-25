use poise::serenity_prelude as serenity;

use crate::{
    bot::{Context, Error},
    db::InstanceData,
};

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
    ctx.send(
        poise::CreateReply::default()
            .content(set_message(&ctx.data().instance_data, guild_id, channel.id).await?)
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// Remove this guild's moderation audit channel.
#[poise::command(slash_command, guild_only)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    ctx.send(
        poise::CreateReply::default()
            .content(clear_message(&ctx.data().instance_data, guild_id).await?)
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// Show this guild's configured moderation audit channel.
#[poise::command(slash_command, guild_only)]
pub async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    ctx.send(
        poise::CreateReply::default()
            .content(show_message(&ctx.data().instance_data, guild_id).await?)
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

pub(crate) async fn set_message(
    instance_data: &InstanceData,
    guild_id: serenity::GuildId,
    channel_id: serenity::ChannelId,
) -> Result<String, Error> {
    instance_data
        .set_admin_log_channel(guild_id, channel_id)
        .await?;
    Ok(format!("Admin log channel set to <#{channel_id}>."))
}

pub(crate) async fn clear_message(
    instance_data: &InstanceData,
    guild_id: serenity::GuildId,
) -> Result<String, Error> {
    instance_data.clear_admin_log_channel(guild_id).await?;
    Ok("Admin log channel cleared.".to_owned())
}

pub(crate) async fn show_message(
    instance_data: &InstanceData,
    guild_id: serenity::GuildId,
) -> Result<String, Error> {
    Ok(match instance_data.admin_log_channel(guild_id).await? {
        Some(channel_id) => format!("Admin log channel: <#{channel_id}>."),
        None => "No admin log channel is configured.".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn configures_and_reports_an_installed_guilds_moderation_audit_channel()
    -> Result<(), Error> {
        let directory = tempfile::tempdir()?;
        let data = InstanceData::open(&directory.path().join("sillybot.db"), false).await?;
        let guild_id = serenity::GuildId::new(42);
        let channel_id = serenity::ChannelId::new(100);

        assert_eq!(
            show_message(&data, guild_id).await?,
            "No admin log channel is configured."
        );
        assert_eq!(
            set_message(&data, guild_id, channel_id).await?,
            "Admin log channel set to <#100>."
        );
        assert_eq!(
            show_message(&data, guild_id).await?,
            "Admin log channel: <#100>."
        );
        assert_eq!(
            clear_message(&data, guild_id).await?,
            "Admin log channel cleared."
        );
        assert_eq!(
            show_message(&data, guild_id).await?,
            "No admin log channel is configured."
        );
        Ok(())
    }
}
