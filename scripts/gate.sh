#!/usr/bin/env bash
# Gate for the Rivet repository: run every quality gate in order and stop
# at the first failure. Exit 0 when all pass, 1 on the first failure.
#
# Usage:
#   scripts/gate.sh
#
# Run the script from any directory; it resolves the repository root from
# its own location. Each stage prints a header and its command output.
# A stage failure prints the failing command and exits 1 immediately.
#
# Stages (in order):
#   1. cargo fmt --all -- --check
#   2. cargo clippy -- -D warnings
#   3. cargo test --workspace
#   4. cargo deny check
#   5. Example build and audit (rivet build + rivet audit --json)
#   6. Repository self-checks (constraint-tools binaries)
set -euo pipefail

# Resolve the repository root: scripts/gate.sh -> repo root.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

run_stage() {
    local name="$1"
    shift
    echo
    echo "== [gate] $name =="
    if "$@"; then
        echo "== [gate] $name: PASS =="
    else
        echo "== [gate] $name: FAIL ==" >&2
        exit 1
    fi
}

run_stage "fmt" cargo fmt --all -- --check
run_stage "clippy" cargo clippy -- -D warnings
run_stage "test" cargo test --workspace
run_stage "deny" cargo deny check

# Build the CLI once, then run the example through the real pipeline.
run_stage "example build" cargo build --release --bin rivet
run_stage "example rivet build" ./target/release/rivet build examples/basic/app.py
run_stage "example rivet audit" ./target/release/rivet audit --json examples/basic/app.py

# Self-checks run against the repository source tree.
run_stage "self-checks build" cargo build -p constraint-tools
run_stage "check-file-length" ./target/debug/check-file-length
run_stage "check-rule-modules" ./target/debug/check-rule-modules
run_stage "check-tracker" ./target/debug/check-tracker

echo
echo "gate: all checks passed"
