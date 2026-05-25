---
status: accepted
---

# Use Turso Database with Snapshot Backups

Sillybot will store durable application data in embedded Turso Database, without designing a fallback database into the initial architecture. Turso is intentionally selected despite its beta status; to manage that trade-off, the application creates consistent `VACUUM INTO` snapshots and instance operators retain tested, encrypted off-host copies in Cloudflare R2 before relying on persisted data.

## Consequences

- Database behavior used by the application must be verified against the pinned Turso release.
- Experimental database modes and multi-process database ownership are excluded unless this decision is revisited.
- Data that cannot be recreated must not rely on an untested backup and restore process.
- Backup configuration, explicitly enabled event/daily snapshot generation, and restore verification are the instance operator's responsibility. When snapshots are enabled, an existing database with pending migrations receives pre- and post-migration snapshots, fresh initialization receives a post-migration snapshot, and first protection of an already-current database receives a baseline snapshot. Event exports are idempotent across startup retries; ordinary restarts do not create them. Failure to produce a required snapshot fails bot startup. The bot does not otherwise gate startup on the external backup service.
- Restoring a pre-migration rollback snapshot requires deploying its corresponding earlier application image before startup, so a faulty newer migration is not immediately reapplied.
