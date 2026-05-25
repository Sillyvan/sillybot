# Local Docker Compose Development

The local Compose setup runs one Sillybot instance and persists its global
counter in a named volume. Commands are synchronized only to the configured
development guild for immediate Discord propagation.

1. Create `deploy/.env` from `.env.example` and set your Discord test guild ID.
2. Create `deploy/secrets/discord_token` containing the token for a development
   Discord application installed in that guild.
3. Start the instance from the repository root:

   ```sh
   docker compose --env-file deploy/.env -f deploy/compose.yaml up --build
   ```

Invoke `/ping` and `/count` in the installed guild. Recreate the container and
invoke `/count` again to confirm that the named volume preserves the counter.
Snapshots and off-host backups are intentionally disabled in this local setup.
