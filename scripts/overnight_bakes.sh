#!/usr/bin/env bash
# Overnight QD/RL chain — everything that fits before a hard 06:50 wall.
#
# WHY A CHAIN AND NOT `train all`: `all` is measured at **12-20 h** on this box (the last real run:
# rl 5 h 38 m across 12 islands, audio ~4 h, levels ~74 s). Nine hours does not hold it, and `all`
# runs rl LAST — so an overrun would drop precisely the archive FVS-H-1's acceptance is about
# ("retrained archive loads at current MODE_COUNT"). Ordering by value instead:
#
#   1. rl      — the policy archive. THE blocker: FVS-H-1's acceptance, and FVS-K-4's endgame
#                manifestation is sequenced behind it. Gets the largest slice.
#   2. levels  — ~74 s, so it is free. Refreshes the archive the difficulty director samples, which
#                as of tonight's FVS-H-8 fix actually reaches the world.
#   3. audio   — stale for an independent reason (`audio_genome::N` grew 15 -> 16 for SCP-610's
#                drone). Takes whatever is left.
#
# No `--apply` anywhere. On `rl` that is the DEFAULT (unlike `train all`, which refuses to start
# without an explicit `--no-apply`) — so config.ron, the goldens and the tracked elites are untouched
# and every winner lands as a `.candidate.ron` for a human to review. Nothing here can change the
# game while nobody is watching.
#
# Each stage is hard-capped by `timeout`, and the caps are sized so the chain cannot run past the
# wall even if every stage overruns. A killed stage still leaves its per-island archives in
# islands_out/ to inspect.
set -uo pipefail
cd /home/ladvien/foundation_vs_slop

LOG="/home/ladvien/foundation_vs_slop/overnight-bakes-$(date +%Y-%m-%d).log"
ISLANDS=24            # = nproc on this box; matches the smoke run the sizing came from
WALL="06:50"

if [ ! -x ./target/release/train ]; then
  echo "overnight_bakes: ./target/release/train missing — cargo build --release --features test-harness --bin train" >&2
  exit 1
fi

secs_until() {  # seconds from now until HH:MM today, or tomorrow if already past
  local t; t=$(date -d "today $1" +%s)
  [ "$t" -le "$(date +%s)" ] && t=$(date -d "tomorrow $1" +%s)
  echo $(( t - $(date +%s) ))
}

stage() { # name, cap_seconds, args...
  local name="$1" cap="$2"; shift 2
  if [ "$cap" -le 60 ]; then
    echo "=== SKIP $name — only ${cap}s left before the $WALL wall ===" >> "$LOG"
    return
  fi
  {
    echo
    echo "=== STAGE $name  start $(date '+%F %T')  cap $((cap/60)) min ==="
  } >> "$LOG"
  timeout "${cap}s" ./target/release/train "$name" --islands "$ISLANDS" --no-progress "$@" >> "$LOG" 2>&1
  local code=$?
  if [ "$code" -eq 124 ]; then
    echo "=== $name HIT ITS CAP at $(date '+%F %T') — per-island archives remain in islands_out/ ===" >> "$LOG"
  else
    echo "=== $name finished $(date '+%F %T') exit=$code ===" >> "$LOG"
  fi
}

{
  echo "=== OVERNIGHT CHAIN starting $(date '+%F %T') ==="
  echo "    wall $WALL  ·  $ISLANDS islands  ·  HEAD $(git rev-parse --short HEAD)"
  echo "    binary $(date -r ./target/release/train '+%F %T')"
  echo "    no --apply on any stage: winners land as .candidate.ron for review"
} >> "$LOG"

# rl takes the lion's share but is capped so levels+audio still get a window.
RL_CAP=$(( $(secs_until "$WALL") - 4*3600 ))     # leave 4 h for the rest
[ "$RL_CAP" -gt 19800 ] && RL_CAP=19800          # and never more than 5 h 30 m
stage rl "$RL_CAP" --generations 26 --batch 8

stage levels 900 --generations 24 --batch 12     # ~74 s historically; 15 min is pure headroom

stage audio "$(secs_until "$WALL")" --generations 20 --batch 8

{
  echo
  echo "=== CHAIN DONE $(date '+%F %T') ==="
  echo "Candidates written (none applied):"
  ls -la assets/config/*.candidate.ron 2>/dev/null || echo "  (none)"
} >> "$LOG"
