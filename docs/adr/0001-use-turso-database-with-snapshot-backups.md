---
status: accepted
---

# Use Turso Database with Snapshot Backups

Sillybot will store durable application data in embedded Turso Database, without designing a fallback database into the initial architecture. Turso is intentionally selected despite its beta status; to manage that trade-off, the application creates consistent `VACUUM INTO` snapshots and operations retain tested, encrypted off-host copies in Cloudflare R2.

## Consequences

- Database behavior used by the application must be verified against the pinned Turso release.
- Experimental database modes and multi-process database ownership are excluded unless this decision is revisited.
- Data that cannot be recreated must not rely on an untested backup and restore process.
