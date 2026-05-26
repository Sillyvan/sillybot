use std::time::Duration;

use anyhow::{Context as _, Result as AnyResult};
use poise::serenity_prelude as serenity;
use serde_json::Value;
use tokio::time;
use tracing::{info, warn};

use crate::{
    bot::{Context, Error},
    db::{InstanceData, PatchNotesSubscription},
};

const PATCH_NOTES_URL: &str = "https://www.leagueoflegends.com/en-us/news/tags/patch-notes/";
const PATCH_NOTES_POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Configure League of Legends patch notes updates for this guild.
#[poise::command(
    slash_command,
    guild_only,
    rename = "patch-notes",
    subcommands("set", "clear", "show"),
    default_member_permissions = "MANAGE_GUILD"
)]
pub async fn patch_notes(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Set the channel receiving future League of Legends patch notes.
#[poise::command(slash_command, guild_only)]
pub async fn set(
    ctx: Context<'_>,
    #[description = "Text channel for League of Legends patch notes"]
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

/// Stop posting League of Legends patch notes in this guild.
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

/// Show the channel receiving League of Legends patch notes.
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
        .set_patch_notes_channel(guild_id, channel_id)
        .await?;
    Ok(format!(
        "League of Legends patch notes channel set to <#{channel_id}>. The latest official notes will be posted on the next update check."
    ))
}

pub(crate) async fn clear_message(
    instance_data: &InstanceData,
    guild_id: serenity::GuildId,
) -> Result<String, Error> {
    instance_data.clear_patch_notes_channel(guild_id).await?;
    Ok("League of Legends patch notes updates are off.".to_owned())
}

pub(crate) async fn show_message(
    instance_data: &InstanceData,
    guild_id: serenity::GuildId,
) -> Result<String, Error> {
    Ok(match instance_data.patch_notes_channel(guild_id).await? {
        Some(channel_id) => format!("League of Legends patch notes channel: <#{channel_id}>."),
        None => "League of Legends patch notes updates are off.".to_owned(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PatchNotesArticle {
    title: String,
    published_at: String,
    url: String,
    image_url: Option<String>,
}

trait ArticlePublisher {
    async fn publish(
        &self,
        subscription: &PatchNotesSubscription,
        article: &PatchNotesArticle,
    ) -> AnyResult<()>;
}

struct DiscordArticlePublisher<'a> {
    http: &'a serenity::Http,
}

impl ArticlePublisher for DiscordArticlePublisher<'_> {
    async fn publish(
        &self,
        subscription: &PatchNotesSubscription,
        article: &PatchNotesArticle,
    ) -> AnyResult<()> {
        let mut embed = serenity::CreateEmbed::new()
            .title(&article.title)
            .url(&article.url)
            .description("New official League of Legends patch notes are available.")
            .footer(serenity::CreateEmbedFooter::new("League of Legends"));
        if let Ok(timestamp) = serenity::Timestamp::parse(&article.published_at) {
            embed = embed.timestamp(timestamp);
        }
        if let Some(image_url) = &article.image_url {
            embed = embed.image(image_url);
        }
        subscription
            .channel_id
            .send_message(self.http, serenity::CreateMessage::new().embed(embed))
            .await
            .with_context(|| {
                format!(
                    "failed to post patch notes in channel {}",
                    subscription.channel_id
                )
            })?;
        Ok(())
    }
}

pub(crate) async fn run_updates(
    http: &serenity::Http,
    instance_data: InstanceData,
) -> AnyResult<()> {
    let publisher = DiscordArticlePublisher { http };
    let mut interval = time::interval(PATCH_NOTES_POLL_INTERVAL);
    loop {
        interval.tick().await;
        if let Err(error) = poll_once(&instance_data, &publisher).await {
            warn!(%error, "failed to check League of Legends patch notes");
        }
    }
}

async fn poll_once<P: ArticlePublisher>(
    instance_data: &InstanceData,
    publisher: &P,
) -> AnyResult<()> {
    if instance_data.patch_notes_subscriptions().await?.is_empty() {
        return Ok(());
    }
    let articles = fetch_articles().await?;
    publish_available_updates(instance_data, &articles, publisher).await
}

async fn fetch_articles() -> AnyResult<Vec<PatchNotesArticle>> {
    let html = reqwest::get(PATCH_NOTES_URL)
        .await
        .context("failed to request official League of Legends patch notes page")?
        .error_for_status()
        .context("official League of Legends patch notes page returned an error")?
        .text()
        .await
        .context("failed to read official League of Legends patch notes page")?;
    parse_articles(&html)
}

fn parse_articles(html: &str) -> AnyResult<Vec<PatchNotesArticle>> {
    let script_id = html
        .find("id=\"__NEXT_DATA__\"")
        .context("official patch notes page did not contain Next.js data")?;
    let json_start = html[script_id..]
        .find('>')
        .map(|offset| script_id + offset + 1)
        .context("official patch notes page had malformed Next.js data")?;
    let json_end = html[json_start..]
        .find("</script>")
        .map(|offset| json_start + offset)
        .context("official patch notes page had unterminated Next.js data")?;
    let data: Value = serde_json::from_str(&html[json_start..json_end])
        .context("failed to decode official patch notes page data")?;
    let items = data["props"]["pageProps"]["page"]["blades"]
        .as_array()
        .and_then(|blades| {
            blades
                .iter()
                .find_map(|blade| blade.get("items").and_then(Value::as_array))
        })
        .context("official patch notes page did not contain articles")?;
    items
        .iter()
        .map(|item| {
            let field = |name: &str| {
                item.get(name)
                    .and_then(Value::as_str)
                    .with_context(|| format!("patch notes article had no {name}"))
            };
            let path = item["action"]["payload"]["url"]
                .as_str()
                .context("patch notes article had no URL")?;
            Ok(PatchNotesArticle {
                title: field("title")?.to_owned(),
                published_at: field("publishedAt")?.to_owned(),
                url: format!("https://www.leagueoflegends.com{path}"),
                image_url: item["media"]["url"].as_str().map(ToOwned::to_owned),
            })
        })
        .collect()
}

async fn publish_available_updates<P: ArticlePublisher>(
    instance_data: &InstanceData,
    articles: &[PatchNotesArticle],
    publisher: &P,
) -> AnyResult<()> {
    let Some(latest) = articles.first() else {
        return Ok(());
    };
    for subscription in instance_data.patch_notes_subscriptions().await? {
        let Some(last_article_url) = &subscription.last_article_url else {
            if let Err(error) = publisher.publish(&subscription, latest).await {
                warn!(
                    guild_id = subscription.guild_id.get(),
                    channel_id = subscription.channel_id.get(),
                    %error,
                    "failed to publish initial League of Legends patch notes update"
                );
                continue;
            }
            instance_data
                .mark_patch_notes_article_seen(subscription.guild_id, &latest.url)
                .await?;
            continue;
        };
        let Some(previous_index) = articles
            .iter()
            .position(|article| &article.url == last_article_url)
        else {
            warn!(
                guild_id = subscription.guild_id.get(),
                "stored patch notes cursor is absent from Riot's feed; resetting to latest article"
            );
            instance_data
                .mark_patch_notes_article_seen(subscription.guild_id, &latest.url)
                .await?;
            continue;
        };
        for article in articles[..previous_index].iter().rev() {
            if let Err(error) = publisher.publish(&subscription, article).await {
                warn!(
                    guild_id = subscription.guild_id.get(),
                    channel_id = subscription.channel_id.get(),
                    %error,
                    "failed to publish League of Legends patch notes update"
                );
                break;
            }
            instance_data
                .mark_patch_notes_article_seen(subscription.guild_id, &article.url)
                .await?;
            info!(
                guild_id = subscription.guild_id.get(),
                channel_id = subscription.channel_id.get(),
                title = %article.title,
                "published League of Legends patch notes update"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[tokio::test]
    async fn configures_and_disables_an_installed_guilds_patch_notes_channel() -> Result<(), Error>
    {
        let directory = tempfile::tempdir()?;
        let data = InstanceData::open(&directory.path().join("sillybot.db"), false).await?;
        let guild_id = serenity::GuildId::new(42);
        let channel_id = serenity::ChannelId::new(100);

        assert_eq!(
            show_message(&data, guild_id).await?,
            "League of Legends patch notes updates are off."
        );
        assert_eq!(
            set_message(&data, guild_id, channel_id).await?,
            "League of Legends patch notes channel set to <#100>. The latest official notes will be posted on the next update check."
        );
        assert_eq!(
            show_message(&data, guild_id).await?,
            "League of Legends patch notes channel: <#100>."
        );
        assert_eq!(
            clear_message(&data, guild_id).await?,
            "League of Legends patch notes updates are off."
        );
        Ok(())
    }

    struct RecordingPublisher {
        titles: Mutex<Vec<String>>,
    }

    impl ArticlePublisher for RecordingPublisher {
        async fn publish(
            &self,
            _subscription: &PatchNotesSubscription,
            article: &PatchNotesArticle,
        ) -> AnyResult<()> {
            self.titles.lock().unwrap().push(article.title.clone());
            Ok(())
        }
    }

    fn article(title: &str) -> PatchNotesArticle {
        PatchNotesArticle {
            title: title.to_owned(),
            published_at: "2026-05-12T18:00:00.000Z".to_owned(),
            url: format!("https://www.leagueoflegends.com/{title}"),
            image_url: None,
        }
    }

    #[tokio::test]
    async fn enabling_updates_posts_the_latest_patch_and_then_future_patches() -> AnyResult<()> {
        let directory = tempfile::tempdir()?;
        let data = InstanceData::open(&directory.path().join("sillybot.db"), false).await?;
        let guild = serenity::GuildId::new(42);
        data.set_patch_notes_channel(guild, serenity::ChannelId::new(100))
            .await?;
        let publisher = RecordingPublisher {
            titles: Mutex::new(Vec::new()),
        };

        publish_available_updates(&data, &[article("26.10")], &publisher).await?;
        assert_eq!(*publisher.titles.lock().unwrap(), vec!["26.10"]);

        publish_available_updates(&data, &[article("26.11"), article("26.10")], &publisher).await?;
        assert_eq!(*publisher.titles.lock().unwrap(), vec!["26.10", "26.11"]);
        Ok(())
    }

    #[test]
    fn extracts_articles_from_riots_public_page_data() -> AnyResult<()> {
        let html = r#"<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"page":{"blades":[{"items":[{"title":"Patch 26.10 Notes","publishedAt":"2026-05-12T18:00:00.000Z","action":{"payload":{"url":"/en-us/news/game-updates/patch-26-10-notes"}},"media":{"url":"https://image.test/patch.jpg"}}]}]}}}}</script>"#;

        assert_eq!(
            parse_articles(html)?,
            vec![PatchNotesArticle {
                title: "Patch 26.10 Notes".to_owned(),
                published_at: "2026-05-12T18:00:00.000Z".to_owned(),
                url: "https://www.leagueoflegends.com/en-us/news/game-updates/patch-26-10-notes"
                    .to_owned(),
                image_url: Some("https://image.test/patch.jpg".to_owned()),
            }]
        );
        Ok(())
    }
}
