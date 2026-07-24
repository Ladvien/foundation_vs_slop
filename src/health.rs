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
/// Bar quad size in world units (wide and short).
const BAR_WIDTH: f32 = 1.1;
const BAR_HEIGHT: f32 = 0.16;

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
            .add_systems(Update, (attach_health_bars, update_health_bars).chain());
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
) {
    for (owner, health) in &owners {
        let material = materials.add(HealthBarMaterial {
            settings: HealthBarUniform {
                fraction: health.fraction(),
                _pad0: 0.0,
                _pad1: 0.0,
                _pad2: 0.0,
            },
        });
        commands.spawn((
            HealthBar { owner },
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(material),
            Transform::default(),
        ));
        commands.entity(owner).insert(HasHealthBar);
    }
}

/// Track each bar to its owner: reposition above the head, face the camera, refresh the fill, and
/// mirror the owner's visibility (so a fog-hidden enemy's bar hides too). Orphaned bars despawn.
fn update_health_bars(
    mut commands: Commands,
    camera: Single<&GlobalTransform, With<Camera3d>>,
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
    for (bar_entity, bar, mut tf, mut vis, mat_handle) in &mut bars {
        let Ok((owner_tf, health, owner_vis)) = owners.get(bar.owner) else {
            // Owner is gone — clean up its bar.
            commands.entity(bar_entity).despawn();
            continue;
        };
        tf.translation = owner_tf.translation + Vec3::Y * BAR_Y;
        tf.rotation = cam_rot;
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
