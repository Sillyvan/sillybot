CREATE TABLE IF NOT EXISTS guild_patch_notes_channel (
    guild_id          INTEGER PRIMARY KEY,
    channel_id        INTEGER NOT NULL,
    last_article_url  TEXT
);
