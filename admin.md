# Admin Features Plan

Status: proposal
Scope: `/ban`, `/kick`, `/timeout`, `/admin-log set` + admin-only audit channel.

## Authorization

Inherit native Discord permissions; no bot-specific roles.

- `/ban` requires `BAN_MEMBERS`
- `/kick` requires `KICK_MEMBERS`
- `/timeout` requires `MODERATE_MEMBERS`
- `/admin-log set` requires `MANAGE_GUILD`

Declared via Poise on each command:

```rust
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "BAN_MEMBERS",
    required_bot_permissions = "BAN_MEMBERS",
)]
```

`default_member_permissions` sets Discord-side gating (greyed out in UI). `required_bot_permissions` makes Poise reject before the handler runs if the bot itself lacks the permission. Server owners can still re-grant per-role/channel in Discord's integration settings — that's intentional.

## Commands

### `/ban user:User reason:String`

- `serenity::GuildId::ban_with_reason(http, user, dmd, reason)` — `dmd` = days of message deletion, default `0`.
- Reason passed to Discord audit log (X-Audit-Log-Reason header — serenity handles via `reason` param).
- Ephemeral success reply to invoker; full record posted in admin log channel.

### `/kick user:User reason:String`

- `Member::kick_with_reason` or `GuildId::kick_with_reason`.
- Same response/log pattern as ban.

### `/timeout user:User duration:String reason:String`

- `duration` parsed humanly (`10m`, `2h`, `1d`). Cap at Discord's max **28 days**; reject over.
- Sets `communication_disabled_until = now + duration` via `Member::disable_communication_until_datetime` or `GuildId::edit_member`.
- `duration:"0"` or `"clear"` clears the timeout.

### `/admin-log set channel:Channel`

- Persist per-guild target channel for moderation logs.
- `/admin-log clear` removes the mapping.
- `/admin-log show` reports current setting.
- All three only need `MANAGE_GUILD`.

## Admin Log Output

When a mod action succeeds and a log channel is configured for the guild, post an embed:

- Action (Ban / Kick / Timeout)
- Target (mention + id)
- Moderator (mention + id)
- Reason
- For timeout: duration + expiry timestamp
- UTC timestamp

If no log channel set, skip — action still executes. Log failures (channel deleted, missing perms) are warnings, not user-facing errors.

## Persistence

New migration `migrations/0002_admin_log_channel.sql`:

```sql
CREATE TABLE IF NOT EXISTS guild_admin_log_channel (
    guild_id   INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL
);
```

New store `src/db/admin_log.rs` (`AdminLogStore`) with `get(guild)`, `set(guild, channel)`, `clear(guild)`. Goes through same serialized connection as `CounterStore`.

`AppState` gains `admin_log_store: AdminLogStore`.

## Module Layout

```
src/commands/
  admin/
    mod.rs        register ban/kick/timeout/admin_log
    ban.rs
    kick.rs
    timeout.rs
    log_channel.rs
    duration.rs   parse "10m"/"2h"/"1d" -> chrono::Duration, capped 28d
```

`declared_commands()` in `bot.rs` extends with the four new commands.

## Intents

None added. Slash-command user args resolve via interaction payload; ban/kick/timeout are REST calls keyed by user id. `GUILD_MEMBERS` not required.

## Hierarchy & Edge Cases

- Bot must outrank target — surface Discord's error message ephemerally.
- Self-target: reject before API call.
- Target is guild owner: Discord rejects; surface message.
- Target is a bot: allowed (mods may need to remove rogue bots).
- Reason length: trim/truncate at 512 chars (Discord audit log limit).
- Don't DM the target — out of scope.

## Tests

- Duration parser: `10m`, `2h`, `1d`, rejects `>28d`, rejects garbage, `0`/`clear` => None.
- `AdminLogStore` set/get/clear round-trip on file-backed Turso.
- Migration idempotency.
- Permission attributes present on each command (analog to existing `declares_only_guild_slash_commands`).

## Unresolved Questions

- Reply visibility: ephemeral to mod, or public in invoking channel? (Default proposal: ephemeral.)
- Message-deletion-days for `/ban` — fixed `0`, optional arg, or per-guild default?
- Embed color per action, or single neutral color?
- Should `/admin-log` log its own setting changes to the configured channel?
- Need a `/unban user:String reason:String` (taking id since user has left)?
