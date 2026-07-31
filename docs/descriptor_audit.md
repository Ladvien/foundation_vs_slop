# FVS-I-6 — descriptor audit

**Run 2026-07-30.** The gate FVS-I-7…I-10 sit behind. The rule it enforces (FVS-N-21): *adding a knob no
descriptor can see makes the archive worse, not better* — two genomes differing only in that knob land in
the same cell, and the winner is decided by evaluation luck.

This is a **static** audit: it traces each knob group to the archive that would score it and asks whether a
causal path to a descriptor axis exists. It does not run the searches. Where a claim needs measurement to
settle, it says so rather than guessing.

---

## 1. The descriptor inventory

Every population and the axes its archive actually bins on:

| Genome | `N` | Archive | Descriptor axes |
|---|---|---|---|
| `world_genome` | 138 | `elites_world.ron` | **deaths × lives** (`world_descriptor`, normalised — `train.rs:1172`) |
| `level_genome` | — | level archive | **clutter × infestation** (`LevelMetrics::descriptor_axes`) — furniture/room ÷ 8, mould ÷ 0.5 |
| `behavior_genome` | 89 | `elites_behavior.ron` | **swarm** aggression × persistence (`swarm_descriptor`, `behavior_eval.rs:28`) |
| `audio_genome` | 16 | `elites_audio.ron` | **swarm** aggression × persistence — the *same* axes (`audio_eval.rs:25`); measured at 3/64 cells occupied, 2026-07-28 |
| `policy_genome` | — | RL archive | **initiative × caretaking** (`rl_eval::policy_descriptor`) |
| squad / swarm (coevolve) | — | `elites_squad/swarm.ron` | aggression × exploration / aggression × persistence |

**One correction to a standing assumption.** The world genome is *not* binned by
`qd::BehaviorDescriptor` (combat_share × exploration). In the POET path (`train.rs:1552`) it is scored by
`surprise::fitness` + `Interest` under a minimal criterion, and in the archive path by
`world_descriptor` = deaths × lives. Anyone reasoning about a world knob against combat-share is reasoning
about the wrong grid.

---

## 2. Findings per item

### FVS-I-10 — swarm cadence · **STALE. Already evolved. Close it.**

The strongest finding here, and it removes an M-sized item plus a genome-length change and its archive
re-bake debt.

`world_genome` already decodes both structs the item names:

- `BreedingTuning` (7 knobs) at `world_genome.rs:452` — including **`respawn_interval`, the nest breed
  rate limiter**, plus `meat_per_crab`, `feed_gain`, `spawn_boost_max`, `spawn_boost_decay`,
  `hunger_rate`, `hunger_sate_rate`.
- `ParasiteTuning` (14 knobs) at `world_genome.rs:490` — including **`initial_count`** and
  **`manca_count_max`**, i.e. swarm population directly.

That *is* "spawn/breed cadence". The `world_genome` header already documents both slices. The item's
premise — "the search cannot touch it" — is false as written.

*Recommended:* close as already-done, the same staleness class as FVS-A-4/O-2/F-2 in the archive. If some
specific cadence knob is genuinely missing, re-file it naming that knob rather than the whole group.

### FVS-I-9 — perception · **Real, but the item points at the wrong file.**

`src/ai/tuning.rs` — the file the item names — is **fully encoded already**: it is `AiTuning`, the 27
field-propagation knobs at the head of `world_genome`'s `BOUNDS`.

The genuinely unevolved knobs are in **`src/behavior_tuning.rs::PerceptionTuning`**, and
`behavior_genome` encodes only **2 of its 13 fields** (`leash`, `squad_think_interval` —
`behavior_genome.rs:108,151`). Unevolved, 11 knobs:

`examine_sight`, `examine_sight_release`, `threat_sight`, `threat_sight_release`, `psi_sight`,
`psi_sight_release`, `ward_sight`, `ward_sight_release`, `wounded_frac`, `wounded_frac_release`,
`leash_in`.

*Descriptor path — and this is the catch.* The behaviour archive bins on **`swarm_descriptor`**, i.e. the
aggression and persistence of **the swarm** (`behavior_eval.rs:28`). But these are **squad** perception
knobs. So the path is indirect: squad sight → squad engages sooner → crabs respond → swarm aggression
shifts. Real, but second-order — and second-order is precisely how N-21 happens.

**There is already evidence this descriptor does not spread for non-swarm knobs.** `audio_genome` is binned
on the *same* `swarm_descriptor` axes, and the one bake that has ever landed occupied **3 of 64 cells**
(measured 2026-07-28). Audio at least has a stated rationale — "the swarm is what reacts to the din"
(`audio_eval.rs:25`). Squad sight radius has a weaker one. Two populations sharing one descriptor, and the
one we have data for is nearly collapsed, is not a good prior for adding 11 more knobs to it.

*Also, a decode constraint regardless of the above:* every `_sight`/`_sight_release` pair is a Schmitt
trigger. They must be encoded as a band with `release ≥ sight` enforced in `decode`, or the search will
produce chattering perception that no descriptor could explain.

*Recommended:* keep, rescope to `src/behavior_tuning.rs`, correct the *Touches* line — **and settle the
descriptor question first**, because on current evidence I-9 is the second-most likely of the four to
reproduce N-21, not the safest. The cheap check is an ablation: mutate only these 11 knobs and measure
whether `swarm_descriptor` spreads at all.

### FVS-I-8 — `MetropolisWeights` · **Fails the audit as scoped.**

15 knobs (`placement/solvers/metropolis.rs:45`), and every one is either a **sampler setting**
(`iterations`, `temp_start`, `temp_end`, `translate_sigma`, `rotate_prob`) or an **arrangement-quality
weight** (`w_overlap`, `w_bounds`, `w_wall`, `w_min_distance`, `w_facing`, `w_clearance`, `w_hard`,
`w_wall_angle`, `w_group`, `coherence`).

The level archive bins on **clutter × infestation**. Clutter is `furniture_per_room` — a *count*, decided
upstream by the placement grammar. Metropolis decides *where* pieces go, never *how many*. Infestation is
mould, unrelated. **So all 15 knobs move neither axis.** This is FVS-N-21 exactly, at 15× the size — and
the backlog already predicted it ("the one most likely to fail I-6's audit").

*Recommended:* **do not encode as scoped.** Three ways forward, and this is a design call, not mine:
1. **Remove** — leave arrangement authored. Cheapest; loses nothing the archive can currently reward.
2. **Add an axis** — an "arrangement coherence" descriptor would make these knobs legible. But a
   descriptor change is exactly the decision the QD-degeneracy watchlist says is yours, and it invalidates
   the level archive.
3. **Couple** — encode only `coherence`, the one knob with a plausible *perceptible* effect, and accept it
   rides fitness rather than a descriptor axis.

### FVS-I-7 — gore · **Real, and it passes — but only for a subset.**

Confirmed: gore appears in **no genome** (no `gore`/`Gore` reference in any `*genome*.rs`).

`GoreSettings` (`gore.rs:453`) is ~30 knobs, and **most are purely cosmetic** — `spray_color_a/b`,
`pool_color`, `spray_quad_size`, `pool_gloss`, `dry_time`, `wall_splat_size` and friends. Encoding those
is textbook N-21: no descriptor axis, no reliable fitness effect, pure archive noise.

The sim-relevant subset has a **direct** path to the world archive's `deaths` axis, which is why the item
records that "a gore knob already tipped a 5/5 win into a wipe": `max_gibs`, `chunk_restitution`,
`gib_friction`, `autogib_pieces_base`, `autogib_min_pieces`, `autogib_max_pieces`, `autogib_speed_mult`,
`meat_count` — physics-chunk population and how it behaves.

*Recommended:* encode **only** the sim-relevant subset into `world_genome`, and say in the code why the
cosmetic knobs are excluded, so nobody "completes" the group later. Deaths × lives is a real path, so this
one is legible to its archive.

---

## 3. A refinement to N-21's framing

N-21 offers three outcomes — remove / add-axis / couple. The audit suggests a fourth question worth asking
first, because it separates two failures that look identical:

> Does the knob move a **descriptor axis**, and separately, does it move **fitness** reliably?

- **Neither** → genuine N-21 poison. Remove. *(the cosmetic gore knobs; most of `MetropolisWeights`)*
- **Fitness only** → not poison. MAP-Elites optimises it *within* a cell, so the winner is skill, not luck.
  It is an intensity dial the archive cannot spread on — acceptable, if recorded as such.
- **Descriptor too** → ideal. *(the sim-relevant gore subset; probably perception)*

The pathology the policy-archive collapse actually exhibited was *neither* — descriptors constant across
feasible policies. A knob that reliably moves fitness alone is not that failure, and treating it as one
would strip real dials out of the search.

---

## 4. What this audit did **not** settle

Stated plainly so nobody reads the table above as complete:

1. **No measurement was taken.** Every "moves an axis" claim is a code-path argument. The honest test is an
   ablation: mutate only that knob group, measure descriptor spread. That is a search run, not a read.
2. **Two populations share one descriptor and nobody decided that.** `behavior_genome` (89 knobs, squad
   tuning) and `audio_genome` (16 knobs, acoustics) are both binned on `swarm_descriptor`. Whether that is
   right for *either* is a live question this audit raises and does not answer — and it is a descriptor
   decision, so it is yours. The 3/64-cell audio bake is the only evidence available and it is bad.
3. **`world_descriptor` = deaths × lives may itself be narrow.** Both axes are outcome counts, so any knob
   that changes *how* a run reaches the same body count is invisible to it. Not in scope here, but it is
   the same question one level up, and it bears on I-7's subset.

---

## 5. Net effect on the queue

| Item | Before | After this audit |
|---|---|---|
| I-10 | blocked, M | **close as stale** — already encoded |
| I-9 | blocked, M | keep; rescope to `behavior_tuning.rs`, 11 knobs, pair the hysteresis bands — **but settle the swarm-vs-squad descriptor question first** |
| I-8 | blocked, L | **do not encode as scoped** — 15 knobs, 0 axes; needs your remove/add-axis/couple call |
| I-7 | blocked, M | keep, but encode the sim-relevant subset only (~8 of ~30 knobs) |

Two of four shrink or disappear, and the genome-length debt that worried H-1 shrinks with them.
