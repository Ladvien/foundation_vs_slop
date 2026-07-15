#!/usr/bin/env bash
# tune.sh — thin shim over the `train` CLI.
#
# All the orchestration that used to live here (island fan-out, apply, prior refresh, dim mapping, winner
# selection, progress) now lives IN the Rust binary. This shim only guarantees a build from the CURRENT
# source (via `cargo run`, a near-no-op when nothing changed) and forwards every argument to `train`.
#
# So instead of `GENERATIONS=60 ISLANDS=12 APPLY=1 ./tune.sh behavior`, use train's flags directly:
#
#   ./tune.sh behavior --generations 60 --islands 12 --apply
#   ./tune.sh evolve3  --generations 30 --jobs 3 --apply
#   ./tune.sh --help
#   ./tune.sh behavior --help
#
# Equivalent to (and interchangeable with):
#   cargo run --release --features test-harness --bin train -- behavior --islands 12 --apply
set -euo pipefail
cd "$(dirname "$0")"
exec cargo run --release --features test-harness --bin train -- "$@"
