#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$root/Cargo.toml"
dry_run=false
level=""

usage() {
    printf 'Usage: %s [patch|minor|major] [--dry-run]\n' "$0"
}

for argument in "$@"; do
    case "$argument" in
        patch|minor|major)
            if [[ -n "$level" ]]; then
                usage >&2
                exit 2
            fi
            level="$argument"
            ;;
        --dry-run) dry_run=true ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            exit 2
            ;;
    esac
done

current_version="$(
    awk '
        /^\[package\]$/ { package = 1; next }
        /^\[/ && package { exit }
        package && /^version = "/ {
            value = $0
            sub(/^version = "/, "", value)
            sub(/"$/, "", value)
            print value
            exit
        }
    ' "$manifest"
)"

if [[ ! "$current_version" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
    printf 'Could not read a SemVer package version from %s\n' "$manifest" >&2
    exit 1
fi

if [[ -z "$level" ]]; then
    printf 'Current version: %s\n' "$current_version"
    printf 'Select release type:\n  1) patch\n  2) minor\n  3) major\n> '
    read -r selection
    case "$selection" in
        1|patch) level="patch" ;;
        2|minor) level="minor" ;;
        3|major) level="major" ;;
        *)
            printf 'No release type selected.\n' >&2
            exit 1
            ;;
    esac
fi

major="${BASH_REMATCH[1]}"
minor="${BASH_REMATCH[2]}"
patch="${BASH_REMATCH[3]}"
case "$level" in
    patch) next_version="$major.$minor.$((patch + 1))" ;;
    minor) next_version="$major.$((minor + 1)).0" ;;
    major) next_version="$((major + 1)).0.0" ;;
esac
tag="v$next_version"

printf '%s release: %s -> %s (%s)\n' "$level" "$current_version" "$next_version" "$tag"
if "$dry_run"; then
    exit 0
fi

cd "$root"
if [[ -n "$(git status --porcelain)" ]]; then
    printf 'Release requires a clean Git worktree. Commit or stash changes first.\n' >&2
    exit 1
fi
if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    printf 'Tag %s already exists.\n' "$tag" >&2
    exit 1
fi

printf 'Create commit and annotated tag %s? [y/N] ' "$tag"
read -r confirmation
case "$confirmation" in
    y|Y|yes|YES) ;;
    *)
        printf 'Release cancelled.\n'
        exit 0
        ;;
esac

temporary_manifest="$(mktemp "$root/Cargo.toml.XXXXXX")"
trap 'rm -f "$temporary_manifest"' EXIT
awk -v new_version="$next_version" '
    /^\[package\]$/ { package = 1 }
    /^\[/ && $0 != "[package]" && package { package = 0 }
    package && /^version = "/ && ! updated {
        print "version = \"" new_version "\""
        updated = 1
        next
    }
    { print }
    END {
        if (! updated) {
            exit 1
        }
    }
' "$manifest" > "$temporary_manifest"
mv "$temporary_manifest" "$manifest"
trap - EXIT

cargo check --quiet
cargo test --locked
git add Cargo.toml Cargo.lock
git commit -m "Release $tag"
git tag --annotate "$tag" --message "Release $tag"

printf 'Created %s. Publish it with: git push origin HEAD --follow-tags\n' "$tag"
