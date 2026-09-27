#!/usr/bin/env bash
# Stamps VERSION into [workspace.package] and Cargo.lock, so every binary reports the version
# the update manifest announces (a mismatch would make installed copies update in a loop).
# Usage: scripts/set-version.sh VERSION
set -euo pipefail
version="${1:?usage: scripts/set-version.sh VERSION}"
cd "$(dirname "${BASH_SOURCE[0]}")/.."
sed -i "/^\[workspace.package\]/,/^\[/ s/^version = \".*\"/version = \"$version\"/" Cargo.toml
grep -q "^version = \"$version\"" Cargo.toml || { echo "failed to set the version in Cargo.toml" >&2; exit 1; }
cargo update --workspace --quiet
