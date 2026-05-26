# Podman + systemd (Quadlet) deployment

Production-style deployment on a Linux host. Runs the bot rootless under a
dedicated service user with systemd lifecycle management. For a quick
evaluation, prefer [Docker Compose](../README.md) instead.

Requires Podman 4.4+ and systemd 250+ (RHEL 9, Debian 12, Ubuntu 22.04+,
Fedora 37+, recent Arch).

## One-time host setup

```sh
# Service user that owns the data directory and runs the container rootless.
sudo useradd --system --create-home --shell /usr/sbin/nologin sillybot
sudo loginctl enable-linger sillybot

# Protected host data directory.
sudo install -d -o sillybot -g sillybot -m 0750 /srv/sillybot/data

# Service user's Quadlet search path.
sudo -u sillybot mkdir -p /var/lib/sillybot/.config/containers/systemd
```

## Install the unit

```sh
# Stage the Discord token without storing it in the repository or environment.
read -rsp 'Discord bot token: ' DISCORD_TOKEN; echo
printf '%s' "$DISCORD_TOKEN" | sudo install -o sillybot -g sillybot -m 0600 \
  /dev/stdin /var/lib/sillybot/discord_token
unset DISCORD_TOKEN

# Load the staged file as a secret in the rootless service user's Podman storage.
uid="$(id -u sillybot)"
sudo -u sillybot env HOME=/var/lib/sillybot XDG_RUNTIME_DIR="/run/user/$uid" \
  podman secret create discord_token /var/lib/sillybot/discord_token
sudo rm -f /var/lib/sillybot/discord_token

# Install the Quadlet and set Image= to your published GHCR tag or digest.
sudo install -o sillybot -g sillybot -m 0600 \
  /path/to/repo/deploy/systemd/sillybot.container \
  /var/lib/sillybot/.config/containers/systemd/sillybot.container
sudoedit /var/lib/sillybot/.config/containers/systemd/sillybot.container

# Run user systemd operations without needing a login shell for the service user.
sudo -u sillybot env HOME=/var/lib/sillybot XDG_RUNTIME_DIR="/run/user/$uid" \
  systemctl --user daemon-reload
sudo -u sillybot env HOME=/var/lib/sillybot XDG_RUNTIME_DIR="/run/user/$uid" \
  systemctl --user start sillybot.service
```

Create and publish version tags using [`../../RELEASING.md`](../../RELEASING.md);
set `Image=` to the resulting immutable `vX.Y.Z` GHCR tag or digest.

`systemctl --user enable` is not needed: quadlets with `WantedBy=default.target`
plus `enable-linger` already start at boot.

## Operate

```sh
systemctl --user status sillybot.service
journalctl --user -u sillybot.service -f
systemctl --user restart sillybot.service
```

## Deliberate releases

Deploy immutable version tags or digests. To release an update, change
`Image=` to the new reviewed tag, run `systemctl --user daemon-reload`, and
restart the service. Do not depend on registry auto-update for immutable tags.

## Backups

The bind mount at `/srv/sillybot/data` contains the live database, WAL
sidecars, and `snapshots/`. Run the off-host backup job (e.g. `restic` against
R2) on the host against that path; the bot has no R2 credentials and is
unaware of the backup destination. `BACKUP_SNAPSHOTS_ENABLED` defaults to
`false`; set it to `true` only once the off-host upload, retention, and restore
verification workflow exists.

The Quadlet volume uses Podman's `U` option because the image runs as a
non-root user. On first start, Podman changes the host data directory ownership
to the mapped subordinate UID/GID used by that container user. Run host backup
jobs with sufficient access to read the completed snapshots directory.

## Migrate from compose

```sh
# Stop the compose stack.
docker compose -f deploy/compose.yaml down

# Copy the named-volume contents into the host bind mount.
docker run --rm -v deploy_sillybot-data:/src -v /srv/sillybot/data:/dst \
    alpine sh -c 'cp -a /src/. /dst/ && chown -R 1000:1000 /dst'

# chown to the sillybot service user's uid/gid, then start the quadlet.
```
