# Docker Compose deployment

Quickstart path for evaluating or self-hosting Sillybot via Docker Compose
(also works with `podman compose`). For production on a Linux host, prefer
the [Podman + systemd quadlet](./systemd/README.md) deployment.

The Compose setup runs one Sillybot instance and persists its global counter
in a named volume. With `DEV_GUILD_ID` set, slash commands sync only to that
guild for immediate Discord propagation; unset it for global registration.

1. Create `deploy/.env` from `.env.example` and set your Discord test guild ID.
2. Create `deploy/secrets/discord_token` containing the bot token for a
   Discord application installed in that guild.
3. Start the instance from the repository root:

   ```sh
   docker compose --env-file deploy/.env -f deploy/compose.yaml up --build
   ```

Invoke `/ping`, `/info`, and `/count` in the installed guild. Recreate the
container and invoke `/count` again to confirm that the named volume preserves
the counter.
Snapshots and off-host backups are disabled in this setup; enable them via
the systemd deployment.
