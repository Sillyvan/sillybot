CREATE TABLE IF NOT EXISTS guild_self_role_menu (
    guild_id    INTEGER PRIMARY KEY,
    channel_id  INTEGER NOT NULL,
    message_id  INTEGER,
    title       TEXT NOT NULL
);
