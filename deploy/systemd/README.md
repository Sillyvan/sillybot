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

# Discord token, kept outside the repo with restrictive permissions.
sudo install -d -m 0700 /etc/sillybot
sudo install -m 0600 /dev/stdin /etc/sillybot/discord_token <<<'YOUR_TOKEN'
```

## Install the unit

```sh
# Run all of the below as the sillybot user.
sudo -iu sillybot

# Load the token as a podman secret.
podman secret create discord_token /etc/sillybot/discord_token

# Drop the quadlet into the user-level search path.
mkdir -p ~/.config/containers/systemd
cp /path/to/repo/deploy/systemd/sillybot.container ~/.config/containers/systemd/

# Edit Image= to your published GHCR tag, then generate the unit and start.
$EDITOR ~/.config/containers/systemd/sillybot.container
systemctl --user daemon-reload
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

## Auto-update on new image push

```sh
systemctl --user enable --now podman-auto-update.timer
```

The container honors `AutoUpdate=registry`, so the timer pulls newer digests of
the configured tag and restarts the service.

## Backups

The bind mount at `/srv/sillybot/data` contains the live database, WAL
sidecars, and `snapshots/`. Run the off-host backup job (e.g. `restic` against
R2) on the host against that path; the bot has no R2 credentials and is
unaware of the backup destination.

## Migrate from compose

```sh
# Stop the compose stack.
docker compose -f deploy/compose.yaml down

# Copy the named-volume contents into the host bind mount.
docker run --rm -v deploy_sillybot-data:/src -v /srv/sillybot/data:/dst \
    alpine sh -c 'cp -a /src/. /dst/ && chown -R 1000:1000 /dst'

# chown to the sillybot service user's uid/gid, then start the quadlet.
```
