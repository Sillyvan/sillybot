# Sillybot Architecture

Status: implemented baseline with current extensions
Last researched: 2026-05-25  
Scope: self-hosted Discord bot with counting and installed-guild moderation commands

## Goals

Sillybot is a small self-hosted Discord bot written in Rust. Each self-hoster operates a separate Sillybot instance. Its first release has two purposes:

1. Prove that Discord interaction handling works with `/ping`.
2. Prove that durable database writes survive process and container restarts with `/count`.

The implementation now also supports moderation commands and an installed guild's optional moderation audit channel. The design should remain small, observable, easy to deploy on one host, and easy to extend without selecting infrastructure for hypothetical scale.

## Constraints and Assumptions

- Each instance operator decides which Discord guilds may use that instance by controlling its Discord installation; no runtime guild allowlist is required initially.
- One Discord application identity and bot token belongs to one running Sillybot instance; do not operate independent data directories under the same token.
- Request volume is low; correctness and operational simplicity matter more than maximum throughput.
- The reference production host is Linux and can run Docker Engine with the Docker Compose plugin.
- Persistent data must remain on the server across container replacements.
- Turso Database is the desired database technology, despite its beta status.
- The source project and its container image will be publicly consumable by other self-hosters.
- Cloudflare R2 is available for private database backup storage.

## Decision Summary

| Concern | Initial decision | Reason |
| --- | --- | --- |
| Discord library | `poise` on top of `serenity` 0.12.x | Serenity recommends Poise for command bots; Serenity's `standard_framework` is deprecated. |
| Command type | Guild-context Discord application slash commands only | Native Discord UX, no privileged `MESSAGE_CONTENT` intent, and alignment with installed guild usage. |
| Discord transport | Gateway connection managed by Serenity | Fits a continuously running bot and does not require a public HTTP endpoint. |
| Async runtime | Tokio | Serenity and Turso expose asynchronous APIs; Serenity's example setup uses Tokio. |
| Database | Embedded `turso` Rust crate | The linked Turso Database is in-process, so a separate database service would add no value. |
| Turso journal mode | Default WAL mode | Suitable for the small workload and documented as the default; MVCC is experimental and unsuitable here. |
| Backups | Operator-managed encrypted backups to private Cloudflare R2 storage | The database is beta; relied-upon durable data requires tested off-host backup and restore. |
| Command registration | Synchronize global application commands on startup | An instance is not bound to one guild, and the running version must not expose stale commands. |
| Deployment | One Docker Compose-managed container with a protected host data directory bind mount | Common self-hosted lifecycle management plus an operator-visible persistence and restore boundary. |
| Container distribution | Public multi-platform GitHub Container Registry (`ghcr.io`) package for `linux/amd64` and `linux/arm64` | Supports typical servers and ARM64 home hosts while retaining a standard OCI pull workflow. |
| Zero downtime | Not an initial requirement | Running duplicate bot processes against one embedded database creates more problems than the brief restart solves. |

## Important Turso Qualification

The linked repository, [tursodatabase/turso](https://github.com/tursodatabase/turso), is **Turso Database**, a Rust rewrite of a SQLite-compatible in-process database.

As of 2026-05-25:

- Turso Database identifies itself as beta and warns users to use caution with production data and maintain backups.
- Its README states that Turso Database is not yet production-ready.
- Turso Database release `0.6.1` was published on 2026-05-22.
- Its documentation describes significant beta/experimental surfaces, including MVCC and multi-process behavior; they are not needed for the initial bot.
- Its `v0.6.1` SQL reference documents `VACUUM INTO` as available for producing a consistent standalone backup copy from an active database, while its C compatibility table shows that `sqlite3_backup_*` is not implemented.

The Rust binding exposes cross-task connection use and includes an MVCC concurrent-write example, but MVCC remains experimental. The initial bot serializes ordinary database access; its backup flow relies only on the `v0.6.1` documented guarantee that `VACUUM INTO` takes a consistent snapshot while other connections may write.

Turso Database is the database choice for this project. Its beta status is managed operationally: keep the database workload simple, avoid experimental database modes, create consistent backup snapshots with `VACUUM INTO`, and require instance operators to maintain tested encrypted copies in Cloudflare R2 before they rely on data that cannot be recreated. The bot process does not enforce backup readiness at startup.

## System Shape

A protected instance configuration contains one application container, one operator-selected host data directory mounted into the container, and an off-host backup destination. An evaluation instance may use a disposable Docker volume and omit the snapshot/export path while its data is disposable:

```text
Discord API
   |  Gateway WebSocket + REST interaction responses
   v
+-----------------------------+
| sillybot container           |
|                              |
| Poise / Serenity             |
| Tokio runtime                |
| command handlers             |
| InstanceData                 |
| Turso embedded database      |
+---------------+-------------+
                |
                | /var/lib/sillybot
                v
        bind-mounted host data directory
        sillybot.db + WAL files
                |
                | live VACUUM INTO snapshots,
                | encrypted off-host upload
                v
        private Cloudflare R2 bucket
```

There is no Turso container and no exposed TCP database port. Database code runs in the bot process and opens a file on the persistent mounted directory.

## Discord Integration

### API Model

Use Discord application commands, beginning with:

| Command | Behavior | Database use |
| --- | --- | --- |
| `/ping` | Respond with `Pong!` | None |
| `/count` | Increment and visibly return the Sillybot instance's durable global counter in the invoking channel | Read/write Turso transaction |
| `/ban`, `/kick`, `/timeout` | Apply a Discord moderation action for an authorized member of an installed guild, then optionally record it in that guild's moderation audit channel | Read configured audit channel |
| `/admin-log set`, `/admin-log clear`, `/admin-log show` | Configure or inspect an installed guild's moderation audit channel | Read/write Turso transaction |

Application commands are preferable to prefix commands such as `!ping`:

- Discord describes application commands as the primary user invocation model.
- Prefix commands require receiving message content; `MESSAGE_CONTENT` is a privileged gateway intent.
- Slash command interaction payloads already identify the user, guild, channel, and submitted options without scraping message text.
- Application commands are limited to Discord guild interaction contexts; do not enable bot-DM invocation without a documented need.

### Serenity and Poise

Serenity is the Discord API client. It provides the client, gateway connection, HTTP operations, Discord models, and event processing. Its current `0.12.5` release supports the relevant Discord interaction API.

Poise is the command framework built on Serenity. Use it for command declaration, registration, argument parsing, command context, and error dispatch. This still uses Serenity rather than replacing it: command handlers can use Serenity models and HTTP access through Poise.

Do not build on Serenity's `standard_framework`. Serenity documents it as deprecated since `0.12.1` and recommends Poise instead.

### Gateway Versus HTTP Interactions

Discord can deliver interactions either through a Gateway connection or to a public interactions HTTP endpoint. Use Gateway delivery initially:

- A bot container is already expected to be continuously running.
- Serenity handles gateway sessions, heartbeats, and reconnect behavior.
- Gateway mode requires outbound network access only; it does not require TLS termination, an inbound port, or signature-validation web server code.

If a future bot is almost entirely command-driven and needs scale-to-zero hosting, reconsider HTTP interactions. It is not useful for this single-server first deployment.

### Intents and Permissions

Start with slash commands and the minimum gateway intents needed by the final Poise setup. Interaction create events do not require reading message content. In particular:

- Do not enable `MESSAGE_CONTENT` for declared application commands.
- Do not enable `GUILD_MEMBERS` or `GUILD_PRESENCES` until a feature needs them.
- Add event-specific standard intents only when a feature consumes those gateway events.

For an initial Sillybot instance:

- Register the declared application commands globally so they are available in every guild where that instance is installed.
- Declare their interaction contexts as guild-only; global registration does not make them available in direct messages.
- For local development only, allow `DEV_GUILD_ID` to synchronize the declared commands to one test guild for immediate Discord propagation; absence of `DEV_GUILD_ID` selects production global synchronization.
- Give each independently operated instance its own Discord application identity and bot token; multiple instances must not share one token while retaining separate global counters.
- Synchronize the declared global command set during startup before beginning gateway service; do not expose a running interaction service with a stale Discord command surface.
- Retry transient Discord network/API-availability failures during startup command synchronization with capped backoff while serving no interactions; exit immediately on non-transient failures such as invalid credentials or rejected command definitions.
- Treat the instance operator's Discord installation control as the initial authorization boundary; do not require an application-configured guild allowlist.
- An operator who needs additional restrictions can configure command visibility in Discord server integration settings.

### Response Timing

Discord requires an initial interaction response within 3 seconds; the interaction token remains usable for follow-up work for 15 minutes. `/ping` and `/count` complete directly within the initial response. `/count` uses a normal channel-visible response; its cross-guild aggregate value is intentionally visible wherever that instance is installed. Moderation commands defer immediately before their Discord mutation and respond ephemerally to the moderator.

## Rust Application Architecture

The implementation remains a single crate with modules organized by responsibility:

```text
src/
  main.rs                 startup, configuration, tracing, graceful shutdown
  bot.rs                  Poise framework construction and gateway lifecycle
  commands/
    mod.rs
    ping.rs
    count.rs
    synchronization.rs    application-command declaration and pre-gateway synchronization
    admin/                moderation commands and common action execution
  db/
    mod.rs                InstanceData: migrations, persisted behavior, snapshot generation
migrations/
  0001_counter.sql
  0002_admin_log_channel.sql
deploy/
  Containerfile
  compose.yaml
```

Keep Discord handler code thin. `/count` calls an `InstanceData` method; it should not contain SQL, migration behavior, or container/path concerns. `InstanceData` owns the serialized Turso implementation for the Sillybot instance's persisted behavior.

### Runtime State

Poise framework data should hold application state such as:

```text
AppState
  instance_data: InstanceData
```

Because this is a low-volume beta database integration, `InstanceData` serializes database operations rather than adding a pool or parallel write paths. A Tokio mutex around the active database connection is acceptable for this workload. Reassess after Turso's concurrency limitations change or an actual feature needs parallel database access.

When scheduled snapshots are enabled, inspect migrations before applying them. For an existing database with pending migrations, create a pre-migration snapshot first; for a fresh database there is no pre-existing state to export. After creating a fresh database or applying migrations, create a post-migration snapshot before Discord command synchronization or gateway service begins. If protection is enabled for an already current database without a protected export for its current schema, create one baseline snapshot. Ordinary restarts with no schema transition and no newly enabled protection create no startup export. Persist successful event-snapshot markers in the protected data directory outside the cleanup-managed exported files so startup retries do not generate duplicate exports for the same baseline or migration boundary. After startup, use a lightweight Tokio background task for daily snapshot creation. All snapshot paths call the snapshotter through the same serialized database boundary as commands. No scheduling crate is needed for the initial daily interval. Scheduled snapshot generation is disabled by default for disposable evaluation instances and must be explicitly enabled as part of an operator's backup workflow.

### Graceful Shutdown

The process should handle `SIGTERM` sent during Docker Compose replacement or shutdown and shut down the Discord client cleanly before exiting. This matters because deployments restart the one bot container and because database files are persisted through WAL.

## Crate Selection

Versions below reflect compatible released lines verified during initial research; commit `Cargo.lock` once implementation begins.

### Required for Phase 1

| Crate | Version line | Use |
| --- | --- | --- |
| `poise` | `0.6.2` | Slash command framework built on Serenity; brings compatible Serenity support. |
| `serenity` | `0.12.5` through Poise, or direct only when needed | Discord gateway and HTTP API types/client. Keep versions compatible with Poise. |
| `tokio` | `1` with `macros`, `rt-multi-thread`, `signal`, `sync`, `time` | Async entry point, runtime, shutdown, serialized database access, daily snapshot interval. |
| `turso` | `0.6.1` | Embedded local database opened from the mounted data path. |
| `tracing` | `0.1` | Structured application logs. |
| `tracing-subscriber` | `0.3` with `env-filter`, `fmt` | Log formatting and `RUST_LOG` filtering. |
| `anyhow` | `1` | Startup/configuration failure context at the binary boundary. |

### Development and Test Dependencies

| Crate | Use |
| --- | --- |
| `tempfile` | File-backed Turso integration tests without touching real data. |

### Add Only When a Feature Requires It

| Need | Candidate | Reason not added now |
| --- | --- | --- |
| Typed domain error API | `thiserror` | Two test commands do not need an exported error hierarchy. |
| Rich configuration files | `serde` plus a configuration crate | Environment variables are sufficient for one deployment. |
| HTTP health/metrics endpoint | `axum` and metrics tooling | The bot has no inbound HTTP requirement initially. |
| Voice | `songbird` | No voice feature has been specified. |
| Complex schedules | `tokio-cron-scheduler` or equivalent | One daily backup interval can use `tokio::time`; no scheduling framework is needed. |

Do not add a generic SQL ORM or SQLx initially. The selected `turso` Rust API is the integration boundary and an unverified adapter would weaken the database test this phase is meant to perform.

## Database Design

### Database Location

Use one local file configured through:

```text
DATABASE_PATH=/var/lib/sillybot/sillybot.db
```

For a protected instance, the entire `/var/lib/sillybot` directory is backed by an operator-selected host directory, for example `/srv/sillybot/data`, bind-mounted into the container. WAL-related sidecar files belong in that directory and must be considered part of the database state.
Completed online snapshot files are staged under `/var/lib/sillybot/snapshots/`, visible on the host as the bind-mounted data directory's `snapshots/` subdirectory for the separate encrypted R2 backup job. Their filenames identify snapshot purpose and use UTC timestamps: `sillybot-pre-migration-<schema boundary>-<UTC timestamp>.db`, `sillybot-post-migration-<schema version>-<UTC timestamp>.db`, `sillybot-baseline-<schema version>-<UTC timestamp>.db`, or `sillybot-daily-<UTC timestamp>.db`. A disposable evaluation instance may instead use an unnamed or named Docker volume while snapshots remain disabled.

### Journal Mode Decision

| Mode | Decision | Rationale |
| --- | --- | --- |
| `wal` | Use | Turso documents WAL as the default mode for new databases and as providing good reader/writer concurrency. It is sufficient for one low-write bot process. |
| `mvcc` | Reject initially | Turso documents MVCC as not production-ready, with serious limitations including no usable indexes, eager memory loading, incomplete checkpoint behavior, and possible incorrect results or panics. |
| Multi-process WAL | Reject initially | Documented as experimental, and the architecture deliberately runs only one database-owning process. |

Do not set `PRAGMA journal_mode = mvcc`. At startup, log the active journal mode and fail or loudly report if configuration unexpectedly selects an experimental mode.

### Initial Counter Schema

The purpose of `/count` is a persistence probe, not an audit log or analytics model. Persist only the instance's current global counter value; do not store user, guild, channel, or per-invocation history for increments. Keep the initial schema deliberately modest while Turso Database remains beta:

```sql
CREATE TABLE IF NOT EXISTS command_counter (
    value INTEGER NOT NULL
);
```

On initialization, application code queries the single row and issues `INSERT INTO command_counter VALUES (0)` only when none exists. This keeps bootstrap behavior single-process and avoids depending on extra SQL features simply to initialize test state.

The store increments and retrieves the value in one transaction:

```sql
BEGIN IMMEDIATE;
UPDATE command_counter SET value = value + 1;
SELECT value FROM command_counter;
COMMIT;
```

This avoids requiring secondary indexes or an advanced upsert path just to prove persistence. Before implementation, turn this SQL into an integration test against the pinned `turso` crate version; beta compatibility must be demonstrated, not assumed.

If `/count` later needs per-user or per-guild values, introduce a keyed table and index only after verifying those schema features on the pinned Turso release.

### Migrations

Use checked-in SQL migration files and a minimal application-owned migration runner for the first schema. The runner should execute migrations once at startup before the gateway client starts accepting work and record applied migration numbers.

A third-party migration framework should be introduced only once its Turso Database Rust API support is verified. Selecting a SQLite integration package on name alone is not sufficient because Turso Database is a new implementation, not merely a `rusqlite` connection.

### Backups and Recovery

Backups are a mandatory instance-operator responsibility before relying on persisted data because Turso Database is beta. The bot does not refuse to start when backup infrastructure is absent: a local or evaluation instance may use disposable counter data, while an operator accepting durable data without tested off-host backups explicitly accepts loss risk.

- Do not copy the live database file and its WAL files directly as the primary backup mechanism.
- When snapshot scheduling is explicitly enabled, create consistent standalone snapshots from inside the running bot process under `/var/lib/sillybot/snapshots/`. Name exports by purpose with UTC timestamps: `sillybot-pre-migration-<schema boundary>-<UTC timestamp>.db`, `sillybot-post-migration-<schema version>-<UTC timestamp>.db`, `sillybot-baseline-<schema version>-<UTC timestamp>.db`, and `sillybot-daily-<UTC timestamp>.db`. Turso `v0.6.1` documents that `VACUUM INTO` uses a consistent view even while other connections write and fully syncs/checkpoints the destination before returning.
- When snapshots are enabled and an existing database has pending migrations, generate a pre-migration snapshot before applying them to retain a rollback point for a faulty data transformation.
- Generate a post-migration snapshot only after a fresh database is initialized or pending migrations are applied, so the operator can exercise upload and restore against the running schema before relying on new data.
- If snapshots are first enabled on an existing database that is already current, generate one baseline snapshot for its current schema. Do not generate a baseline or post-migration export on every ordinary restart.
- Persist event-snapshot completion state in the protected host data directory outside local exported-file cleanup, keyed to baseline/schema-transition purpose, so startup retries after later failures do not repeatedly export an unchanged database.
- After successful startup, generate operational snapshots daily.
- Execute snapshot generation through the serialized database store so that no other statement is active on that connection. `/count` may wait briefly while a snapshot is being built, but the bot does not stop.
- Generate unique purpose-labeled UTC snapshot filenames rather than overwriting: Turso requires the destination file not to exist, and the purpose/schema label makes rollback exports recognizable during restore.
- Store backups off-host in a private Cloudflare R2 bucket. R2 provides an S3-compatible API, so existing S3 backup clients can target it using the R2 endpoint.
- Use `restic` against the R2 S3 endpoint as the recommended upload/retention client: it provides client-side encrypted backups while leaving the application container unaware of R2 credentials.
- Run `restic` from a dedicated host systemd timer/service against completed files in the host data directory's `snapshots/` subdirectory; it uploads snapshots only and never opens the active database.
- Scope the R2 API token to the backup bucket with object read/write access only and store it outside the bot container as backup-service credentials.
- Enforce backup retention through `restic forget --prune`, not by expiring arbitrary restic repository objects with an R2 lifecycle rule. Initial default: daily snapshots retained for 14 days and weekly snapshots retained for 8 weeks, adjusted when the stored data becomes important.
- Retain each purpose-labeled pre-migration snapshot off-host for at least 30 days, independently of ordinary daily pruning, for example by tagging the corresponding restic backup for migration rollback retention.
- Clean local exported snapshots only after a successful off-host backup and restore checks.
- Test restoration by restoring an R2 snapshot to a temporary database file and running `/count` or a direct integration check.
- A rollback from a pre-migration snapshot must deploy the corresponding pre-migration image/version before starting the bot; starting the newer image against that restored database would reapply its pending migration.
- Do not rely on experimental Turso encryption-at-rest or multi-process facilities for data safety.
- Do not enable scheduled snapshots for an evaluation instance without an upload and cleanup workflow; unmanaged local snapshots accumulate without satisfying the off-host backup requirement.

The initial implementation should test `VACUUM INTO` using the pinned Turso crate before treating backups as operationally complete.

## Configuration and Secrets

Initial runtime configuration:

| Variable | Required | Meaning |
| --- | --- | --- |
| `DISCORD_TOKEN_FILE` | yes | Path to a file containing the bot token; the reference Compose deployment mounts it at `/run/secrets/discord_token`. |
| `DATABASE_PATH` | yes | Persistent Turso database file path inside the container. |
| `BACKUP_SNAPSHOTS_ENABLED` | no | Set to `true` only as part of an operator-configured snapshot upload, retention, and restore-tested workflow; defaults to disabled. |
| `DEV_GUILD_ID` | no | Development-only test guild used instead of global command synchronization for immediate command iteration; omit in production. |
| `RUST_LOG` | no | Logging filter, for example `sillybot=info,serenity=warn`. |

Production command registration is global and does not require a guild identifier or guild allowlist in application configuration. `DEV_GUILD_ID`, when set in local development, replaces global synchronization with one guild-scoped synchronization path for fast iteration; it must be absent from production configuration. Startup initializes the database and determines pending migrations; if snapshots are enabled and an existing database needs migration, it first creates or reuses the completed pre-migration export for that transition; it applies migrations and creates a post-migration export only after a fresh initialization or applied migration; if snapshots were newly enabled for an already current database, it creates one baseline export. It next synchronizes its declared application commands in the selected development or production scope and only then begins gateway service. During synchronization it retries transient network/API-availability failures with capped backoff while serving no interactions; invalid credentials or rejected command definitions terminate startup immediately. The bot exits on any required enabled snapshot failure rather than serving without its selected protection, and it does not serve interactions with a stale command surface. Retrying a later startup failure does not create duplicate event exports. Each instance operator controls which guilds may use the instance through Discord installation configuration.

The token read through `DISCORD_TOKEN_FILE` identifies exactly one Sillybot instance. Keep the secret file outside the repository with restrictive host permissions; do not put the token in a tracked Compose file or `.env` file. Docker Compose mounts the granted secret as a file inside the container, without embedding it in the image or ordinary environment configuration. R2 backup credentials belong to the backup service, not to the running Discord bot. Consequently, the bot cannot treat backup readiness as a startup prerequisite; the instance operator must configure and test the separate backup/restore workflow before relying on stored data.

## Packaging and Deployment

### Container Image

Package the bot as an OCI image using a multi-stage `Containerfile`:

1. Builder stage: compile the Rust binary in release mode with a locked dependency graph.
2. Runtime stage: copy only the binary and the minimal required certificates/runtime files, run as a non-root user, and create or mount `/var/lib/sillybot`.

The runtime image has no listening port in the Gateway design. It needs outbound TLS access to Discord and a writable mounted data directory.

### Docker Compose Reference Deployment

Maintain Docker Compose as the initial reference deployment:

- Docker documents Compose deployment on a single production server and provides restart policies for the application container.
- There is one long-running bot service and one operator-visible data directory; Compose is a widely familiar way to declare that container configuration.
- Compose mounts the Discord token as a service-granted file secret, which the bot reads through `DISCORD_TOKEN_FILE`.
- Docker Swarm is not useful for a single Gateway consumer that owns an embedded local database.
- The OCI image remains usable by Podman users, but the initial project does not maintain a separate Quadlet deployment recipe.
- The application retries transient pre-service Discord synchronization failures itself because Docker restart policies do not cover containers that fail before being up for at least 10 seconds.

Conceptual protected-instance `compose.yaml`, with `/srv/sillybot/data` and `/etc/sillybot/discord_token` replaceable by operator-selected host paths:

```yaml
services:
  sillybot:
    image: ghcr.io/OWNER/sillybot:VERSION
    restart: unless-stopped
    stop_grace_period: 30s
    environment:
      DISCORD_TOKEN_FILE: /run/secrets/discord_token
      DATABASE_PATH: /var/lib/sillybot/sillybot.db
      BACKUP_SNAPSHOTS_ENABLED: "true"
      RUST_LOG: sillybot=info,serenity=warn
    secrets:
      - discord_token
    volumes:
      - type: bind
        source: /srv/sillybot/data
        target: /var/lib/sillybot

secrets:
  discord_token:
    file: /etc/sillybot/discord_token
```

Production deployment artifacts should replace placeholders and select a protected host data directory accessible to the host backup service. That directory and the token file must exist with appropriately restrictive permissions before `docker compose up -d`; the token file remains outside the repository.

### Public Container Distribution

Here, a **container image** means the packaged Rust bot binary and its runtime filesystem that a container runtime executes. It does not mean images posted to Discord or application media.

Publish container images to GitHub Container Registry (`ghcr.io`):

1. Keep the source project public on GitHub and publish an OCI image package named `ghcr.io/OWNER/sillybot`.
2. Use GitHub Actions to build and push a multi-platform `linux/amd64` and `linux/arm64` image after the Turso counter, migration, and enabled snapshot integration behavior has passed for both target platforms.
3. Publish immutable version tags and a commit-SHA tag; do not deploy the moving `latest` tag to the production server.
4. Set the GHCR package visibility to public so other self-hosters can pull the image anonymously.
5. Configure Compose to a chosen version or digest and recreate the service with `docker compose up -d`.
6. Roll back by returning `image:` to the previously known-good tag or digest and recreating the service.

This is separate from GitHub Releases. A release page is optional; GHCR is the distribution mechanism required for users and the server to pull a runnable container image. There is no reason to host a custom registry for this project.

Do not enable automatic production deployment on every image publication initially. Once migrations, backups, and rollback tests are established, automated deployment of a deliberate stable channel may be considered.

### Downtime and Rollback

A short restart deployment is the correct starting tradeoff. A second simultaneous bot process is not a free zero-downtime mechanism:

- Two Gateway consumers can execute the same logical functionality unexpectedly unless shard/session ownership is designed.
- Two instances must not independently open the same embedded database data directory.
- An interaction already in flight has Discord's short response deadline.

Deployment therefore stops/replaces/starts the single service. For this small self-hosted instance, a brief reconnection window is preferable to distributed coordination.

If future functionality makes uninterrupted handling necessary, redesign deliberately: move data to a server/remote database capable of multi-instance writes and choose an interaction delivery or leader-election model before attempting rolling deployment.

## Observability and Operations

Initial operations should include:

- Structured stdout/stderr logging via `tracing`; inspect it with `docker compose logs`.
- Startup logs for application version, database path without secrets, active Turso journal mode, pending/applied migrations, any enabled pre-migration, post-migration, or baseline snapshot creation/reuse, command synchronization attempts/backoff without secrets, successful global command synchronization, and successful Discord readiness.
- Error logs for command failures with Discord command name and correlation identifiers, without logging the bot token or sensitive content.
- Internal capped-backoff retries for transient pre-service Discord synchronization failures, plus Docker Compose restart policy for failures after the container has been running.

Do not expose a health HTTP port only for appearances. For phase 1, `docker compose ps` plus successful gateway-ready logging is adequate. Add a real readiness mechanism only when another system must consume it.

## Testing Plan

### Automated Tests

| Test | Purpose |
| --- | --- |
| Counter store on a temporary file-backed Turso database | Verify initialization and increments use the selected Turso API and SQL subset. |
| Reopen the temporary database after increment | Verify durability across process-equivalent reopen. |
| Multiple sequential increments | Verify counter correctness under the serialized access policy. |
| Migration application on a fresh and already migrated database | Verify idempotent startup. |
| Enabled event snapshots | Verify pending migrations get purpose-labeled pre- and post-migration UTC snapshots, a fresh database gets only a post-migration snapshot, first enablement on a current database gets one baseline snapshot, retry-only restarts duplicate none, and required snapshot failure is fatal only when enabled. |
| Command declaration/registration adapter test | Verify unset `DEV_GUILD_ID` uses global synchronization, set `DEV_GUILD_ID` uses only guild synchronization, startup retries transient failures with capped backoff, fails non-transient registration/authentication errors immediately, and never begins gateway service before synchronization succeeds. |
| Container platform integration matrix | Run Turso counter durability, migration, and enabled snapshot tests for `linux/amd64` and `linux/arm64` before publishing a multi-platform GHCR tag. |

### Deployment Smoke Test

1. Deploy the container with a new protected host data directory bind-mounted at `/var/lib/sillybot`.
2. With snapshots enabled, observe successful migration, a `sillybot-post-migration-<schema version>-<UTC timestamp>.db` snapshot, and startup synchronization of the declared application commands; then invoke `/ping` and observe `Pong!`.
3. Invoke `/count` twice; observe values `1` then `2`.
4. Restart or replace the container without deleting the host data directory.
5. Invoke `/count`; observe `3`.
6. While the bot remains online, observe a scheduled `sillybot-daily-<UTC timestamp>.db` snapshot generation and invoke `/count` again successfully.
7. Run the R2 upload job for the completed snapshot.
8. Restore a running-schema R2 backup to a temporary test data directory and confirm the stored value; separately exercise any pre-migration rollback snapshot with its corresponding earlier image before depending on upgrade rollback.

This tests the two critical integration boundaries: Discord interactions and embedded database persistence in the deployment environment.

## Evolution Rules

- Add Discord intents only alongside a documented feature needing the associated events/data.
- Keep initial application commands guild-context only; introduce direct-message invocation only with a documented feature needing it.
- Add bot modules by feature; do not accumulate SQL in command handlers.
- Synchronize the complete declared global command set during startup; do not introduce manual production registration drift.
- Keep guild-scoped synchronization behind development-only `DEV_GUILD_ID`; production synchronization remains global.
- Keep the instance out of gateway service and retry transient Discord synchronization failures with capped backoff until it can expose the declared command surface; fail immediately for non-transient synchronization errors.
- Require one Discord application identity and bot token per independently operated Sillybot instance.
- Require instance operators to enable scheduled snapshots and keep tested R2 backups before relying on persisted data; do not make bot startup depend on external backup-service configuration.
- When snapshots are enabled, snapshot existing data before applying pending migrations and snapshot the migrated database before Discord service starts; fail startup if either required export fails.
- When protection is first enabled without a migration, create one baseline export; do not create event exports on ordinary or retry-only restarts.
- Label event snapshots by purpose and schema boundary/version with UTC timestamps so restore operators can distinguish rollback and running-schema exports directly from filenames.
- Retain pre-migration rollback snapshots off-host for at least 30 days regardless of ordinary daily backup pruning.
- Pair restoration of a pre-migration rollback export with the earlier compatible container image so the defective migration is not immediately rerun.
- Use an operator-selected bind-mounted host data directory for protected instances so host backup and restore tooling has a documented persistence boundary.
- Revisit HTTP interactions only when there is a clear deployment or availability benefit.
- Revisit container orchestration only when there are genuinely multiple services or replicas to coordinate.
- Publish runnable images through GHCR; do not make end users build the bot unless they specifically choose to.
- Gate supported `linux/amd64` and `linux/arm64` image publication on platform-specific Turso persistence and snapshot integration verification.

## Confirmed Owner Decisions

- Use Turso Database; do not design a fallback database into this architecture.
- Require instance operators to explicitly enable online Turso backup snapshots with `VACUUM INTO`, including pre-migration snapshots for existing databases with pending migrations, post-migration snapshots only for initialization or applied migration, a baseline on first protection of already-current data, and daily snapshots, and store tested encrypted off-host copies in Cloudflare R2 before relying on persisted data; do not enforce external backup-service readiness at bot startup.
- Do not scope application commands to a single Discord guild or require a runtime guild allowlist initially; synchronize production commands globally on startup and leave installation control to each instance operator.
- Restrict initial `/ping` and `/count` command interaction contexts to guilds; do not enable bot-DM invocation.
- Require one Discord application identity and bot token per Sillybot instance; sharing one bot identity across independent data directories is unsupported.
- Maintain Docker Compose as the initial reference deployment with a host data bind mount and file-mounted Discord token secret; the OCI image remains usable through other runtimes without maintained deployment recipes.
- Retry transient Discord command-synchronization failures internally before gateway service begins; fail immediately for invalid credentials or rejected command definitions.
- Publish the open-source bot as a public `linux/amd64` and `linux/arm64` container image in GitHub Container Registry after platform-specific Turso persistence and snapshot integration verification; no self-hosted registry is needed.

## Sources

Primary sources consulted on 2026-05-25:

- [Serenity repository and README](https://github.com/serenity-rs/serenity) - version, features, Tokio setup, deprecation of `standard_framework`, and Poise recommendation.
- [Serenity `Cargo.toml` on the `current` branch](https://github.com/serenity-rs/serenity/blob/current/Cargo.toml) - released version and feature definitions.
- [Poise repository](https://github.com/serenity-rs/poise) - Serenity-based slash command framework and current release.
- [Discord Interactions and Commands documentation](https://docs.discord.com/developers/platform/interactions) - application commands and Gateway versus HTTP delivery.
- [Discord Application Commands documentation](https://docs.discord.com/developers/interactions/application-commands) - scopes, guild commands, contexts, and command permissions.
- [Discord Gateway documentation](https://docs.discord.com/developers/events/gateway) - intents and privileged message content behavior.
- [Discord Receiving and Responding to Interactions documentation](https://docs.discord.com/developers/interactions/receiving-and-responding) - response deadline and follow-up token lifetime.
- [Turso Database repository and README](https://github.com/tursodatabase/turso) - beta status, Rust API entry point, and release information.
- [Turso Database Manual at `v0.6.1`](https://github.com/tursodatabase/turso/blob/v0.6.1/docs/manual.md) - embedded architecture, limitations, transactions, WAL and MVCC journal modes.
- [Turso `v0.6.1` `VACUUM INTO` reference](https://github.com/tursodatabase/turso/blob/v0.6.1/docs/sql-reference/statements/vacuum.mdx) and [compatibility reference](https://github.com/tursodatabase/turso/blob/v0.6.1/COMPAT.md) - active consistent snapshot support and unavailable `sqlite3_backup_*` API.
- [Turso Rust binding entry point](https://github.com/tursodatabase/turso/blob/v0.6.1/bindings/rust/src/lib.rs), [connection type](https://github.com/tursodatabase/turso/blob/v0.6.1/bindings/rust/src/connection.rs), and [concurrent-write example](https://github.com/tursodatabase/turso/blob/v0.6.1/bindings/rust/examples/concurrent_writes.rs) - `Builder::new_local` API shape and exposed concurrency surface.
- [Cloudflare R2 S3 API documentation](https://developers.cloudflare.com/r2/get-started/s3/) - private object storage access using S3-compatible tools and scoped R2 credentials.
- [Cloudflare R2 upload/download documentation](https://developers.cloudflare.com/r2/objects/upload-objects/) - backup storage transfers.
- [Restic S3-compatible storage documentation](https://restic.readthedocs.io/en/latest/030_preparing_a_new_repo.html) - encrypted backup repository operation against S3-compatible endpoints.
- [Docker Compose production documentation](https://docs.docker.com/compose/how-tos/production/) - single-server deployment and service recreation.
- [Docker Compose service documentation](https://docs.docker.com/reference/compose-file/services/) - restart policy, stop grace period, secrets, and bind-mounted volume configuration.
- [Docker Compose secrets documentation](https://docs.docker.com/reference/compose-file/secrets/) - mounting the Discord token as a service-granted file secret.
- [Docker bind mount documentation](https://docs.docker.com/engine/storage/bind-mounts/) - exposing protected host data to the container and host backup tooling.
- [Docker restart policy documentation](https://docs.docker.com/engine/containers/start-containers-automatically/) - restart behavior and the successful-start requirement.
- [GitHub Container registry documentation](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry) - public OCI image publishing and anonymous pulls without a GitHub Release.
