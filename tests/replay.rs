//! Full-sim replay + repeatability (feature `test-harness`). Only compiled with the harness feature.
//!
//! Two oracles at two altitudes (the vetted split — Ostrowski & Aroudj 2013; Bécares 2017; and the
//! "unstable oracle" caveat, Kato et al. 2026):
//!   * **Deterministic gameplay core** (Avian solver OFF) → **exact same-seed hash**. This is the
//!     repeatability guarantee for the game LOGIC: AI, movement, combat, economy.
//!   * **Full sim** (physics ON) → **liveness oracle** (no panic / NaN / out-of-range health / runaway
//!     spawn). Avian's float solver is not bit-reproducible (a documented invariant), so exact hashing
//!     is the wrong tool there; liveness degrades gracefully instead.
//!
//! Runs the real game plugins headless (no window). Each test holds `serial_guard()` for the whole App
//! lifetime — two headless Apps must not run concurrently (shared global task pool + GPU device).
#![cfg(feature = "test-harness")]

use foundation_vs_slop::sim_harness::{
    build_headless_app, field_hash, liveness_violations, serial_guard, snapshot_hash, step, SimConfig,
};

#[test]
fn headless_app_boots_and_steps_without_panicking() {
    let _serial = serial_guard();
    let cfg = SimConfig::default();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 10);
    assert_ne!(snapshot_hash(&mut app), 0, "a booted, stepped sim must have non-trivial state");
}

#[test]
fn deterministic_core_is_bit_identical() {
    // THE repeatability proof. The gameplay LOGIC (physics OFF) is bit-reproducible: two independent
    // same-seed runs, stepped the same fixed ticks, hash identically. This is the direct answer to
    // "is everything repeatable from the same seed?" — yes, for everything the solver doesn't touch.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();

    let mut a = build_headless_app(&cfg);
    step(&mut a, &cfg, 180); // ~3 s: dungeon gen, spawns, AI think, movement, combat, economy
    let ha = snapshot_hash(&mut a);
    drop(a);

    let mut b = build_headless_app(&cfg);
    step(&mut b, &cfg, 180);
    let hb = snapshot_hash(&mut b);

    assert_eq!(ha, hb, "physics-free core must be bit-identical across same-seed runs");
}

// Phase-1 byte-identity gate for the const→config (`SimTuning`) migration. Promoting the combat /
// economy / deposit / fear / boss numbers out of Rust `const`s and into the `sim:` config slice must
// be a PURE refactor: the deterministic core, run from the shipped config (dungeon seed 0x5C09191) for
// 1800 fixed ticks, must still hash to the value measured BEFORE the migration. A drifted default — in
// `SimTuning::default()` or the `config.ron` `sim:` slice — reds this test instead of silently shifting
// a gameplay value. This is the absolute-value lock the same-seed reproducibility tests above cannot
// provide.
//
// Re-pinned twice since the migration: first for diegetic lighting (crabs went photophobic), then for
// the SCP-150 parasite — mancae now spawn into the core, hunt/embed hosts, and (over 1800 ticks)
// manipulate infested units + trip the crab alarm on embed, all of which move actors. So the core
// moved from the lighting-era `0x3ecce611f2403172` to `0x4b6f6d7f454559c7`. Re-pinned AGAIN for the
// SCP-150 huddle/dormancy behaviour — mancae now spawn clustered at corner/furniture harborages, stay
// dormant (passive) until roused, and huddle via cohesion/separation, so real actor motion changed.
// Legitimate: the same-seed reproducibility tests above (`deterministic_core_is_bit_identical`,
// `..._across_many_builds`) still pass, so the sim is still bit-reproducible — just different, because a
// real feature was added.
//
// MERGE with main (ATTENTION channel PR #48 + SCP color PR #47) into this WIP branch: this actor golden
// did NOT move — it still matches the pre-merge WIP value. ATTENTION adds `ai::field::deposit_attention`
// (a new `FixedUpdate` producer) and the 10th stigmergy channel, but no core actor reads ATTENTION (its
// consumer, the mould, is windowed-only and absent from the harness), and the added producer did not
// perturb any actor's trajectory here — so only the field-grid oracle below moved (it folds the new
// channel). The color PR is cosmetic (palette/HUD) and moves no actor either.
//
// Re-pinned for the COMBAT-FEEL pass: the SCP-150 parasite population defaults moved (initial_count 3→8,
// manca_count_max 12→20) and the mancae spawn geometry/arousal changed (HUDDLE_SIZE 40→4, MIN_SPAWN_DIST
// 8→5, ROUSE_THREAT 0.04→0.02, ROUSE_PROXIMITY 5→7 in src/parasite.rs) — more mancae, seeded into more
// huddles at different cells, rousing more readily, so real actor motion changed. The crab light-push is
// now gain-gated by AI mode (committed Muster/Rally/Latch/Carry crabs ignore the photophobic push); in
// this no-player seed few crabs commit, but the parasite change alone moves actors. Same-seed
// reproducibility (`deterministic_core_is_bit_identical`) still passes, so the sim stays bit-reproducible
// — just different, because real gameplay changed. Folds translation only, so arch-stable. Was
// `0x6716f1718a9774d1`. Re-measured once more within the same pass after the balance nerf that keeps the
// shipped brains survivable under the new pressure (crab_contact_dps 3.0→2.3, parasite initial_count
// 8→6); was the intermediate `0xd18a68ffc4e949b7`.
//
// Re-pinned for ALMOND WATER (the `almond_water` resource field + consuming heal): squad units and crabs
// now carry the `Biological` marker and heal while standing in seeps (the heal writes `Health` on
// FixedUpdate, so it enters `snapshot_hash`), and a wounded crab forages up the water gradient (moving
// actors). Adding the marker also shifted the unit/crab archetypes and thus deterministic iteration
// order. All legitimate: `deterministic_core_is_bit_identical` still passes (same seed → same hash), so
// the sim is still bit-reproducible — just a different, richer sim. Folds translation only, arch-stable.
// Was `0xc2fe3752a1fd1f66`. Re-measured once more within the same pass: pinning `almond_water_heal` to
// run AFTER every `Health` writer (the `HealthDamage` set + the medic) — required so the consuming heal
// composes deterministically with same-tick combat once foraging brings wounded crabs into weapon range
// — changed the net HP of those overlaps (the water now gets the last word), so the actor golden moved
// from the intermediate `0x2c9da14a81d01faa`.
//
// Re-pinned for the ALMOND-WATER SEEP-MODEL change (sparse springs): `bake_almond_sources` now seeps
// from a sparse, spaced-out set of springs (greedy `pool_spacing` scatter) instead of every wall-adjacent
// cell + a weak everywhere-baseline, and drops the weak baseline entirely. The water field the crabs
// forage on/heal from is therefore different (discrete 2–5 tile pools, not a sheet), moving the foraging
// trajectory + heal outcomes this actor golden folds. Same-seed reproducibility still passes (just a
// different, correct sim). Was `0xfd576e421bb17cf6`.
//
// Re-pinned for the CRAB DETERMINISM fix: the deterministic core was ~1–3% non-reproducible ACROSS
// PROCESSES (only caught by `train verify` run in fresh processes; `deterministic_core_is_bit_identical`
// compares two Apps in ONE process and shares the seed, so it stayed green). Two non-associative float
// sums over the NON-reproducible crab query order: (1) the crab separation spatial-hash buckets
// (`crab::crab_movement`) and (2) the wounded-crab ALARM deposit batch (`crab::crab_alarm_on_damage`).
// Both now sort into canonical order before summing (the same fix the parasite swarm + `sort_deposits`
// already use), making the core bit-reproducible across processes (verified 65/65 fresh processes). The
// old value was never a single correct golden — just the most common outcome of the flaky sum. Was
// `0xc044a98e9f910d9d` (snapshot) / `0xbcb2b8c38e3219a9` (field, below).
//
// Re-pinned for the PHASE-3 CPU MOLD FIELD (`src/mold.rs`): a new deterministic reaction-diffusion
// gameplay mold now runs on `FixedUpdate` in the harness (registered like `LightFieldPlugin`). It moves
// no actor yet (couplings wired incrementally), but inserting its `mold_update` system perturbed an
// ambiguous `FixedUpdate` order (the documented schedule-insertion effect), shifting the actor golden;
// and `MoldField` is now folded into `field_hash` (below). Deterministic across processes (verified
// 40/40 via `train verify`). Was `0x45b960069537d712` (snapshot) / `0xee06882d2f1421d9` (field).
//
// Re-pinned for the MOLD COUPLINGS (load-bearing ecosystem): the mold now (1) dims the LightField
// (`mold_dim_light`) so photophobic crabs react to mold-made dark zones, (2) occludes LOS (`fog::
// update_los`) so a crab denned in thick mold is unseen/un-targetable, and (3) boosts almond-water seep
// live (`AlmondWater::tick`) so moldy zones weep more healing water. All three move real actors, and the
// field golden folds the couplings' effect on the light/water/Stig grids. Deterministic across processes
// (44/44). Was `0x5b5a84cf56eadcbe` (snapshot) / `0x5ff6dc475cad0375` (field).
//
// Re-pinned for the MOLD SEEP-BOOST retune (3.0 -> 1.5): at seep_boost 3× the grown mold weeps enough
// that moldy almond pools merged past the 10-tile cap into a sheet (defeating fog of war —
// `almond_pools_stay_small_and_isolated` red). 1.5 mirrors the old static `mold_seep_mult`; the live ramp
// `1 + 0.5·mold01` stays <= it, so pools keep their sparse footprint while the coupling stays live +
// optimizer-tunable. Was `0xcdca49900d7da832` (snapshot) / `0xd705e971d0480409` (field).
//
// Re-pinned for the ALMOND-WATER BELIEF/INVERSION mechanic (Stage 2): the water now does what the
// population believes — belief at a cell selects heal (+HP) OR cyanide poison (−HP), and a
// `belief_poison_frac` slice of cells is seeded cyanide, so some biologicals now take poison damage that
// moves their `Health` (folded into `snapshot_hash`). The anomaly factions (Manca + the Smiley boss) are
// now `Biological` too, so the water heals/poisons them and their added marker shifts the archetype
// iteration order the actor grids fold. Deterministic across processes (verified bit-identical over two
// runs). Was `0x06760dc03aeb5ed3`.
//
// Re-pinned for BELIEF-MODULATED CRAB FORAGING (Stage 3): a wounded crab now steers toward water it reads
// as heal and AWAY from water it reads as cyanide (an anosmic crab can't tell and walks into poison), so
// the forage nudge that moves crab positions — folded into `snapshot_hash` — changed. Verified
// bit-identical over two runs. Was `0x14ac65f6ef9c649e`.
// RESTORED, not re-measured. `train apply --dim levels` (run 2026-07-16 08:54 by `cargo train all`)
// spliced a machine-baked levels elite over the authored `dungeon`/`mycelia`/`placement` slices of
// `config.ron` — replacing the authored seed, widening corridors 2→6, switching topology to `Graph`,
// and stripping ~279 lines of hand-written rationale — then AUTO-RE-PINNED this golden to the baked
// level's hash, `0x1794420ff06a57d8`. That elite came from a search run while G0 was live, i.e. scored
// against a wobbling objective, so it was partly selected by evaluation luck. The authored level has
// been restored (keeping the hand-authored `almond_water` belief/inversion work), and `train verify
// --reps 8` recomputes exactly the pre-bake value below — which is what this constant held before the
// bake. Five `cargo test` failures (dungeon/placement/level_genome/mycelia) were that swap being
// correctly detected; all five pass again.
//
// (This paragraph previously named the baked hash as `0x38d3c9107d4eed33` — the RESTORED value, not the
// baked one. A transcription error, corrected here against the field golden's log below, which recorded
// its own baked value `0x9b19982055f7413d` correctly. Left visible because an audit trail that quietly
// fixes itself is not an audit trail.)
//
// The hashes quoted in the prose above are deliberate archaeology: they are how a future reader
// reconstructs what moved and why. `train apply` used to rewrite them as collateral of re-pinning the
// const below (an unbounded whole-file `str::replace`) — it no longer does, and it no longer re-pins at
// all without an explicit `--repin-goldens`. Changing a golden is a deliberate, human-reviewed act
// (TESTING.md); the tool's job is to REFUSE and report the drift, not to resolve it.
//
// Re-pinned for the G0c FIX — the determinism total-order pass. Was `0x38d3c9107d4eed33`.
//
// This one is worth understanding, because "the golden was stable, so why did it move?" is the obvious
// objection and the answer is the whole point. The old value WAS reproducible on this box — but only
// because ECS query order happened to come out the same way for this particular no-player scenario. It was
// consistent by luck, not by construction. Several sums it folds (the flashlight-cone `compose`, the manca
// swarm's heading/commit, the Almond Water drink contention) were being ordered by whatever the query
// yielded; they are now ordered canonically, so the value changed. The new one does not depend on query
// order at all. Precedent and reasoning are the CRAB DETERMINISM re-pin's, above: *"The old value was never
// a single correct golden — just the most common outcome of the flaky sum."*
//
// Verified before pinning, per TESTING.md: `train verify --reps 8` plus three further FRESH processes —
// 17 independent measurements, all `0xe11eed83902ee648`. `deterministic_core_is_bit_identical_across_many_builds`
// (24 builds) and `search_rollouts_are_reproducible_under_load` (12 rollouts × both held-in seeds × 7200
// ticks, under CPU load) are green on this value, which is a stronger statement than the old one could make.
// Re-pinned 2026-07-19 across a run of player-reported worldgen fixes: doorway width, desk-lamp→worktop,
// almond-water rarity (`pool_spacing` 8→12), and finally the CEILING-LIGHT RECLASSIFICATION — the kit's
// "Ceiling Light" model was a misclassified table lamp anchored overhead; making it a Scatter worktop
// lamp removes the room-centre light, so the `LightField` (and the crab photophobia it drives) shifts the
// units/crabs `snapshot_hash` folds. Not a determinism break — `authored_world_config_override_is_a_noop`
// measured the SAME new value (world-config seam untouched). Prior chain: 0xe11eed83902ee648 →
// 0xed748bc555d5529e → 0xf175e0f71ce92183.
//
// Re-pinned 2026-07-19 for the WALL-SCONCE ROW rule (player region-capture request: "3-to-X sconces in
// a row along a wall, gap before the corner"). `furnish.rs` Pass 1b now lays a per-wall row instead of a
// single mid-room pick, and `wall_lights_per_room` became a real per-room budget (shipped 6, up from 1).
// More sconces = more `LightEmitter`s = a brighter `LightField`, and the crab photophobia it drives moves
// the units/crabs `snapshot_hash` folds — the SAME mechanism as the ceiling-light re-pin above, opposite
// sign (adding light, not removing it). Not a determinism break: `deterministic_core_is_bit_identical`
// stays green and the value was measured identical across 3 fresh processes. Was `0x819ab83bc5c5540b`.
//
// Re-pinned 2026-07-19 for the TRASHCAN MIN-DISTANCE rule (player region-capture request: bins must not
// cluster). `furnish.rs` Pass 2 now greedily disperses tiled props to `TILED_MIN_GAP` apart, so bin
// positions moved — and furniture is a nav obstacle the crabs path around, so the crab trajectory (and the
// `snapshot_hash` it folds) shifts. Not a determinism break: `deterministic_core_is_bit_identical` stays
// green and the value was identical across 3 fresh processes. Was `0xbf77f8e2024b0c86`.
//
// Re-pinned 2026-07-20 for the FURNITURE FOOTPRINT/PIVOT correction + DOORWAY KEEP-CLEAR rule (player
// region-capture requests: "furniture must not sit in a doorway" and "not halfway through a wall"). The
// manifest footprints were re-measured off the glbs and off-centre meshes now carry a `pivot` so they
// recentre on their placement point, and `furnish.rs` rejects any footprint overlapping a doorway
// approach band. Both change which furniture lands where — furniture is a nav obstacle the crabs path
// around (and support pieces carry the scatter lamps whose `LightEmitter`s drive crab photophobia), so
// the crab trajectory the `snapshot_hash` folds shifts. Not a determinism break:
// `deterministic_core_is_bit_identical` stays green and `authored_world_config_override_is_a_noop`
// measured the SAME new value (world-config seam untouched). Was `0x6bd480d83f264117`.
//
// Re-pinned 2026-07-20 for the BACKLOG.md correctness-bug sweep — several deliberate gameplay changes in
// one pass, each individually documented as golden-moving in BACKLOG.md at the time it was written:
//   * H1/Health root fix: `Health::apply_damage`/`kill()` now clamp `current` at a 0 floor at every damage
//     site, so a unit killed in a heal pool can no longer be over-healed back past `max` and resurrected.
//   * M10: nest breeding no longer gates on a hard population cap (`crab_count_max`) or a local crowding
//     gate (`crowd_cap`) — removed per design decision; the meat economy is now the swarm's only size
//     lever, so population (and therefore crab trajectories/combat) diverges from the old capped run.
//   * M8: `crab_alarm_on_damage`/`manca_rouse` switched from `Health::is_changed()` to a stored `last_hp`
//     delta, so they no longer false-fire "damaged"/"shot" on an Almond Water heal tick — fewer spurious
//     ALARM deposits and manca rouses change crab/manca motion.
//   * M6: the Smiley's `Scared` flee vector now falls back to its current heading (instead of `Vec2::ZERO`)
//     when no unit is alive to flee from.
//   * M1: the `HealthDamage` system set's 7 writers are now an explicit `.after()` chain (`smiley_zap` →
//     `smiley_defense` → `crab_jump` → `crab_contact_damage` → `manca_embed` → `parasite_burst` →
//     `fire_laser`) instead of accidental plugin-registration order — same effective order as before, but
//     making it explicit surfaces float non-associativity that was previously masked.
// Not a determinism break: `deterministic_core_is_bit_identical` and
// `deterministic_core_is_bit_identical_across_many_builds` stay green, and
// `authored_world_config_override_is_a_noop` measured the SAME new value (world-config seam untouched).
// Was `0x793366008d9878fb`.
//
// Re-pinned 2026-07-24 for **SCP-1048, the Builder Bear**. Four things moved the core, and the last is
// the interesting one:
//   * a benign original now seeds out in the level carrying `Health` (so it folds into the snapshot) and
//     moves on `FixedUpdate`;
//   * the squad gains a `Faction::Bear` neighbour whose brain runs through the same `think`;
//   * four new `Mode` variants widen the action alphabet the utility `decide` selects among;
//   * and — unlike every other creature in this sim — **the bear BUILDS more of itself mid-episode.** An
//     unobserved original scavenges for ~12 s and assembles a hostile copy, so a 1800-tick core no longer
//     ends with the population it started with. That is a genuine new source of divergence, not just a
//     re-shuffle, and it is exactly what `scp1048::the_bear_breeds_unattended_with_nothing_forced` pins.
//
// Measured, not assumed: three fresh processes run CONCURRENTLY (an idle-box probe proves nothing) all
// reported this same value; `deterministic_core_is_bit_identical` and
// `deterministic_core_is_bit_identical_across_many_builds` (24 builds) stayed green — so the sim is still
// bit-reproducible, just different — and `authored_world_config_override_is_a_noop` measured the SAME new
// value, proving the world-config seam is untouched.
//
// Two intermediate values were measured and discarded during this landing (`0xd9e2765d0f35d881` before
// replication was reachable, and one before a brain fix that had left the bear unable to start
// gathering). If you are re-pinning this, take the measurement *after* the behaviour is final, not
// stage by stage.
// Was `0x991b80282f2def20`.
//
// ── Re-pinned 2026-07-25, M0 (Push 1). Was `0xc9c8c93f82ab5857`. THREE stacked causes, each measured by
// disabling it and re-running, so this is an accounting rather than a shrug:
//   1. Uncommitted in-tree work that pre-dates this push (light/gore/health/dungeon/fog/laser + the
//      `config.ron` edits). On its own it moved the actor golden to `0x35624e2f9d31d10c`.
//   2. FVS-A-1's `session` module. Adding RESOURCES was hash-neutral (measured: no change — good evidence
//      nothing keys off entity ids, cf. `util::nearest_planar_keyed`); adding two `FixedUpdate` SYSTEMS
//      was not, and no ordering edge fixes it — pinning them `.after(HealthDamage)` produced byte-identical
//      results to leaving them unordered. A new schedule node permutes the linearisation of other
//      unconstrained systems. Expect this from every future `FixedUpdate` addition.
//   3. FVS-D-2's squad↔member relationship, which puts `MemberOf` on every `Unit` and so changes the
//      hashed squad's archetype — the same class as the `Biological` marker re-pin recorded above.
// FVS-A-5 (run-scoped world construction) moved it by exactly NOTHING: measured identical before and
// after, because the first run still generates from the configured seed through the same `GameConfig`
// seam. That is the strongest evidence the refactor is behaviour-preserving.
//
// ── Re-pinned again at the end of Push 2 (M1) for the containment systems (`tick_containment`,
// `deploy_devices`, `tick_quarantine`, `release_finished_devices`, `track_secured_sites`). Worth
// recording because the number LANDED BACK on the value measured before FVS-A-1's session systems were
// added: none of these systems writes `Transform` or `Health`, so they can only reach this hash by
// permuting the schedule's topological sort — and adding enough nodes happened to restore the original
// relative order of the systems that do move actors. That is benign, and it is also the cleanest
// available demonstration of the standing caveat: an added `FixedUpdate` node moves this hash by
// re-linearising its neighbours, not by changing gameplay.
// Legitimate: `deterministic_core_is_bit_identical`, `..._across_many_builds` and — the only probe that
// counts under TESTING.md invariant 9 — `search_rollouts_are_reproducible_under_load` are all green, so
// the sim is still bit-reproducible, just different.
/// **Goldens are PER-PLATFORM** (decision 2026-07-27, FVS-J-3 / BACKLOG §7).
///
/// The determinism model was an open question: `f32` gameplay math is not guaranteed identical across
/// instruction sets, so one hash cannot hold on both x86-64 and aarch64. Three options were on the
/// table — fixed-point the core, one golden with an epsilon tolerance, or a golden per platform. The
/// last was chosen:
///
/// * **A tolerance was rejected** because exact-hash discipline is what has caught every determinism
///   bug this project has found, including two on the day of this decision. Comparing with an epsilon
///   would blind precisely the oracle that works.
/// * **Fixed-point was rejected for now** as a large invasive change to movement/fields/ORCA. It stays
///   the only option that makes a replay portable *between* machines, so it is the right answer if
///   cross-platform replay ever becomes a requirement — see §7.
///
/// What this buys: each platform stays held to **bit-exact** reproducibility against itself, which is
/// the property every golden here actually relies on. What it costs, stated plainly: a replay or
/// campaign captured on one architecture is **not** verifiable on another, and `sim_harness` results
/// cannot be compared across a heterogeneous fleet.
///
/// aarch64 is deliberately left **unpinned** rather than guessed. The `determinism-arm` CI lane exists
/// to measure it; once it reports a stable value, fill it in here and that lane can stop being
/// advisory. A `todo!()` would fail at runtime with no explanation, so the arm below explains itself.
/// ### Re-pinned 2026-07-30 for FVS-K-1 — `0x3563f0f69281ce4c` → `0x9f7a0787fdcb487f`
///
/// **One cause, and it is the intended one: SCP-610 gained `Health`.** `snapshot_hash` folds
/// `(&Transform, &Health)`, and until now a bloom had no `Health` at all — so the three blooms every
/// level seeds were invisible to this oracle. FVS-K-1 made 610 killable (which is what makes "killing
/// yields nothing" a player-reachable choice rather than an assertion), so three stationary
/// Transform+Health rows now enter the fold.
///
/// Measured from a settled tree, and **reproducibility was verified before pinning, not after**:
/// `deterministic_core_is_bit_identical` and `..._across_many_builds` were both green on this value
/// first. That order matters — pinning a number and then checking it reproduces tells you nothing you
/// did not already assume.
///
/// The other FVS-K-1 additions are accounted for individually and none of them reaches this hash:
/// * the flesh/eye materials, the cordon gizmos and the audio are all `Update` and windowed-only, and
///   the harness registers neither `Scp610VisualsPlugin` nor `UiPlugin`;
/// * `deposit_flesh_drone` writes stigmergy, which is [`GOLDEN_FIELD`]'s business, not this one;
/// * `kill_blooms` moves no actor while nothing damages a bloom in the pinned run.
///
/// Budget a re-pin for every future `FixedUpdate` addition regardless (see the M0 note below): a new
/// schedule node permutes the linearisation of other unconstrained systems, and no ordering edge
/// fixes it.
///
/// ## Re-pinned 2026-08-01 — FVS-C-7's watch feed entered the deterministic core
///
/// `0x9f7a0787fdcb487f` -> `0xdbff6e94a7fa3d0e` (actors) and `0x82d9fc45c7e06f63` ->
/// `0x0feb0281e67e81b8` (fields). The cause was **isolated by ablation rather than assumed**, and the
/// first two hypotheses were both wrong — which is why the table is kept:
///
/// | ablation | actors | fields |
/// |---|---|---|
/// | `broadcast.count: 0` (systems registered, no screens) | pinned | MOVED |
/// | `BroadcastPlugin` unregistered, `Subject::ALL` still 8 | pinned | pinned |
///
/// So it is neither the `Subject` enum growing nor a lossy genome encode (both were suspected;
/// `authored_round_trips_exactly` and a config-vs-default check cleared the latter). It is simply
/// **the creature being in the world and working**.
///
/// The last piece was a red herring worth naming: `deterministic_core_is_bit_identical` stayed green
/// throughout, which looked like "the actor golden did not move". It did. That test steps **180**
/// ticks and only compares two runs against *each other* — it never reads [`GOLDEN`]. The tests that
/// do step **1800**, and by ~30 s of play the squad has walked past a screen, watched it, and the feed
/// has generated a crab. A crab carries `Health` + `Transform`, so it enters `snapshot_hash`; a screen
/// does not. **A green reproducibility test is not a green golden test.**
///
/// ## Re-pinned AGAIN 2026-08-01, and it went BACK — FVS-B-10's lure
///
/// `0xdbff6e94a7fa3d0e` -> `0x9f7a0787fdcb487f` (actors) and `0x0feb0281e67e81b8` ->
/// `0x82d9fc45c7e06f63` (fields) — i.e. **exactly the values pinned before the watch feed landed a
/// few hours earlier**. The lure adds two schedule nodes and nothing else in this scenario (nobody
/// throws a lure in a golden run, so `tick_lures` iterates an empty query), so the schedule
/// linearisation was permuted back to its earlier order. This repo has seen that before: the Push 2
/// re-pin "landed back on the value measured before M0's session systems existed".
///
/// ⚠️ **But there is a second reading, and it is the one worth acting on.** The watch-feed re-pin
/// hours earlier was attributed to the feed *generating a crab* inside the 1800-tick window (a crab
/// carries `Health`+`Transform` and enters the hash; a screen does not). If that was right, a pure
/// scheduling nudge could not undo it — the crab would still be there. So the honest conclusion is
/// that **the feed no longer charges to full inside the golden window**, and a small perturbation was
/// enough to flip it.
///
/// That makes the feed's activation *marginal in passive play*: it needs ~7 s of sustained attention
/// (`charge_rate` 0.14), and whether an unscripted squad ever looks that long is close to a coin
/// flip. Filed as FVS-N-30. The mechanic is proven by
/// `containment::watching_the_feed_makes_it_generate_and_ignoring_it_stops`, which floods attention
/// deliberately; that test is unaffected and still green. This is a balance finding, not a
/// correctness one — but it means a player may never see the anomaly do anything.
///
/// ## Re-pinned a THIRD time 2026-08-01 — and this one is a real gameplay change (FVS-N-30)
///
/// `0x9f7a0787fdcb487f` -> `0x4d170fd316e6e5bf` (actors), `0x82d9fc45c7e06f63` ->
/// `0x8145db22fc83542c` (fields). Unlike the two re-pins above — both pure schedule-node
/// permutation — this one is the watch feed **actually working for the first time**.
///
/// Measured, not inferred (`containment::the_watch_feed_fires_in_passive_play_on_the_held_in_seeds`):
///
/// | | before | after |
/// |---|---|---|
/// | nearest squad approach to a screen | 101-137 m | 13.8-15.0 m |
/// | peak ambient ATTENTION at a screen | 0.000 | 0.007-0.012 |
/// | emissions in 60 s of passive play | 0, 0, 0 | 14, 7, 14 |
///
/// Two authoring bugs, both invisible to every existing test:
///  1. **Placement.** `spawn_screens` took the FIRST floor cells past `spawn_min_dist` in raster
///     scan order — i.e. the map corner. A minimum distance consumed in scan order is a *maximum*
///     distance in disguise. Now ranks eligible cells by distance and takes the nearest.
///  2. **Threshold scale.** `watch_threshold` was 0.30, copied by analogy from SCP-1048's 0.45
///     containment bar. The ATTENTION field measures ~1.4 at a squad member's own cell and ~0.01 at
///     14 m — it is steeply local, so 0.30 means "standing on it". Both the threshold and the
///     containment ceiling were re-derived from that measurement, and the genome BOUNDS were widened
///     for the same reason: the old `(0.05, 0.80)` put *every* genome in the inert region, so a
///     world search would have spent its entire budget on an anomaly that never fires.
#[cfg(target_arch = "x86_64")]
const GOLDEN: u64 = 0x4d170fd316e6e5bf;

/// Not yet measured — see [`GOLDEN`]. `0` is never a real snapshot hash, so this fails loudly and the
/// message says exactly what to do.
#[cfg(not(target_arch = "x86_64"))]
const GOLDEN: u64 = 0;

#[test]
fn migrated_defaults_reproduce_the_shipped_golden_hash() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1800);
    let got = snapshot_hash(&mut app);
    assert_ne!(
        GOLDEN, 0,
        "no golden is pinned for this architecture yet (goldens are PER-PLATFORM — see the GOLDEN doc). \
         This run measured {got:#018x}; if the `determinism-arm` lane reproduces it across builds, pin \
         it in the `cfg(not(target_arch = \"x86_64\"))` arm and drop that lane's continue-on-error."
    );
    assert_eq!(
        got, GOLDEN,
        "deterministic-core hash drifted from the pre-migration golden — the const→config promotion \
         changed a gameplay value (or the shipped `sim:` slice differs from SimTuning::default())"
    );
}

// The direct oracle for the "iterate only floor cells" optimization of the evaporate/diffuse/hotspot
// passes (commit 973319d). `snapshot_hash` folds only actor Transform+Health, so it catches a diffusion
// regression only *transitively* — if the perturbed gradient happens to move a crab to a different cell —
// and never exercises `saturation_stats` at all. `field_hash` folds the field grids themselves (every
// Stig channel cell + every RallyField vector, full grid, plus saturation_stats), so a reordered
// neighbour sum, a broken floor mask, or a rock cell that stops being 0 reds this test outright. Same
// deterministic-core config and tick count as the actor golden above, so the two are directly comparable.
// Re-pinned again for the SCP-150 parasite (was `0xf56b_eabb_d8d3_aa57`): mancae embed hosts, which
// damages crabs and trips the ALARM channel, and manipulated units move — both perturb the stigmergy
// grids `field_hash` folds. Previously re-pinned for the audio + lighting merge (`field_hash` folds the
// `NOISE_SQUAD`/`NOISE_SWARM` channels and the `light::LightField` grid).
// [Later SUPERSEDED by the D1 re-pin at the bottom of this block — the cone forward is now arch-stable,
// so `fold_fingerprint` folds `cells` again.] Reverted to `0xa35b_eaeb_288a_fbca` after the flashlight
// re-pin (`0x3db0_1bf8_5c5d_d822`) proved
// ARCH-DEPENDENT: `LightField::fold_fingerprint` now folds the static `base`, not `cells`. The dynamic
// flashlight cone in `cells` derives its beam direction from unit `Transform.rotation`, computed with
// glam quaternion/`slerp` transcendentals that are not bit-identical across ARM↔x86 — so an ARM-pinned
// cone-inclusive value failed `field_passes` on x86 CI while `migrated_defaults` (which folds
// translation, never rotation) passed. Folding the arch-stable scalar-`f32` base restores a value that
// matches on both arches (it is the pre-flashlight static field). The cone's determinism is covered
// within-arch by `deterministic_core_is_bit_identical` and its unit tests. See `light::fold_fingerprint`.
//
// [MERGE re-pin] Combined with main's ATTENTION channel: `Stig::fold_fingerprint` now folds the 10th
// channel (attention, deposited over the squad LOS set by `ai::field::deposit_attention`) on top of the
// WIP field state below — arch-stable (fog visibility is position/integer-LOS, no rotation). Value below
// is the measured merged-tree hash.
//
// Re-measured at the restored clean-defaults baseline: `config.ron`'s `sim:` + `ai_tuning:` slices were
// reset to `SimTuning::default()` / `AiTuning::default()`, resolving the evolved drift + the three
// `TEMP — RESTORE` overrides (laser_damage ⅓, parasite initial_count/manca_count_max at 300). This value
// now also captures the SCP-150 readable-swarm change (alignment + collective roused motion + commitment
// ramp) that the prior `0xa35b_eaeb_288a_fbca` predated. The ACTOR golden above did NOT move: it was
// already pinned to the pure-defaults value — proven by `authored_world_config_override_is_a_noop`, which
// runs `decode(authored())` == (AiTuning::default(), SimTuning::default()) and still matches it — so the
// config restore only moved the field grids this oracle folds.
//
// Re-pinned again for FIX 1 (roused SCP-150 mancae now deposit `THREAT_ANOMALY` via
// `parasite::deposit_manca_dread`, so the whole brood is legible to the squad's anomaly-fear + psi-vision
// instead of being a silent parallel stack): the golden run rouses mancae, so new dread cells enter the
// field grids this oracle folds. The ACTOR golden was NOT affected — in this no-player seed the added
// dread moved no unit's final Transform/Health — so only this field value changed (was
// `0x5d60_2962_2213_5600`, the clean-defaults baseline).
//
// Re-pinned again for the D1 flashlight-determinism fix, which SUPERSEDES the `base`-only workaround
// described above: `apply_dynamic_lights` now derives the cone's beam direction from the Researcher's
// deterministic gameplay state (FacingOverride/AimTarget/velocity) with arch-stable ops instead of the
// slerped `Transform.rotation`, so `cells` (base + cones) is bit-identical across ARM↔x86 again. So
// `LightField::fold_fingerprint` folds `cells` once more (restoring the moving cone to this oracle's
// coverage), which moved this value (was `0xe1bb_9db0_7822_411f`). The ACTOR golden did NOT move: in
// this no-player seed no photophobe is warded into a cone cell, so the cone perturbs no unit's final
// Transform (the cone→actor coupling stays latent). See `light::apply_dynamic_lights`/`fold_fingerprint`.
// Re-pinned for the COMBAT-FEEL pass (was `0x03f9_6217_e5b5_fb62`): more mancae (initial_count 3→8) in
// more huddles rouse and deposit `THREAT_ANOMALY`, and changed crab motion re-writes the CRAB_DENSITY /
// SCENT / ALARM channels this oracle folds. No rotation-derived folding was touched (the light change is
// a read-only gradient sample gated by AI mode; the mancae dread is position/integer-cell), so the value
// stays arch-stable across ARM↔x86. Re-measured once more within the same pass after the balance nerf
// (crab_contact_dps 3.0→2.3, parasite initial_count 8→6); was the intermediate `0xf212_b7c1_4ef0_9a8c`.
//
// Re-pinned for ALMOND WATER: `field_hash` now folds the `AlmondWater` field (`level` + `sources`, full
// grid, via `AlmondWater::fold_fingerprint`, added to `sim_harness::field_hash`) on top of the Stig /
// Rally / Light grids. The seeps also accumulate/evaporate/diffuse each tick and the heal drinks them
// down, so the folded water grid is live state. And the `Biological`-marker archetype shift moved the
// crab/unit trajectory the stigmergy channels fold. Arch-stable (pure scalar-f32 field ops, no rotation).
// Was `0x4557_fa4d_8f4b_6262`. Re-measured once more within the same pass for the `almond_water_heal`
// ordering pin (`.after(HealthDamage)`): the heal now drinks the water field AFTER same-tick combat
// resolves, shifting which cells drain and the actor motion the stigmergy grids fold. Was the
// intermediate `0x280d_34a4_87f1_1a3c`.
//
// Re-pinned for the ALMOND-WATER SEEP-MODEL change (sparse springs): the `AlmondWater` `sources`/`level`
// grids this oracle folds are now the sparse-spring field (only spaced springs seep; no weak baseline),
// and the changed water changes the crab motion the Stig channels fold. Arch-stable (scalar-f32 field
// ops). Was `0x6f0e_14d6_3ad5_206c`.
//
// Re-pinned for the CRAB DETERMINISM fix (see the `GOLDEN` note above): sorting the wounded-crab ALARM
// deposit batch (`crab::crab_alarm_on_damage`) canonicalised the ALARM channel's non-associative sum,
// which this field oracle folds. Was `0xbcb2_b8c3_8e32_19a9`.
//
// Re-pinned for the ALMOND-WATER BELIEF field (Stage 1 of the belief/inversion redesign): `field_hash`
// now also folds the `AlmondWater::belief` grid (`AlmondWater::fold_fingerprint`). At this stage belief is
// inert — every floor cell is seeded to `belief_prior` (1.0) at the bake and no tick dynamics touch it yet
// — so this is a pure additive fold of a constant grid (1.0 on floor, 0.0 on rock), no behaviour change.
// Verified bit-identical across two runs. Arch-stable (scalar-f32 fold, no rotation). Was `0x272a_e3b0_2e95_d28b`.
//
// Re-pinned for the BELIEF/INVERSION mechanic (Stage 2): belief is now seeded per-cell (a
// `belief_poison_frac` slice = cyanide) and evolves each tick (relax toward base + diffuse + rumor
// deposits), so the folded belief grid is live state; the poison also drinks cells down differently, and
// the anomaly-faction `Biological` shift moves the actors the Stig grids fold. Verified bit-identical over
// two runs. Arch-stable (scalar-f32 field ops, no rotation). Was `0x64ce_5d24_e542_b2ab`.
//
// Re-pinned for BELIEF-MODULATED CRAB FORAGING (Stage 3): the wounded-crab forage nudge now depends on the
// belief the crab reads (seek heal / flee cyanide / anosmic seeks any), so crab motion — and the Stig
// channels the field oracle folds — changed. Verified bit-identical over two runs. Was `0xb5c1_285d_724c_5a92`.
// RESTORED alongside `GOLDEN` above — the machine bake re-pinned this to the baked level
// (`0x9b19982055f7413d`); `train verify --reps 8` recomputes the pre-bake value below on the restored
// authored level.
// Re-pinned alongside `GOLDEN` for the G0c fix (the determinism total-order pass) — see the long note
// there for why a golden that WAS stable still moved: it was consistent by luck (query order happened to
// repeat for this scenario), not by construction. This field golden folds the light/Stig/water grids whose
// per-cell sums are now canonically ordered. Was `0xe1ec_dc58_3c8d_bfca`. Verified over 17 independent
// measurements (`train verify --reps 8` + three fresh processes), all `0xd504e6a2f019f3fb`.
// Re-pinned 2026-07-19 alongside `GOLDEN` across the same worldgen-fix run (doorway, desk-lamp, almond
// rarity, and the CEILING-LIGHT RECLASSIFICATION). This oracle folds the `LightField` and the `AlmondWater`
// grids directly, so removing the room-centre light and thinning the springs both move it, plus the changed
// crab motion the Stig channels fold. Arch-stable (scalar-f32 field ops, no rotation). Prior chain:
// 0xd504e6a2f019f3fb → 0xc609b6efd2e6da78 → 0x131098b2650bd15a.
//
// Re-pinned 2026-07-19 alongside `GOLDEN` for the WALL-SCONCE ROW rule (see the `GOLDEN` note). This oracle
// folds the `LightField` grid directly, so laying a row of sconces along every wall (shipped budget 6, up
// from one per room) rewrites it outright — and the brighter field moves the crab photophobia the Stig
// channels fold. Arch-stable (scalar-f32 field ops, no rotation). Verified identical across fresh processes.
// Was `0x01dbc17ff855b586`.
//
// Re-pinned 2026-07-19 alongside `GOLDEN` for the TRASHCAN MIN-DISTANCE rule (see the `GOLDEN` note). Bins
// are nav obstacles; dispersing them moves the crab trajectory, which re-writes the CRAB_DENSITY / SCENT /
// ALARM stigmergy channels this oracle folds (the LightField itself is unchanged — bins don't emit).
// Arch-stable (scalar-f32 field ops, no rotation). Verified identical across fresh processes. Was
// `0xebd044119a67f842`.
//
// Re-pinned 2026-07-20 alongside `GOLDEN` for the FURNITURE FOOTPRINT/PIVOT + DOORWAY KEEP-CLEAR changes
// (see the `GOLDEN` note). Recentring off-centre support pieces and rejecting doorway-blocking furniture
// moves where scatter lamps rest, so their `LightEmitter`s rewrite the `LightField` this oracle folds —
// and the changed crab photophobia moves the stigmergy channels it also folds. Arch-stable (scalar-f32
// field ops, no rotation). Was `0x5692ad7429ff5736`.
//
// Re-pinned 2026-07-20 alongside `GOLDEN` for the BACKLOG.md correctness-bug sweep (see the `GOLDEN` note
// for the full list). M10 (nest cap removal) and M8 (alarm/rouse false-fire fix) both change crab/manca
// motion and ALARM-channel deposits directly, which this oracle folds. Arch-stable (scalar-f32 field ops,
// no rotation). Was `0xd4db701cc41588ac`.
//
// Re-pinned 2026-07-24 for the SCP-999 / squad-rework branch. The ACTOR golden (`GOLDEN`) did NOT move —
// only this field oracle did, which is exactly the blind spot it exists to cover: state that steers agents
// without (yet) relocating one. Attribution was measured, not assumed: a probe run with the comfort blob
// zeroed out of the core (`scp999.count = 0`) produced this SAME fingerprint, so SCP-999 moves nothing
// here (the calm no-player squad keeps the blob inert — the same reason the actor golden held); the
// movement is the branch's squad rework + config re-tuning + creature changes. Verified bit-stable across
// two fresh single-process runs under load. Was `0xdf805ab8088f34ee`.
//
// **NOT re-pinned for SCP-1048 (2026-07-24), and that is a measured result, not an oversight.** The bear
// family moved the ACTOR golden above but leaves this fingerprint bit-identical: in a 1800-tick no-player
// core the seeded bear is the benign original, which deposits into no stigmergy channel at all (only a
// *raging copy* emits dread, and no copy is ever built in that scenario), so the field grids never see it.
// This is the mirror image of the SCP-999 landing recorded above, where the field oracle moved and the
// actor one did not — together they are decent evidence that the two oracles really are covering
// different blind spots rather than restating each other.
//
// Recorded because it was briefly got wrong: an intermediate re-pin to `0x54d12d1892c5bf6f` was measured
// mid-landing and later failed to reproduce. Three fresh concurrent processes under load agree on the
// value below, which is also the value it held before the bear existed. If you find yourself re-pinning
// this for a creature that emits nothing, measure again before believing it.
//
// Re-pinned 2026-07-25 for the fog-of-war "picket fence" fix (`Dungeon::line_of_sight_reveal`,
// `fog::update_los`). The ACTOR golden (`GOLDEN`) did NOT move — same blind spot as the SCP-999 landing
// above. `update_los`'s old strict corner rule blocked a diagonal reveal step whenever the "far" neighbour
// was merely the sightline's OWN corridor wall, not just a true diagonal pinch; the squad's idle spawn-point
// fog disc — ticking every `FixedUpdate` in this "no-player" core, no order ever issued — reveals a
// different, larger set of cells under the corrected rule. `seen_by_squad` (crab AI perception,
// `.after(fog::LosWritten)`) reads that same-tick visibility, so which crabs read as "seen" shifts, which
// shifts their behaviour and the ALARM/THREAT Stig deposits this oracle folds — without (in this 1800-tick
// no-order run) relocating any actor far enough to move `GOLDEN`. Attribution measured by isolating the
// change (dungeon.rs + fog.rs alone, no other file in that landing touched) and confirming this exact value
// bit-stable across three fresh single-process runs. Was `0x2e884ae0bb33f60c`.
//
// Re-pinned 2026-07-25 alongside `GOLDEN` — same three causes, same evidence (see the note there).
// Was `0x244e3af59ff9d65a`.
// Re-pinned 2026-07-30 for the furniture/TV import (16 rows added to `placement.furniture.items`).
// The ACTOR golden did NOT move — the same blind-spot asymmetry recorded twice above.
//
// **Attributed by bisect, not by assumption**, because "I added content, so the content oracle moved"
// is exactly the reasoning that buries a real regression. Running this test alone at each commit of
// that landing:
//
//   relight (HDR/bloom/env-map/shadows/NotShadowCaster) ... PASS
//   derived normal + ORM maps ......................... PASS
//   surface biomes .................................... PASS
//   furniture + TV import ............................. FAIL  <- moved here
//   biome footsteps ................................... FAIL  (same value, inherited)
//
// So the render work never reached the simulation, and neither did the biome field — which is the
// property its "pure function of (seed, cell), draws nothing from the carve RNG" design exists to have.
//
// The mechanism is the one this oracle is built to see: `PlacementPlugin` runs in the headless core, so
// new catalogue rows change which furniture the solver picks; several of the new rows afford `"emit"`
// (a desk lamp and three CRTs), which moves `LightEmitter` positions, which moves the `LightField`,
// which moves photophobic crab steering, which moves the ALARM/THREAT deposits this hash folds.
//
// Value measured bit-stable across three fresh single-process runs. Was `0x60b5c51fcc20a281`.
//
// Unrelated and still open at the time of this re-pin:
// `search_rollouts_of_mutants_are_reproducible_under_load` fails, and it fails at the commit BEFORE
// this landing too (mutant #2 on world 0xa11ce there, mutant #3 on 0x5c09191 after) — a pre-existing
// latent order-dependence, not something this landing introduced. Do not read this re-pin as having
// addressed it.
// ### Re-pinned 2026-07-30 for FVS-K-1 — `0xc95454f3ca28b71c` → `0x82d9fc45c7e06f63`
//
// **SCP-610 now radiates.** `scp610::deposit_flesh_drone` pushes `NOISE_SWARM` and `THREAT_ANOMALY`
// every fixed tick from each living bloom, which is exactly the kind of thing this hash exists to
// fold. Before FVS-K-1 a bloom was inert scenery: it deposited nothing, perceived nothing, and could
// not be damaged, so neither golden could see it.
//
// The deposit is `rate * dt`, not a raw per-tick push — the 60× error that distinction guards against
// is not hypothetical, it is what the first run of
// `tests/containment.rs::the_loudest_evolvable_bloom_can_still_be_contained` caught. Had it shipped,
// this hash would still have moved and looked equally "expected".
//
// Measured from a settled tree; `field_passes_are_bit_identical` is itself the reproducibility check,
// and the two `deterministic_core_is_bit_identical*` tests were green on the same tree first.
/// Re-pinned 2026-08-01 alongside [`GOLDEN`], twice in one day — see there for why the second
/// re-pin went back to this file's pre-watch-feed value, and what that implies about the feed.
#[cfg(target_arch = "x86_64")]
const GOLDEN_FIELD: u64 = 0x8145db22fc83542c;

/// Per-platform, like [`GOLDEN`] — not yet measured on aarch64.
#[cfg(not(target_arch = "x86_64"))]
const GOLDEN_FIELD: u64 = 0;

#[test]
fn field_passes_are_bit_identical() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1800);
    let got = field_hash(&mut app);
    assert_ne!(
        GOLDEN_FIELD, 0,
        "no field golden is pinned for this architecture yet (goldens are PER-PLATFORM). This run \
         measured {got:#018x}."
    );
    assert_eq!(
        got, GOLDEN_FIELD,
        "stigmergy field grids drifted from the golden — the evaporate/diffuse/hotspot floor-cell \
         iteration is no longer bit-identical to the full-grid scan"
    );
}

#[test]
fn authored_world_config_override_is_a_noop() {
    // Phase-2 seam identity: installing the *shipped* world (decoded from the authored world genome) through
    // `SimConfig::config` must be byte-identical to installing nothing. This pins the whole
    // encode → decode → WorldConfig → GameConfig(ai_tuning, sim) → running-sim path as lossless — it must
    // reproduce the Phase-1 golden exactly. If the override seam or encode/decode drifted a single knob,
    // this reds.
    use foundation_vs_slop::squad_ai::world_genome::{authored, decode};
    let _serial = serial_guard();
    let authored_world = decode(&authored()).expect("the authored world genome decodes");
    let cfg = SimConfig::deterministic_core().with_world_config(authored_world);
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1800);
    assert_eq!(
        snapshot_hash(&mut app),
        // Tracks the Phase-1 actor golden. It stays byte-identical to it because `authored()` encodes the
        // parasite counts straight from `SimTuning::default()` (world_genome.rs), and the new values
        // (initial_count 6, manca_count_max 20) sit inside the genome's normalization bounds (1–12, 4–40) so
        // encode→decode is still lossless. Tracks the Almond Water re-pins (incl. the sparse-spring
        // seep-model re-pin, the belief/inversion re-pin, the belief-modulated forage re-pin) and the
        // crab-determinism re-pin.
        //
        // This REFERENCES `GOLDEN` rather than repeating its literal. It used to be a hand-maintained copy
        // of the same hex — two places holding one fact, free to drift apart silently. Worse, that duplicate
        // is why `train apply`'s `repin_replay` did an unbounded whole-file `str::replace` to keep them in
        // step, which also rewrote the value wherever it appeared in PROSE (the incident log above quotes
        // hashes deliberately). One fact, one declaration site: the tracking is now a compile-time identity.
        GOLDEN,
        "installing the authored world config changed the sim — the override seam or encode/decode is lossy"
    );
}

#[test]
fn every_world_config_slice_reaches_the_game_config() {
    // Seam guard for the class FVS-I-7 fell into: the gore slice was wired through encode/decode,
    // `apply_dim`, the artifacts doc, and the `train apply` splice — every seam except the ONE that scores
    // rollouts (`build_headless_app`'s `cfg.config` block) — so a world search would have assigned fitness
    // to the 8 gore genes against the authored slice. The behavioural pair above cannot see that shape:
    // `a_mutated_world_config_changes_the_sim` mutates every slice at once, so the hash moves on the wired
    // slices and the dropped one hides. This test perturbs ONE representative knob per `WorldConfig` slice
    // and asserts each individually arrives in `GameConfig` — a slice wired everywhere but the seam fails
    // here, not in a five-hour bake. For gore it also checks the `GoreSettings` resource `GorePlugin`
    // cloned out of `gc.gore` at plugin build (the copy the systems actually read), pinning the
    // seam-before-plugin ordering the apply relies on.
    use foundation_vs_slop::config::GameConfig;
    use foundation_vs_slop::gore::{GoreDynamics, GoreSettings};
    use foundation_vs_slop::squad_ai::world_genome::{authored, decode};
    let _serial = serial_guard();

    let mut w = decode(&authored()).expect("the authored world genome decodes");
    // Exact-in-f32 nudges, small enough to sit inside every slice validator's range.
    w.ai.fields.scent.evaporate += 0.0625;
    w.sim.fear.per_crab += 0.0625;
    w.mold.growth += 0.0625;
    w.almond.strong_seep += 0.0625;
    w.lighting.field_intensity += 0.0625;
    w.gore.max_gibs += 7;

    let cfg = SimConfig::deterministic_core().with_world_config(w.clone());
    let app = build_headless_app(&cfg);

    let gc = app.world().resource::<GameConfig>();
    assert_eq!(gc.ai_tuning.fields.scent.evaporate, w.ai.fields.scent.evaporate, "ai slice never applied at the seam");
    assert_eq!(gc.sim.fear.per_crab, w.sim.fear.per_crab, "sim slice never applied at the seam");
    assert_eq!(gc.mold.growth, w.mold.growth, "mold slice never applied at the seam");
    assert_eq!(gc.almond_water.strong_seep, w.almond.strong_seep, "almond slice never applied at the seam");
    assert_eq!(gc.lighting.field_intensity, w.lighting.field_intensity, "lighting slice never applied at the seam");
    assert_eq!(
        GoreDynamics::from_config(&gc.gore),
        w.gore,
        "gore slice never applied at the seam"
    );
    assert_eq!(
        GoreDynamics::from_config(app.world().resource::<GoreSettings>()),
        w.gore,
        "`GorePlugin` cloned `gc.gore` before the seam wrote it — the seam-before-plugins ordering regressed"
    );
}

#[test]
fn every_director_dialled_slice_reaches_the_run_build() {
    // The run-build sibling of the seam guard above, and the acceptance test for FVS-H-8.
    //
    // `director::pick_next_challenge` samples a cell from the level archive on `OnEnter(Active)` and
    // writes four `GameConfig` fields — `dungeon`, `mycelia`, `placement.metropolis`,
    // `placement.density`. Every consumer snapshotted its slice into a resource at **plugin-build**
    // time and never read `GameConfig` again, so all four writes were dead: the log said a challenge
    // was sampled, FVS-H-7's briefing announced `BRANCH UNIVERSE {seed} · SECTOR x,y`, and every
    // expedition was the authored world. That is worse than a silent no-op — the player perceived a
    // distinction that did not exist.
    //
    // The defect only exists on the SECOND expedition, which is why no existing test could see it: the
    // first run's snapshot is correct by construction. So this drives a real campaign shape —
    // expedition, `RETURN TO SITE`, expedition — and dials `GameConfig` between them exactly as the
    // director does. `RunBuild::Config` is what makes the second run see it.
    //
    // Coverage, stated honestly: `dungeon` and `placement.density` are asserted directly, and
    // `Dungeon::width` proves the dialled slice reached *generation* rather than merely a resource.
    // `placement.metropolis` has no reader outside `Orchestrator`'s type-erased `Box<dyn Solver>`, so
    // it is not asserted here — it is written by the same unconditional `resnapshot_placement_config`
    // body as `Density`, so a `Density` pass means that system ran. `mycelia` cannot be asserted at
    // all in this build: `MyceliaPlugin` is GPU/windowed-only and deliberately absent from the harness
    // (`sim_harness.rs:402`), and its re-snapshot system lives inside that same plugin — resource and
    // refresher live and die together.
    use bevy::prelude::{NextState, State};
    use foundation_vs_slop::config::GameConfig;
    use foundation_vs_slop::dungeon::{Dungeon, DungeonConfigRes};
    use foundation_vs_slop::placement::furnish::Density;
    use foundation_vs_slop::session::RunState;
    let _serial = serial_guard();

    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    // `begin_first_run` is `PostStartup`; the transition lands on the following frame.
    step(&mut app, &cfg, 2);
    assert_eq!(
        app.world().resource::<State<RunState>>().get(),
        &RunState::Active,
        "the first expedition never started — the rest of this test would be vacuous"
    );

    let authored_rooms = app.world().resource::<Dungeon>().regions.len();
    assert_eq!(
        app.world().resource::<DungeonConfigRes>().0.coarse_w,
        6,
        "authored `coarse_w` moved; re-pick the dial below so it still differs"
    );
    assert_eq!(
        app.world().resource::<Density>().0.wall_lights_per_room,
        6,
        "authored `wall_lights_per_room` moved; re-pick the dial below so it still differs"
    );

    // Dial `GameConfig` mid-campaign, the way `elite_overlay::apply_dim(Dim::Levels, …)` does.
    //
    // ⚠️ `coarse_w * block` MUST stay 192. That is not a style preference: `CONTROL_SIZE = 192` is the
    // world extent the mycelia field and the dungeon both assume, `mycelia::habitat::build` refuses any
    // other size loudly, and `level_genome::FACTORS` is *defined* as the four pairs that preserve it —
    // so the archive can only ever trade block size against block count. Dialling `coarse_w` alone
    // (the obvious way to write this test) produces a 256x192 dungeon no director pick can produce and
    // panics `bake_mold`/`bake_almond_sources`. This moves `(6, 32)` -> `(8, 24)`: same 192 tiles per
    // side, 36 room slots instead of 64 — a real archive cell, and a visibly different level.
    {
        let mut gc = app.world_mut().resource_mut::<GameConfig>();
        gc.dungeon.coarse_w = 8;
        gc.dungeon.coarse_h = 8;
        gc.dungeon.block = 24;
        gc.dungeon.doorway_ratio = 0.25;
        gc.placement.density.wall_lights_per_room = 11;
        gc.placement.metropolis.iterations += 1;
    }

    // `RETURN TO SITE`, then back through the ASYNC door — the transition pair `ui::debrief` drives.
    app.world_mut().resource_mut::<NextState<RunState>>().set(RunState::Idle);
    step(&mut app, &cfg, 2);
    app.world_mut().resource_mut::<NextState<RunState>>().set(RunState::Active);
    step(&mut app, &cfg, 2);

    let dialled = &app.world().resource::<DungeonConfigRes>().0;
    assert_eq!(
        (dialled.coarse_w, dialled.block, dialled.doorway_ratio),
        (8, 24, 0.25),
        "the dialled `dungeon:` slice never reached `DungeonConfigRes` — the plugin-build snapshot is \
         still winning, so every expedition is the authored world (FVS-H-8)"
    );
    assert_eq!(
        app.world().resource::<Density>().0.wall_lights_per_room,
        11,
        "the dialled `placement.density` never reached `Density` — `resnapshot_placement_config` did \
         not run, which also means `PlacementSolvers` still holds the authored Metropolis weights"
    );

    // The player-observable half: the dial has to reach *generation*, not just a resource. Room count,
    // not extent — the extent is pinned at 192 (see the dial comment above), so the coarse
    // factorisation is what a player actually sees change.
    assert_ne!(
        app.world().resource::<Dungeon>().regions.len(),
        authored_rooms,
        "the second expedition laid out the same rooms as the first despite a dialled coarse \
         factorisation — a Branch universe that is the authored world with a different label"
    );
}

#[test]
fn a_mutated_world_config_changes_the_sim() {
    // The dual of the no-op test: a *mutated* world genome, installed the same way, must change
    // `snapshot_hash`. Proves the config actually reaches the running sim (crab fields/fear, combat,
    // economy) rather than being silently dropped — the world-population analogue of
    // `search_calibration::a_candidate_genome_actually_changes_the_simulation`.
    use foundation_vs_slop::rng::seeded;
    use foundation_vs_slop::squad_ai::world_genome::{authored, decode, mutate};
    let _serial = serial_guard();

    let base = SimConfig::deterministic_core()
        .with_world_config(decode(&authored()).expect("decode authored"));
    let mut a = build_headless_app(&base);
    step(&mut a, &base, 600);
    let ha = snapshot_hash(&mut a);
    drop(a);

    // A large sigma so many knobs (field rates, fear gains, combat, economy) move unmistakably.
    let mutant = mutate(&authored(), 1.0, &mut seeded(0xB0A7)).expect("mutate");
    let mcfg = SimConfig::deterministic_core().with_world_config(decode(&mutant).expect("decode mutant"));
    let mut b = build_headless_app(&mcfg);
    step(&mut b, &mcfg, 600);
    let hb = snapshot_hash(&mut b);

    assert_ne!(
        ha, hb,
        "a mutated world config produced an identical sim — the config override is not reaching gameplay"
    );
}

#[test]
fn a_mutated_audio_config_changes_the_sim() {
    // The acoustic-stimulus analogue of `a_mutated_world_config_changes_the_sim`. Audio only reaches agents
    // THROUGH din, and din is only emitted by combat — so a bare `build + step` with no player never fights,
    // makes no din, and the knobs are correctly inert (that is why the shipped no-player golden above is
    // unchanged by this branch — expected, not a bug). So this drives a real episode through `rollout`.
    //
    // The lever that bites in the OFFLINE rollout is `unit_fear_of_din`. The squad never fires here (crabs
    // die to the boss cull, not gunfire — measured: zero THREAT_GUN deposits on every held-in seed), so
    // NOISE_SQUAD is empty and the crab-side din (fear + the investigate draw) is dormant offline — those
    // are live-play features. But crab DEATHS fill NOISE_SWARM every episode, and the additive
    // `DriveRule::TrackMaxPlusDin` lets that din lift the squad's FEAR above the (saturated) crab-menace it
    // co-occurs with — where a `max` reduction would drown it. So a cranked `unit_fear_of_din` provably
    // moves the squad, which is exactly the additive-din gradient the audio search climbs.
    //
    // `rollout` takes `serial_guard` internally, so this test must NOT hold it (a second lock deadlocks).
    use foundation_vs_slop::ai::brain::BrainSource;
    use foundation_vs_slop::audio_tuning::AudioTuning;
    use foundation_vs_slop::squad_ai::evaluate::rollout;

    let seed = 0x5C09191;
    let ticks = 1800;

    let base = rollout(BrainSource::Authored, None, None, None, seed, ticks);

    // Crank the din-fear gains off their dormant (0.0) default. `unit_fear_of_din` reacts to the crab-death
    // din (NOISE_SWARM), which the rollout actually produces; `crab_fear_of_din` is the swarm analogue,
    // dormant offline (no gunfire → no NOISE_SQUAD) but set here to document the intended symmetric lever.
    let mut audio = AudioTuning::default();
    audio.perception.unit_fear_of_din = 0.5;
    audio.perception.crab_fear_of_din = 0.5;
    let mutant = rollout(BrainSource::Authored, None, Some(audio), None, seed, ticks);

    // DECISIVE: the final actor state (Transform+Health) must differ. Same world, brains and seed — the ONLY
    // difference is the audio slice, so a changed final state proves the acoustic din reaches gameplay.
    assert_ne!(
        base.snapshot, mutant.snapshot,
        "a cranked audio config produced a byte-identical final state — the acoustic coupling is inert"
    );
}

#[test]
fn manca_dread_reaches_the_shared_anomaly_field() {
    // FIX 1 regression guard. Roused SCP-150 mancae deposit `THREAT_ANOMALY` via `deposit_manca_dread`, so
    // the brood is legible to the squad's anomaly-fear machinery + psi-vision instead of being a silent
    // parallel AI stack. A/B on the new `manca_dread_rate` knob (mutate-tuning-at-the-seam, exactly as
    // `photophobia_pulls_crabs_into_shadow` overrides `photophobic_gain`): at rate 0 the deposit lays
    // `amount = 0·dt = 0` and the field matches the dread-off baseline; at the shipped rate the golden run's
    // roused mancae fill THREAT_ANOMALY cells, so `field_hash` differs. This pins that the deposit is wired
    // to the knob and gated on a positive rate. The READ side — units fear THREAT_ANOMALY — is pinned
    // separately by `ai::tests::units_fear_every_hostile_creature_channel`; the two together cover the whole
    // write→read coupling the fix restores.
    use foundation_vs_slop::sim::SimTuning;
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let field_at_rate = |rate: f32| -> u64 {
        let mut app = build_headless_app(&cfg);
        app.world_mut().resource_mut::<SimTuning>().deposit.manca_dread_rate = rate;
        // Rouse the freshly-spawned brood directly, then sample a few ticks later, so the dread A/B is
        // independent of the emergent rouse. Adding the `Biological` marker to units/crabs (for Almond Water
        // healing) shifted their archetypes and thus the deterministic iteration order, so the shipped mancae
        // no longer happen to be roused-and-depositing at tick 1800 for this seed — collapsing the A/B (in
        // fact they now embed within ~2 ticks in this trajectory). Rousing them the instant they spawn and
        // sampling 3 ticks on — while they still hold the huddle and deposit dread — keeps `manca_dread_rate`
        // the ONLY variable between the two arms. (`rouse_all_mancae` parks the calm timer so they can't
        // re-settle to Dormant mid-window; cranking `rouse_proximity` instead over-rouses them into a mass
        // embed→despawn, so the `THREAT_ANOMALY` deposit has already evaporated by the sample — timing-fragile.)
        step(&mut app, &cfg, 1); // one update spawns the mancae (PostStartup); grab them before any embed
        let roused = foundation_vs_slop::parasite::rouse_all_mancae(&mut app);
        assert!(roused > 0, "the sim must have mancae to rouse");
        step(&mut app, &cfg, 3); // deposit dread while roused, before they embed and despawn
        field_hash(&mut app)
    };
    assert_ne!(
        field_at_rate(0.0),
        field_at_rate(0.1),
        "manca_dread_rate had no effect on the field grids — deposit_manca_dread is not reaching \
         THREAT_ANOMALY (a roused brood would stay invisible to the squad's dread + psi-vision)"
    );
}

#[test]
fn deterministic_core_is_bit_identical_across_many_builds() {
    // Stronger guard than the two-build check above. Entity enumeration order is NOT stable across
    // same-seed `App` instances in one process (GLB scene-child instantiation + entity-id reuse permute
    // it), so any gameplay decision that keys on iteration order — a "keep the first on a tie" pick, a
    // non-associative float sum over an entity list, a value fed by an async-loaded asset — diverges
    // only intermittently. The two-build test catches such a bug just ~1% of the time, so it slipped
    // through for months; building MANY apps and hashing each makes a per-instance-order dependence fail
    // reliably. Keep N high enough that a ~1%-per-build regression is caught essentially every run.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();

    let mut reference: Option<u64> = None;
    for build in 0..24 {
        let mut app = build_headless_app(&cfg);
        step(&mut app, &cfg, 180);
        let h = snapshot_hash(&mut app);
        match reference {
            None => reference = Some(h),
            Some(r) => assert_eq!(
                h, r,
                "physics-free core diverged on build {build}: gameplay must not depend on entity \
                 enumeration order (see util::nearest_planar / crab::assign_meat_targets)"
            ),
        }
    }
}

/// **The G0 guard** — the oracle this project needed and never had.
///
/// `deterministic_core_is_bit_identical_across_many_builds` misses G0 *by construction*: 180 ticks with no
/// synthetic player, so the squad idles at spawn and **never fires**. G0 lived in `laser::fire_laser`, which
/// only runs once a firefight starts — so the strongest guard in the suite was blind to it for months. This
/// runs the SYNTHETIC PLAYER at the search's real episode length (7200 ticks, matching
/// `search_parallel::EPISODE_TICKS`) and demands every rollout agree bit-for-bit.
///
/// **Why the background load is load-bearing, not paranoia.** G0 was a race whose outcome depended on ECS
/// enumeration order, and on an *idle* box that order came out the same way every single time: 12 identical
/// rollouts in one process, and 5 identical across fresh processes, all while the bug was live. It only
/// split into distinct outcomes when the machine was busy. A quiet CI runner would therefore green-light a
/// reintroduced G0 every time. The busy threads below are plain OS threads outside Bevy — they do not touch
/// the sim (which stays pinned to one compute thread, asserted in `build_headless_app`); they only contend
/// for cores so the scheduler actually varies. Without them this test is decoration.
///
/// **Why TWO seeds, not one.** This test shipped covering only `0x5C09191` and passed 12/12 — while
/// `0xA11CE` split **3 ways on an idle box**. The guard was green on a lucky seed. A reproducibility
/// guarantee is a property of the SIM, not of one dungeon: a single seed only exercises the layouts, spawn
/// positions, and fights that seed happens to produce, and order-dependence needs the contended path to
/// actually occur (invariant 11).
///
/// **`0xA11CE` is kept as a determinism STRESSOR, not as a search world.** It is no longer held-in — the
/// mold retired it and `0xBEEF` into squad wipes, and `coevolve::HELD_IN_SEEDS` is the live set. Its value
/// to *this* test never depended on the search running it; it earned its place by splitting. An earlier
/// version of this note called it "the search's *other* held-in world" and claimed these were "the exact
/// seeds `train prior` sweeps": both went stale at the re-selection, and the stale claim survived long
/// enough to send a later reader re-tuning the episode floor against a world the search never runs.
///
/// Do NOT add `serial_guard()`: `evaluate::run_episode` takes it internally and `HARNESS_LOCK` is not
/// reentrant, so holding it here deadlocks (same trap as `a_mutated_audio_config_changes_the_sim`).
#[test]
fn search_rollouts_are_reproducible_under_load() {
    use foundation_vs_slop::ai::brain::BrainSource;
    use foundation_vs_slop::squad_ai::evaluate::rollout;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Enough reps to catch a regression reliably: G0 split ~30% of rollouts under load, so 12 reps miss it
    /// with probability ~0.7^11 < 2%. Cheap enough for the harness lane (~3 min per seed).
    const REPS: usize = 12;
    const TICKS: u32 = 7200;
    /// One held-in world + one retired-but-splitty stressor. NOT the search's held-in set (that is
    /// `coevolve::HELD_IN_SEEDS`) — see the note above on why this test wants a splitter, not a search world.
    const SEEDS: [u64; 2] = [0x5C09191, 0xA11CE];

    let stop = Arc::new(AtomicBool::new(false));
    let load: Vec<_> = (0..8)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x: u64 = 0;
                while !stop.load(Ordering::Relaxed) {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                }
                x
            })
        })
        .collect();

    let mut split: Vec<(u64, Vec<(u64, usize)>)> = Vec::new();
    for seed in SEEDS {
        let mut seen: Vec<(u64, usize)> = Vec::new();
        for _ in 0..REPS {
            let r = rollout(BrainSource::Authored, None, None, None, seed, TICKS);
            let key = (r.snapshot, r.trace.decisions.len());
            if !seen.contains(&key) {
                seen.push(key);
            }
        }
        if seen.len() > 1 {
            split.push((seed, seen));
        }
    }

    stop.store(true, Ordering::Relaxed);
    for t in load {
        let _ = t.join();
    }

    assert!(
        split.is_empty(),
        "G0 REGRESSION: {REPS} identical rollouts produced more than one outcome on {} of {} held-in \
         seed(s): {split:x?} — the offline search is scoring against a wobbling objective again, so a \
         MAP-Elites cell can be won by evaluation luck rather than by the genome. Look for a gameplay \
         decision keyed on ECS query order (a shared-RNG draw, a non-associative float sum, or a \
         keep-the-first-on-a-tie pick) — see docs/rl/2026-07-16-search-rollout-nondeterminism.md",
        split.len(),
        SEEDS.len(),
    );
}

// The G0 localization probe that used to live here is GONE (2026-07-27).
//
// It was explicitly labelled TEMP — "Remove once the tie-break is found" — and G0 *was* found and
// fixed (`docs/rl/2026-07-16-search-rollout-nondeterminism.md`: four causes, all pinned). What it cost
// to keep: **25 full 7200-tick episodes under 8 busy-loop threads**, which is ~53 minutes, the
// overwhelming majority of the whole harness lane's runtime.
//
// The property it was diagnosing is still pinned, twice, by tests that assert rather than merely
// report: `search_rollouts_are_reproducible_under_load` and
// `search_rollouts_of_mutants_are_reproducible_under_load` both run replicate rollouts under the same
// 8-thread load and fail on any split. A *localizer* — which reports the earliest divergent tick and
// which oracle split first — is the right tool once one of those goes red, and it is 40 lines of
// `trace_episode` to write when that happens. Paying an hour per CI run to keep it warm is not.
//
// This matters beyond tidiness: FVS-J-5 wants the harness lane promoted to a hard merge gate, and a
// lane nobody will wait for does not get promoted.

#[test]
fn core_state_evolves_over_time() {
    // Guards against a dead sim silently "passing" repeatability: state after 180 ticks must differ from
    // the freshly-spawned state (things actually moved / fought / were born). Physics-free so it's stable.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1);
    let early = snapshot_hash(&mut app);
    step(&mut app, &cfg, 179);
    let late = snapshot_hash(&mut app);
    assert_ne!(early, late, "the simulation should evolve — state must change over 180 ticks");
}

#[test]
fn speed_setting_is_deterministic_and_has_effect() {
    // The speed knob (`Time<Virtual>` relative speed) drives fast-forward without compromising
    // determinism: two runs at the same non-unit speed reach the same state, and a higher speed advances
    // the sim further per update.
    //
    // NOTE we deliberately do NOT assert exact equality ACROSS different speeds. The pinned sim advances
    // by a fixed sub-step, but cosmetic per-frame `Update` systems that legitimately touch the wall clock
    // — hitstop scaling `Time<Virtual>`, etc. — run once per update regardless of how many fixed
    // sub-steps that update contains, so the sub-step COUNT can differ by one across speeds. Same-seed /
    // same-speed reproducibility is the guarantee (see `deterministic_core_is_bit_identical`).
    let _serial = serial_guard();
    let fast = SimConfig { speed: 2.0, ..SimConfig::deterministic_core() };

    let mut a = build_headless_app(&fast);
    step(&mut a, &fast, 90);
    let ha = snapshot_hash(&mut a);
    drop(a);

    let mut b = build_headless_app(&fast);
    step(&mut b, &fast, 90);
    let hb = snapshot_hash(&mut b);
    assert_eq!(ha, hb, "same seed at the same speed must be reproducible");

    // 2× speed for 90 updates advances further than 1× for 90 updates.
    let base = SimConfig::deterministic_core();
    let mut c = build_headless_app(&base);
    step(&mut c, &base, 90);
    let hc = snapshot_hash(&mut c);
    assert_ne!(ha, hc, "a higher speed must advance the sim further per update");
}

#[test]
fn ui_never_leaks_into_deterministic_core() {
    // Determinism firewall. The windowed `UiPlugin` (states, HUD, menus) is registered only in
    // `lib::run`, never in the harness — so its `AppState` must be absent here. The pause resources
    // `UserPaused`/`SimBlocked` DO exist (owned by `TimeControlPlugin`), but the UI is their only
    // writer, so in the headless core they must stay at their inert `false` defaults. A stray
    // `SimBlocked=true` would freeze replay; this asserts that can't happen.
    use bevy::prelude::State;
    use foundation_vs_slop::time_control::{OrdersBlocked, SimBlocked, UserPaused};
    use foundation_vs_slop::ui::state::AppState;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 5);

    assert!(
        app.world().get_resource::<State<AppState>>().is_none(),
        "UI AppState must not exist in the headless deterministic core"
    );
    assert!(
        !app.world().resource::<SimBlocked>().0,
        "SimBlocked must stay false in the core (no UI writer present)"
    );
    assert!(
        !app.world().resource::<OrdersBlocked>().0,
        "OrdersBlocked must stay false in the core — `selection`'s order input is gated on it, so a \
         stray `true` would silently stop the harness issuing move orders. Same contract as \
         SimBlocked: owned by TimeControlPlugin, written only by `ui::state::sync_order_block`."
    );
    assert!(
        !app.world().resource::<UserPaused>().0,
        "UserPaused must stay false in the core (no key input present)"
    );
}

#[test]
fn ui_screens_spawn_and_pause_blocks_the_sim() {
    // OPERABILITY liveness (`docs/ui.md` §1.5): boot the *real* windowed UI headless and prove
    // the screens actually spawn and the state flow works — the substitute for a pixel screenshot,
    // which this headless env can't produce (no monitor → black drawable). Not a determinism test:
    // it builds its own UI-inclusive app; the core reference app (`build_headless_app`) is untouched.
    use bevy::prelude::*;
    use foundation_vs_slop::sim_harness::build_headless_app_unfinished;
    use foundation_vs_slop::time_control::SimBlocked;
    use foundation_vs_slop::input::KeyBindings;
    use foundation_vs_slop::ui::containment_hud::ContainmentHudRoot;
    use foundation_vs_slop::ui::controls_screen::{control_lines, ControlsRoot};
    use foundation_vs_slop::ui::hint::HintRoot;
    use foundation_vs_slop::ui::hud::{BossBarRoot, HudRoot, RosterStripRoot, SpeedText};
    use foundation_vs_slop::ui::layout::{HudFrame, Region, RegionNode};
    use foundation_vs_slop::ui::pause::PauseRoot;
    use foundation_vs_slop::ui::verb_bar::VerbBarRoot;
    use foundation_vs_slop::ui::state::{AppState, MenuState};
    use foundation_vs_slop::ui::UiPlugin;

    let _serial = serial_guard();
    // Redirect settings IO to a temp dir so the test never writes the real user config.
    // SAFETY: `serial_guard` is held, so this is the only thread touching the environment.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", std::env::temp_dir().join("fvs_ui_liveness"));
    }

    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app_unfinished(&cfg);
    app.add_plugins(UiPlugin);
    app.finish();
    app.cleanup();

    // Boot gates to the title (font-ready or its frame cap) within a few dozen frames.
    for _ in 0..40 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<State<AppState>>().get(),
        &AppState::Title,
        "boot should reach the title screen"
    );
    assert!(
        app.world().resource::<SimBlocked>().0,
        "the title screen must block the sim underneath it"
    );

    // Enter the game → HUD spawns, sim unblocks.
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    app.update();
    assert!(
        !app.world().resource::<SimBlocked>().0,
        "in-game with no menu open must unblock the sim"
    );
    // The HUD is up. Asserted by its NAMED PARTS rather than by a root count: the elements live in
    // three different layout regions, so "the HUD spawned" is three entities, and a count is the
    // wrong oracle — it would pass for three copies of the boss bar and fail for a correct HUD.
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<HudRoot>>();
        assert!(
            q.iter(app.world()).count() >= 1,
            "the HUD should spawn on entering the game"
        );
    }
    for (name, present) in [
        ("speed readout", {
            let mut q = app.world_mut().query_filtered::<Entity, With<SpeedText>>();
            q.iter(app.world()).next().is_some()
        }),
        ("squad roster strip", {
            let mut q = app
                .world_mut()
                .query_filtered::<Entity, With<RosterStripRoot>>();
            q.iter(app.world()).next().is_some()
        }),
        ("boss bar", {
            let mut q = app.world_mut().query_filtered::<Entity, With<BossBarRoot>>();
            q.iter(app.world()).next().is_some()
        }),
    ] {
        assert!(present, "HUD {name} should exist in game");
    }

    // LAYOUT liveness (`docs/ui.md` §1.5). Every panel is parented into a region of the 3×3 frame;
    // a panel that failed to resolve its region would silently not render at all. Assert the frame
    // is up and that all nine regions exist exactly once — the machine-checkable form of the
    // overlap bug that had `containment_hud` and the roster strip drawing on top of each other.
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<HudFrame>>();
        assert_eq!(q.iter(app.world()).count(), 1, "exactly one layout frame in game");
    }
    {
        let mut q = app.world_mut().query::<&RegionNode>();
        let mut seen: Vec<Region> = q.iter(app.world()).map(|r| r.0).collect();
        assert_eq!(seen.len(), 9, "the frame should own nine regions");
        for region in Region::ALL {
            let n = seen.iter().filter(|r| **r == region).count();
            assert_eq!(n, 1, "{region:?} should exist exactly once, found {n}");
        }
        seen.clear();
    }
    // Every in-game panel resolved into a region rather than vanishing.
    for (name, present) in [
        ("containment readout", {
            let mut q = app
                .world_mut()
                .query_filtered::<Entity, With<ContainmentHudRoot>>();
            q.iter(app.world()).next().is_some()
        }),
        ("verb bar", {
            let mut q = app.world_mut().query_filtered::<Entity, With<VerbBarRoot>>();
            q.iter(app.world()).next().is_some()
        }),
        // The controls hint. It is the panel most likely to vanish unnoticed, because its *content*
        // is legitimately empty once the player has learned the key — so "nothing on screen" is a
        // valid state and only the root entity distinguishes "retired" from "never resolved its
        // region". `MidCenter` had no occupant before this, which is exactly the case
        // `layout::panel_in` returns `None` for without anyone noticing.
        ("controls hint", {
            let mut q = app.world_mut().query_filtered::<Entity, With<HintRoot>>();
            q.iter(app.world()).next().is_some()
        }),
    ] {
        assert!(present, "{name} should be parented into a layout region");
    }

    // Open the pause menu → overlay spawns, sim blocks again.
    app.world_mut()
        .resource_mut::<NextState<MenuState>>()
        .set(MenuState::Pause);
    app.update();
    app.update();
    assert!(
        app.world().resource::<SimBlocked>().0,
        "the pause menu must block the sim"
    );
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<PauseRoot>>();
        assert!(q.iter(app.world()).next().is_some(), "pause overlay should spawn");
    }

    // The controls screen (`docs/ui.md` §1.5: extend this test when you add a screen). It carries
    // the game's only complete statement of what the keys do, and a screen that silently fails to
    // spawn renders NOTHING — the exact failure mode this whole test exists to catch.
    app.world_mut()
        .resource_mut::<NextState<MenuState>>()
        .set(MenuState::Controls);
    app.update();
    app.update();
    assert!(
        app.world().resource::<SimBlocked>().0,
        "the controls screen must block the sim like every other overlay"
    );
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<ControlsRoot>>();
        assert!(
            q.iter(app.world()).next().is_some(),
            "the controls screen should spawn"
        );
    }
    // And it must actually have content. An empty list would still satisfy "the root spawned",
    // which is precisely the kind of pass `docs/ui.md` §1.4 calls a bug that reads as a feature.
    {
        let bindings = app.world().resource::<KeyBindings>();
        assert!(
            control_lines(bindings).len() >= 20,
            "the controls screen listed almost nothing — the registry is not reaching it"
        );
    }
}

#[test]
fn full_sim_stays_live() {
    // Full physics-inclusive sim (the real production plugin set). Not exact-hashable (Avian isn't
    // bit-reproducible), so we assert LIVENESS every 30 ticks over ~5 s: no panic, no NaN transforms, no
    // out-of-range health, no runaway spawn. This is the soft-lock / crash net (Stage 4 in miniature).
    let _serial = serial_guard();
    let cfg = SimConfig::default();
    let mut app = build_headless_app(&cfg);
    for checkpoint in 1..=10 {
        step(&mut app, &cfg, 30);
        let v = liveness_violations(&mut app);
        assert!(v.is_empty(), "liveness violated at tick {}: {v:?}", checkpoint * 30);
    }
}

/// **Does photophobia bias crabs toward darker ground?**
///
/// # Why one seed at 360 ticks was the wrong question
///
/// This asserted `mean_on < mean_off` on the shipped seed after 360 ticks, and it was **red** — with
/// `on=0.195 off=0.114`, i.e. inverted rather than merely unmet. The mechanism was never wrong.
/// Measured across five seeds and five horizons (2026-08-05):
///
/// | ticks | seeds with `on < off` | pooled off → on |
/// |---|---|---|
/// | 30 | 5/5 | 0.772 → 0.568 |
/// | 120 | 4/5 | 0.317 → 0.199 (−37%) |
/// | 240 | 4/5 | 0.328 → 0.202 (−38%) |
/// | 360 | **1/5** | 0.138 → 0.204 (inverted) |
///
/// So the effect is real, large and early, and by 360 ticks it is gone. Two things cause that, and
/// both say the horizon was the defect:
///
/// * **The push stops acting once a crab arrives.** `light_push` is zero on a flat field, and the
///   probe counted how many crabs stand on one: with the gain off, 26–38 of 40 crabs have diffused
///   into flat *deep dark* by tick 360 — because unbiased crabs random-walk, and most of the map is
///   dark. Their mean illuminance falls for a reason that has nothing to do with light response.
/// * **Photophobia is a within-patch effect.** `crab_locomotion` runs the push through
///   `clamp_to_patch` on purpose — *"gate crossings stay with the mode's flow-field"* — so a
///   photophobic crab settles at the darkest point of *its own surface patch*, which is generally
///   mid-gradient. That is why 13–19 of 40 are still on a gradient at tick 360 while the unbiased
///   arm has left. Over a long horizon, diffusion into other rooms beats steering within one.
///
/// Whether the push *should* cross patches is a design question, not a bug, and it is in `BACKLOG.md`
/// rather than decided here. The oracle's job is to check the claim the feature makes.
///
/// # What it asserts now
///
/// Crabs **pooled across five dungeon seeds** at 120 ticks, so one unlucky dungeon cannot decide it —
/// the shipped seed is exactly such a dungeon, and it is deliberately still in the set rather than
/// swapped out. A single-seed A/B on a chaotic sim compares two decorrelated worlds and reads their
/// difference as an effect.
#[test]
fn photophobia_pulls_crabs_into_shadow() {
    // Ecosystem liveness (Phase 2): crabs carry `light::Photophobic` and steer down the `LightField`
    // gradient, so they should settle into darker cells than they otherwise would. A/B isolation — the
    // SAME seed and tick count, differing ONLY in `lighting.photophobic_gain` (shipped vs 0) — so any gap
    // in mean illuminance-at-crabs is caused by the photophobia and nothing else. Behavioural oracle over
    // the light field, not an exact hash (Physarum-style photoavoidance, Nakagaki et al., PRL 2007).
    use bevy::prelude::{Transform, Vec3, With};
    use foundation_vs_slop::config::GameConfig;
    use foundation_vs_slop::crab::Crab;
    use foundation_vs_slop::dungeon::Dungeon;
    use foundation_vs_slop::light::LightField;
    use foundation_vs_slop::sim_harness::build_headless_app_unfinished;

    fn mean_crab_light(cfg: &SimConfig, gain_override: Option<f32>, ticks: u32) -> f32 {
        let mut app = build_headless_app_unfinished(cfg);
        // `photophobic_gain` is read live by `crab_locomotion` (not at plugin build), so overriding it
        // here before stepping cleanly selects the A/B arm — the "mutate GameConfig at the seam" trick the
        // harness already uses for `dungeon_seed`.
        if let Some(g) = gain_override {
            app.world_mut().resource_mut::<GameConfig>().lighting.photophobic_gain = g;
        }
        // Isolate photophobia from Almond Water too: crabs are `Biological`, so they heal in seeps (which
        // reshapes which crabs survive to be measured) and a wounded crab forages toward water (which
        // competes with the light gradient). Zero both so this measures the light response alone — the same
        // "mutate tuning at the seam" isolation the parasite zeroing below uses.
        {
            let mut gc = app.world_mut().resource_mut::<GameConfig>();
            gc.almond_water.heal_rate = 0.0;
            gc.almond_water.forage_gain = 0.0;
        }
        // Isolate the variable under study (photophobia) from the SCP-150 parasite: zero the initial mancae
        // so their embed-damage can't trip the crab alarm → muster, which pulls crabs OUT of shadow and
        // would mask the light response. Same "mutate tuning at the seam" trick as the gain override above.
        app.world_mut()
            .resource_mut::<foundation_vs_slop::sim::SimTuning>()
            .parasite
            .initial_count = 0;
        app.finish();
        app.cleanup();
        step(&mut app, cfg, ticks);
        let mut q = app.world_mut().query_filtered::<&Transform, With<Crab>>();
        let positions: Vec<Vec3> = q.iter(app.world()).map(|t| t.translation).collect();
        assert!(!positions.is_empty(), "the sim must have crabs to measure");
        let dungeon = app.world().resource::<Dungeon>();
        let field = app.world().resource::<LightField>();
        positions.iter().map(|p| field.sample(dungeon, *p)).sum::<f32>() / positions.len() as f32
    }

    let _serial = serial_guard();
    // ~2 s. Long enough for the bias to move crabs a visible distance, short enough that the light
    // push is still acting on most of them rather than having parked them — see the doc above for the
    // measured curve, and why 360 ticks measured diffusion instead.
    const TICKS: u32 = 120;
    // `None` is the shipped dungeon, kept in the set on purpose: it is the seed on which the old
    // single-seed form inverted, so dropping it would be tuning the test to the answer.
    const SEEDS: [Option<u64>; 5] = [None, Some(1), Some(2), Some(3), Some(4)];

    let mut off = Vec::new();
    let mut on = Vec::new();
    for seed in SEEDS {
        let cfg = match seed {
            None => SimConfig::deterministic_core(),
            Some(s) => SimConfig::deterministic_core_seeded(s),
        };
        off.push(mean_crab_light(&cfg, Some(0.0), TICKS));
        on.push(mean_crab_light(&cfg, None, TICKS)); // shipped photophobic_gain
    }
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let (pooled_off, pooled_on) = (mean(&off), mean(&on));
    // The per-seed table, so a future failure says which dungeons moved rather than only that the
    // average did.
    let table: Vec<String> = SEEDS
        .iter()
        .zip(&off)
        .zip(&on)
        .map(|((s, o), n)| {
            let name = s.map(|s| s.to_string()).unwrap_or_else(|| "shipped".into());
            format!("{name}: off={o:.4} on={n:.4}{}", if n < o { "" } else { "  <-- not darker" })
        })
        .collect();

    assert!(
        pooled_on < pooled_off,
        "photophobic crabs (gain>0) should occupy darker cells than gain=0 crabs, pooled over          {} seeds at {TICKS} ticks: on={pooled_on:.4} off={pooled_off:.4}\n  {}",
        SEEDS.len(),
        table.join("\n  ")
    );
}

#[test]
fn dramatic_burst_is_live_and_deterministic() {
    // The SCP-150 host-burst (⅓-HP damage, chest wound, slow climb-out, blood + flesh chunks) fires only
    // after a FULL gestation — 120 s shipped, far longer than any replay-test budget — so the exact-hash
    // goldens above never see it. Force a fast gestation so the whole eruption (embed → gestate → convulse →
    // erupt → bleed → emerge) actually runs, then prove it stays LIVE (no panic / NaN / out-of-range HP /
    // runaway spawn) and DETERMINISTIC (two same-seed runs hash identically). The behavioural payoff — the
    // host SURVIVES, wounded, instead of instakilling — is verified visually; this guards that the new
    // phase machine can neither crash nor desync the pinned core.
    use foundation_vs_slop::sim::SimTuning;
    use foundation_vs_slop::sim_harness::build_headless_app_unfinished;
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let run = || {
        let mut app = build_headless_app_unfinished(&cfg);
        // Shorten gestation so embed→erupt completes inside the step budget (mutate-tuning-at-the-seam trick,
        // as `photophobia_pulls_crabs_into_shadow` does for the photophobic gain).
        app.world_mut().resource_mut::<SimTuning>().parasite.gestation_seconds = 1.0;
        app.finish();
        app.cleanup();
        for checkpoint in 1..=12 {
            step(&mut app, &cfg, 50);
            let v = liveness_violations(&mut app);
            assert!(v.is_empty(), "burst liveness violated at tick {}: {v:?}", checkpoint * 50);
        }
        snapshot_hash(&mut app)
    };
    let a = run();
    let b = run();
    assert_eq!(a, b, "the dramatic host-burst must be bit-reproducible across same-seed runs");
}

/// **FVS-J-6 fast reproducer** — the one known-red cell of
/// `search_rollouts_of_mutants_are_reproducible_under_load`, pinned: mutant #3 (drawn 4th from the
/// fixed mutant-rng stream) on world `0x5C09191`, whose bimodal snapshot pair has reproduced
/// byte-identically across commits. The full guard costs ~25 min and, by libtest name order, reports
/// this red ~35-50 min into the pass; this one cell reports it in ~2-3 min and the `a0_` prefix sorts
/// it FIRST in the binary. Ordering tests by failure-detection yield per unit cost is the Spieker et
/// al. result ("Reinforcement Learning for Automatic Test Case Prioritization and Selection in
/// Continuous Integration", ISSTA 2017, doi:10.1145/3092703.3092709). Breadth is NOT lost: the full
/// guard below runs unchanged; when FVS-J-6 closes, this stays as a cheap first-in-line canary for
/// the same class of regression. Same 8-busy-thread load environment as the full guard — the load is
/// part of the failing recipe, not decoration.
///
/// Do NOT add `serial_guard()`: `rollout` takes it internally and the guard is not reentrant.
#[test]
fn a0_fvs_j6_mutant3_on_world_0x5c09191_reproduces() {
    use foundation_vs_slop::squad_ai::coevolve::{
        brains_of, mutate_squad_feasible, mutate_swarm_feasible, SquadGenome, SwarmGenome, Templates,
    };
    use foundation_vs_slop::squad_ai::evaluate::rollout;
    use foundation_vs_slop::squad_ai::world_genome;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// The pinned red cell of the full guard (constants' rationale lives there).
    const MUTANT_INDEX: usize = 3;
    const REPS: usize = 3;
    const TICKS: u32 = 7200;
    const SEED: u64 = 0x5C09191;
    const MUTANT_RNG_SEED: u64 = 0x6D07A17;

    let t = Templates::authored();
    let mut rng = foundation_vs_slop::rng::seeded(MUTANT_RNG_SEED);
    // The mutant stream is sequential: reproducing mutant #3 means drawing #0-#2 first, exactly as the
    // full guard does — the draw loop below must stay byte-for-byte in step with the guard's.
    let mut genomes = Vec::new();
    for _ in 0..=MUTANT_INDEX {
        let squad = mutate_squad_feasible(&t, &SquadGenome::authored(&t), &mut rng)
            .expect("feasible squad mutant");
        let swarm = mutate_swarm_feasible(&t, &SwarmGenome::authored(&t), &mut rng)
            .expect("feasible swarm mutant");
        let world = world_genome::mutate(&world_genome::authored(), 0.15, &mut rng)
            .expect("feasible world mutant");
        genomes.push((squad, swarm, world));
    }
    let (squad, swarm, world) = &genomes[MUTANT_INDEX];
    let wc = world_genome::decode(world).expect("world mutant decodes");

    let stop = Arc::new(AtomicBool::new(false));
    let load: Vec<_> = (0..8)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x: u64 = 0;
                while !stop.load(Ordering::Relaxed) {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                }
                x
            })
        })
        .collect();

    let mut seen: Vec<(u64, usize)> = Vec::new();
    for _ in 0..REPS {
        let brains = brains_of(&t, squad, swarm).expect("brains from mutant");
        let r = rollout(brains, Some(wc.clone()), None, None, SEED, TICKS);
        let key = (r.snapshot, r.trace.decisions.len());
        if !seen.contains(&key) {
            seen.push(key);
        }
    }

    stop.store(true, Ordering::Relaxed);
    for h in load {
        let _ = h.join();
    }

    assert!(
        seen.len() == 1,
        "FVS-J-6 reproduced in the pinned cell: mutant #{MUTANT_INDEX} (rng seed \
         {MUTANT_RNG_SEED:#x}) on world {SEED:#x} gave {} distinct (snapshot, decisions) outcomes \
         {seen:x?}. The full mutant guard later in this binary carries the breadth; localize with the \
         `trace_episode`/`row_trace` tooling its failure message names.",
        seen.len()
    );
}

/// **The mutant guard** — same-seed reproducibility of the rollouts the SEARCH actually evaluates.
///
/// Its sibling `search_rollouts_are_reproducible_under_load` runs the **authored** genome, and that is the
/// hole this fills. The search evaluates **mutants**, and a mutant reaches code the authored config never
/// arms: a behaviour gated on a knob that *ships* clear of its threshold but whose genome bound sits on the
/// field's noise floor, or a mode the shipped brains never enter. So the authored guard went green while the
/// search was still scoring noise, and that green was read — twice, by me — as "the search is reproducible".
/// **A guard proves what it tests. Nothing more.** (Worked example: `bc.rally_live` ships at 0.15 but the
/// genome bound is 0.02, where one ULP of an unsorted rally accumulate flips a crab's caste.)
///
/// **Breadth over depth, deliberately.** K distinct mutants × few reps beats 1 mutant × many reps: different
/// mutants arm *different code*, and a rep only re-rolls the same dice. Squad AND swarm AND world are
/// mutated — the world genome is the important one, because it is what moves the config knobs.
///
/// Runs at the search's REAL episode length on BOTH held-in worlds, so nothing hides in the tail or on the
/// lucky seed. It is slow (~40 min) and that is a known cost: a slow green test is one nobody re-examines,
/// which is exactly how the single-seed guard survived for months. The mitigation is the failure message —
/// it prints the mutant index, both seeds, and the distinct outcomes, so a red run hands you a reproducer
/// rather than a mystery.
///
/// Do NOT add `serial_guard()`: `run_episode` takes it internally and `HARNESS_LOCK` is not reentrant.
#[test]
fn search_rollouts_of_mutants_are_reproducible_under_load() {
    use foundation_vs_slop::squad_ai::coevolve::{
        brains_of, mutate_squad_feasible, mutate_swarm_feasible, SquadGenome, SwarmGenome, Templates,
    };
    use foundation_vs_slop::squad_ai::evaluate::rollout;
    use foundation_vs_slop::squad_ai::world_genome;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Distinct mutants. Breadth is the point — see the note above.
    const MUTANTS: usize = 8;
    /// Reps per (mutant, world). 3 catches a ~30%-of-runs split with ~66% probability *per cell*, and there
    /// are `MUTANTS × SEEDS` = 16 cells, so the test as a whole is far more sensitive than any one cell.
    const REPS: usize = 3;
    const TICKS: u32 = 7200;
    /// Same pairing as `search_rollouts_are_reproducible_under_load`: one held-in world plus `0xA11CE`, a
    /// retired-but-splitty stressor. NOT the search's held-in set — see that test's note.
    const SEEDS: [u64; 2] = [0x5C09191, 0xA11CE];
    /// Fixed, so the mutant set is identical run to run — a red here must be reproducible by re-running.
    const MUTANT_RNG_SEED: u64 = 0x6D07A17;

    let t = Templates::authored();
    let mut rng = foundation_vs_slop::rng::seeded(MUTANT_RNG_SEED);

    // Draw the mutants up front, serially — the draw order is then independent of anything the rollouts do.
    let mut genomes = Vec::new();
    for _ in 0..MUTANTS {
        let squad = mutate_squad_feasible(&t, &SquadGenome::authored(&t), &mut rng)
            .expect("feasible squad mutant");
        let swarm = mutate_swarm_feasible(&t, &SwarmGenome::authored(&t), &mut rng)
            .expect("feasible swarm mutant");
        let world = world_genome::mutate(&world_genome::authored(), 0.15, &mut rng)
            .expect("feasible world mutant");
        genomes.push((squad, swarm, world));
    }

    let stop = Arc::new(AtomicBool::new(false));
    let load: Vec<_> = (0..8)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x: u64 = 0;
                while !stop.load(Ordering::Relaxed) {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                }
                x
            })
        })
        .collect();

    let mut split: Vec<String> = Vec::new();
    for (m, (squad, swarm, world)) in genomes.iter().enumerate() {
        let wc = world_genome::decode(world).expect("world mutant decodes");
        for seed in SEEDS {
            let mut seen: Vec<(u64, usize)> = Vec::new();
            for _ in 0..REPS {
                let brains = brains_of(&t, squad, swarm).expect("brains from mutant");
                let r = rollout(brains, Some(wc.clone()), None, None, seed, TICKS);
                let key = (r.snapshot, r.trace.decisions.len());
                if !seen.contains(&key) {
                    seen.push(key);
                }
            }
            if seen.len() > 1 {
                split.push(format!(
                    "mutant #{m} (rng seed {MUTANT_RNG_SEED:#x}) on world {seed:#x}: {} distinct {seen:x?}",
                    seen.len()
                ));
            }
        }
    }

    stop.store(true, Ordering::Relaxed);
    for h in load {
        let _ = h.join();
    }

    assert!(
        split.is_empty(),
        "MUTANT-ROLLOUT NON-DETERMINISM — the search is scoring noise, so its archives are unusable for \
         `train apply` (a MAP-Elites cell can be won by evaluation luck rather than by the genome):\n  {}\n\n\
         Reproduce: re-run this test — the mutant set is fixed by MUTANT_RNG_SEED, so mutant #N is the same \
         genome every time. Then bisect that (mutant, world) pair with `evaluate::trace_episode` (it folds \
         snapshot + field + gib hashes) and row-diff at the first divergent tick with `evaluate::row_trace` \
         (same pair, MULTISET diff — a set-difference lies when tied actors share a row).\n\n\
         Look for a gameplay decision keyed on ECS query order. Note this guard exists because the AUTHORED \
         guard cannot see this class: a mutant walks config knobs onto thresholds the shipped values sit \
         clear of. See docs/rl/2026-07-16-search-rollout-nondeterminism.md",
        split.join("\n  "),
    );
}

/// **The localizer for the two `*_reproducible_under_load` detectors above (FVS-J-6).** `#[ignore]`d:
/// it costs CI nothing and exists for the day one of them goes red.
///
/// This replaces `zz_localize_g0`, which was deleted for good reason — it ran 25 full episodes under
/// load on EVERY harness run, cost **1172 s ≈ 19.5 min** (28% of the lane), and a lane nobody will
/// wait for never gets promoted to a gate. The mistake would be to conclude a localizer is not worth
/// having; the right shape is one that is written, reviewed, and *dormant*. When J-6 last recurred the
/// replacement had never been written, so the investigation started from nothing twice.
///
/// # Running it
///
/// The detector prints the pair, e.g. `mutant #3 (rng seed 0x6d07a17) on world 0x5c09191`. Set
/// `MUTANT_INDEX` / `WORLD_SEED` to match, then:
///
/// ```text
/// cargo test --features test-harness --test replay localize_rollout_divergence \
///     -- --ignored --nocapture --test-threads=1
/// ```
///
/// # Why it is built this way
///
/// * **One run, one record.** It uses `row_trace` (every tick) rather than `trace_episode` to bisect
///   and *then* a fresh `row_trace` to diff. The first divergent tick VARIES between runs — this is a
///   race that can fire at several points — so bisecting with one sample set and diffing with another
///   compares two unrelated pairs, and the diff then shows accumulated drift rather than the
///   originating change. `TickProbe::RowTrace`'s own doc comment records that lesson; this honours it.
/// * **MULTISET diff, not set difference.** `snapshot_rows` sorts by WHOLE row, so two actors that are
///   bit-identical in everything hashed occupy interchangeable slots. A set difference reports nothing
///   when a value merely moves between two tied actors, and reports spurious "changes" when a tie
///   reorders. Counting occurrences is the only honest comparison.
/// * **Load well past the detector's 8 threads.** The failure is CI-only; the runner is a strictly
///   harsher probe than this 24-core box, because 8 busy-loops on 2-4 cores is 2-4x oversubscription
///   and here it is mild. Reproducing locally needs that oversubscription emulated, which is the
///   cheapest first experiment this box can actually run. **A concurrent `cargo` build is also worth
///   trying** — the 2026-07-31 occurrence happened alongside a full 24-core compile, which is arguably
///   closer to the real condition than a busy-loop generator.
///
/// A clean result is **not** an exoneration: every local pass so far was measured under a weaker
/// condition than the one that failed. Raise `LOAD_THREADS`, then say so.
#[test]
#[ignore = "diagnostic: run by hand when a *_reproducible_under_load detector goes red"]
fn localize_rollout_divergence() {
    use foundation_vs_slop::squad_ai::coevolve::{
        brains_of, mutate_squad_feasible, mutate_swarm_feasible, SquadGenome, SwarmGenome, Templates,
    };
    use foundation_vs_slop::squad_ai::evaluate::row_trace;
    use foundation_vs_slop::squad_ai::world_genome;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // ── Set these from the detector's failure message ──────────────────────────────────────────
    /// Must match `search_rollouts_of_mutants_are_reproducible_under_load`, or mutant #N is a
    /// different genome and this localizes nothing.
    const MUTANT_RNG_SEED: u64 = 0x6D07A17;
    let mutant_index: usize = env_or("FVS_LOCALIZE_MUTANT", 3);
    let world_seed: u64 = env_or("FVS_LOCALIZE_WORLD", 0x5C09191);
    // Full episode length, matching the detector. Reduce for a cheap smoke of this harness itself.
    let ticks: u32 = env_or("FVS_LOCALIZE_TICKS", 7200);
    // Replicates. More than the detector's 3 — a localizer wants the split to actually occur.
    let reps: usize = env_or("FVS_LOCALIZE_REPS", 6);
    // Deliberately far above the detector's 8, to emulate CI oversubscription.
    let load_threads: usize = env_or("FVS_LOCALIZE_THREADS", 24);

    // Env-tunable rather than recompiled, because the *first* thing this entry tells you to do is
    // raise the thread count until the failure reproduces — an edit-rebuild cycle per attempt on a
    // Bevy tree is the reason that experiment never got run.
    //
    // Runtime, stated only as far as it was actually measured: `TICKS=600 REPS=2 THREADS=4` runs in
    // **5.2 s**. The full defaults (6 x 7200 ticks under 24 threads) are ~36x that work before
    // contention and have NOT been timed end-to-end — budget generously rather than trusting an
    // extrapolation, which this repo has been burned by twice.
    fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
        std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    // ⚠️ **NO `serial_guard()` here, and that is load-bearing.** `evaluate.rs:400` takes it for each
    // `App`'s lifetime inside the rollout path, and it is a non-reentrant `MutexGuard` — so a guard
    // held by the test deadlocks the first `row_trace` call, silently and forever. The detectors above
    // omit it for the same reason. Caught by running this diagnostic rather than merely compiling it:
    // shipped untested it would have hung on the one day someone needed it, during a red CI.

    // Draw the mutant set exactly as the detector does — same calls, same order, same seed — so index
    // N here IS index N there. Any divergence in this loop makes the whole exercise meaningless.
    let t = Templates::authored();
    let mut rng = foundation_vs_slop::rng::seeded(MUTANT_RNG_SEED);
    let mut genomes = Vec::new();
    for _ in 0..=mutant_index {
        let squad = mutate_squad_feasible(&t, &SquadGenome::authored(&t), &mut rng)
            .expect("feasible squad mutant");
        let swarm = mutate_swarm_feasible(&t, &SwarmGenome::authored(&t), &mut rng)
            .expect("feasible swarm mutant");
        let world = world_genome::mutate(&world_genome::authored(), 0.15, &mut rng)
            .expect("feasible world mutant");
        genomes.push((squad, swarm, world));
    }
    let (squad, swarm, world) = &genomes[mutant_index];
    let wc = world_genome::decode(world).expect("world mutant decodes");

    let stop = Arc::new(AtomicBool::new(false));
    let load: Vec<_> = (0..load_threads)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x: u64 = 0;
                while !stop.load(Ordering::Relaxed) {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                }
                x
            })
        })
        .collect();

    eprintln!(
        "localizer: mutant #{mutant_index} (seed {MUTANT_RNG_SEED:#x}) on world {world_seed:#x}, \
         {reps} reps x {ticks} ticks under {load_threads} busy threads"
    );
    let mut traces: Vec<Vec<Vec<[u32; 5]>>> = Vec::new();
    for rep in 0..reps {
        let brains = brains_of(&t, squad, swarm).expect("brains from mutant");
        let mut out = Vec::new();
        row_trace(brains, Some(wc.clone()), world_seed, ticks, &mut out);
        eprintln!("  rep {rep}: {} ticks recorded", out.len());
        traces.push(out);
    }

    stop.store(true, Ordering::Relaxed);
    for h in load {
        let _ = h.join();
    }

    // Group the replicates by their FULL trace. Two groups == the bimodal signature J-6 recorded:
    // the same two outcomes across separate CI runs, which is a flipped discrete decision rather than
    // accumulated float drift (drift gives a fresh result every time).
    let mut groups: Vec<(usize, &Vec<Vec<[u32; 5]>>)> = Vec::new();
    for tr in &traces {
        match groups.iter_mut().find(|(_, g)| *g == tr) {
            Some((n, _)) => *n += 1,
            None => groups.push((1, tr)),
        }
    }
    eprintln!("localizer: {} distinct trace(s) over {reps} reps", groups.len());
    if groups.len() < 2 {
        eprintln!(
            "localizer: NO DIVERGENCE at {load_threads} threads. This is NOT an exoneration — it is a \
             weaker condition than the CI failure. Raise load_threads, or re-run with a concurrent \
             `cargo build --all-targets` alongside (the 2026-07-31 occurrence looked like that)."
        );
        return;
    }

    // First tick where any two groups disagree. Both traces index tick t at [t - 1].
    let (a, b) = (groups[0].1, groups[1].1);
    let split = (0..a.len().min(b.len()))
        .find(|&i| a[i] != b[i])
        .expect("groups differ, so some recorded tick must differ");
    eprintln!(
        "localizer: FIRST DIVERGENT TICK = {} (group sizes {} vs {})",
        split + 1,
        groups[0].0,
        groups[1].0
    );

    // Multiset diff at that tick. Rows are [x, y, z, hp, hp_max] as f32 bit patterns.
    let count = |rows: &Vec<[u32; 5]>| {
        let mut m: BTreeMap<[u32; 5], i64> = BTreeMap::new();
        for r in rows {
            *m.entry(*r).or_default() += 1;
        }
        m
    };
    let (ca, cb) = (count(&a[split]), count(&b[split]));
    let show = |r: &[u32; 5]| {
        format!(
            "pos({:.4}, {:.4}, {:.4}) hp {:.3}/{:.3}",
            f32::from_bits(r[0]),
            f32::from_bits(r[1]),
            f32::from_bits(r[2]),
            f32::from_bits(r[3]),
            f32::from_bits(r[4])
        )
    };
    eprintln!("localizer: rows only in A (or more numerous):");
    for (row, n) in &ca {
        let d = n - cb.get(row).copied().unwrap_or(0);
        if d > 0 {
            eprintln!("  +{d}  {}", show(row));
        }
    }
    eprintln!("localizer: rows only in B (or more numerous):");
    for (row, n) in &cb {
        let d = n - ca.get(row).copied().unwrap_or(0);
        if d > 0 {
            eprintln!("  +{d}  {}", show(row));
        }
    }
    eprintln!(
        "localizer: A had {} actors, B had {}. An equal count with positions differing in the LOW BITS \
         is float drift; a differing count, or a large positional jump, is a flipped discrete decision \
         — look for a gameplay choice keyed on ECS query order that moves or damages an actor WITHOUT \
         writing a stigmergy field (J-6's field hashes matched across the split).",
        a[split].len(),
        b[split].len()
    );

    panic!("localizer: divergence reproduced and reported above — see stderr for the first split tick");
}

/// **Reproduce the RETURN-TO-SITE crash** — the transition a player reported panicking (2026-07-28).
///
/// # What the report showed
///
/// Play log, in order: the O5 report filed (`OnEnter(AppState::Debrief)`), the campaign saved
/// (`OnEnter(AppState::Site)`), then two panics from parallel systems — *"No entities fit the query"*
/// and *"Parameter … failed validation: Resource does not exist"*. Both anonymised, because the shipped
/// binary is built without `bevy/debug`.
///
/// # Why this is a test rather than a manual repro
///
/// The crash sits on a state transition, not on gameplay: `RETURN TO SITE` sets `RunState::Idle` **and**
/// `AppState::Site` in one step, and `DespawnOnExit(RunState::Active)` then removes the squad and the
/// whole expedition world. Any system still running at the Site that requires a run-scoped **entity**
/// (a `Single<…>` param, a `.single()`) or a run-scoped **resource** panics on the next frame.
///
/// That is reproducible without a player, and it is exactly the shape FVS-G-6 is about — which makes it
/// worth pinning permanently rather than diagnosing once. This binary is built with `test-harness`,
/// which enables `bevy/debug`, so a failure here **names the offending system** where the shipped build
/// cannot.
#[test]
fn returning_to_the_site_after_a_run_does_not_panic() {
    use bevy::prelude::*;
    use foundation_vs_slop::session::RunState;
    use foundation_vs_slop::sim_harness::build_headless_app_unfinished;
    use foundation_vs_slop::ui::state::AppState;
    use foundation_vs_slop::ui::UiPlugin;

    let _serial = serial_guard();
    // SAFETY: `serial_guard` is held, so this is the only thread touching the environment.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", std::env::temp_dir().join("fvs_site_repro"));
        // Never touch the real campaign: `persist` saves on entering the Site, which this test does.
        std::env::set_var("XDG_DATA_HOME", std::env::temp_dir().join("fvs_site_repro_data"));
    }

    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app_unfinished(&cfg);
    // The WINDOWED plugin set, not just `UiPlugin`. `ui::site_hud` and `ui::research_hud` read
    // `StudySubject` and write `RunExperiment`, both of which come from `ResearchLabPlugin` — which
    // `sim_harness` deliberately omits. Adding the UI alone reproduces a plugin-set mismatch of the
    // test's own making rather than anything a player can hit, and the panic it produces looks
    // exactly like the real one. Mirror `lib::run`'s windowed group so this test fails only for
    // reasons the shipped game can actually reach.
    app.add_plugins((
        UiPlugin,
        foundation_vs_slop::research::ResearchLabPlugin,
        foundation_vs_slop::site::O5Plugin,
        foundation_vs_slop::knowledge::RosterPlugin,
        foundation_vs_slop::knowledge::RecordsPlugin,
        foundation_vs_slop::antagonist::AntagonistPlugin,
        foundation_vs_slop::director::DirectorPlugin,
        foundation_vs_slop::ui::briefing::BriefingPlugin,
    ));
    app.finish();
    app.cleanup();

    for _ in 0..40 {
        app.update();
    }

    // Into an expedition, and let the world actually build and run.
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::InGame);
    for _ in 0..30 {
        app.update();
    }

    // The debrief — where the O5 report is filed while the world is still alive.
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Debrief);
    app.update();
    app.update();

    // RETURN TO SITE: both halves, in one transition, exactly as `ui::debrief` does it. This is the
    // step that despawns the squad and the expedition world out from under anything still running.
    app.world_mut().resource_mut::<NextState<RunState>>().set(RunState::Idle);
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Site);

    // Several frames, not one: a missing-`Res` panic is *parameter validation*, which fires the first
    // time each system actually runs — and a `Single<…>` miss needs the despawn commands to have been
    // applied. One frame would miss both.
    for _ in 0..60 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<State<AppState>>().get(),
        &AppState::Site,
        "the Site must be reachable and survivable after a run ends"
    );
}

/// **FVS-L-6's acceptance test: the roster opens at the Site.**
///
/// The "Done when" is player-observable — *the roster is openable at the Site* — so this drives the
/// real key, not the resource. That matters here more than usual: the previous attempt at this
/// feature was a `.or_else(in_state(AppState::Site))` on the in-game toggle, which type-checked,
/// read as support, and could never have worked — `spawn_roster` hung off a `MenuState` that does not
/// exist at the Site. A test that poked the state directly would have passed against that too.
///
/// Pressing the bound key exercises the whole chain: binding -> `Actions` gating -> `SiteRosterOpen`
/// -> spawn/despawn. `returning_to_the_site_after_a_run_does_not_panic` already pins the crash that
/// the broken version caused; this pins that the replacement does the thing.
#[test]
fn the_roster_opens_and_closes_at_the_site() {
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::ButtonState;
    use bevy::prelude::*;
    use foundation_vs_slop::input::{Action, KeyBindings};
    use foundation_vs_slop::knowledge::roster::{RosterScreenRoot, SiteRosterOpen};
    use foundation_vs_slop::session::RunState;
    use foundation_vs_slop::sim_harness::build_headless_app_unfinished;
    use foundation_vs_slop::ui::state::AppState;
    use foundation_vs_slop::ui::UiPlugin;

    let _serial = serial_guard();
    // SAFETY: `serial_guard` is held, so this is the only thread touching the environment. Never
    // touch the real campaign — entering the Site saves.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", std::env::temp_dir().join("fvs_site_roster"));
        std::env::set_var("XDG_DATA_HOME", std::env::temp_dir().join("fvs_site_roster_data"));
    }

    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app_unfinished(&cfg);
    // Mirror `lib::run`'s windowed group, as the site-return test does, so this fails only for
    // reasons a player can reach rather than for a plugin-set mismatch of the test's own making.
    app.add_plugins((
        UiPlugin,
        foundation_vs_slop::research::ResearchLabPlugin,
        foundation_vs_slop::site::O5Plugin,
        foundation_vs_slop::knowledge::RosterPlugin,
        foundation_vs_slop::knowledge::RecordsPlugin,
        foundation_vs_slop::antagonist::AntagonistPlugin,
        foundation_vs_slop::director::DirectorPlugin,
        foundation_vs_slop::ui::briefing::BriefingPlugin,
    ));
    app.finish();
    app.cleanup();
    for _ in 0..40 {
        app.update();
    }

    // To the Site, the way `RETURN TO SITE` does it.
    app.world_mut().resource_mut::<NextState<RunState>>().set(RunState::Idle);
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Site);
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<State<AppState>>().get(),
        &AppState::Site,
        "the test never reached the Site, so the rest proves nothing"
    );

    let key = app.world().resource::<KeyBindings>().get(Action::ToggleRoster).primary.key;
    // Real `KeyboardInput` messages, NOT `ButtonInput::press`. Bevy's `keyboard_input_system` calls
    // `clear()` on `ButtonInput` in `PreUpdate` before draining the message queue, so a press written
    // directly into the resource is wiped before any `Update` system can see it as `just_pressed` —
    // the test then fails against working code, which is exactly what happened writing this.
    let send = |app: &mut App, state: ButtonState| {
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: Key::Character("r".into()),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    };
    let open = |app: &mut App| {
        send(app, ButtonState::Pressed);
        app.update();
        send(app, ButtonState::Released);
        app.update();
    };
    let showing = |app: &mut App| {
        app.world_mut().query::<&RosterScreenRoot>().iter(app.world()).count()
    };

    assert_eq!(showing(&mut app), 0, "the roster must not be open before it is asked for");

    open(&mut app);
    assert_eq!(
        showing(&mut app),
        1,
        "pressing ToggleRoster at the Site did not open the roster — FVS-L-6's whole point"
    );

    open(&mut app);
    assert_eq!(showing(&mut app), 0, "the same key must close it again");

    // Re-open, then leave: the overlay must not outlive the screen it belongs to, and must not
    // reappear unasked on the next visit.
    open(&mut app);
    assert_eq!(showing(&mut app), 1, "re-open failed");
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Title);
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(showing(&mut app), 0, "leaving the Site must tear the roster overlay down");
    assert_eq!(
        *app.world().resource::<SiteRosterOpen>(),
        SiteRosterOpen(false),
        "leaving the Site must also clear the flag, or the next visit opens a panel nobody asked for"
    );
}
