//! Shared health + floating health bars.
//!
//! `Health` is a single component worn by both squad units and enemies, so one pair of systems can
//! render a bar over anything with hit points. Bars are **camera-facing quads** (the project's
//! established billboard recipe — see `impact_fx.rs` / `enemy.rs`), each carrying a tiny
//! [`HealthBarMaterial`] whose `fraction` uniform drives fill width and color
//! (`assets/shaders/health_bar.wgsl`). Legible health feedback keeps a fight readable and tunable,
//! an adaptive-difficulty affordance (McKay et al., "Implementing Adaptive Game Difficulty Balancing
//! in Serious Games", IEEE Trans. Games 2018, DOI 10.1109/tg.2018.2791019).

use bevy::pbr::{Material, MaterialPlugin};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

/// Hit points for any combatant (units and enemies alike). `current` is clamped-consumed by damage;
/// `max` is fixed at spawn and used for the bar fraction.
#[derive(Component)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Health {
    pub fn new(max: f32) -> Self {
        Health { current: max, max }
    }

    /// Remaining health in [0, 1].
    pub fn fraction(&self) -> f32 {
        if self.max <= 0.0 {
            0.0
        } else {
            (self.current / self.max).clamp(0.0, 1.0)
        }
    }

    /// Apply `amount` of damage, clamping `current` at a 0 floor. Every [`HealthDamage`] writer must
    /// go through this instead of `current -=` directly — a negative `current` is observable to any
    /// system that reads `Health` before the despawn pass (most acutely `almond_water`'s heal, which
    /// computed `max - current` and over-healed a negative-HP unit back past `max`, resurrecting a unit
    /// killed in a heal pool). Centralizing the floor here means a future damage site can't reintroduce
    /// that class of bug.
    pub fn apply_damage(&mut self, amount: f32) {
        self.current = (self.current - amount).max(0.0);
    }

    /// Kill outright (an instant-kill swat/zap, not a magnitude of damage). Routed through the same
    /// clamp API as [`Self::apply_damage`] for a single, consistent way to mutate `current` downward.
    pub fn kill(&mut self) {
        self.current = 0.0;
    }
}

/// Height of the bar above the owner's transform origin (owners sit near Y=0). Calibrated to float
/// just above the unit figurine's head — the figurine is ~1.82 m tall (0.7 m base mesh × `squad::
/// FIGURINE_SCALE` 2.6), so the bar clears it with a small gap. Tune by eye via devshot.
const BAR_Y: f32 = 2.0;
/// Bar quad size in world units (wide and short), authored to read correctly at [`BAR_REF_ZOOM`].
const BAR_WIDTH: f32 = 1.1;
const BAR_HEIGHT: f32 = 0.16;

/// The camera zoom (`CameraView::viewport_height`) these bar dimensions were tuned at — the startup
/// zoom, `camera::VIEWPORT_HEIGHT`.
///
/// The bar is scaled by `viewport_height / BAR_REF_ZOOM` so it holds a **constant apparent size**
/// across the zoom range. Without it the quad is a fixed size in *world* units while the viewport is
/// not, so its share of the screen is `BAR_WIDTH / viewport_height` — 3% of screen width at
/// `camera::MAX_ZOOM` (34) but **22% at `camera::MIN_ZOOM` (5)**. A player zoomed all the way in on a
/// clustered squad got five overlapping green slabs across a fifth of the screen each, which is what
/// `debug_screenshots/region_2026-07-29_13-12-23-426` caught. At the reference zoom the scale is
/// exactly 1.0, so the authored calibration above is preserved.
const BAR_REF_ZOOM: f32 = 12.0;

/// A bar entity's link back to the combatant it displays.
#[derive(Component)]
struct HealthBar {
    owner: Entity,
}

/// Marks an owner that already has a bar, so `attach_health_bars` runs once per combatant.
#[derive(Component)]
struct HasHealthBar;

/// Opt-out marker: a `Health` entity carrying this gets NO floating bar. For swarm chaff (the crab
/// infestation) where 40 bars would bury the screen — they still take damage and die, just silently.
#[derive(Component)]
pub struct NoHealthBar;

/// System set for every `FixedUpdate` system that **damages** `Health` (laser, crab contact/jump, boss
/// zap/defense, parasite embed/burst). These writers overlap in component access but rarely touch the same
/// entity the same tick. [`crate::almond_water::almond_water_effect`] orders itself `.after(HealthDamage)`
/// so the consuming heal/poison always composes on top of the tick's damage deterministically — otherwise,
/// once foraging clusters wounded crabs into weapon range, heal-vs-damage clamping races and `snapshot_hash`
/// flips per process. Each damage system opts in with `.in_set(...)` at its own registration; the set name
/// alone sequences the heal behind the whole group, but the writers' MUTUAL order among themselves is an
/// explicit `.after()` chain across four plugins/files — `smiley_zap` → `smiley_defense` → `crab_jump` →
/// `crab_contact_damage` → `manca_embed` → `parasite_burst` → `fire_laser` (see each system's registration
/// comment) — not accidental plugin-registration order. Three-way float subtraction on a near-death host is
/// non-associative (`(a−b)−c ≠ (a−c)−b` in IEEE-754), so an unpinned composition order is a real
/// reproducibility hazard, the same class `ai::field::sort_deposits` exists to prevent for stigmergy
/// deposits (Defour & Collange, "Reproducible floating-point atomic addition in data-parallel environment",
/// FedCSIS 2015, DOI 10.15439/2015f86 — unordered concurrent writers to a shared float are a reproducibility
/// hazard, the same shape this chain closes).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HealthDamage;

/// Living flesh that [`crate::almond_water`] can heal **or poison** — a **positive** tag, inserted at spawn on
/// every flesh creature (squad units, crabs, mancae, the Smiley boss), so `Health`-bearing non-flesh is
/// excluded *by construction*: the stone `Nest` has `Health` but no `Biological`. `Health` alone is not a
/// valid "the water affects me" predicate; this marker is. Inserted at spawn, never mid-sim, to avoid a
/// runtime archetype migration. Every `Biological` also carries [`CyanideSmell`].
#[derive(Component)]
pub struct Biological;

/// Can this creature smell the bitter-almond / hydrogen-cyanide warning? The odour sensitivity is inherited as
/// an **x-linked recessive**, so roughly **one in four** cannot detect it (Gidlow, *Hydrogen cyanide — an
/// update*, Occupational Medicine 2017, doi:10.1093/occmed/kqx121). An anosmic creature can't perceive that a
/// pool reads as cyanide — it is blind to the danger (partial observability for the learned policy), yet the
/// poison still affects it. Present on **every** [`Biological`] (only the bool differs), never a subset marker:
/// a component on only some units would split the hashed archetype and make ECS iteration order run-dependent.
#[derive(Component)]
pub struct CyanideSmell {
    /// True ⇒ cannot smell the warning (blind to a pool's cyanide reading).
    pub anosmic: bool,
    /// **Stable per-spawn identity** — the mixed spawn seed, kept rather than discarded.
    ///
    /// It exists because `Biological` is heterogeneous (units, crabs, mancae, the boss), so there is no one
    /// stable key across it: `SquadMember` is units-only, `CrabSeed` is crabs-only, and a raw `Entity` id is
    /// recycled and NOT reproducible across same-seed runs — it is the very instability being guarded
    /// against. This is the only spawn-time identity every `Biological` already carries.
    ///
    /// [`crate::almond_water`]'s drink contention sorts on it. That sort's key was
    /// `(cell, health, pos.x, pos.z)`, which its own comment called a total order — it is not: two crabs
    /// `clamp_to_patch`-ed against the same wall land on BIT-IDENTICAL coordinates, and at equal health they
    /// tie, at which point `sort_unstable` resolves them by the ECS query order the sort exists to erase.
    /// Measured on held-in world `0xA11CE`: **6 fully-tied pairs at tick 1580**, all at
    /// `pos=(77.94, 12.94) hp=25/25`. Tied drinkers are NOT interchangeable — they differ in `anosmic`,
    /// mode, and carry phase — so who drinks first (both `drink` and `nudge_belief` clamp, and a clamp makes
    /// even equal magnitudes order-dependent) decides who heals and who reads the pool as cyanide.
    pub id: u64,
}

/// Species namespaces for [`CyanideSmell::from_seed_in`]. The raw per-species spawn seeds — unit
/// `member + 1` (1..=N), the crab and manca sequence counters (0, 1, 2, …), the boss's position hash —
/// **overlap as bare integers**, and the splitmix64 finalizer is a bijection, so identical raw seeds
/// would mint identical [`CyanideSmell::id`]s *across* species. `id` is the cross-species total-sort
/// tiebreak (the almond-water drink contention, every `nearest_planar_keyed` host/prey pick), so its
/// uniqueness must hold by construction, not by luck: each species ORs a distinct base into the 64-bit
/// seed before mixing. Raw seeds are `u32`-sized (< 2^32), far below the 2^40 bases, so the namespaced
/// inputs are disjoint by construction and the bijection keeps the outputs disjoint too. (Same shape as
/// `laser::target_id`'s `TargetKind` namespace.)
pub mod smell_seed {
    pub const UNIT: u64 = 1 << 40;
    pub const CRAB: u64 = 2 << 40;
    pub const MANCA: u64 = 3 << 40;
    pub const BOSS: u64 = 4 << 40;
    /// SCP-1048 and its copies. Added 2026-07-28: the bears were minting smells under [`MANCA`] with
    /// their own independent counter starting at the same values, so a bear and a manca in one run
    /// collided on the id `smell_tests::same_raw_seed_yields_distinct_ids_across_species` exists to
    /// keep unique across species *by construction*.
    pub const BEAR: u64 = 5 << 40;
}

/// The splitmix64 finalizer — a bijection on `u64`, so distinct inputs give distinct outputs, which is
/// what makes [`CyanideSmell::id`] usable as a sort tiebreak rather than merely a well-mixed hash.
fn mix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl CyanideSmell {
    /// Deterministic per-spawn assignment: ~1 in 4 biologicals are anosmic. A pure function of the
    /// entity's spawn seed (no RNG enters the determinism hash, no archetype churns at runtime), with the
    /// species' [`smell_seed`] namespace folded into the **id only**:
    ///
    /// - `anosmic` draws from the RAW seed — the namespace exists for id disjointness, and must not
    ///   redistribute a gameplay trait across the population;
    /// - `id` mixes the namespaced seed, so it is unique within a species (bijection over the species'
    ///   own seed stream) *and* across species (the namespaced input ranges are disjoint).
    pub fn from_seed_in(species: u64, seed: u64) -> Self {
        Self { anosmic: mix64(seed) % 4 == 0, id: mix64(species | seed) }
    }
}

#[cfg(test)]
mod smell_tests {
    use super::*;

    /// The cross-species disjointness contract: the same raw seed through different species namespaces
    /// yields distinct ids (unit member 0's seed `1` vs the crab spawned with sequence seed `1` vs the
    /// manca with sequence seed `1` — the exact collision that let a keyed nearest-host pick fall through
    /// to query order), while the anosmic draw is namespace-independent by design.
    #[test]
    fn same_raw_seed_yields_distinct_ids_across_species() {
        let bases = [smell_seed::UNIT, smell_seed::CRAB, smell_seed::MANCA, smell_seed::BOSS];
        for raw in [0u64, 1, 5, 0xFFFF_FFFF] {
            let ids: Vec<u64> = bases.iter().map(|&b| CyanideSmell::from_seed_in(b, raw).id).collect();
            for i in 0..ids.len() {
                for j in (i + 1)..ids.len() {
                    assert_ne!(ids[i], ids[j], "raw seed {raw}: namespaces {i} and {j} collide");
                }
            }
            for &b in &bases {
                assert_eq!(
                    CyanideSmell::from_seed_in(b, raw).anosmic,
                    CyanideSmell::from_seed_in(smell_seed::UNIT, raw).anosmic,
                    "anosmic must draw from the raw seed, not the namespace"
                );
            }
        }
    }
}

/// GPU uniform — mirrors `HealthBarSettings` in `health_bar.wgsl` (field order + types).
#[derive(Clone, ShaderType)]
struct HealthBarUniform {
    fraction: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

/// The custom health-bar material.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct HealthBarMaterial {
    #[uniform(0)]
    settings: HealthBarUniform,
}

impl Material for HealthBarMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/health_bar.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    // `AlphaMode::Blend` writes no depth, so Bevy orders bars purely by a painter's-algorithm sort on
    // `rangefinder.distance(aabb_centre) + depth_bias` (one key per mesh). Every bar floats at the same
    // fixed `BAR_Y` above its owner, so a tightly clustered squad — exactly the case a health bar most
    // needs to read clearly — puts several bars' AABB centres within millimetres of each other and the
    // camera. With `depth_bias` defaulted to 0.0 for every bar, that's a near-tie, and which bar paints
    // on top is then decided by ECS extraction order — not stable across frames (this project's own
    // rule: "ECS query order decides nothing"). That reads as flickering, exactly the class of bug
    // already fixed for `BloodPoolMaterial` in `gore.rs`; here the tiebreak is cheaper: `_pad0` carries
    // a stable per-bar seed (set once in `attach_health_bars`, never touching the shader, which only
    // reads `fraction`) instead of a `Transform` jitter, since a bar's own screen position must stay
    // exact for its fill to read as attached to its owner. `attach_health_bars` stamps `_pad0` already
    // normalised to `[0, 1)`.
    fn depth_bias(&self) -> f32 {
        (self.settings._pad0 - 0.5) * 0.004
    }
}

/// Shared quad mesh for every bar.
#[derive(Resource)]
struct HealthBarAssets {
    quad: Handle<Mesh>,
}

pub struct HealthPlugin;

impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<HealthBarMaterial>::default())
            .add_systems(Startup, setup_health_bar_assets)
            .add_systems(Update, (attach_health_bars, update_health_bars).chain().distributive_run_if(in_state(crate::session::RunState::Active)));
    }
}

fn setup_health_bar_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(HealthBarAssets {
        quad: meshes.add(Rectangle::new(BAR_WIDTH, BAR_HEIGHT)),
    });
}

/// Give every combatant that doesn't have one a floating bar entity (top-level, not a child — so the
/// figurine's non-unit scale doesn't distort it).
fn attach_health_bars(
    mut commands: Commands,
    assets: Res<HealthBarAssets>,
    mut materials: ResMut<Assets<HealthBarMaterial>>,
    owners: Query<(Entity, &Health), (Without<HasHealthBar>, Without<NoHealthBar>)>,
    // Bars attached so far. See below — this ordinal, not the owner's position, is the tiebreak.
    mut attached: Local<u32>,
) {
    for (owner, health) in &owners {
        // Stable per-bar tiebreak seed for `HealthBarMaterial::depth_bias`.
        //
        // **An ordinal, NOT a hash of the owner's position.** A tiebreak computed from the very
        // quantity that ties is not a tiebreak: two `Health` owners standing on one point hash
        // identically, get the same `depth_bias`, and the sort falls back to extraction order exactly
        // as it did before — the flicker returns in precisely the tightest-cluster case this exists
        // for. That is reachable today (the Research Room's `spawn_unit` callers pass an explicit
        // `pos` and can seat several units on one cell), and it is the same trap `crab::setup`
        // already documents for per-crab draws: "Every per-crab random draw comes from the unique
        // spawn seed, NOT the spawn position — bred crabs share a delivery cell, so a position hash
        // would clone them." The recipe was borrowed from `light::attach_fixture_lights`, where it is
        // sound only because fixtures are immobile level geometry that never shares a cell.
        //
        // A counter is unique by construction. Spread it through the golden-ratio multiply
        // (`0x9E37_79B9` ≈ 2³²·φ⁻¹, a bijection on `u32`) so consecutive bars land far apart in
        // `[0, 1)` rather than adjacent, then normalise by `u32::MAX` — dividing a whole `u32` range
        // instead of `.fract()`-ing a float, since an already-whole-number float has no fractional
        // part for `.fract()` to return.
        let ordinal = *attached;
        *attached = attached.wrapping_add(1);
        let material = materials.add(HealthBarMaterial {
            settings: HealthBarUniform {
                fraction: health.fraction(),
                _pad0: ordinal.wrapping_mul(0x9E37_79B9) as f32 / u32::MAX as f32,
                _pad1: 0.0,
                _pad2: 0.0,
            },
        });
        commands.spawn((
            HealthBar { owner },
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(material),
            NotShadowCaster, // worldspace HUD: casts no shadow (see world::setup_lighting)
            Transform::default(),
        ));
        commands.entity(owner).insert(HasHealthBar);
    }
}

/// Uniform scale that holds a bar at a constant **apparent** size as the camera zooms.
///
/// The quad is a fixed size in world units while the viewport is not, so without this a bar's share
/// of the screen is `BAR_WIDTH / viewport_height` — it balloons as the player zooms in. Pure and
/// tested because the degenerate inputs matter: `CameraView` is readable before `setup_camera` runs
/// (its `Default` seeds the startup zoom, but a future change could not), and a zero or NaN here
/// would silently collapse every bar in the game to nothing.
fn bar_zoom_scale(viewport_height: Option<f32>) -> f32 {
    match viewport_height {
        Some(h) if h.is_finite() && h > 0.0 => h / BAR_REF_ZOOM,
        // No camera resource (the headless harness) or a nonsense viewport: draw at the authored
        // size, which is exactly the behaviour before this existed.
        _ => 1.0,
    }
}

/// Track each bar to its owner: reposition above the head, face the camera, refresh the fill, and
/// mirror the owner's visibility (so a fog-hidden enemy's bar hides too). Orphaned bars despawn.
fn update_health_bars(
    mut commands: Commands,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    // Optional: `HealthPlugin` runs in the headless harness (`sim_harness.rs`) where `CameraPlugin`
    // — the only registrar of `CameraView` (`camera.rs:105`) — is absent. In Bevy 0.19 a missing
    // `Res<T>` PANICS the system rather than skipping it, so a non-optional read here would take
    // every headless run down. Absent → scale 1.0, i.e. exactly the old behaviour.
    view: Option<Res<crate::camera::CameraView>>,
    owners: Query<(&Transform, &Health, &Visibility), Without<HealthBar>>,
    mut bars: Query<
        (
            Entity,
            &HealthBar,
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<HealthBarMaterial>,
        ),
        Without<Health>,
    >,
    mut materials: ResMut<Assets<HealthBarMaterial>>,
) {
    let cam_rot = camera.rotation();
    let zoom_scale = bar_zoom_scale(view.map(|v| v.viewport_height));

    for (bar_entity, bar, mut tf, mut vis, mat_handle) in &mut bars {
        let Ok((owner_tf, health, owner_vis)) = owners.get(bar.owner) else {
            // Owner is gone — clean up its bar.
            commands.entity(bar_entity).despawn();
            continue;
        };
        tf.translation = owner_tf.translation + Vec3::Y * BAR_Y;
        tf.rotation = cam_rot;
        tf.scale = Vec3::splat(zoom_scale);
        *vis = *owner_vis;
        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.settings.fraction = health.fraction();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyanide_smell_is_deterministic_and_about_a_quarter() {
        use super::smell_seed::CRAB;
        // Pure function of the spawn seed — same seed, same result (no RNG in the determinism hash).
        assert_eq!(
            CyanideSmell::from_seed_in(CRAB, 42).anosmic,
            CyanideSmell::from_seed_in(CRAB, 42).anosmic
        );
        assert_eq!(
            CyanideSmell::from_seed_in(CRAB, 0).anosmic,
            CyanideSmell::from_seed_in(CRAB, 0).anosmic
        );
        // ~1 in 4 are anosmic (Gidlow 2017: the HCN-odour sensitivity is x-linked recessive).
        let n = 20_000u64;
        let anosmic = (0..n).filter(|&s| CyanideSmell::from_seed_in(CRAB, s).anosmic).count();
        let frac = anosmic as f32 / n as f32;
        assert!((frac - 0.25).abs() < 0.02, "anosmia fraction {frac} is not ~1/4");
    }
}

#[cfg(test)]
mod bar_scale_tests {
    use super::*;

    #[test]
    fn a_bar_holds_a_constant_share_of_the_screen_across_the_zoom_range() {
        // The bug this fixes: the quad is fixed in WORLD units, so its share of the screen is
        // BAR_WIDTH / viewport_height — 3% of screen width at MAX_ZOOM but 22% at MIN_ZOOM. Five of
        // those over a clustered squad is the wall of green in
        // debug_screenshots/region_2026-07-29_13-12-23-426.
        let share = |zoom: f32| (BAR_WIDTH * bar_zoom_scale(Some(zoom))) / zoom;
        let near = share(crate::camera::MIN_ZOOM);
        let far = share(crate::camera::MAX_ZOOM);
        assert!(
            (near - far).abs() < 1.0e-5,
            "a bar must occupy the same share of the screen at both zoom extremes: \
             {near} at MIN_ZOOM vs {far} at MAX_ZOOM"
        );
    }

    #[test]
    fn the_authored_size_is_preserved_at_the_reference_zoom() {
        // BAR_WIDTH/BAR_HEIGHT/BAR_Y were all tuned by eye at the startup zoom. Scaling must be a
        // no-op there, or this "fix" silently re-tunes a calibration someone did against devshot.
        assert_eq!(bar_zoom_scale(Some(BAR_REF_ZOOM)), 1.0);
    }

    #[test]
    fn the_health_bar_fill_matches_the_theme_and_stays_desaturated() {
        // A shader cannot read a Rust resource, so `assets/shaders/health_bar.wgsl` carries the fill
        // colour as a literal and this is the only thing that can notice it drifting from
        // `UiTheme::health_fill`.
        //
        // It is worth a test because the drift already happened once, invisibly: the 2026-07-29 palette
        // pass desaturated the UI and left the shader's `vec3(0.30, 0.85, 0.38)` behind, which made the
        // worldspace health bars the single most saturated thing on screen — chroma 0.55, *higher* than
        // the phosphor accent that pass had just removed. Every unit test passed. A live screen capture
        // is what found it, and this test is so the next one is found earlier.
        let src = std::fs::read_to_string("assets/shaders/health_bar.wgsl")
            .expect("the health-bar shader must be readable from the crate root");
        let marker = "let fill = vec3<f32>(";
        let at = src.find(marker).expect("the shader must declare its fill colour") + marker.len();
        let rest = &src[at..];
        let end = rest.find(')').expect("unterminated vec3 in the shader");
        let parts: Vec<f32> = rest[..end]
            .split(',')
            .map(|t| t.trim().parse::<f32>().expect("shader fill components must be literals"))
            .collect();
        assert_eq!(parts.len(), 3, "the fill must be a vec3");

        let theme = crate::ui::theme::UiTheme::default();
        let want = theme.health_fill.to_srgba();
        for (got, want) in parts.iter().zip([want.red, want.green, want.blue]) {
            assert!(
                (got - want).abs() < 0.02,
                "the shader fill {parts:?} has drifted from UiTheme::health_fill {want:?}"
            );
        }

        // And independently of the theme: it must obey the same chroma ceiling every other
        // reality-describing colour does. A worldspace readout is not exempt from `docs/ui.md` §1.3
        // just because it is drawn by a shader instead of by `bevy_ui`.
        let (hi, lo) = parts.iter().fold((f32::MIN, f32::MAX), |(h, l), v| (h.max(*v), l.min(*v)));
        assert!(
            hi - lo <= crate::ui::theme::MAX_UI_CHROMA,
            "the health bar fill has chroma {:.3}; reality is desaturated (max {})",
            hi - lo,
            crate::ui::theme::MAX_UI_CHROMA
        );
    }

    #[test]
    fn the_selection_ring_is_bright_rather_than_green() {
        // `crate::palette::SELECTION_RING` marks the operatives an order will move. It was
        // `srgb(0.10, 1.00, 0.20)` — chroma 0.90, the most saturated colour in the game, and
        // specifically the GOC's Type Green, i.e. the *anomaly* colour painted onto the player's own
        // squad. Selection is a status, and status rides luminance (`docs/ui.md` §1.3).
        let c = crate::palette::SELECTION_RING.to_srgba();
        let (hi, lo) = (c.red.max(c.green).max(c.blue), c.red.min(c.green).min(c.blue));
        assert!(
            hi - lo <= crate::ui::theme::MAX_UI_CHROMA,
            "the selection ring has chroma {:.3} — it must read by brightness, not by hue",
            hi - lo
        );
        // Still the brightest thing on the floor, which is what actually made it visible.
        assert!(lo > 0.8, "the ring must stay bright against a near-black floor: {c:?}");
    }

    #[test]
    fn a_missing_or_nonsense_viewport_draws_at_the_authored_size() {
        // `HealthPlugin` runs in the headless harness, where `CameraView` does not exist. A zero or
        // NaN must not collapse every bar in the game to nothing.
        assert_eq!(bar_zoom_scale(None), 1.0);
        assert_eq!(bar_zoom_scale(Some(0.0)), 1.0);
        assert_eq!(bar_zoom_scale(Some(-4.0)), 1.0);
        assert_eq!(bar_zoom_scale(Some(f32::NAN)), 1.0);
        assert_eq!(bar_zoom_scale(Some(f32::INFINITY)), 1.0);
    }
}
