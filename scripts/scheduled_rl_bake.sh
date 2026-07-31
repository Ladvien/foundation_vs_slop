#!/usr/bin/env bash
# FVS-H-1 — the policy-archive bake, scheduled and time-boxed.
#
# Sizing is MEASURED, not estimated: the 2026-07-28 smoke run did 32 genomes/island in ~45 min under
# 24-way island contention => ~1.4 min/genome/island. 26 generations x batch 8 = 208 genomes/island
# => ~4.9 h. Patience-8 early-stop may finish it sooner; it cannot make it longer.
#
# --no-apply: never touches config.ron, the goldens, or the TRACKED elites_levels.ron (FVS-N-12).
#   The policy archive ships via FVS_POLICY_ELITE, so nothing needs baking for H-1's acceptance.
# timeout: a hard ceiling well inside the 7 h budget, so an unexpected slowdown cannot run into
#   the morning. If it fires, the per-island archives in islands_out/ still exist to inspect.
set -uo pipefail
cd /home/ladvien/foundation_vs_slop

START_AT="${1:?usage: scheduled_rl_bake.sh HH:MM}"
LOG="/home/ladvien/foundation_vs_slop/rl-bake-$(date +%Y-%m-%d).log"

target=$(date -d "today $START_AT" +%s)
now=$(date +%s)
[ "$target" -le "$now" ] && target=$(date -d "tomorrow $START_AT" +%s)
sleep $(( target - now ))

{
  echo "=== FVS-H-1 policy bake starting $(date '+%F %T') ==="
  echo "    26 generations x batch 8 (~208 genomes/island, projected ~4.9 h)"
  echo "    hard ceiling 6 h; --no-apply so config.ron and the goldens are untouched"
  echo
} >> "$LOG"

timeout 6h ./target/release/train rl \
    --generations 26 --batch 8 --no-apply --no-progress >> "$LOG" 2>&1
code=$?

{
  echo
  if [ "$code" -eq 124 ]; then
    echo "=== HIT THE 6 h CEILING at $(date '+%F %T') — killed, per-island archives remain in islands_out/ ==="
  else
    echo "=== finished $(date '+%F %T') exit=$code ==="
  fi
} >> "$LOG"
