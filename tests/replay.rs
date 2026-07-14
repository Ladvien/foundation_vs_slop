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

#[test]
fn migrated_defaults_reproduce_the_shipped_golden_hash() {
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
    const GOLDEN: u64 = 0x45b960069537d712;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1800);
    assert_eq!(
        snapshot_hash(&mut app),
        GOLDEN,
        "deterministic-core hash drifted from the pre-migration golden — the const→config promotion \
         changed a gameplay value (or the shipped `sim:` slice differs from SimTuning::default())"
    );
}

#[test]
fn field_passes_are_bit_identical() {
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
    const GOLDEN_FIELD: u64 = 0xee06_882d_2f14_21d9;
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1800);
    assert_eq!(
        field_hash(&mut app),
        GOLDEN_FIELD,
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
        // Tracks the Phase-1 actor golden above (combat-feel re-pin). It stays byte-identical to it because
        // `authored()` encodes the parasite counts straight from `SimTuning::default()` (world_genome.rs), and
        // the new values (initial_count 6, manca_count_max 20) sit inside the genome's normalization bounds
        // (1–12, 4–40) so encode→decode is still lossless. Tracks the Almond Water re-pins above (incl. the
        // sparse-spring seep-model re-pin) and the crab-determinism re-pin.
        0x45b960069537d712,
        "installing the authored world config changed the sim — the override seam or encode/decode is lossy"
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
    use foundation_vs_slop::time_control::{SimBlocked, UserPaused};
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
        !app.world().resource::<UserPaused>().0,
        "UserPaused must stay false in the core (no key input present)"
    );
}

#[test]
fn ui_screens_spawn_and_pause_blocks_the_sim() {
    // OPERABILITY liveness (Game-UI Guidance §1.5): boot the *real* windowed UI headless and prove
    // the screens actually spawn and the state flow works — the substitute for a pixel screenshot,
    // which this headless env can't produce (no monitor → black drawable). Not a determinism test:
    // it builds its own UI-inclusive app; the core reference app (`build_headless_app`) is untouched.
    use bevy::prelude::*;
    use foundation_vs_slop::sim_harness::build_headless_app_unfinished;
    use foundation_vs_slop::time_control::SimBlocked;
    use foundation_vs_slop::ui::hud::{HudRoot, SpeedText};
    use foundation_vs_slop::ui::pause::PauseRoot;
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
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<HudRoot>>();
        assert_eq!(q.iter(app.world()).count(), 1, "HUD root should spawn on entering the game");
    }
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<SpeedText>>();
        assert!(q.iter(app.world()).next().is_some(), "HUD speed readout should exist");
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
    let cfg = SimConfig::deterministic_core();
    const TICKS: u32 = 360; // ~6 s — long enough for the light bias to accumulate against mode motion

    let mean_off = mean_crab_light(&cfg, Some(0.0), TICKS);
    let mean_on = mean_crab_light(&cfg, None, TICKS); // shipped photophobic_gain

    assert!(
        mean_on < mean_off,
        "photophobic crabs (gain>0) should occupy darker cells than gain=0 crabs: on={mean_on} off={mean_off}"
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
