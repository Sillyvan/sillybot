# Sillybot Architecture

Status: initial architecture proposal  
Last researched: 2026-05-25  
Scope: private Discord bot, initially `/ping` and `/count`

## Goals

Sillybot is a small private Discord bot written in Rust. Its first release has two purposes:

1. Prove that Discord interaction handling works with `/ping`.
2. Prove that durable database writes survive process and container restarts with `/count`.

The bot's later purpose is intentionally unknown. The initial design should therefore be small, observable, easy to deploy on one server, and easy to extend without selecting infrastructure for hypothetical scale.

## Constraints and Assumptions

- The application is private, but may be installed in multiple explicitly approved Discord guilds.
- Request volume is low; correctness and operational simplicity matter more than maximum throughput.
- The server is Linux and can run rootless Podman with systemd and cgroup v2.
- Persistent data must remain on the server across container replacements.
- Turso Database is the desired database technology, despite its beta status.
- The source project and its container image will be publicly consumable by other self-hosters.
- Cloudflare R2 is available for private database backup storage.

## Decision Summary

| Concern | Initial decision | Reason |
| --- | --- | --- |
| Discord library | `poise` on top of `serenity` 0.12.x | Serenity recommends Poise for command bots; Serenity's `standard_framework` is deprecated. |
| Command type | Discord application slash commands only | Native Discord UX and no privileged `MESSAGE_CONTENT` intent. |
| Discord transport | Gateway connection managed by Serenity | Fits a continuously running bot and does not require a public HTTP endpoint. |
| Async runtime | Tokio | Serenity and Turso expose asynchronous APIs; Serenity's example setup uses Tokio. |
| Database | Embedded `turso` Rust crate | The linked Turso Database is in-process, so a separate database service would add no value. |
| Turso journal mode | Default WAL mode | Suitable for the small workload and documented as the default; MVCC is experimental and unsuitable here. |
| Backups | Encrypted backups to private Cloudflare R2 storage | The database is beta; R2 provides S3-compatible durable off-host object storage. |
| Command registration | Global application commands | The private bot is not artificially bound to one guild; installation remains controlled. |
| Deployment | One rootless Podman container managed by Quadlet/systemd | Simple lifecycle management, restarts, secrets, and persistent volume support on one server. |
| Container distribution | Public GitHub Container Registry (`ghcr.io`) package | Publishes a standard OCI image that other users and the production server can pull without a hosted registry. |
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

Turso Database is the database choice for this project. Its beta status is managed operationally: keep the database workload simple, avoid experimental database modes, create consistent backup snapshots with `VACUUM INTO`, and maintain tested encrypted copies in Cloudflare R2 before the bot stores data that cannot be recreated.

## System Shape

The initial running system contains one application container, one persistent volume, and an off-host backup destination:

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
| CounterStore                 |
| Turso embedded database      |
+---------------+-------------+
                |
                | /var/lib/sillybot
                v
        persistent Podman volume
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
| `/count` | Increment and return a durable global counter | Read/write Turso transaction |

Application commands are preferable to prefix commands such as `!ping`:

- Discord describes application commands as the primary user invocation model.
- Prefix commands require receiving message content; `MESSAGE_CONTENT` is a privileged gateway intent.
- Slash command interaction payloads already identify the user, guild, channel, and submitted options without scraping message text.

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

- Do not enable `MESSAGE_CONTENT` for `/ping` or `/count`.
- Do not enable `GUILD_MEMBERS` or `GUILD_PRESENCES` until a feature needs them.
- Add event-specific standard intents only when a feature consumes those gateway events.

For initial development and a private bot:

- Register `/ping` and `/count` as global application commands so they are available in every explicitly authorized guild where the bot is installed.
- Keep installation private by controlling who can add the application and which guilds are approved; global command scope does not require public installation.
- Configure command visibility in Discord server integration settings if only selected users or channels should invoke it.

### Response Timing

Discord requires an initial interaction response within 3 seconds; the interaction token remains usable for follow-up work for 15 minutes. `/ping` and `/count` should complete directly within the initial response. Any future operation that can block on external APIs or long computation must defer the interaction immediately, then edit or follow up.

## Rust Application Architecture

The repository currently contains only a new binary crate and a placeholder `main.rs`. The initial implementation should stay a single crate with modules organized by responsibility:

```text
src/
  main.rs                 startup, configuration, tracing, graceful shutdown
  bot.rs                  Poise framework construction and command registration
  commands/
    mod.rs
    ping.rs
    count.rs
  db/
    mod.rs                database initialization and migration runner
    counter.rs            CounterStore implementation
    backup.rs             VACUUM INTO snapshot generation
migrations/
  0001_counter.sql
deploy/
  Containerfile
  quadlet/
    sillybot.container
    sillybot-data.volume
```

Keep Discord handler code thin. `/count` calls a `CounterStore` method; it should not contain SQL, migration behavior, or container/path concerns. This provides a clean place to change storage drivers if Turso maturity becomes unacceptable.

### Runtime State

Poise framework data should hold application state such as:

```text
AppState
  counter_store: CounterStore
  backup_snapshotter: BackupSnapshotter
```

Because this is a low-volume beta database integration, serialize counter database operations through the store initially rather than adding a pool or parallel write paths. A Tokio mutex around the active database connection is acceptable for this first workload. Reassess after Turso's concurrency limitations change or an actual feature needs parallel database access.

Use a lightweight Tokio background task for daily snapshot creation; it calls the snapshotter through the same serialized database boundary as commands. No scheduling crate is needed for the initial daily interval.

### Graceful Shutdown

The process should handle `SIGTERM` sent by Podman/systemd and shut down the Discord client cleanly before exiting. This matters because deployments restart the one bot container and because database files are persisted through WAL.

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

The entire `/var/lib/sillybot` directory is a persistent volume. WAL-related sidecar files belong on the same volume and must be considered part of the database state.
Completed online snapshot files are staged under `/var/lib/sillybot/snapshots/` for the separate encrypted R2 backup job.

### Journal Mode Decision

| Mode | Decision | Rationale |
| --- | --- | --- |
| `wal` | Use | Turso documents WAL as the default mode for new databases and as providing good reader/writer concurrency. It is sufficient for one low-write bot process. |
| `mvcc` | Reject initially | Turso documents MVCC as not production-ready, with serious limitations including no usable indexes, eager memory loading, incomplete checkpoint behavior, and possible incorrect results or panics. |
| Multi-process WAL | Reject initially | Documented as experimental, and the architecture deliberately runs only one database-owning process. |

Do not set `PRAGMA journal_mode = mvcc`. At startup, log the active journal mode and fail or loudly report if configuration unexpectedly selects an experimental mode.

### Initial Counter Schema

The purpose of `/count` is a persistence probe, not a full analytics model. Keep the initial schema deliberately modest while Turso Database remains beta:

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

Backups are required from the first deployment because Turso Database is beta.

- Do not copy the live database file and its WAL files directly as the primary backup mechanism.
- Create a consistent standalone snapshot from inside the running bot process with `VACUUM INTO '/var/lib/sillybot/snapshots/sillybot-<timestamp>.db'`. Turso `v0.6.1` documents that `VACUUM INTO` uses a consistent view even while other connections write and fully syncs/checkpoints the destination before returning.
- Execute snapshot generation through the serialized database store so that no other statement is active on that connection. `/count` may wait briefly while a snapshot is being built, but the bot does not stop.
- Generate snapshot filenames rather than overwriting: Turso requires the destination file not to exist.
- Store backups off-host in a private Cloudflare R2 bucket. R2 provides an S3-compatible API, so existing S3 backup clients can target it using the R2 endpoint.
- Use `restic` against the R2 S3 endpoint as the recommended upload/retention client: it provides client-side encrypted backups while leaving the application container unaware of R2 credentials.
- Run `restic` from a dedicated systemd timer/service against completed files in `/var/lib/sillybot/snapshots`; it uploads snapshots only and never opens the active database.
- Scope the R2 API token to the backup bucket with object read/write access only and store it outside the bot container as backup-service credentials.
- Enforce backup retention through `restic forget --prune`, not by expiring arbitrary restic repository objects with an R2 lifecycle rule. Initial default: daily snapshots retained for 14 days and weekly snapshots retained for 8 weeks, adjusted when the stored data becomes important.
- Clean local exported snapshots only after a successful off-host backup and restore checks.
- Test restoration by restoring an R2 snapshot to a temporary database file and running `/count` or a direct integration check.
- Do not rely on experimental Turso encryption-at-rest or multi-process facilities for data safety.

The initial implementation should test `VACUUM INTO` using the pinned Turso crate before treating backups as operationally complete.

## Configuration and Secrets

Initial runtime configuration:

| Variable | Required | Meaning |
| --- | --- | --- |
| `DISCORD_TOKEN` | yes | Bot token, injected as a Podman secret and never committed or logged. |
| `DATABASE_PATH` | yes | Persistent Turso database file path inside the container. |
| `RUST_LOG` | no | Logging filter, for example `sillybot=info,serenity=warn`. |

Production command registration is global and does not require a guild identifier in application configuration.

Avoid `.env` files on the production server for the bot token. Podman secrets can expose the token as the `DISCORD_TOKEN` environment variable inside the container without embedding it in the container image or tracked deployment files. R2 backup credentials belong to the backup service, not to the running Discord bot.

## Packaging and Deployment

### Container Image

Package the bot as an OCI image using a multi-stage `Containerfile`:

1. Builder stage: compile the Rust binary in release mode with a locked dependency graph.
2. Runtime stage: copy only the binary and the minimal required certificates/runtime files, run as a non-root user, and create or mount `/var/lib/sillybot`.

The runtime image has no listening port in the Gateway design. It needs outbound TLS access to Discord and a writable data volume.

### Podman Quadlet, Not Compose or Swarm

Use rootless Podman Quadlet units managed by systemd for the server deployment:

- There is one long-running service and one volume, so a Compose orchestrator is unnecessary.
- `podman compose` delegates to an external Compose provider, whereas Quadlet directly describes systemd-managed Podman services.
- Quadlet provides normal systemd restart behavior, logs, dependencies, secret mounting, volumes, and optional auto-update policy.
- Docker Swarm is not useful for a single Gateway consumer that owns an embedded local database.

Conceptual Quadlet files:

```ini
# sillybot-data.volume
[Volume]
VolumeName=sillybot-data
```

```ini
# sillybot.container
[Unit]
Description=Sillybot Discord bot

[Container]
Image=ghcr.io/OWNER/sillybot:VERSION
ContainerName=sillybot
Volume=sillybot-data.volume:/var/lib/sillybot
Secret=sillybot-discord-token,type=env,target=DISCORD_TOKEN
Environment=DATABASE_PATH=/var/lib/sillybot/sillybot.db
Environment=RUST_LOG=sillybot=info,serenity=warn

[Service]
Restart=on-failure
TimeoutStopSec=30

[Install]
WantedBy=default.target
```

Production deployment artifacts should replace placeholders with server configuration or a non-secret environment file readable only by the deployment user. The Discord token remains a Podman secret.

For rootless operation, place Quadlet files under `~/.config/containers/systemd/`, enable user lingering so the service runs after logout and at boot, and reload the user systemd manager after updates. Quadlet requires cgroup v2 on the host.

### Public Container Distribution

Here, a **container image** means the packaged Rust bot binary and its runtime filesystem that Podman executes. It does not mean images posted to Discord or application media.

Publish container images to GitHub Container Registry (`ghcr.io`):

1. Keep the source project public on GitHub and publish an OCI image package named `ghcr.io/OWNER/sillybot`.
2. Use GitHub Actions to build and push the container after tests pass for a tagged version.
3. Publish immutable version tags and a commit-SHA tag; do not deploy the moving `latest` tag to the production server.
4. Set the GHCR package visibility to public so other self-hosters can pull the image anonymously.
5. Configure the server Quadlet to a chosen version or digest, pull it, and restart `sillybot.service`.
6. Roll back by returning `Image=` to the previously known-good tag or digest and restarting the service.

This is separate from GitHub Releases. A release page is optional; GHCR is the distribution mechanism required for users and the server to pull a runnable container image. There is no reason to host a custom registry for this project.

Do not enable automatic production deployment on every image publication initially. Once migrations, backups, and rollback tests are established, Podman registry auto-update may be considered for a deliberate stable channel.

### Downtime and Rollback

A short restart deployment is the correct starting tradeoff. A second simultaneous bot process is not a free zero-downtime mechanism:

- Two Gateway consumers can execute the same logical functionality unexpectedly unless shard/session ownership is designed.
- Two instances must not independently open this embedded database volume.
- An interaction already in flight has Discord's short response deadline.

Deployment therefore stops/replaces/starts the single service. For this private bot, a brief reconnection window is preferable to distributed coordination.

If future functionality makes uninterrupted handling necessary, redesign deliberately: move data to a server/remote database capable of multi-instance writes and choose an interaction delivery or leader-election model before attempting rolling deployment.

## Observability and Operations

Initial operations should include:

- Structured stdout/stderr logging via `tracing`; collect it with `journalctl --user -u sillybot.service`.
- Startup logs for application version, database path without secrets, active Turso journal mode, migrations applied, and successful Discord readiness.
- Error logs for command failures with Discord command name and correlation identifiers, without logging the bot token or sensitive content.
- Systemd restart-on-failure for process crashes.

Do not expose a health HTTP port only for appearances. For phase 1, systemd process state plus successful gateway-ready logging is adequate. Add a real readiness mechanism only when another system must consume it.

## Testing Plan

### Automated Tests

| Test | Purpose |
| --- | --- |
| Counter store on a temporary file-backed Turso database | Verify initialization and increments use the selected Turso API and SQL subset. |
| Reopen the temporary database after increment | Verify durability across process-equivalent reopen. |
| Multiple sequential increments | Verify counter correctness under the serialized access policy. |
| Migration application on a fresh and already migrated database | Verify idempotent startup. |

### Deployment Smoke Test

1. Deploy the container with a new persistent volume.
2. Invoke `/ping`; observe `Pong!`.
3. Invoke `/count` twice; observe values `1` then `2`.
4. Restart or replace the container without deleting the volume.
5. Invoke `/count`; observe `3`.
6. While the bot remains online, run its `VACUUM INTO` snapshot generation and invoke `/count` again successfully.
7. Run the R2 upload job for the completed snapshot.
8. Restore the R2 backup to a test volume and confirm the stored value.

This tests the two critical integration boundaries: Discord interactions and embedded database persistence in the deployment environment.

## Evolution Rules

- Add Discord intents only alongside a documented feature needing the associated events/data.
- Add bot modules by feature; do not accumulate SQL in command handlers.
- Keep tested R2 backups before saving valuable data; revisit database usage when requiring indexes/query breadth or multiple replicas.
- Revisit HTTP interactions only when there is a clear deployment or availability benefit.
- Revisit container orchestration only when there are genuinely multiple services or replicas to coordinate.
- Publish runnable images through GHCR; do not make end users build the bot unless they specifically choose to.

## Confirmed Owner Decisions

- Use Turso Database; do not design a fallback database into this architecture.
- Create online Turso backup snapshots with `VACUUM INTO` and store encrypted off-host copies in Cloudflare R2.
- Do not scope application commands to a single Discord guild; register production commands globally while keeping installation private.
- Publish the open-source bot as a public container image in GitHub Container Registry; no self-hosted registry is needed.

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
- [Podman Quadlet/systemd unit documentation](https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html) - managed containers, volumes, secrets, boot behavior, and cgroup v2.
- [Podman auto-update documentation](https://docs.podman.io/en/latest/markdown/podman-auto-update.1.html) - update/restart behavior for systemd-managed containers.
- [Podman Compose documentation](https://docs.podman.io/en/latest/markdown/podman-compose.1.html) - external-provider nature of `podman compose`.
- [GitHub Container registry documentation](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry) - public OCI image publishing and anonymous pulls without a GitHub Release.
- [Podman pull documentation](https://docs.podman.io/en/stable/markdown/podman-pull.1.html) - pulling images from a fully qualified registry reference.
