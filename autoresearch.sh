#!/usr/bin/env bash
#
# The carnage frame-cost benchmark, as one command.
#
#   METRIC carnage_frame_ms   <- the primary metric. Mean milliseconds of carnage work per simulated
#                                tick, median of 5 timed reps. LOWER IS BETTER, and the point of
#                                lowering it is to afford more gore per frame, not less of it.
#
# The workload is a scripted massacre over the deterministic half of `bevy_carnage`: sixteen two-shell
# bodies die on fixed ticks, each taking two bullet channels and a radial blast, and every wound they
# open then bleeds on its own heartbeat until it clots. 600 ticks at 60 Hz. No window, no GPU, no
# `App`, no clock feeding any decision — see `crates/bevy_carnage/examples/bench_carnage.rs`.
#
# **The benchmark refuses to report a timing if the carnage changed.** Every fragment centre, wound,
# stain and droplet is folded into an FNV-1a digest in a separate untimed pass and checked against a
# golden, and the eight output counts are checked field by field. That gate is the whole reason a pure
# performance metric is trustworthy here: without it, the cheapest way to win is to throw less blood,
# and this harness would be an engine for making the game drier. A moved digest exits non-zero and
# prints a copy-pasteable re-bless block.
#
# Exit codes: 0 = measured and comparable. Non-zero = build failed, or the carnage moved.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

# Build first, and let a compile failure fail the harness. Without this the run below could silently
# measure a stale binary from a previous iteration, which is the worst failure mode a benchmark has:
# it reports a number, and the number is about code that no longer exists.
cargo build --release -p bevy_carnage --example bench_carnage >&2

# `.cargo/config.toml` redirects `build.target-dir` to a shared directory, so the path cannot be
# assumed to be `./target`. Ask Cargo instead of guessing.
target_dir="$(cargo metadata --format-version 1 --no-deps | jq -r '.target_directory')"
bench="${target_dir}/release/examples/bench_carnage"

if [[ ! -x "${bench}" ]]; then
    echo "autoresearch.sh: built the example but found no binary at ${bench}" >&2
    exit 1
fi

# Run the binary directly rather than through `cargo run`: no dependency re-resolution in the middle
# of a measurement, and nothing of Cargo's on stdout next to the METRIC lines.
exec "${bench}"
