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
    // moved from the lighting-era `0x3ecce611f2403172` to the value below. Legitimate: the same-seed
    // reproducibility tests above (`deterministic_core_is_bit_identical`, `..._across_many_builds`) still
    // pass, so the sim is still bit-reproducible — just different, because a real feature was added.
    //
    // Re-pinned again (was `0x4b6f6d7f454559c7`) for the ATTENTION channel: a new `FixedUpdate` producer
    // (`ai::field::deposit_attention`) was added to the pinned schedule. NOTE: no core actor reads
    // ATTENTION (its only consumer, the mould, is windowed-only and absent from the harness) — verified by
    // temporarily skipping the deposit, which reproduced THIS exact hash. So the shift is purely the
    // single-threaded executor re-solving its topological order once a system is inserted, not a data
    // effect. The core is still bit-identical run-twice and arch-stable (ATTENTION is position/LOS-derived,
    // never rotation). Flag for maintainers: that adding a pure producer shifts the trajectory means some
    // core systems lack explicit relative ordering — a latent hygiene item, pre-existing, not introduced here.
    const GOLDEN: u64 = 0xb8d5dc7d27ac37b1;
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
    // Reverted to `0xa35b_eaeb_288a_fbca` after the flashlight re-pin (`0x3db0_1bf8_5c5d_d822`) proved
    // ARCH-DEPENDENT: `LightField::fold_fingerprint` now folds the static `base`, not `cells`. The dynamic
    // flashlight cone in `cells` derives its beam direction from unit `Transform.rotation`, computed with
    // glam quaternion/`slerp` transcendentals that are not bit-identical across ARM↔x86 — so an ARM-pinned
    // cone-inclusive value failed `field_passes` on x86 CI while `migrated_defaults` (which folds
    // translation, never rotation) passed. Folding the arch-stable scalar-`f32` base restores a value that
    // matches on both arches (it is the pre-flashlight static field). The cone's determinism is covered
    // within-arch by `deterministic_core_is_bit_identical` and its unit tests. See `light::fold_fingerprint`.
    // Re-pinned (was `0xa35b_eaeb_288a_fbca`) for the ATTENTION channel: `Stig::fold_fingerprint` folds every
    // channel, so the new 10th channel — deposited over the squad's line-of-sight set by
    // `ai::field::deposit_attention` — enters this hash. It stays ARCH-STABLE (unlike the flashlight cone
    // above): fog visibility is a pure function of unit cell positions + integer LOS, no rotation, so the
    // deposit deliberately does NOT read the arch-sensitive flashlight beam direction.
    const GOLDEN_FIELD: u64 = 0xd8deb83da6e1beb4;
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
        0xb8d5dc7d27ac37b1,
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

    let base = rollout(BrainSource::Authored, None, None, seed, ticks);

    // Crank the din-fear gains off their dormant (0.0) default. `unit_fear_of_din` reacts to the crab-death
    // din (NOISE_SWARM), which the rollout actually produces; `crab_fear_of_din` is the swarm analogue,
    // dormant offline (no gunfire → no NOISE_SQUAD) but set here to document the intended symmetric lever.
    let mut audio = AudioTuning::default();
    audio.perception.unit_fear_of_din = 0.5;
    audio.perception.crab_fear_of_din = 0.5;
    let mutant = rollout(BrainSource::Authored, None, Some(audio), seed, ticks);

    // DECISIVE: the final actor state (Transform+Health) must differ. Same world, brains and seed — the ONLY
    // difference is the audio slice, so a changed final state proves the acoustic din reaches gameplay.
    assert_ne!(
        base.snapshot, mutant.snapshot,
        "a cranked audio config produced a byte-identical final state — the acoustic coupling is inert"
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
