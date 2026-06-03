use anyhow::Context as _;
use poise::serenity_prelude as serenity;
use tracing::warn;

use crate::{
    bot::{Context, Error},
    db::{InstanceData, SelfRoleMenu, SelfRoleOption},
};

const SELECT_CUSTOM_ID: &str = "self-role:select";
const CLEAR_VALUE: &str = "clear";
const MAX_CONFIGURED_OPTIONS: usize = 24;

/// Configure this guild's self-role menu.
#[poise::command(
    slash_command,
    guild_only,
    rename = "self-role",
    subcommands("create", "option_add", "option_remove", "publish", "show", "remove"),
    default_member_permissions = "MANAGE_ROLES",
    required_bot_permissions = "MANAGE_ROLES"
)]
pub async fn self_role(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Create or move this guild's self-role menu draft.
#[poise::command(slash_command, guild_only)]
pub async fn create(
    ctx: Context<'_>,
    #[description = "Channel where the role menu will be published"]
    #[channel_types("Text", "News")]
    channel: serenity::GuildChannel,
    #[description = "Heading shown on the self-role menu"] title: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    if let Some(existing) = ctx.data().instance_data.self_role_menu(guild_id).await?
        && existing.message_id.is_some()
        && existing.channel_id != channel.id
    {
        return Err(anyhow::anyhow!(
            "Remove the published self-role menu before moving it to another channel."
        )
        .into());
    }
    ctx.data()
        .instance_data
        .create_self_role_menu(guild_id, channel.id, &title)
        .await?;
    update_published_menu(ctx.http(), &ctx.data().instance_data, guild_id).await?;
    respond(
        &ctx,
        format!("Self-role menu draft set for <#{}>.", channel.id),
    )
    .await
}

/// Add or edit an assignable role on this guild's self-role menu.
#[poise::command(slash_command, guild_only, rename = "option-add")]
pub async fn option_add(
    ctx: Context<'_>,
    #[description = "Existing Discord role members may select"] role: serenity::Role,
    #[description = "Unicode emoji displayed for this choice"] emoji: String,
    #[description = "Name displayed in the menu"] label: String,
    #[description = "Short description displayed in the menu"] description: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    validate_role(&ctx, &role).await?;
    validate_option_text(&label, &description)?;
    let data = &ctx.data().instance_data;
    let menu = data
        .self_role_menu(guild_id)
        .await?
        .context("Create the self-role menu before adding options.")?;
    if menu.options.len() >= MAX_CONFIGURED_OPTIONS
        && !menu.options.iter().any(|option| option.role_id == role.id)
    {
        return Err(anyhow::anyhow!(
            "A self-role menu supports at most {MAX_CONFIGURED_OPTIONS} assignable roles."
        )
        .into());
    }
    data.add_self_role_option(guild_id, role.id, &label, &emoji, &description)
        .await?;
    update_published_menu(ctx.http(), data, guild_id).await?;
    respond(&ctx, format!("Added `{label}` to the self-role menu.")).await
}

/// Remove an assignable role from this guild's self-role menu.
#[poise::command(slash_command, guild_only, rename = "option-remove")]
pub async fn option_remove(
    ctx: Context<'_>,
    #[description = "Role to remove from the menu"] role: serenity::Role,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    ctx.data()
        .instance_data
        .remove_self_role_option(guild_id, role.id)
        .await?;
    update_published_menu(ctx.http(), &ctx.data().instance_data, guild_id).await?;
    respond(
        &ctx,
        format!("Removed `{}` from the self-role menu.", role.name),
    )
    .await
}

/// Publish this guild's configured self-role menu.
#[poise::command(slash_command, guild_only)]
pub async fn publish(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    let data = &ctx.data().instance_data;
    let menu = data
        .self_role_menu(guild_id)
        .await?
        .context("Create the self-role menu before publishing it.")?;
    if menu.options.is_empty() {
        return Err(anyhow::anyhow!(
            "Add at least one role option before publishing the self-role menu."
        )
        .into());
    }
    if menu.message_id.is_some() {
        update_published_menu(ctx.http(), data, guild_id).await?;
    } else {
        let message = menu
            .channel_id
            .send_message(ctx.http(), create_menu_message(&menu))
            .await
            .context("failed to publish self-role menu message")?;
        data.set_self_role_message(guild_id, message.id).await?;
    }
    respond(
        &ctx,
        format!("Self-role menu published in <#{}>.", menu.channel_id),
    )
    .await
}

/// Show this guild's self-role menu configuration.
#[poise::command(slash_command, guild_only)]
pub async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    let content = show_message(&ctx.data().instance_data, guild_id).await?;
    respond(&ctx, content).await
}

/// Delete this guild's self-role menu and its published message.
#[poise::command(slash_command, guild_only)]
pub async fn remove(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    let data = &ctx.data().instance_data;
    if let Some(menu) = data.self_role_menu(guild_id).await?
        && let Some(message_id) = menu.message_id
        && let Err(error) = menu.channel_id.delete_message(ctx.http(), message_id).await
    {
        warn!(%guild_id, %message_id, %error, "failed to delete published self-role menu message");
    }
    data.clear_self_role_menu(guild_id).await?;
    respond(&ctx, "Self-role menu removed.".to_owned()).await
}

async fn respond(ctx: &Context<'_>, content: String) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default()
            .content(content)
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

async fn validate_role(ctx: &Context<'_>, role: &serenity::Role) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild ID");
    if role.id.get() == guild_id.get() {
        return Err(anyhow::anyhow!(
            "The @everyone role cannot be assigned through a self-role menu."
        )
        .into());
    }
    if role.managed {
        return Err(anyhow::anyhow!(
            "Discord-managed roles cannot be assigned through a self-role menu."
        )
        .into());
    }
    let current_user = ctx.http().get_current_user().await?;
    let bot_member = guild_id.member(ctx.http(), current_user.id).await?;
    let guild_roles = guild_id.roles(ctx.http()).await?;
    let highest_bot_position = bot_member
        .roles
        .iter()
        .filter_map(|role_id| guild_roles.get(role_id))
        .map(|role| role.position)
        .max()
        .unwrap_or(0);
    if role.position >= highest_bot_position {
        return Err(anyhow::anyhow!(
            "Move Sillybot's role above `{}` before adding it.",
            role.name
        )
        .into());
    }
    Ok(())
}

fn validate_option_text(label: &str, description: &str) -> Result<(), Error> {
    if label.chars().count() > 100 {
        return Err(
            anyhow::anyhow!("Self-role option names must be 100 characters or fewer.").into(),
        );
    }
    if description.chars().count() > 100 {
        return Err(anyhow::anyhow!(
            "Self-role option descriptions must be 100 characters or fewer."
        )
        .into());
    }
    Ok(())
}

fn create_menu_message(menu: &SelfRoleMenu) -> serenity::CreateMessage {
    serenity::CreateMessage::new()
        .content(format!(
            "**{}**\nChoose any roles you want. Submit every role from this menu that you want to keep.",
            menu.title
        ))
        .components(menu_components(menu))
}

fn edit_menu_message(menu: &SelfRoleMenu) -> serenity::EditMessage {
    serenity::EditMessage::new()
        .content(format!(
            "**{}**\nChoose any roles you want. Submit every role from this menu that you want to keep.",
            menu.title
        ))
        .components(menu_components(menu))
}

fn menu_components(menu: &SelfRoleMenu) -> Vec<serenity::CreateActionRow> {
    let mut options = menu
        .options
        .iter()
        .map(|option| {
            serenity::CreateSelectMenuOption::new(&option.label, option.role_id.to_string())
                .description(&option.description)
                .emoji(serenity::ReactionType::Unicode(option.emoji.clone()))
        })
        .collect::<Vec<_>>();
    options.push(
        serenity::CreateSelectMenuOption::new("Remove my roles", CLEAR_VALUE)
            .description("Remove your selected roles from this menu")
            .emoji(serenity::ReactionType::Unicode("🚫".to_owned())),
    );
    vec![serenity::CreateActionRow::SelectMenu(
        serenity::CreateSelectMenu::new(
            SELECT_CUSTOM_ID,
            serenity::CreateSelectMenuKind::String { options },
        )
        .placeholder("Choose your roles...")
        .min_values(1)
        .max_values(menu.options.len().max(1) as u8),
    )]
}

async fn update_published_menu(
    http: &serenity::Http,
    data: &InstanceData,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let Some(menu) = data.self_role_menu(guild_id).await? else {
        return Ok(());
    };
    let Some(message_id) = menu.message_id else {
        return Ok(());
    };
    menu.channel_id
        .edit_message(http, message_id, edit_menu_message(&menu))
        .await
        .context("failed to update published self-role menu message")?;
    Ok(())
}

pub(crate) async fn show_message(
    data: &InstanceData,
    guild_id: serenity::GuildId,
) -> Result<String, Error> {
    Ok(match data.self_role_menu(guild_id).await? {
        None => "No self-role menu is configured.".to_owned(),
        Some(menu) => format!(
            "Self-role menu in <#{}>: `{}` with {} option(s){}.",
            menu.channel_id,
            menu.title,
            menu.options.len(),
            if menu.message_id.is_some() {
                ", published"
            } else {
                ", not published"
            }
        ),
    })
}

fn selected_options<'a>(
    menu: &'a SelfRoleMenu,
    values: &[String],
) -> Result<Vec<&'a SelfRoleOption>, Error> {
    values
        .iter()
        .map(|value| {
            menu.options
                .iter()
                .find(|option| option.role_id.to_string() == *value)
                .context("The selected self-role option no longer exists.")
                .map_err(Into::into)
        })
        .collect()
}

fn role_changes(
    menu: &SelfRoleMenu,
    current_roles: &[serenity::RoleId],
    selected: &[serenity::RoleId],
) -> (Vec<serenity::RoleId>, Vec<serenity::RoleId>) {
    let to_add = selected
        .iter()
        .copied()
        .filter(|role_id| !current_roles.contains(role_id))
        .collect();
    let to_remove = menu
        .options
        .iter()
        .map(|option| option.role_id)
        .filter(|role_id| current_roles.contains(role_id) && !selected.contains(role_id))
        .collect();
    (to_add, to_remove)
}

pub(crate) async fn handle_component(
    ctx: &serenity::Context,
    interaction: &serenity::ComponentInteraction,
    data: &InstanceData,
) -> Result<(), Error> {
    if interaction.data.custom_id != SELECT_CUSTOM_ID {
        return Ok(());
    }
    let Some(guild_id) = interaction.guild_id else {
        return Ok(());
    };
    let Some(menu) = data.self_role_menu(guild_id).await? else {
        return Ok(());
    };
    if menu.message_id != Some(interaction.message.id) {
        return Ok(());
    }
    let serenity::ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind
    else {
        return Ok(());
    };
    if values.is_empty() {
        return Err(anyhow::anyhow!("self-role menu selection is empty").into());
    }
    let selected = if values.iter().any(|value| value == CLEAR_VALUE) {
        Vec::new()
    } else {
        selected_options(&menu, values)?
    };
    let member = interaction
        .member
        .as_ref()
        .context("Self-role selections are only available inside a guild.")?;

    interaction
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Defer(
                serenity::CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;
    let selected_role_ids = selected
        .iter()
        .map(|option| option.role_id)
        .collect::<Vec<_>>();
    let (to_add, to_remove) = role_changes(&menu, &member.roles, &selected_role_ids);
    if !selected.is_empty() {
        let roles = guild_id.roles(&ctx.http).await?;
        if selected
            .iter()
            .any(|option| !roles.contains_key(&option.role_id))
        {
            interaction
                .edit_response(
                    ctx,
                    serenity::EditInteractionResponse::new().content(
                        "That role no longer exists. Ask an administrator to update this menu.",
                    ),
                )
                .await?;
            return Ok(());
        }
        for role_id in to_add {
            member.add_role(&ctx.http, role_id).await?;
        }
    }
    for role_id in to_remove {
        member.remove_role(&ctx.http, role_id).await?;
    }
    let response = if selected.is_empty() {
        "Your self-roles have been removed.".to_owned()
    } else {
        let selected_labels = selected
            .iter()
            .map(|option| format!("{} {}", option.emoji, option.label))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Your self-roles are now {selected_labels}.")
    };
    interaction
        .edit_response(
            ctx,
            serenity::EditInteractionResponse::new().content(response),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_a_configured_unpublished_self_role_menu() -> Result<(), Error> {
        let directory = tempfile::tempdir()?;
        let data = InstanceData::open(&directory.path().join("sillybot.db"), false).await?;
        let guild_id = serenity::GuildId::new(42);
        data.create_self_role_menu(guild_id, serenity::ChannelId::new(100), "Creatures")
            .await?;
        data.add_self_role_option(
            guild_id,
            serenity::RoleId::new(200),
            "Frog",
            "🐸",
            "Just froggin.",
        )
        .await?;

        assert_eq!(
            show_message(&data, guild_id).await?,
            "Self-role menu in <#100>: `Creatures` with 1 option(s), not published."
        );
        Ok(())
    }

    #[test]
    fn resolves_only_configured_role_values_from_a_selection() {
        let menu = SelfRoleMenu {
            guild_id: serenity::GuildId::new(42),
            channel_id: serenity::ChannelId::new(100),
            message_id: None,
            title: "Creatures".to_owned(),
            options: vec![SelfRoleOption {
                role_id: serenity::RoleId::new(200),
                label: "Frog".to_owned(),
                emoji: "🐸".to_owned(),
                description: "Just froggin.".to_owned(),
            }],
        };
        assert_eq!(
            selected_options(&menu, &["200".to_owned()])
                .unwrap()
                .iter()
                .map(|option| option.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Frog"]
        );
        assert!(selected_options(&menu, &["999".to_owned()]).is_err());
    }

    #[test]
    fn selected_menu_roles_are_applied_as_the_member_desired_set() {
        let menu = SelfRoleMenu {
            guild_id: serenity::GuildId::new(42),
            channel_id: serenity::ChannelId::new(100),
            message_id: None,
            title: "Creatures".to_owned(),
            options: vec![
                SelfRoleOption {
                    role_id: serenity::RoleId::new(200),
                    label: "Fairy".to_owned(),
                    emoji: "🧚".to_owned(),
                    description: String::new(),
                },
                SelfRoleOption {
                    role_id: serenity::RoleId::new(201),
                    label: "Frog".to_owned(),
                    emoji: "🐸".to_owned(),
                    description: String::new(),
                },
            ],
        };
        let unrelated = serenity::RoleId::new(999);
        assert_eq!(
            role_changes(
                &menu,
                &[serenity::RoleId::new(200), unrelated],
                &[serenity::RoleId::new(200), serenity::RoleId::new(201)]
            ),
            (
                vec![serenity::RoleId::new(201)],
                Vec::<serenity::RoleId>::new()
            )
        );
        assert_eq!(
            role_changes(&menu, &[serenity::RoleId::new(201), unrelated], &[]),
            (
                Vec::<serenity::RoleId>::new(),
                vec![serenity::RoleId::new(201)]
            )
        );
    }

    #[test]
    fn rejects_menu_option_text_that_discord_cannot_render() {
        assert!(validate_option_text("Frog", "Just froggin.").is_ok());
        assert!(validate_option_text(&"a".repeat(101), "description").is_err());
        assert!(validate_option_text("Frog", &"a".repeat(101)).is_err());
    }
}
