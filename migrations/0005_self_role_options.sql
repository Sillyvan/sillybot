CREATE TABLE IF NOT EXISTS guild_self_role_option (
    guild_id       INTEGER NOT NULL,
    role_id        INTEGER NOT NULL,
    label          TEXT NOT NULL,
    emoji          TEXT NOT NULL,
    description    TEXT NOT NULL,
    display_order  INTEGER NOT NULL,
    PRIMARY KEY (guild_id, role_id)
);
