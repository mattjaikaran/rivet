#!/usr/bin/env bash
# Remove the output Rivet generates, on disk and in the temp dir.
#
# `rivet build` writes a transpiled crate to `<project>/generated`, and each
# crate carries its own cargo `target/` directory (about 140 MB per app).
# The test suite creates throwaway fixture projects under the system temp
# dir; a fixture that compiles a crate carries a `target/` too. Both grow
# without bound, so run this when you are done for the day or before a
# fresh build.
#
# Usage:
#   scripts/clean.sh          remove generated crates and test fixtures
#   scripts/clean.sh --all    also run `cargo clean` on the workspace
#
# Run the script from any directory; it resolves the repository root from
# its own location.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Transpiled crates: one per example app, each with a nested cargo target.
for generated in examples/*/generated; do
    if [ -d "$generated" ]; then
        echo "clean: removing $generated"
        rm -rf "$generated"
    fi
done

# Temp fixture projects and replacement modules from the test suite. Only
# Rivet-named paths are touched; the temp dir itself is left alone.
TMP="${TMPDIR:-/tmp}"
shopt -s nullglob
for leftover in "$TMP"/rivet-*; do
    rm -rf "$leftover"
done
shopt -u nullglob
echo "clean: removed Rivet test fixtures under $TMP"

if [ "${1:-}" = "--all" ]; then
    echo "clean: running cargo clean"
    cargo clean
fi

echo "clean: done"
