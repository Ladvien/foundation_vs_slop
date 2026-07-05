# Dev Journal: 2026-07-05 - Emergent AI Systemic Layer

**Session Duration:** ~1 long session (research + design + full phased build)
**Walkthrough:** None

## What We Did

Designed and built a **layered emergent-AI systemic layer** (`src/ai/`) so complex stories emerge on
their own from interacting systems, rather than being scripted. The design was researched heavily
against the home-still corpus first, then a plan was approved, then built in 7 verifiable phases.

**The architecture (three layers, each an open extension point):**
1. **Stigmergy fields** (`ai/field.rs`) — decaying scalar influence grids agents *write and read*
   (`SCENT`, `THREAT`, `CRAB_DENSITY`): deposit + evaporate + diffuse over the dungeon cell grid. This
   is the layer emergence actually comes from — creatures coordinate *through the environment*.
2. **Extensible drives** (`ai/drives.rs`) — needs (hunger/fear/fatigue/bloodlust/libido/crowding) as a
   data-defined `[f32; N]` array with pluggable `DriveRule`s, including a `Custom(fn)` escape hatch for
   esoteric extra-dimensional drives. The developer's headline ask: add a drive with one literal.
3. **Utility decisions** (`ai/utility.rs`) — dual-utility: behaviours score as a product of
   considerations (`Input × response Curve`), selected by rank bucket + weighted-random.
   Plus **brain glue** (`ai/brain.rs`): per-agent perception → decide → cached `ActiveBehavior`,
   throttled by a think-timer.

**Wired across all three actor types (wrap the decision, keep the mechanics):**
- **Smiley boss** — brain picks Chase / Wander / **HuntBlood** (drawn to the biggest blood frenzy via
  the SCENT field, pathing there around walls with a dedicated scent flow field).
- **Crabs** — brain picks Forage / Latch / **Flee**; the rank ordering *is* the emergent
  frenzy→scatter story (Flee > Latch > Forage).
- **Squad** — field writers only (gunfire → THREAT, blood → SCENT), player stays in control.
- **Reproduction/crowding** — feeding crabs breed (positive feedback), self-limited by a local
  CRAB_DENSITY cap + a global population cap.

Numeric-tuning knobs load from `ai_tuning.ron` (hot-reload, mirrors `vhs_fx.ron`). Gated diagnostics
(`ai/diag.rs`, `AI_DIAG`) log fields/drives/modes since the window can't be screenshotted.

**Research grounding (home-still corpus):** Dill "Dual-Utility Reasoning" (utility); "Context Steering"
Ch.18; Holland & Melhuish 1999 + Tang 2021 ACO (stigmergy); Lewis Ch.29 + Mark Ch.30 (influence maps);
Colledanchise & Ögren 2017 (composable-unit modularity).

## Bugs & Challenges

### Dual-utility "high-rank tiny-tail dominates"

**Symptom:** All 40 crabs were stuck in `Flee` even with `threat=0` and `fear=0` (and separately, the
boss chose `HuntBlood` with `scent=0`).

**Investigation:** Logged the decision inputs. The Flee behaviour is rank 2; its FEAR consideration is a
Logistic that returns ~0.011 at fear=0 (the curve's tail, not zero). The boss's HuntBlood Logistic did
the same (~0.018 at scent=0).

**Root Cause:** `decide` took the highest rank with any score `> 1e-4`. A high-rank behaviour's near-zero
curve *tail* still cleared that epsilon, so it always claimed its rank and dominated lower ranks.

**Solution:** A meaningful floor — `MIN_SCORE = 0.1`: a behaviour must score ≥0.1 to "turn on" and claim
its rank (Dill: *screen out low-weight options*). For the boss I also switched the HuntBlood gate to a
hard `Step` curve. After the fix: crabs forage at rest, flee only when FEAR crosses ~0.23.

**Lesson:** With rank-bucketed utility, "any positive score" is a trap — response-curve tails are never
exactly zero. Gate rank eligibility on a real threshold, or use `Step` for hard on/off behaviours.

### "Drawn to blood" that couldn't reach the blood

**Symptom:** The boss correctly switched to `HuntBlood` when a crab died, but `dist_to_hotspot` stayed
flat (~24 tiles) — it never closed on the frenzy.

**Root Cause:** HuntBlood steered *straight-line* toward the global scent hotspot; in a walled dungeon
that stalls (the boss only had wall-aware pathing for Chase, via its flow field).

**Solution:** A `ScentNav` resource — a `FlowField` seeded at the scent-hotspot cell (reusing
`FlowField::build_from`), rebuilt only when the hotspot moves cells (mirrors `EnemyField`). HuntBlood now
follows that field. Result: `dist_to_hotspot` shrank 29→10 as the boss pathed to the frenzy.

**Lesson:** "Move toward a point" in a walled world needs a nav field, not a direction vector. Reuse the
existing flow-field infra rather than straight-lining.

### Verifying emergence with the window occluded

**Symptom:** As in prior sessions, `devshot` screenshots come back black (occluded window).

**Solution:** Every phase ships a const-gated diagnostic system logging *objective numbers* — field
peaks, drive means, mode histograms, positional std-dev, population. Emergence was verified as signal
trajectories (e.g. gunfire → THREAT↑ → FEAR↑ → flee-count↑ → spread↑) rather than by eye.

**Lesson:** For emergent/AI work, numeric diagnostics are *better* than screenshots — precise, and they
prove causation across the pipeline.

### Flee getting stuck at surface-patch edges

**Symptom (anticipated):** The surface-nav patch transfer only follows the field's *gate* (toward the
squad). A fleeing crab moving the opposite way would hit its patch edge and clamp (stuck).

**Solution:** `crab_flee` uses *free* floor-cell transfer — re-home onto `graph.floor_patch_cell(cell)`
under the new position each step, so flight can head any direction; walls clamp it back.

**Lesson:** Goal-field pathing and free movement need different transfer logic; don't force flight
through the pursuit graph.

## Code Changes Summary

- `src/ai/{mod,field,drives,steering?,utility,brain,tuning,diag}.rs` (new): the whole framework.
  (Context steering was folded into per-creature bespoke movement, not a separate resolver — see Open
  Questions.)
- `src/util.rs` (new): consolidated the duplicated `rand01` LCG (was copied in enemy/laser) + `hash01`.
- `src/enemy.rs`: wrapped `enemy_seek` decision on `ActiveBehavior.mode` (Chase/Wander/HuntBlood),
  kept the momentum-charge + `resolve_move` tail verbatim; boss brain components at spawn; blood→SCENT.
- `src/crab.rs`: wrapped `crab_locomotion` on `active.mode` (Latch/Flee/Forage) with the piranha/surface
  blocks intact + new `crab_flee`; `deposit_crab_density`; `crab_reproduce` (+ `CrabAssets` resource);
  blood→SCENT; brain components at spawn.
- `src/laser.rs`: gunfire→THREAT deposits; `LASER_DAMAGE` restored to 0.2 (user's observation setting).
- `src/main.rs`: `mod util; mod ai;`, `AiPlugin` slotted into the `(ai, enemy, crab)` nested tuple.
- `ai_tuning.ron` (new): hot-tunable field rates.

## Patterns Learned

- **Wrap-the-decision, keep-the-mechanics**: to add a brain to hard-won movement code, move each motion
  block *intact* under a `match mode` arm and only substitute the mode selector. Zero regression risk.
- **Stigmergy substrate = the emergence multiplier**: cross-actor stories (crab blood → boss arrives →
  crabs scatter) happen *through shared fields*, with no pairwise "A-meets-B" code.
- **Index-newtype IDs + fn-pointer rules**: `FieldId(usize)`/`DriveId(usize)` over fixed arrays +
  `DriveRule::Custom(fn)` — extensible, cache-friendly, type-safe, alloc-free, one-path.
- **Rank-gated utility needs a MIN_SCORE**: never select on "score > 0".
- **Phased build with a numeric gate per phase**: each phase compiles + is provable in isolation.

## Open Questions

- **Context steering** (Ch.18) was *not* built as a separate resolver — mode→movement uses each
  creature's existing bespoke steering + a new flee. The decision/stigmergy layers are complete; a
  formal context-steering layer could slot in later without disturbing them.
- **RON tuning migration** is partial: field rates are in `ai_tuning.ron`; drive/curve/think numbers are
  still documented consts. Expanding `AiTuning` is straightforward follow-on.
- **Reproduction is modest under fire**: constant gunfire keeps crabs fearful/scattered → few latch →
  few breed. That's a reasonable self-balancing loop, but if we want visible swarms growing, tune the
  fear threshold or breeding rate.
- All verification was numeric (window occlusion) — the *feel* (drive rates, curve steepness,
  evaporation, ranks) needs a live playtest to dial in.

## Next Session

- Live playtest with `AI_DIAG = true` (and maybe guns back to 10.0 briefly) to tune emergence feel.
- Decide whether to add the formal context-steering layer.
- Finish the RON tuning migration (drives/curves/think interval) if hot-tuning those matters.
- Add a first *esoteric* extra-dimensional drive via `DriveRule::Custom` to exercise the extension path.
