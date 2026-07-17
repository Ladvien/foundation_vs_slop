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
/// entity the same tick, so their mutual order was never pinned. [`crate::almond_water::almond_water_effect`]
/// orders itself `.after(HealthDamage)` so the consuming heal/poison always composes on top of the tick's damage
/// deterministically — otherwise, once foraging clusters wounded crabs into weapon range, heal-vs-damage
/// clamping races and `snapshot_hash` flips per process. Each damage system opts in with `.in_set(...)` at
/// its own registration; the set carries no ordering of its own, only a name to sequence the heal behind.
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

impl CyanideSmell {
    /// Deterministic per-spawn assignment: ~1 in 4 biologicals are anosmic. A pure function of the entity's
    /// already-hashed spawn seed (a splitmix64 finalizer), so no RNG enters the determinism hash and no
    /// archetype churns at runtime.
    ///
    /// The finalizer is a **bijection**, so distinct spawn seeds give distinct [`id`](Self::id)s — which is
    /// what makes it usable as a sort tiebreak, not merely a well-mixed one.
    pub fn from_seed(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Self { anosmic: z % 4 == 0, id: z }
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
        // Pure function of the spawn seed — same seed, same result (no RNG in the determinism hash).
        assert_eq!(CyanideSmell::from_seed(42).anosmic, CyanideSmell::from_seed(42).anosmic);
        assert_eq!(CyanideSmell::from_seed(0).anosmic, CyanideSmell::from_seed(0).anosmic);
        // ~1 in 4 are anosmic (Gidlow 2017: the HCN-odour sensitivity is x-linked recessive).
        let n = 20_000u64;
        let anosmic = (0..n).filter(|&s| CyanideSmell::from_seed(s).anosmic).count();
        let frac = anosmic as f32 / n as f32;
        assert!((frac - 0.25).abs() < 0.02, "anosmia fraction {frac} is not ~1/4");
    }
}
