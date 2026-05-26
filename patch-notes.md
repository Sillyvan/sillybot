# League Of Legends Patch Notes

Status: implemented
Scope: `/patch-notes set`, `/patch-notes clear`, `/patch-notes show`, and official patch-notes publication.

## Command

`/patch-notes set channel:Channel` stores an installed guild's text or
announcement channel. `/patch-notes clear` turns updates off for that guild,
and `/patch-notes show` displays the current setting. All three require
`MANAGE_GUILD`.

On first enablement, Sillybot posts the most recent article already present in
Riot's feed and does not replay older patch notes. New official patch-note
articles observed afterward are posted to the configured channel.

## Source And Publication

The worker polls Riot's public
`https://www.leagueoflegends.com/en-us/news/tags/patch-notes/` page hourly.
The page includes structured Next.js data for the official patch-note article
list; Sillybot uses each article title, publication timestamp, URL, and image
to create a Discord embed.

The public page exposes a publishing-content backend URL, but that endpoint
rejects unauthenticated requests. The feature consumes the public official
page rather than depending on a third-party feed or a private Riot credential.

Delivery state is tracked independently per installed guild. A successful post
advances only that guild's stored article cursor. A failed Discord post is
logged and retried on a later poll; it does not disable other configured
guilds.

## Persistence

Migration `migrations/0003_patch_notes_channel.sql` stores the configured
channel and latest successfully observed or delivered official article URL:

```sql
CREATE TABLE IF NOT EXISTS guild_patch_notes_channel (
    guild_id          INTEGER PRIMARY KEY,
    channel_id        INTEGER NOT NULL,
    last_article_url  TEXT
);
```
