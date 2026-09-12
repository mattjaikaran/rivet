#!/usr/bin/env bash
# Remove the output Rivet generates, on disk and in the temp dir.
#
# `rivet build` writes a transpiled crate to `<project>/generated`, and each
# crate carries its own cargo `target/` directory (about 140 MB per app).
# `--target wasm` writes `generated-wasm/` beside it. The test suite creates
# throwaway fixture projects under the temp dirs; on macOS `$TMPDIR` and
# `/tmp` are separate trees, so the sweep covers both.
#
# The largest single consumer is usually the workspace build cache
# (`target/`), which holds the debug and release artifacts for every crate
# and test binary in the workspace. `--all` removes that one too.
#
# Usage:
#   scripts/clean.sh                 remove generated crates and temp fixtures
#   scripts/clean.sh --all           also run `cargo clean` on the workspace
#   scripts/clean.sh --dry-run       report what would be removed, delete nothing
#   scripts/clean.sh --disk          report disk usage only, delete nothing
#
# Removal is best effort: a path that survives is reported, and the script
# still cleans everything else and exits non-zero.
#
# Run the script from any directory; it resolves the repository root from
# its own location.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DRY_RUN=0
ALL=0
DISK_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        --all) ALL=1 ;;
        --disk) DISK_ONLY=1 ;;
        -h | --help)
            # Print the header comment block whatever its length: start after
            # the shebang and stop at the first line that is not a comment.
            sed -n '2,/^[^#]/p' "${BASH_SOURCE[0]}" | sed -e '$d' -e 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "clean: unknown option $arg" >&2
            exit 2
            ;;
    esac
done

# The percentage of used disk space above which a warning is worth printing.
HIGH_DISK_PERCENT=90

# Format a kilobyte count as a human-readable size.
human() {
    awk -v kb="$1" 'BEGIN {
        if (kb >= 1073741824) printf "%.1f TB", kb / 1073741824;
        else if (kb >= 1048576) printf "%.1f GB", kb / 1048576;
        else if (kb >= 1024) printf "%.1f MB", kb / 1024;
        else printf "%d KB", kb;
    }'
}

# Kilobyte footprint of a path, or 0 when it does not exist.
size_kb() {
    [ -e "$1" ] || { echo 0; return; }
    du -sk "$1" 2>/dev/null | awk '{print $1}'
}

# Remove one tree. macOS reports a transient "Directory not empty" when the
# filesystem recreates a metadata file (`.DS_Store`) mid-delete, so retry a
# few times before giving up. Returns non-zero when the path survives.
remove_tree() {
    local path="$1" attempt
    for attempt in 1 2 3; do
        rm -rf "$path" 2>/dev/null || true
        if [ ! -e "$path" ]; then
            return 0
        fi
        sleep 1
    done
    return 1
}

TOTAL_KB=0
FAILURES=0

# Report and, unless this is a dry run, remove one path.
remove() {
    local path="$1"
    [ -e "$path" ] || return 0
    local kb
    kb="$(size_kb "$path")"
    if [ "$DRY_RUN" -eq 1 ] || [ "$DISK_ONLY" -eq 1 ]; then
        printf 'clean: %s (%s)\n' "$path" "$(human "$kb")"
        TOTAL_KB=$((TOTAL_KB + kb))
        return 0
    fi
    printf 'clean: removing %s (%s)\n' "$path" "$(human "$kb")"
    if remove_tree "$path"; then
        TOTAL_KB=$((TOTAL_KB + kb))
    else
        FAILURES=$((FAILURES + 1))
        printf 'clean: could not remove %s; close anything using it and rerun\n' \
            "$path" >&2
    fi
}

# Transpiled crates: one per example app per target.
for generated in examples/*/generated examples/*/generated-wasm; do
    remove "$generated"
done

# Rivet-named scratch under the temp dirs: test fixture projects, replacement
# modules, and probe crates. On macOS `$TMPDIR` and `/tmp` are different
# trees, and fixtures land in both, so sweep every root and de-duplicate by
# real path. Only Rivet-named entries are touched; both temp dirs are left
# otherwise alone.
TMP_ROOTS=()
for candidate in "${TMPDIR:-/tmp}" /tmp; do
    [ -d "$candidate" ] || continue
    resolved="$(cd "$candidate" && pwd -P)"
    duplicate=0
    for seen in "${TMP_ROOTS[@]+"${TMP_ROOTS[@]}"}"; do
        [ "$seen" = "$resolved" ] && duplicate=1
    done
    [ "$duplicate" -eq 1 ] || TMP_ROOTS+=("$resolved")
done

for root in "${TMP_ROOTS[@]+"${TMP_ROOTS[@]}"}"; do
    found=0
    shopt -s nullglob
    for leftover in "$root"/rivet-*; do
        found=$((found + 1))
        remove "$leftover"
    done
    shopt -u nullglob
    if [ "$found" -eq 0 ]; then
        echo "clean: no Rivet scratch under $root"
    else
        printf 'clean: %d Rivet scratch path(s) under %s\n' "$found" "$root"
    fi
done

# The workspace build cache is the largest item by far, and the slowest to
# rebuild, so `--all` owns it.
TARGET_KB="$(size_kb target)"
if [ "$ALL" -eq 1 ]; then
    if [ "$DRY_RUN" -eq 1 ] || [ "$DISK_ONLY" -eq 1 ]; then
        printf 'clean: would run cargo clean (target/, %s)\n' "$(human "$TARGET_KB")"
    else
        printf 'clean: running cargo clean (target/, %s)\n' "$(human "$TARGET_KB")"
        if cargo clean; then
            TOTAL_KB=$((TOTAL_KB + TARGET_KB))
        else
            FAILURES=$((FAILURES + 1))
            echo "clean: cargo clean failed" >&2
        fi
    fi
    if [ "$DRY_RUN" -eq 1 ] || [ "$DISK_ONLY" -eq 1 ]; then
        TOTAL_KB=$((TOTAL_KB + TARGET_KB))
    fi
else
    printf 'clean: leaving target/ (%s); pass --all to remove it\n' \
        "$(human "$TARGET_KB")"
fi

echo
if [ "$DISK_ONLY" -eq 1 ]; then
    printf 'clean: %s in generated crates and test fixtures\n' "$(human "$TOTAL_KB")"
    printf 'clean: %s in the workspace build cache (target/)\n' "$(human "$TARGET_KB")"
elif [ "$DRY_RUN" -eq 1 ]; then
    printf 'clean: dry run; %s would be freed\n' "$(human "$TOTAL_KB")"
else
    printf 'clean: freed %s\n' "$(human "$TOTAL_KB")"
    if [ "$ALL" -ne 1 ]; then
        printf 'clean: target/ still holds %s; pass --all to remove it\n' \
            "$(human "$TARGET_KB")"
    fi
fi

# Warn when the disk is nearly full, because the next build needs headroom.
if command -v df >/dev/null 2>&1; then
    use_percent="$(df -Pk . | awk 'NR == 2 {gsub(/%/, "", $5); print $5}')"
    if [ -n "$use_percent" ] && [ "$use_percent" -ge "$HIGH_DISK_PERCENT" ]; then
        printf 'clean: warning: the filesystem holding this repo is %s%% full\n' \
            "$use_percent" >&2
    fi
fi

if [ "$FAILURES" -gt 0 ]; then
    printf 'clean: %d path(s) could not be removed\n' "$FAILURES" >&2
    exit 1
fi
