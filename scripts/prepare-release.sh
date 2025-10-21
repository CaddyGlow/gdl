#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <semver-version>" >&2
  exit 1
fi

version="$1"

if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "Error: version must follow semver (e.g. 1.2.3 or 1.2.3-beta)." >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: cargo is required." >&2
  exit 1
fi

if ! cargo set-version --help >/dev/null 2>&1; then
  echo "Error: cargo-edit (cargo set-version) is required. Install with 'cargo install cargo-edit'." >&2
  exit 1
fi

if ! cargo fmt --version >/dev/null 2>&1; then
  echo "Error: rustfmt component is required. Install with 'rustup component add rustfmt'." >&2
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Error: working tree has uncommitted changes. Commit or stash them first." >&2
  exit 1
fi

current_branch=$(git rev-parse --abbrev-ref HEAD)
if [[ "$current_branch" != "main" ]]; then
  echo "Error: releases must be cut from main (current: $current_branch)." >&2
  exit 1
fi

tag="v$version"
if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "Error: tag $tag already exists." >&2
  exit 1
fi

echo "==> Updating crate version to $version"
cargo set-version "$version"

echo "==> Running checks"
cargo fmt
cargo test

echo "==> Creating release commit"
git add Cargo.toml Cargo.lock
if git diff --cached --quiet; then
  echo "==> No changes detected; skipping release commit"
else
  git commit -m "Release $tag"
fi

echo "==> Tagging"
git tag -a "$tag" -m "Release $tag"

cat <<EOF

Release prepared. Next steps:
  1. git push origin main
  2. git push origin $tag
  3. Draft GitHub release if needed

EOF
