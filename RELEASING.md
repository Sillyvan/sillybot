# Releasing Sillybot

`Cargo.toml` is the source of truth for the Sillybot software version. The
binary reports that version through `/info` and at startup.

## Create a release tag

Begin from a clean worktree on the commit to release:

```sh
scripts/release.sh
```

The script displays the current version and asks whether to bump `patch`,
`minor`, or `major`. It updates `Cargo.toml` and `Cargo.lock`, runs the test
suite, creates a release commit, and creates an annotated `vX.Y.Z` tag.

For non-interactive use or to inspect a bump without changing files:

```sh
scripts/release.sh patch --dry-run
scripts/release.sh minor
```

Push the created tag and commit:

```sh
git push origin HEAD --follow-tags
```

## Container image publication

Pushing a `vX.Y.Z` tag triggers `.github/workflows/release.yml`. The workflow
rejects a tag that does not match `Cargo.toml`, runs the Turso behavior tests
for `linux/amd64` and `linux/arm64`, then publishes a multi-platform image:

```text
ghcr.io/OWNER/sillybot:vX.Y.Z
ghcr.io/OWNER/sillybot:sha-<commit>
```

Deploy an immutable version tag or digest. No moving `latest` image is
published. GitHub Releases pages are optional; GHCR holds the runnable
release artifact.

The workflow becomes usable once this clone is pushed to its intended GitHub
repository and the repository/package permissions permit GHCR publication.
