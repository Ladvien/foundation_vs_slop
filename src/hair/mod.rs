//! Physics-reactive accent hair for squad figurines — a small number of guide-hair "wisp" clumps
//! (Ward, Bertails, Kim, Marschner, Cani & Lin, "A Survey on Hair Modeling: Styling, Simulation, and
//! Rendering", IEEE TVCG 2007, DOI 10.1109/tvcg.2007.30 — the survey's case for simulating a handful
//! of guide strands rather than every fiber). Every squad member shares the same
//! `characters/valkyrie.glb` rig (recolored per outfit — see `squad::recolor_units`), so this applies
//! to all five.
//!
//! **These wisps are now the figurine's ONLY hair.** They used to be layered over a static hair card
//! baked into the rig; the 2026-08-01 decimation removed it (`valkyrie_body.001`, materials
//! `hair_cap_valkyrie`/`hair_cards_valkyrie`, which were also the rig's only two textures). Nothing
//! here read that mesh — roots are placed by a hand-tuned offset from the `head` bone, and the "sample
//! the scalp cap's triangles" upgrade noted below was never wired — so the removal cost this module no
//! wiring, only its backdrop. `tests/valkyrie_asset.rs` asserts the cap stays gone, because a
//! re-export that reinstates it would put two sets of hair in the same place.
//!
//! Each clump is a short particle chain anchored to the `head` bone. The solver lives in [`sim`] —
//! **Dynamic Follow-The-Leader** (Müller, Kim & Chentanez, "Fast Simulation of Inextensible Hair and
//! Fur", VRIPHYS 2012), which enforces inextensibility in a single root→tip projection pass and then
//! applies the paper's velocity correction (Eq. 9) to cancel the momentum that bare FTL invents. It
//! replaced a semi-implicit Verlet predictor with an XPBD *distance* constraint (Misra, IJSRP 2021,
//! DOI 10.29322/ijsrp.11.02.2021.p11053; Müller, Macklin, Chentanez, Jeschke & Kim, CGF 2020, DOI
//! 10.1111/cgf.14105), which could only *converge toward* the rest length and visibly stretched under
//! load. The XPBD skip-1 **bend** constraint survived that swap — a chain that cannot stretch still
//! has nothing stopping it curling under gravity/wind (Ward et al. 2007's point about wisp bending
//! stiffness) — as did the substepping, per Macklin, Storey, Lu, Terdiman, Chentanez, Jeschke &
//! Müller, "Small Steps in Physics Simulation", MIG 2019, DOI 10.1145/3309486.3340247. A full
//! Kirchhoff elastic-rod solver (Bertails, Audoly, Cani, Querleux, Leroy & Lévêque, "Super-Helices
//! for Predicting the Dynamics of Natural Hair", SIGGRAPH 2006, DOI 10.1145/1141911.1142012) was
//! considered and rejected as overkill at this character/camera scale. The ribbon-billboard rendering
//! follows Tariq & Bavoil, "Real Time Hair Simulation and Rendering on the GPU", SIGGRAPH 2008, DOI
//! 10.1145/1401032.1401080 (camera-facing thin geometry for hair fins, to avoid the aliasing a true
//! cylindrical cross-section would show at this scale).
//!
//! **Material: a direct Rust port of the character-asset generator's hair-card shader, not a custom
//! one.** An earlier version of this module used a hand-rolled `ExtendedMaterial<StandardMaterial,
//! HairExt>` with a single-lobe Kajiya-Kay WGSL term and no texture at all — in-game it rendered as
//! flat, untextured black jagged fins (a player region-capture flagged this directly: "the shader on
//! the hair is super stupid"). The actual game-standard hair-card technique this project's own asset
//! pipeline already uses (`/mnt/codex_fs/game_assets/SCP_Characters/scp_characters/hair.py`,
//! `HairCards._strand_image`/`_card_material` — pinned by that repo's `hair_range.hair_report`, which
//! explicitly checks `has_alpha_texture`/`cards_uv_mapped`, i.e. "is this actually a hair shader, not
//! a solid strap") is a plain lit `StandardMaterial` sampling a procedural strand-ALPHA texture in
//! `AlphaMode::Mask` — soft lock side-edges, vertical strand-brightness striations, and a frayed,
//! slit tip, with the hair colour baked directly into the texture. [`build_strand_texture`] is a
//! line-for-line port of that Python function, so the runtime physics strands read as the SAME
//! hair-card material family the rig's own baked card used before it was decimated away, per the
//! same real-time hair-card literature (Tariq & Bavoil 2008; Scheuermann, "Practical Real-Time Hair
//! Rendering and Shading", SIGGRAPH 2004, DOI 10.1145/1186223.1186408) rather than inventing a new
//! look.
//!
//! Purely cosmetic. The discovery/lifecycle pass runs on `Update`; the solve runs on **`PostUpdate`,
//! after `TransformSystems::Propagate`**, which is the only place a *fresh* skeleton can be read —
//! `bevy_animation`'s `advance_animations`/`animate_targets` are themselves `PostUpdate`
//! (`docs/animation.md`'s schedule table). The superseded version ran everything on `Update` and so
//! read a one-frame-stale pose: the scalp is skinned on the GPU from *this* frame's joint matrices
//! while the roots came from *last* frame's, so the hair detached from the head by `v_head · dt` every
//! frame — centimetres on a sprinting unit, on a strand that is ~25 cm long. `PostUpdate` is not
//! `FixedUpdate`; `TESTING.md`'s rule is "if it would appear in `snapshot_hash`, it belongs on
//! `FixedUpdate`", and nothing here can. `HairPlugin` is never registered in `sim_harness` (mirrors
//! `mycelia::MyceliaPlugin`'s precedent, not `vhs::VhsPlugin`'s — see
//! `lib::run`'s cosmetic-tuple comment), and every [`HairRig`] is a fully TOP-LEVEL entity — never a
//! `Children`-descendant of `Unit` — so the fracture bake's bounding-box DFS
//! (`bevy_carnage::bake_fractures`) can never walk into it. That DFS is what flipped held-in seed
//! `0xD00D`→`0xFEED` after the prior mesh swap (`squad_ai::coevolve`'s `HELD_IN_SEEDS` history), so
//! this boundary is load-bearing, not decorative — see [`HairRig`]'s doc comment.
//!
//! Exempt from the RL/QD genome for the same reason `vhs`/mycelia's pure-ambience knobs are: hair has
//! no collider, feeds no AI perception field, is never read by `laser::fire_laser`'s targeting, and
//! never touches `(&Transform, &Health)` — the only query `snapshot_hash` folds. Considered and
//! rejected, not silently skipped.

mod bind;
mod sim;

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::{Deserialize, Serialize};

use crate::squad::FigurineModel;

/// Same frame-delta clamp idiom `squad.rs`/other cosmetic systems use, so a hitch can't fling the
/// chain across several seconds of simulated motion in one jump.
const MAX_FRAME_DT: f32 = 1.0 / 30.0;
/// Exact glTF joint name this rig anchors to (confirmed present on the MPFB2 `game_engine` skeleton).
const HEAD_BONE_NAME: &str = "head";

// ---------------------------------------------------------------------------------------------
// Strand-card texture constants — a line-for-line port of `HairCards`' class constants in
// `/mnt/codex_fs/game_assets/SCP_Characters/scp_characters/hair.py`. Fixed (not RON-exposed) for the
// same reason the Python source keeps them as class constants rather than per-character builder
// args: they define "what a hair card looks like" for this project, not a per-unit tunable.
// ---------------------------------------------------------------------------------------------

const STRAND_TEX_W: u32 = 64;
const STRAND_TEX_H: u32 = 128;
const STRAND_COUNT: f32 = 6.0;
const EDGE_SOFT: f32 = 0.18;
const SLIT_W: f32 = 0.35;
const SLIT_DEPTH: f32 = 0.9;
const SLIT_START: f32 = 0.45;
const TIP_START: f32 = 0.55;
const TIP_RAGGED: f32 = 0.18;
const SHADE_LO: f32 = 0.62;
/// glTF `alphaMode: MASK` cutoff — matches `HairCards.ALPHA_THRESH`.
const CARD_ALPHA_THRESHOLD: f32 = 0.5;
/// Matches `HairCards.ROUGH`.
const CARD_ROUGHNESS: f32 = 0.85;

// ---------------------------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------------------------

/// Resolved once the figurine's scene has streamed in — the `head` bone entity this rig's clumps
/// anchor to. Lives on the `FigurineModel` child, same place `ValkyrieAnimPlayer`/`Recolored` already
/// live, never on `Unit` (the async, wall-clock-dependent resolution must not churn the pinned squad
/// archetype — see `squad.rs`'s issue #18 discussion).
#[derive(Component)]
struct HeadBoneRef(Entity);

/// Marks a `FigurineModel` that already has a spawned [`HairRig`], so `spawn_hair_rigs` runs once.
#[derive(Component)]
struct HasHairRig;

/// One squad member's simulated accent hair. **Must stay a fully top-level entity — never a
/// `Children`-descendant of `Unit`, at any depth, including not a sibling of `FigurineModel`.**
/// The fracture bake's DFS (`bevy_carnage::bake_fractures`) starts at `Query<(&FractureSubject,
/// &Children)>` — `FigurineSource` IS that component — and walks EVERY descendant of `Unit` into its
/// fracture bounding-box scan, with no opt-out tag today —
/// so any child of `Unit` would be folded into that scan and could re-perturb the mesh-extent-derived
/// gib piece count (the same measurement the prior Valkyrie mesh swap flipped a held-in RL/QD
/// calibration seed over). This follows `health::HealthBar`'s verified top-level pattern instead: a
/// bare `commands.spawn(...)` carrying an owner back-reference, never `.with_children`.
#[derive(Component)]
struct HairRig {
    /// Back-reference to the `FigurineModel` child that carries this rig's `HeadBoneRef`. Its absence
    /// is what retires this rig — see [`despawn_orphan_rigs`].
    figurine: Entity,
    clumps: Vec<sim::Guide>,
    /// This rig's ribbon mesh, mutated in place every frame via `Mesh::attribute_mut` — never rebuilt.
    mesh: Handle<Mesh>,
    /// False until `HeadBoneRef` resolves and the chain is initialized from its first live pose.
    seeded: bool,
    /// Reused `p_i^old` buffer for the solver, so stepping a rig never allocates.
    scratch: Vec<Vec3>,
}

// ---------------------------------------------------------------------------------------------
// Config — the `hair:` slice of the unified `assets/config/config.ron` (see `GoreSettings` for the
// sibling per-domain-slice convention this mirrors).
// ---------------------------------------------------------------------------------------------

/// Human-facing, serializable knobs — the `hair:` slice.
#[derive(Resource, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct HairSettings {
    pub clumps_per_unit: usize,
    pub segments_per_strand: usize,
    /// Per-segment rest length, world units (metres).
    pub rest_length: f32,
    /// XPBD skip-1 anti-curl constraint compliance (inverse stiffness; smaller = stiffer). This
    /// resists coiling, not stretching — [`sim::ftl_pass`] owns length outright, which is why the
    /// former `compliance` distance knob no longer exists.
    pub bend_compliance: f32,
    /// Fraction of velocity RETAINED PER SECOND, `[0, 1]`. Applied as `damping.powf(h)` so 30 fps and
    /// 240 fps settle identically. The superseded Verlet solver damped once per *substep*, so changing
    /// `substeps` silently changed the look.
    pub damping: f32,
    /// `s` in DFTL Eq. 9 (Müller et al. 2012 illustrate 0.9). 0 = raw FTL, which visibly gains energy;
    /// 1 = the successor's correction is fully absorbed by its leader.
    pub ftl_correction: f32,
    /// m/s^2 — a dedicated constant, deliberately NOT `main::GIB_GRAVITY` (hair is much lighter than
    /// a gib chunk and tuned independently).
    pub gravity: f32,
    /// Per-strand multiplier on `gravity`.
    pub gravity_scale: f32,
    pub wind_strength: f32,
    /// rad/s.
    pub wind_freq: f32,
    pub substeps: u32,
    /// Per-particle speed ceiling, m/s. A numeric backstop, not a second solver path — see
    /// [`sim::step_guide`]'s note on why it exists.
    pub max_speed: f32,
    pub strand_width_root: f32,
    pub strand_width_tip: f32,
    /// Linear-RGB base tint, baked into the procedural strand texture (see [`build_strand_texture`]).
    pub tint: [f32; 3],
}

/// Validated once at config load (`config::load_game_config`), alongside `gore::validate_settings` —
/// this project's "one path, no fallback" rule applied to hair's tunables: a bad value is a loud
/// startup panic, never a silently-clamped default.
pub fn validate_hair(c: &HairSettings) -> Result<(), String> {
    if !(1..=32).contains(&c.clumps_per_unit) {
        return Err(format!("hair.clumps_per_unit {} out of [1,32] (entity/vertex budget)", c.clumps_per_unit));
    }
    if !(1..=16).contains(&c.segments_per_strand) {
        return Err(format!("hair.segments_per_strand {} out of [1,16]", c.segments_per_strand));
    }
    if !(c.rest_length > 0.0 && c.rest_length.is_finite()) {
        return Err(format!("hair.rest_length must be > 0 and finite, got {}", c.rest_length));
    }
    if !(c.bend_compliance >= 0.0 && c.bend_compliance.is_finite()) {
        return Err(format!("hair.bend_compliance must be >= 0 and finite, got {}", c.bend_compliance));
    }
    if !(0.0..=1.0).contains(&c.damping) {
        return Err(format!("hair.damping must be in [0,1] (fraction retained per second), got {}", c.damping));
    }
    if !(0.0..=1.0).contains(&c.ftl_correction) {
        return Err(format!("hair.ftl_correction must be in [0,1], got {}", c.ftl_correction));
    }
    if !(c.gravity >= 0.0 && c.gravity.is_finite()) {
        return Err(format!("hair.gravity must be >= 0 and finite, got {}", c.gravity));
    }
    if !(c.gravity_scale >= 0.0 && c.gravity_scale.is_finite()) {
        return Err(format!("hair.gravity_scale must be >= 0 and finite, got {}", c.gravity_scale));
    }
    if !(c.wind_strength >= 0.0 && c.wind_strength.is_finite()) {
        return Err(format!("hair.wind_strength must be >= 0 and finite, got {}", c.wind_strength));
    }
    if !(c.wind_freq >= 0.0 && c.wind_freq.is_finite()) {
        return Err(format!("hair.wind_freq must be >= 0 and finite, got {}", c.wind_freq));
    }
    if !(1..=8).contains(&c.substeps) {
        return Err(format!("hair.substeps {} out of [1,8] (cost cap)", c.substeps));
    }
    if !(c.max_speed > 0.0 && c.max_speed.is_finite()) {
        return Err(format!("hair.max_speed must be > 0 and finite, got {}", c.max_speed));
    }
    if !(c.strand_width_root > 0.0 && c.strand_width_root.is_finite()) {
        return Err(format!("hair.strand_width_root must be > 0 and finite, got {}", c.strand_width_root));
    }
    if !(c.strand_width_tip > 0.0 && c.strand_width_tip <= c.strand_width_root) {
        return Err(format!(
            "hair.strand_width_tip ({}) must be in (0, strand_width_root ({})]",
            c.strand_width_tip, c.strand_width_root
        ));
    }
    if c.tint.iter().any(|&x| !(0.0..=1.0).contains(&x)) {
        return Err(format!("hair.tint out of [0,1]: {:?}", c.tint));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Material — a plain, lit `StandardMaterial` sampling a procedural strand-alpha texture in
// `AlphaMode::Mask`, ported from the asset generator's `HairCards` (see the module doc). Built ONCE
// at `Startup` and shared by every squad member's rig (all units share one hair tint, matching
// `recolor_units` leaving hair materials untouched across outfits).
// ---------------------------------------------------------------------------------------------

/// Shared, built-once hair-card texture + material — every [`HairRig`] clones the same
/// `Handle<StandardMaterial>`.
#[derive(Resource)]
struct HairAssets {
    material: Handle<StandardMaterial>,
}

#[inline]
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Procedural hair-strand alpha texture — a line-for-line port of `HairCards._strand_image`
/// (`/mnt/codex_fs/game_assets/SCP_Characters/scp_characters/hair.py`): a soft-edged lock body with
/// vertical strand-brightness striations (RGB) and a frayed, slit tip (alpha), with `tint` baked
/// directly into the colour channels exactly as the Python does (`col * shade`). Per-column tip
/// raggedness uses `util::hash01_u32` in place of the Python's seeded `random.Random` — this project
/// has no RNG crate, and it's a one-shot startup bake, not per-frame or hashed sim state, so any
/// stateless deterministic hash is fine here (unlike a spawn-seed key, this never needs to survive a
/// determinism-sensitive tie-break).
fn build_strand_texture(tint: Vec3) -> Image {
    let (w, h) = (STRAND_TEX_W, STRAND_TEX_H);

    let mut tip_start_per_col = vec![0.0f32; w as usize];
    for (col, slot) in tip_start_per_col.iter_mut().enumerate() {
        let jitter = crate::util::hash01_u32(col as u32) * 2.0 - 1.0;
        *slot = (TIP_START + jitter * TIP_RAGGED).clamp(0.2, 0.98);
    }

    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for row in 0..h {
        let v = (row as f32 + 0.5) / h as f32; // 0 at root, 1 at tip
        for col in 0..w {
            let u = (col as f32 + 0.5) / w as f32;

            let side_raw = ((0.5 - (u - 0.5).abs()) / EDGE_SOFT).clamp(0.0, 1.0);
            let side = side_raw * side_raw * (3.0 - 2.0 * side_raw); // smoothstep

            let strand = 0.5 + 0.5 * (std::f32::consts::TAU * STRAND_COUNT * u).cos();
            let shade = SHADE_LO + (1.0 - SHADE_LO) * strand;
            let slit = (1.0 - strand / SLIT_W).clamp(0.0, 1.0);

            let tip_start = tip_start_per_col[col as usize];
            let tipf = if v > tip_start {
                ((1.0 - v) / (1.0 - tip_start).max(1.0e-3)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let tipw = ((v - SLIT_START) / (1.0 - SLIT_START).max(1.0e-3)).clamp(0.0, 1.0);
            let alpha = (side * tipf * (1.0 - SLIT_DEPTH * slit * tipw)).clamp(0.0, 1.0);

            let rgb = tint * shade;
            let idx = ((row * w + col) * 4) as usize;
            rgba[idx] = (linear_to_srgb(rgb.x.clamp(0.0, 1.0)) * 255.0).round() as u8;
            rgba[idx + 1] = (linear_to_srgb(rgb.y.clamp(0.0, 1.0)) * 255.0).round() as u8;
            rgba[idx + 2] = (linear_to_srgb(rgb.z.clamp(0.0, 1.0)) * 255.0).round() as u8;
            rgba[idx + 3] = (alpha * 255.0).round() as u8;
        }
    }

    Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

/// Builds the shared strand texture + material once. Mirrors `setup_health_bar_assets`/
/// `setup_gore_assets`'s "shared assets built once at `Startup`" pattern.
fn setup_hair_assets(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    settings: Res<HairSettings>,
) {
    let texture = images.add(build_strand_texture(Vec3::from_array(settings.tint)));
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(texture),
        // Sort-free alpha MASK (matches `HairCards._card_material`'s `blend_method = "CLIP"`) — no
        // per-triangle sort needed, unlike the earlier `AlphaMode::Blend` attempt.
        alpha_mode: AlphaMode::Mask(CARD_ALPHA_THRESHOLD),
        perceptual_roughness: CARD_ROUGHNESS,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(HairAssets { material });
}

// ---------------------------------------------------------------------------------------------
// Bone anchoring
// ---------------------------------------------------------------------------------------------

/// DFS-walk each newly-streamed `FigurineModel` scene for the `head` bone, exactly mirroring
/// `carnage::tag_valkyrie_rifle`'s retry-next-frame pattern (that one matches `contains("rifle")` and
/// tags a mesh node; this one matches the bone name exactly and stores the bone entity, not a mesh).
fn locate_head_bone(
    mut commands: Commands,
    figurines: Query<Entity, (With<FigurineModel>, Without<HeadBoneRef>)>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    for figurine in &figurines {
        let mut stack: Vec<Entity> = match children.get(figurine) {
            Ok(c) => c.iter().collect(),
            Err(_) => continue, // scene not instantiated yet — retry next frame
        };
        let mut found: Option<Entity> = None;
        while let Some(e) = stack.pop() {
            if names.get(e).map(|n| n.as_str() == HEAD_BONE_NAME).unwrap_or(false) {
                found = Some(e);
                break; // exact match — "head" is the one node wanted, not a substring hit
            }
            if let Ok(ch) = children.get(e) {
                stack.extend(ch.iter());
            }
        }
        if let Some(bone) = found {
            commands.entity(figurine).insert(HeadBoneRef(bone));
        }
        // else: retry next frame.
    }
}

/// Once a figurine's head bone is known, spawn its (top-level — see [`HairRig`]) rig entity: a
/// pre-built ribbon mesh (topology only, positions zeroed until the first simulate tick) and a small
/// set of clumps arranged across the front hairline. Placement is a first-pass approximation — tune
/// by devshot, per this project's established convention for eyeballed cosmetic offsets.
fn spawn_hair_rigs(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    hair_assets: Res<HairAssets>,
    settings: Res<HairSettings>,
    figurines: Query<Entity, (With<FigurineModel>, With<HeadBoneRef>, Without<HasHairRig>)>,
) {
    for figurine in &figurines {
        let clump_count = settings.clumps_per_unit;
        let mut clumps = Vec::with_capacity(clump_count);
        for c in 0..clump_count {
            let t = if clump_count > 1 { c as f32 / (clump_count - 1) as f32 } else { 0.5 };
            let spread = (t - 0.5) * 0.16; // ~16 cm arc across the crown/nape
            // Head-bone-local axes, MEASURED at runtime (not assumed) via a temporary debug print
            // comparing each local axis (rotated to world) against the unit's known world-forward
            // (0,0,-1), per `squad.rs`'s documented convention: local +Y ≈ world up (dot(fwd) ≈
            // -0.04), local +Z ≈ world FORWARD/face (dot(fwd) ≈ +0.999) — not backward, as an earlier
            // version of this code wrongly assumed. That assumption made hair grow from the front
            // hairline and drape down over the face; a player region-capture flagged it directly. Roots
            // now sit at the crown/nape (local -Z, away from the face) and hang down-and-back.
            let root_local_offset = Vec3::new(spread, 0.15, -0.06);
            let root_local_dir = Vec3::new(0.0, -1.0, -0.12).normalize();
            // Bound to the single `head` joint at full weight, expressed in the general four-influence
            // form so the solver reads its root through `bind::eval_root` like every other groom will.
            // This is exactly equivalent to the superseded `bone_tf.transform_point(offset)` — see
            // `bind::tests::eval_root_follows_a_single_rigid_joint_exactly` — so it changes the code
            // path without changing a pixel. Real triangle sampling over the `hair_cap_valkyrie`
            // primitive replaces the hand-placed offset next.
            let root_bind = bind::RootBind {
                rest: root_local_offset,
                normal: root_local_dir,
                slots: [0; bind::INFLUENCES],
                weights: [1.0, 0.0, 0.0, 0.0],
            };
            // Position-independent per-clump phase (this is unhashed cosmetic state, unlike
            // `CyanideSmell::id`, so a raw `Entity` index is fine here — it only needs to look varied
            // across clumps/units, not survive a determinism-sensitive tie-break).
            let seed = figurine.index_u32().wrapping_mul(0x9E37_79B1).wrapping_add(c as u32);
            let phase = crate::util::hash01_u32(seed) * std::f32::consts::TAU;
            clumps.push(sim::Guide::new(settings.segments_per_strand, root_bind, phase));
        }

        let mesh_handle = meshes.add(build_hair_mesh(clump_count, settings.segments_per_strand));

        commands.spawn((
            HairRig { figurine, clumps, mesh: mesh_handle.clone(), seeded: false, scratch: Vec::new() },
            Mesh3d(mesh_handle),
            MeshMaterial3d(hair_assets.material.clone()),
            NotShadowCaster, // alpha-masked ribbon strands: casts no shadow (see world::setup_lighting)
            Transform::IDENTITY,
        ));
        commands.entity(figurine).insert(HasHairRig);
    }
}

/// Builds a hair-rig's ribbon-mesh topology (2 side-vertices per particle, quad-per-segment indices),
/// following the hand-authored `Mesh` idiom `nest::nest_dome_mesh` establishes. Positions/normals are
/// placeholder here — `update_hair_mesh` overwrites them in place every frame via
/// `Mesh::attribute_mut`; UVs are fixed at spawn and never touched again. No `ATTRIBUTE_TANGENT` — the
/// plain `StandardMaterial` this rig uses has no normal map, so vertex tangents are unused.
fn build_hair_mesh(clumps: usize, segments: usize) -> Mesh {
    let particles = segments + 1;
    let vert_count = clumps * particles * 2;
    let positions = vec![[0.0f32; 3]; vert_count];
    let normals = vec![[0.0f32, 1.0, 0.0]; vert_count];
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(vert_count);
    let mut indices: Vec<u32> = Vec::with_capacity(clumps * segments * 6);
    for c in 0..clumps {
        let base = (c * particles * 2) as u32;
        for p in 0..particles {
            let v = p as f32 / segments as f32; // 0 at root, 1 at tip
            uvs.push([0.0, v]);
            uvs.push([1.0, v]);
        }
        for s in 0..segments {
            let i0 = base + (s * 2) as u32;
            let (i1, i2, i3) = (i0 + 1, i0 + 2, i0 + 3);
            indices.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ---------------------------------------------------------------------------------------------
// Integration lives in `sim` — DFTL + XPBD bend, pure math, GPU-free and unit-tested there.
// ---------------------------------------------------------------------------------------------

/// Retire a rig whose figurine is gone, and only then.
///
/// This is the half the superseded version was missing entirely: `simulate_hair` did
/// `let Ok(..) = head_refs.get(rig.figurine) else { continue }` — it *skipped* an orphaned rig instead
/// of despawning it, so every `HairRig` leaked when its unit died and the `RunState::Idle → Active`
/// world rebuild leaked five more per run. With four static squad members that leaks nothing visible;
/// it stops being invisible the moment a groom is attached to anything that spawns and dies at
/// runtime. Copied from `health::update_health_bars`' orphan branch, which is the verified pattern for
/// a top-level entity holding an owner back-reference.
///
/// Ordering: every despawn path in the game lands strictly before `Update` — `despawn_dead_units` and
/// `crab_despawn_dead` are on `FixedUpdate` (inside `RunFixedMainLoop`), and `DespawnOnExit` fires in
/// `StateTransition`. So a rig whose owner died this frame is retired in this frame's `Update`, and the
/// `PostUpdate` solve never sees it.
fn despawn_orphan_rigs(mut commands: Commands, figurines: Query<(), With<FigurineModel>>, rigs: Query<(Entity, &HairRig)>) {
    for (entity, rig) in &rigs {
        if figurines.get(rig.figurine).is_err() {
            commands.entity(entity).despawn();
        }
    }
}

/// Advance every rig by one frame.
///
/// Runs on `PostUpdate` after `TransformSystems::Propagate`, so `bone_tf` is *this* frame's pose — see
/// the module doc for why the superseded `Update` placement was a one-frame lag rather than a saving.
/// Guides are stepped in fixed root→tip order inside [`sim::step_guide`], never via an ECS query, so no
/// `sort_total!`/`sort_value_canonical`/`SORT-OK` annotation is needed anywhere in this module.
fn simulate_hair(
    time: Res<Time>,
    settings: Res<HairSettings>,
    bones: Query<&GlobalTransform>,
    head_refs: Query<&HeadBoneRef>,
    mut rigs: Query<&mut HairRig>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    if dt <= 0.0 {
        return;
    }
    let elapsed = time.elapsed_secs();

    for mut rig in &mut rigs {
        let Ok(head_ref) = head_refs.get(rig.figurine) else { continue };
        let Ok(bone_tf) = bones.get(head_ref.0) else { continue };

        // The joint palette, composed ONCE per rig rather than once per root: a groom's roots share far
        // fewer joints than they have influences, and composing per root would multiply the sparse
        // `GlobalTransform` lookups by four. Today's palette is one entry — the `head` bone — because
        // the roots are still hand-placed in its local space; a mesh-bound groom will fill it from
        // `SkinnedMesh::joints` x `inverse_bindposes`.
        let palette = [bone_tf.affine()];

        // Split the borrow: `clumps` and `scratch` are both fields of the same `Mut<HairRig>`.
        let rig = &mut *rig;
        if !rig.seeded {
            for guide in &mut rig.clumps {
                let f = bind::eval_root(&guide.bind, &palette);
                sim::reseed(guide, f.pos, f.normal, settings.rest_length);
            }
            rig.seeded = true;
            continue; // start simulating from the next frame, once at rest
        }

        for guide in &mut rig.clumps {
            let f = bind::eval_root(&guide.bind, &palette);
            sim::step_guide(guide, f.pos, f.normal, dt, elapsed, &settings, &mut rig.scratch);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Mesh update — camera-facing ribbon billboard (Tariq & Bavoil 2008's motivation for camera-facing
// hair-fin geometry at this rendering scale).
// ---------------------------------------------------------------------------------------------

fn lerp_width(i: usize, particles: usize, root_w: f32, tip_w: f32) -> f32 {
    if particles <= 1 {
        return root_w;
    }
    let t = i as f32 / (particles - 1) as f32;
    root_w + (tip_w - root_w) * t
}

fn update_hair_mesh(
    camera: Single<&GlobalTransform, With<crate::MainCamera>>,
    settings: Res<HairSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    rigs: Query<&HairRig>,
) {
    let cam_right = camera.rotation() * Vec3::X;

    for rig in &rigs {
        if !rig.seeded {
            continue; // still degenerate-zero from spawn; nothing to draw yet
        }

        // Compute this frame's vertex data from the solver's particle positions into scratch buffers
        // first — `Mesh::attribute_mut` borrows the mesh mutably one attribute at a time, so the two
        // writes below can't overlap a single borrow of `mesh`.
        let mut new_positions: Vec<[f32; 3]> = Vec::new();
        let mut new_normals: Vec<[f32; 3]> = Vec::new();

        for clump in &rig.clumps {
            let n = clump.pos.len();
            for i in 0..n {
                let tangent = if i + 1 < n {
                    (clump.pos[i + 1] - clump.pos[i]).normalize_or_zero()
                } else {
                    (clump.pos[i] - clump.pos[i - 1]).normalize_or_zero()
                };
                let binormal = tangent.cross(cam_right).normalize_or_zero();
                let normal = binormal.cross(tangent).normalize_or_zero();
                let width = lerp_width(i, n, settings.strand_width_root, settings.strand_width_tip);
                let left = clump.pos[i] - binormal * width;
                let right = clump.pos[i] + binormal * width;
                new_positions.push(left.to_array());
                new_positions.push(right.to_array());
                new_normals.push(normal.to_array());
                new_normals.push(normal.to_array());
            }
        }

        let Some(mut mesh) = meshes.get_mut(&rig.mesh) else { continue };
        if let Some(VertexAttributeValues::Float32x3(buf)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
            buf.copy_from_slice(&new_positions);
        }
        if let Some(VertexAttributeValues::Float32x3(buf)) = mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL) {
            buf.copy_from_slice(&new_normals);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------------------------

/// Registered only in `lib::run`'s cosmetic render/FX tuple — never in `src/sim_harness.rs` (see the
/// module doc's determinism discussion).
pub struct HairPlugin;

impl Plugin for HairPlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `hair:` slice comes from the unified
        // `assets/config/config.ron`, loaded + validated once by `ConfigPlugin` (registered first).
        let settings = app.world().resource::<crate::config::GameConfig>().hair.clone();
        app.insert_resource(settings)
            .add_systems(Startup, setup_hair_assets)
            // Discovery + lifecycle: command-heavy retry-each-frame passes, and `Update` is the first
            // schedule after every despawn path in the game (see `despawn_orphan_rigs`).
            .add_systems(Update, (despawn_orphan_rigs, locate_head_bone, spawn_hair_rigs).chain())
            // The solve reads joint `GlobalTransform`s, which only exist for THIS frame after
            // `TransformSystems::Propagate` — itself downstream of `bevy_animation`'s `PostUpdate`
            // `animate_targets`. See the module doc.
            .add_systems(
                PostUpdate,
                (simulate_hair, update_hair_mesh).chain().after(TransformSystems::Propagate),
            );
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The solver's own tests live in [`sim`] — it is pure math over slices, which is what lets the
    /// whole thing run in the GPU-free `cargo test` hard gate. What is left here is the config
    /// contract.
    pub(super) fn test_settings() -> HairSettings {
        HairSettings {
            clumps_per_unit: 1,
            segments_per_strand: 5,
            rest_length: 0.05,
            bend_compliance: 0.002,
            damping: 0.05,
            ftl_correction: 0.9,
            gravity: 9.8,
            gravity_scale: 0.6,
            wind_strength: 0.0,
            wind_freq: 1.6,
            substeps: 4,
            max_speed: 12.0,
            strand_width_root: 0.062,
            strand_width_tip: 0.010,
            tint: [0.25, 0.14, 0.08],
        }
    }

    #[test]
    fn default_shipped_values_pass_validation() {
        validate_hair(&test_settings()).expect("the settings this module ships with must validate");
    }

    #[test]
    fn tip_wider_than_root_is_rejected() {
        let mut bad = test_settings();
        bad.strand_width_tip = bad.strand_width_root * 2.0;
        assert!(validate_hair(&bad).is_err(), "tip must not be wider than root");
    }

    #[test]
    fn a_negative_bend_compliance_is_rejected() {
        let mut bad = test_settings();
        bad.bend_compliance = -1.0;
        assert!(validate_hair(&bad).is_err(), "compliance is an inverse stiffness; negative is meaningless");
    }

    /// `ftl_correction` outside [0,1] is not a taste question — it is `s` in DFTL Eq. 9, and a value
    /// above 1 over-subtracts the successor's correction and drives the leader backwards.
    #[test]
    fn an_out_of_range_ftl_correction_is_rejected() {
        for bad_value in [-0.1, 1.5] {
            let mut bad = test_settings();
            bad.ftl_correction = bad_value;
            assert!(validate_hair(&bad).is_err(), "ftl_correction {bad_value} must be rejected");
        }
    }

    /// A zero speed ceiling would freeze the hair solid rather than merely cap it.
    #[test]
    fn a_non_positive_max_speed_is_rejected() {
        let mut bad = test_settings();
        bad.max_speed = 0.0;
        assert!(validate_hair(&bad).is_err(), "max_speed must be > 0");
    }

    /// A bare-`App`-with-one-system harness, following `anim::tests::harness`'s precedent. This is
    /// deliberately NOT `HairPlugin`: registering the plugin would need `GameConfig`, `Startup` asset
    /// bakes, and a render world, and `TESTING.md`'s plugin boundary is what keeps hair outside
    /// `snapshot_hash`. One system in isolation is enough to pin its contract.
    fn orphan_harness() -> App {
        let mut app = App::new();
        app.add_systems(Update, despawn_orphan_rigs);
        app
    }

    fn spawn_rig(app: &mut App, figurine: Entity) -> Entity {
        app.world_mut()
            .spawn(HairRig { figurine, clumps: Vec::new(), mesh: Handle::default(), seeded: true, scratch: Vec::new() })
            .id()
    }

    /// The leak this module shipped with: `simulate_hair` skipped an orphaned rig with `continue`
    /// instead of despawning it, so every `HairRig` outlived its unit and the `RunState::Idle → Active`
    /// world rebuild leaked five more per run. Invisible with four static squad members; not invisible
    /// the moment a groom is attached to anything that spawns and dies at runtime.
    #[test]
    fn a_rig_is_retired_when_its_figurine_despawns() {
        let mut app = orphan_harness();
        let figurine = app.world_mut().spawn(FigurineModel).id();
        let rig = spawn_rig(&mut app, figurine);

        app.update();
        assert!(app.world().get_entity(rig).is_ok(), "a rig with a live figurine must survive");

        app.world_mut().entity_mut(figurine).despawn();
        app.update();
        assert!(app.world().get_entity(rig).is_err(), "a rig whose figurine died must be despawned, not skipped");
    }

    /// Retiring one unit's rig must not touch its squadmates' — the sweep keys on each rig's own
    /// back-reference, and getting that wrong would bald the whole squad when one member dies.
    #[test]
    fn retiring_one_rig_leaves_its_squadmates_alone() {
        let mut app = orphan_harness();
        let doomed = app.world_mut().spawn(FigurineModel).id();
        let survivor = app.world_mut().spawn(FigurineModel).id();
        let doomed_rig = spawn_rig(&mut app, doomed);
        let survivor_rig = spawn_rig(&mut app, survivor);

        app.world_mut().entity_mut(doomed).despawn();
        app.update();

        assert!(app.world().get_entity(doomed_rig).is_err(), "the dead unit's rig must go");
        assert!(app.world().get_entity(survivor_rig).is_ok(), "the living unit's rig must stay");
    }

    // There is deliberately NO test here for "a rig pointed at a recycled entity id is still
    // retired". That property holds because `Entity` carries a generation and `Query::get` rejects a
    // stale one — a Bevy invariant, not this module's logic. Staging it would mean forcing the entity
    // allocator to reuse an index on demand, which it does not promise to do (an attempt at this test
    // failed on its own setup assertion, not on the behaviour), and a test that depends on allocator
    // internals is brittle against every Bevy upgrade. The `sort_total!` panic message's warning that a
    // raw `Entity` is never a safe *sort key* is a different concern: that is about ordering across
    // separate `App` instances, whereas this is a same-world liveness check.
}
