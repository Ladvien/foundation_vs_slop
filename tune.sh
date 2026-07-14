#!/usr/bin/env bash
# tune.sh — run an offline search that AUTO-EVOLVES the game's tuning, then show how to apply it.
#
# The search mutates → evaluates headlessly → selects by fitness (witnessed-learnable-surprise) →
# fills a MAP-Elites archive, checkpointing every generation. That IS the automatic adjustment; the
# only manual step is choosing the winner and applying it (this script prints/does that too).
#
# Usage:
#   ./tune.sh <dim> [extra `train` flags…]
#     dim = behavior | audio | levels | rl | poet | evolve3
#
# Env overrides (all optional):
#   GENERATIONS=30  BATCH=16  TICKS=1800  SEEDS=0x5C09191,0xA11CE,0xBEEF,0xF00D,0x1CE
#   JOBS=3          # evolve3 only (capped at 3 by the archive read-after-write dependency)
#   CMA=1           # rl only — use the CMA-ME adaptive emitter
#   APPLY=1         # after the search, PERMANENTLY bake the best elite (behavior/audio/levels/evolve3→world)
#   REBUILD=1       # force a rebuild of the trainer
#
# Examples:
#   ./tune.sh behavior                       # evolve creature/squad tuning
#   GENERATIONS=60 ./tune.sh behavior        # longer run
#   CMA=1 ./tune.sh rl                        # neuroevolve a squad policy (CMA-ME)
#   APPLY=1 ./tune.sh behavior                # evolve AND ship it as the default in one go
set -euo pipefail
cd "$(dirname "$0")"

DIM="${1:-behavior}"; shift || true
GENERATIONS="${GENERATIONS:-30}"
BATCH="${BATCH:-16}"
TICKS="${TICKS:-1800}"
SEEDS="${SEEDS:-0x5C09191,0xA11CE,0xBEEF,0xF00D,0x1CE}"
JOBS="${JOBS:-3}"
CMA="${CMA:-0}"
APPLY="${APPLY:-0}"
REBUILD="${REBUILD:-0}"
ISLANDS="${ISLANDS:-1}"        # run N parallel searches (distinct seeds), then pick the best elite across them
BIN=./target/release/train
ODIR=islands_out              # where island archives + logs land

# The headless search boots the real game with no renderer, so Bevy logs a flood of harmless
# "asset not found" ERRORs (.glb/.ogg/.png it never needs). Quiet those at the source, and grep out the
# rest — keeping the search's own stdout (the `gen N:` progress, `wrote …`, and any real error/panic).
export RUST_LOG="${RUST_LOG:-warn,bevy_asset=off,bevy_render=off,bevy_gltf=off,bevy_gizmos=off,wgpu=off,naga=off}"
NOISE='Path not found|bevy_(asset|render|gltf|gizmos|log|app::terminal)|CompressedImageFormat|RenderApp was not detected|SystemInfo|Could not set global logger|surface graph built|spawned .* crabs|seeded .* (SCP|mancae)|no autogib bake|Skipping installing'

# Run a command, drop the harmless Bevy noise, keep progress + real errors, and PRESERVE its exit code.
run_quiet() {
  set +e
  "$@" 2>&1 | grep --line-buffered -vE "$NOISE"
  local rc=${PIPESTATUS[0]}
  set -e
  return "$rc"
}

# ISLAND MODEL — the only way to use many cores. One search is single-threaded by design (determinism),
# so N cores == N independent searches at once: same objective (same eval worlds via --seeds), distinct
# search trajectories (distinct --seed), then take the single best-fitness elite across all their archives.
# You get N× the exploration in ~one search's wall-clock. This box has 12 PHYSICAL cores — 12 islands run
# at full speed; >12 oversubscribes the 24 SMT threads (~1.3× slower each) for a little more coverage.
run_islands() {
  case "$DIM" in
    behavior) DIMKEY=behavior ;; audio) DIMKEY=audio ;; levels) DIMKEY=levels ;;
    rl) DIMKEY=policy ;; poet) DIMKEY= ;;
    *) echo "ISLANDS supports behavior|audio|levels|rl|poet (evolve3 writes fixed paths — run it alone)" >&2; exit 2 ;;
  esac
  mkdir -p "$ODIR"
  local extra=(); [[ "$DIM" == rl && "$CMA" == 1 ]] && extra=(--cma)
  echo ">> launching $ISLANDS parallel '$DIM' islands → $ODIR/  (12 physical cores; >12 oversubscribes)"
  local pids=() i seed out
  for i in $(seq 1 "$ISLANDS"); do
    seed=$(( (i * 2654435761) & 0x7FFFFFFF ))   # distinct per-island search seed (Knuth multiplicative)
    out="$ODIR/elites_${DIM}_${i}.ron"
    "$BIN" "$DIM" "${extra[@]}" "${common[@]}" --seed "$seed" --out "$out" \
      > "$ODIR/${DIM}_${i}.log" 2>&1 &
    pids+=($!)
  done
  echo ">> ${#pids[@]} searches running (per-island logs in $ODIR/). watch one:"
  echo "     tail -f $ODIR/${DIM}_1.log | grep -E 'gen '"
  local fail=0 p
  for p in "${pids[@]}"; do wait "$p" || fail=$((fail + 1)); done
  echo ">> all islands finished ($fail failed). scanning for the best elite across archives…"
  local best_file="" best_fit="-1e30" m
  for out in "$ODIR"/elites_"${DIM}"_*.ron; do
    [[ -f "$out" ]] || continue
    m=$(grep -oE 'fitness: *[-0-9.eE]+' "$out" | awk '{print $2}' | sort -g | tail -1)
    if [[ -z "$m" ]]; then echo "   $(basename "$out"): (no elites — see its log)"; continue; fi
    printf '   %-30s best fitness %s\n' "$(basename "$out")" "$m"
    if awk "BEGIN{exit !($m > $best_fit)}"; then best_fit="$m"; best_file="$out"; fi
  done
  [[ -n "$best_file" ]] || { echo ">> no archives produced — inspect $ODIR/*.log" >&2; exit 1; }
  echo
  echo ">> WINNER: $best_file  (fitness $best_fit)"
  case "$DIMKEY" in
    behavior|audio|levels)
      echo "   try it live:  FVS_${DIMKEY^^}_ELITE=$best_file cargo run --release --bin foundation_vs_slop"
      if [[ "$APPLY" == 1 ]]; then
        echo ">> APPLY=1 → baking the winner into the shipped defaults…"
        run_quiet "$BIN" apply "$DIMKEY" "$best_file"
        echo ">> regenerating prior for the new tuning…"
        run_quiet "$BIN" prior --ticks "$TICKS" --seeds "$SEEDS"
        echo ">> baked. review with 'git diff'; verify with 'cargo test --features test-harness'."
      else
        echo "   ship it:      $BIN apply $DIMKEY $best_file  &&  $BIN prior"
        echo "   (or re-run with APPLY=1 to bake the winner automatically.)"
      fi ;;
    policy)
      echo "   try it live:  FVS_POLICY_ELITE=$best_file cargo run --release --bin foundation_vs_slop"
      echo "   (a learned policy has no permanent bake — run it via the env var.)" ;;
    *)
      echo "   ('$DIM' holds niches for analysis; inspect $best_file)" ;;
  esac
}

# 1. Build the trainer once (release = fast rollouts).
if [[ "$REBUILD" == 1 || ! -x "$BIN" ]]; then
  echo ">> building train (release, test-harness)…"
  cargo build --release --bin train --features test-harness
fi

# 2. Freeze the baseline expectation. Surprise-based searches need it; `levels` (static objective) doesn't.
if [[ "$DIM" != levels && ! -f assets/config/baseline_prior.ron ]]; then
  echo ">> generating baseline_prior.ron…"
  run_quiet "$BIN" prior --ticks "$TICKS" --seeds "$SEEDS"
fi

# 3. Run the search. `common` is the shared flag set; `$@` appends anything extra you pass.
common=(--generations "$GENERATIONS" --batch "$BATCH" --ticks "$TICKS" --seeds "$SEEDS")

# ISLANDS>1 → fan the search across cores, pick the best elite, and stop (skips the single-run path below).
if [[ "$ISLANDS" -gt 1 ]]; then
  run_islands
  exit 0
fi

echo ">> evolving '$DIM'  (generations=$GENERATIONS batch=$BATCH ticks=$TICKS)…"
case "$DIM" in
  behavior) cmd=("$BIN" behavior "${common[@]}" "$@"); ARCH=assets/config/elites_behavior.ron; DIMKEY=behavior ;;
  audio)    cmd=("$BIN" audio    "${common[@]}" "$@"); ARCH=assets/config/elites_audio.ron;    DIMKEY=audio ;;
  levels)   cmd=("$BIN" levels   "${common[@]}" "$@"); ARCH=assets/config/elites_levels.ron;   DIMKEY=levels ;;
  poet)     cmd=("$BIN" poet     "${common[@]}" "$@"); ARCH=assets/config/elites_poet.ron;     DIMKEY= ;;
  rl)       CMAFLAG=(); [[ "$CMA" == 1 ]] && CMAFLAG=(--cma)
            cmd=("$BIN" rl "${CMAFLAG[@]}" "${common[@]}" "$@"); ARCH=assets/config/elites_policy.ron; DIMKEY=policy ;;
  evolve3)  cmd=("$BIN" evolve3 "${common[@]}" --jobs "$JOBS" "$@"); ARCH=assets/config/elites_world.ron; DIMKEY=world ;;
  *) echo "unknown dim '$DIM' (behavior|audio|levels|rl|poet|evolve3)" >&2; exit 2 ;;
esac
run_quiet "${cmd[@]}"

echo
echo ">> done. archive: $ARCH"

# 4. How to use the result.
case "$DIMKEY" in
  behavior|audio|levels|world)
    ENV="FVS_${DIMKEY^^}_ELITE"
    echo "   try it live:  $ENV=$ARCH cargo run --release --bin foundation_vs_slop"
    echo "   ship it:      $BIN apply $DIMKEY $ARCH  &&  $BIN prior"
    if [[ "$APPLY" == 1 ]]; then
      echo ">> APPLY=1 → baking '$DIMKEY' into the shipped defaults…"
      run_quiet "$BIN" apply "$DIMKEY" "$ARCH"
      echo ">> regenerating prior for the new tuning…"
      run_quiet "$BIN" prior --ticks "$TICKS" --seeds "$SEEDS"
      echo ">> baked. review with 'git diff'; verify with 'cargo test --features test-harness'."
    fi
    ;;
  policy)
    echo "   try it live:  FVS_POLICY_ELITE=$ARCH cargo run --release --bin foundation_vs_slop"
    echo "   (a learned policy is not a config slice, so there's no permanent bake — run it via the env var.)"
    ;;
  *)
    echo "   ('$DIM' produces niches for analysis; extract a world/agent to apply, or inspect $ARCH.)"
    ;;
esac
