#!/usr/bin/env bash
# FVS-H-1 — the policy-archive bake, scheduled and time-boxed.
#
# Sizing is MEASURED, not estimated: the 2026-07-28 smoke run did 32 genomes/island in ~45 min under
# 24-way island contention => ~1.4 min/genome/island. 26 generations x batch 8 = 208 genomes/island
# => ~4.9 h. Patience-8 early-stop may finish it sooner; it cannot make it longer.
#
# ⚠️ TWO FLAG BUGS FIXED 2026-07-31, both found by pre-flighting the binary before scheduling a night
#    on it. Neither would have surfaced until the job fired:
#
#    1. `--no-apply` DOES NOT EXIST on `train rl`. It belongs to `train all` (which refuses to start
#       without it). `rl` takes the opposite polarity — `--apply` is opt-in — so the safe behaviour
#       this script wants is simply *omitting* the flag, which is the default. As written the run
#       died instantly with "unexpected argument '--no-apply'", i.e. the scheduled night produced
#       nothing at all.
#    2. `--islands` was missing, so it defaulted to **1**. Every sizing number in the comment above is
#       per-island under 24-way fan-out; a single-process run is a different job with a different
#       runtime and one archive instead of 24. `islands_out/elites_rl_{1..24}.ron` from the smoke run
#       is the evidence it was meant to be 24.
#
# Without `--apply` this never touches config.ron, the goldens, or the TRACKED elites_levels.ron
#   (FVS-N-12). The winner is copied to assets/config/elites_policy.candidate.ron (gitignored) and
#   ships via FVS_POLICY_ELITE, so nothing needs baking for H-1's acceptance.
# Expect `baseline_prior.ron` to auto-re-sweep first (~1 min): `ensure_prior_fresh` is mtime-driven
#   and config.ron is newer. Harmless and expected; the backlog's H-1 entry predicts it.
# timeout: a hard ceiling well inside the budget, so an unexpected slowdown cannot run into the
#   morning. If it fires, the per-island archives in islands_out/ still exist to inspect.
set -uo pipefail
cd /home/ladvien/foundation_vs_slop

START_AT="${1:?usage: scheduled_rl_bake.sh HH:MM}"
ISLANDS="${2:-24}"
LOG="/home/ladvien/foundation_vs_slop/rl-bake-$(date +%Y-%m-%d).log"

# Fail loudly NOW rather than at 23:30: a missing binary is the other way this job silently produces
# nothing, and `train` needs the `test-harness` feature to build at all.
if [ ! -x ./target/release/train ]; then
  echo "scheduled_rl_bake: ./target/release/train is missing." >&2
  echo "  build it first: cargo build --release --features test-harness --bin train" >&2
  exit 1
fi

target=$(date -d "today $START_AT" +%s)
now=$(date +%s)
[ "$target" -le "$now" ] && target=$(date -d "tomorrow $START_AT" +%s)

{
  echo "=== FVS-H-1 policy bake SCHEDULED at $(date '+%F %T') ==="
  echo "    fires $(date -d "@$target" '+%F %T')  (in $(( (target - now) / 60 )) min)"
  echo "    26 generations x batch 8 across $ISLANDS islands (~208 genomes/island, projected ~4.9 h)"
  echo "    hard ceiling 6 h; no --apply, so config.ron and the goldens are untouched"
  echo "    binary: $(date -r ./target/release/train '+%F %T')  ·  HEAD $(git rev-parse --short HEAD)"
  echo
} >> "$LOG"

sleep $(( target - now ))

echo "=== starting $(date '+%F %T') ===" >> "$LOG"

timeout 6h ./target/release/train rl \
    --generations 26 --batch 8 --islands "$ISLANDS" --no-progress >> "$LOG" 2>&1
code=$?

{
  echo
  if [ "$code" -eq 124 ]; then
    echo "=== HIT THE 6 h CEILING at $(date '+%F %T') — killed, per-island archives remain in islands_out/ ==="
  else
    echo "=== finished $(date '+%F %T') exit=$code ==="
  fi
} >> "$LOG"
