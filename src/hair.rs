//! Physics-reactive accent hair for squad figurines — a small number of guide-hair "wisp" clumps
//! (Ward, Bertails, Kim, Marschner, Cani & Lin, "A Survey on Hair Modeling: Styling, Simulation, and
//! Rendering", IEEE TVCG 2007, DOI 10.1109/tvcg.2007.30 — the survey's case for simulating a handful
//! of guide strands rather than every fiber) layered over the static `valkyrie_hair` card already
//! baked into the figurine rig. Every squad member shares the same `characters/valkyrie.glb` rig
//! (recolored per outfit — see `squad::recolor_units`), so this applies to all five.
//!
//! Each clump is a short particle chain anchored to the `head` bone, integrated with semi-implicit
//! Verlet (Misra, "Real-Time Dynamic Fur and Hair Simulation using Verlet Integration", IJSRP 2021,
//! DOI 10.29322/ijsrp.11.02.2021.p11053 — the applied-games recipe this module follows most directly)
//! and XPBD-compliant distance + bend constraints (Müller, Macklin, Chentanez, Jeschke & Kim,
//! "Detailed Rigid Body Simulation with Extended Position Based Dynamics", CGF 2020, DOI
//! 10.1111/cgf.14105 — the compliance parameter decouples perceived stiffness from iteration count
//! and timestep, which matters running on variable-dt `Update` rather than fixed-dt). Substepped per
//! Macklin, Storey, Lu, Terdiman, Chentanez, Jeschke & Müller, "Small Steps in Physics Simulation",
//! MIG 2019, DOI 10.1145/3309486.3340247 — a few substeps with one relaxation pass each beats few
//! substeps with many iterations, for both stability and cost. The bend constraint exists because a
//! pure distance-constraint chain has nothing stopping it curling under gravity/wind (Ward et al.
//! 2007's point about wisp bending stiffness). A full Kirchhoff elastic-rod solver (Bertails, Audoly,
//! Cani, Querleux, Leroy & Lévêque, "Super-Helices for Predicting the Dynamics of Natural Hair",
//! SIGGRAPH 2006, DOI 10.1145/1141911.1142012) was considered and rejected as overkill at this
//! character/camera scale. The ribbon-billboard rendering follows Tariq & Bavoil, "Real Time Hair
//! Simulation and Rendering on the GPU", SIGGRAPH 2008, DOI 10.1145/1401032.1401080 (camera-facing
//! thin geometry for hair fins, to avoid the aliasing a true cylindrical cross-section would show at
//! this scale).
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
//! hair-card material family as the static `valkyrie_hair` card sitting right next to them, per the
//! same real-time hair-card literature (Tariq & Bavoil 2008; Scheuermann, "Practical Real-Time Hair
//! Rendering and Shading", SIGGRAPH 2004, DOI 10.1145/1186223.1186408) rather than inventing a new
//! look.
//!
//! Purely cosmetic: every system here is `Update`-scheduled only, `HairPlugin` is never registered in
//! `sim_harness` (mirrors `mycelia::MyceliaPlugin`'s precedent, not `vhs::VhsPlugin`'s — see
//! `lib::run`'s cosmetic-tuple comment), and every [`HairRig`] is a fully TOP-LEVEL entity — never a
//! `Children`-descendant of `Unit` — so `autogib::bake_autogib`'s fracture-bounding-box DFS
//! (`autogib.rs`, `bake_autogib`) can never walk into it. That DFS is what flipped held-in seed
//! `0xD00D`→`0xFEED` after the prior mesh swap (`squad_ai::coevolve`'s `HELD_IN_SEEDS` history), so
//! this boundary is load-bearing, not decorative — see [`HairRig`]'s doc comment.
//!
//! Exempt from the RL/QD genome for the same reason `vhs`/mycelia's pure-ambience knobs are: hair has
//! no collider, feeds no AI perception field, is never read by `laser::fire_laser`'s targeting, and
//! never touches `(&Transform, &Health)` — the only query `snapshot_hash` folds. Considered and
//! rejected, not silently skipped.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
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

/// One guide-hair clump (see the module doc's Ward et al. 2007 citation). `pos[0]`/`prev[0]` are the
/// root, hard-pinned to the head bone every frame (inverse mass 0); the rest are simulated.
struct HairClump {
    pos: Vec<Vec3>,
    prev: Vec<Vec3>,
    /// Attachment point, in the head bone's local space, fixed at seed time.
    root_local_offset: Vec3,
    /// Initial growth direction, head-bone-local, fixed at seed time.
    root_local_dir: Vec3,
    /// Per-clump wind-phase offset (radians) so clumps don't sway in lockstep.
    phase: f32,
}

impl HairClump {
    fn new(segments: usize, root_local_offset: Vec3, root_local_dir: Vec3, phase: f32) -> Self {
        let particles = segments + 1;
        HairClump {
            pos: vec![Vec3::ZERO; particles],
            prev: vec![Vec3::ZERO; particles],
            root_local_offset,
            root_local_dir,
            phase,
        }
    }
}

/// One squad member's simulated accent hair. **Must stay a fully top-level entity — never a
/// `Children`-descendant of `Unit`, at any depth, including not a sibling of `FigurineModel`.**
/// `bake_autogib`'s DFS (`autogib.rs`) starts at `Query<(&FigurineSource, &Children), With<Unit>>` and
/// walks EVERY descendant of `Unit` into its fracture bounding-box scan, with no opt-out tag today —
/// so any child of `Unit` would be folded into that scan and could re-perturb the mesh-extent-derived
/// gib piece count (the same measurement the prior Valkyrie mesh swap flipped a held-in RL/QD
/// calibration seed over). This follows `health::HealthBar`'s verified top-level pattern instead: a
/// bare `commands.spawn(...)` carrying an owner back-reference, never `.with_children`.
#[derive(Component)]
struct HairRig {
    /// Back-reference to the `FigurineModel` child that carries this rig's `HeadBoneRef`.
    figurine: Entity,
    clumps: Vec<HairClump>,
    /// This rig's ribbon mesh, mutated in place every frame via `Mesh::attribute_mut` — never rebuilt.
    mesh: Handle<Mesh>,
    /// False until `HeadBoneRef` resolves and the chain is initialized from its first live pose.
    seeded: bool,
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
    /// XPBD distance-constraint compliance (inverse stiffness; smaller = stiffer).
    pub compliance: f32,
    /// XPBD skip-1 anti-curl constraint compliance — looser than `compliance` on purpose (resists
    /// coiling, not stretching).
    pub bend_compliance: f32,
    /// Verlet velocity damping factor, `(0, 1]`.
    pub damping: f32,
    /// m/s^2 — a dedicated constant, deliberately NOT `main::GIB_GRAVITY` (hair is much lighter than
    /// a gib chunk and tuned independently).
    pub gravity: f32,
    /// Per-strand multiplier on `gravity`.
    pub gravity_scale: f32,
    pub wind_strength: f32,
    /// rad/s.
    pub wind_freq: f32,
    pub substeps: u32,
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
    if !(c.compliance >= 0.0 && c.compliance.is_finite()) {
        return Err(format!("hair.compliance must be >= 0 and finite, got {}", c.compliance));
    }
    if !(c.bend_compliance >= c.compliance && c.bend_compliance.is_finite()) {
        return Err(format!(
            "hair.bend_compliance ({}) must be finite and >= hair.compliance ({}) — bend must not \
             out-stiffen the chain",
            c.bend_compliance, c.compliance
        ));
    }
    if !(0.0..=1.0).contains(&c.damping) {
        return Err(format!("hair.damping must be in [0,1], got {}", c.damping));
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
/// `autogib::tag_valkyrie_rifle`'s retry-next-frame pattern (that one matches `contains("rifle")` and
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
            // Position-independent per-clump phase (this is unhashed cosmetic state, unlike
            // `CyanideSmell::id`, so a raw `Entity` index is fine here — it only needs to look varied
            // across clumps/units, not survive a determinism-sensitive tie-break).
            let seed = figurine.index_u32().wrapping_mul(0x9E37_79B1).wrapping_add(c as u32);
            let phase = crate::util::hash01_u32(seed) * std::f32::consts::TAU;
            clumps.push(HairClump::new(settings.segments_per_strand, root_local_offset, root_local_dir, phase));
        }

        let mesh_handle = meshes.add(build_hair_mesh(clump_count, settings.segments_per_strand));

        commands.spawn((
            HairRig { figurine, clumps, mesh: mesh_handle.clone(), seeded: false },
            Mesh3d(mesh_handle),
            MeshMaterial3d(hair_assets.material.clone()),
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
// Integration — semi-implicit Verlet predictor + XPBD-with-compliance constraint projection, 1
// relaxation pass per substep (see the module doc's citations).
// ---------------------------------------------------------------------------------------------

#[inline]
fn inv_mass(i: usize) -> f32 {
    if i == 0 { 0.0 } else { 1.0 }
}

/// Hand-rolled layered-sine ambient wind (no RNG crate, matching project convention) — a CPU force,
/// not sampled from the shader-side `assets/shaders/noise.wgsl` library (that library is
/// fragment/GPU-side; this is a `Update`-scheduled Rust force calculation, so a shader import doesn't
/// apply here). Tip particles sway more than root particles since the tip is the free end.
fn wind_accel(phase: f32, particle_idx: usize, elapsed: f32, s: &HairSettings) -> Vec3 {
    let w = elapsed * s.wind_freq + phase;
    let sway = (w.sin() + 0.4 * (w * 2.3 + phase).sin()) * s.wind_strength;
    let tip_factor = (particle_idx as f32 / 4.0).min(1.0);
    Vec3::new(sway, 0.15 * sway, sway * 0.7 * (w * 0.6).cos()) * tip_factor
}

/// Single XPBD relaxation pass for one distance constraint between `pos[i]` and `pos[j]` (Müller et
/// al. 2020, eq. 4-6, single-iteration-per-substep form: the Lagrange multiplier resets to 0 each
/// call, so `Δλ = -C / (w_i + w_j + α̃)`).
fn xpbd_distance_correct(pos: &mut [Vec3], i: usize, j: usize, rest: f32, alpha_tilde: f32, inv_i: f32, inv_j: f32) {
    let d = pos[j] - pos[i];
    let len = d.length();
    if len < 1.0e-6 {
        return; // degenerate separation — skip rather than divide by ~0
    }
    let dir = d / len;
    let c = len - rest;
    let w_sum = inv_i + inv_j;
    if w_sum <= 0.0 {
        return; // both ends pinned, nothing to correct
    }
    let d_lambda = -c / (w_sum + alpha_tilde);
    pos[i] -= inv_i * d_lambda * dir;
    pos[j] += inv_j * d_lambda * dir;
}

/// Initialize a clump's chain hanging straight along its rest direction from the bone's first live
/// pose — the seed tick after `HeadBoneRef` resolves, before any simulation runs.
fn seed_clump(clump: &mut HairClump, bone_tf: &GlobalTransform, rest_length: f32) {
    let root = bone_tf.transform_point(clump.root_local_offset);
    let dir = (bone_tf.rotation() * clump.root_local_dir).normalize_or_zero();
    for i in 0..clump.pos.len() {
        let p = root + dir * (i as f32 * rest_length);
        clump.pos[i] = p;
        clump.prev[i] = p;
    }
}

/// Advance one clump by one `Update` frame: pin the root to `root_world`, then run `settings.substeps`
/// XPBD substeps (predict non-root particles under gravity + wind, then project the distance + bend
/// constraints in fixed root→tip order — never via an ECS query, so no `sort_total!`/
/// `sort_value_canonical`/`SORT-OK` annotation is needed anywhere in this module).
fn step_clump(clump: &mut HairClump, root_world: Vec3, dt: f32, elapsed: f32, settings: &HairSettings) {
    let substeps = settings.substeps.max(1);
    let dt_sub = dt / substeps as f32;
    let alpha_dist = settings.compliance / (dt_sub * dt_sub);
    let alpha_bend = settings.bend_compliance / (dt_sub * dt_sub);
    let gravity_accel = Vec3::NEG_Y * settings.gravity * settings.gravity_scale;

    // The bone's `GlobalTransform` is sampled once per frame (animation only updates once per `Update`
    // tick regardless), so the pin is the same position across this frame's substeps.
    clump.pos[0] = root_world;
    clump.prev[0] = root_world;

    for _ in 0..substeps {
        let n = clump.pos.len();
        for i in 1..n {
            let accel = gravity_accel + wind_accel(clump.phase, i, elapsed, settings);
            let vel = (clump.pos[i] - clump.prev[i]) * settings.damping;
            let new_pos = clump.pos[i] + vel + accel * dt_sub * dt_sub;
            clump.prev[i] = clump.pos[i];
            clump.pos[i] = new_pos;
        }
        for i in 0..n - 1 {
            xpbd_distance_correct(&mut clump.pos, i, i + 1, settings.rest_length, alpha_dist, inv_mass(i), inv_mass(i + 1));
        }
        if n >= 3 {
            for i in 0..n - 2 {
                xpbd_distance_correct(&mut clump.pos, i, i + 2, 2.0 * settings.rest_length, alpha_bend, inv_mass(i), inv_mass(i + 2));
            }
        }
    }
}

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

        if !rig.seeded {
            for clump in &mut rig.clumps {
                seed_clump(clump, bone_tf, settings.rest_length);
            }
            rig.seeded = true;
            continue; // start simulating from the next frame, once at rest
        }

        for clump in &mut rig.clumps {
            let root_world = bone_tf.transform_point(clump.root_local_offset);
            step_clump(clump, root_world, dt, elapsed, &settings);
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
    camera: Single<&GlobalTransform, With<Camera3d>>,
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
        app.insert_resource(settings).add_systems(Startup, setup_hair_assets).add_systems(
            Update,
            (locate_head_bone, spawn_hair_rigs, simulate_hair, update_hair_mesh).chain(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> HairSettings {
        HairSettings {
            clumps_per_unit: 1,
            segments_per_strand: 5,
            rest_length: 0.05,
            compliance: 0.0002,
            bend_compliance: 0.002,
            damping: 0.96,
            gravity: 9.8,
            gravity_scale: 0.6,
            wind_strength: 0.0,
            wind_freq: 1.6,
            substeps: 4,
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
    fn bend_compliance_below_compliance_is_rejected() {
        let mut bad = test_settings();
        bad.bend_compliance = bad.compliance * 0.5;
        assert!(validate_hair(&bad).is_err(), "bend must not out-stiffen the primary chain");
    }

    #[test]
    fn tip_wider_than_root_is_rejected() {
        let mut bad = test_settings();
        bad.strand_width_tip = bad.strand_width_root * 2.0;
        assert!(validate_hair(&bad).is_err(), "tip must not be wider than root");
    }

    #[test]
    fn a_pinned_clump_settles_without_stretching_past_rest_length() {
        let settings = test_settings();
        let n = settings.segments_per_strand + 1;
        let mut clump = HairClump::new(settings.segments_per_strand, Vec3::ZERO, Vec3::NEG_Y, 0.0);
        for i in 0..n {
            let p = Vec3::NEG_Y * (i as f32 * settings.rest_length);
            clump.pos[i] = p;
            clump.prev[i] = p;
        }
        let root = Vec3::ZERO;
        // Settle under gravity alone (no wind) for several seconds of simulated frames.
        for _ in 0..300 {
            step_clump(&mut clump, root, 1.0 / 60.0, 0.0, &settings);
        }
        assert_eq!(clump.pos[0], root, "root must stay pinned exactly at the bone");
        for i in 0..n - 1 {
            let len = (clump.pos[i + 1] - clump.pos[i]).length();
            let rest = settings.rest_length;
            assert!(
                (len - rest).abs() < rest * 0.15,
                "segment {i} length {len} strayed too far from rest {rest} after settling"
            );
        }
        for p in &clump.pos {
            assert!(p.is_finite(), "a settled clump must never produce NaN/Inf: {p:?}");
        }
    }

    #[test]
    fn xpbd_distance_correct_pulls_a_stretched_pair_toward_rest_length() {
        let mut pos = vec![Vec3::ZERO, Vec3::new(0.0, -1.0, 0.0)]; // 1.0 apart
        let rest = 0.05;
        let alpha_tilde = 0.0002 / (1.0_f32 / 60.0).powi(2);
        for _ in 0..50 {
            xpbd_distance_correct(&mut pos, 0, 1, rest, alpha_tilde, 0.0, 1.0);
        }
        let len = (pos[1] - pos[0]).length();
        assert!((len - rest).abs() < rest * 0.2, "did not converge toward rest length, got {len}");
        assert_eq!(pos[0], Vec3::ZERO, "an inv_mass-0 endpoint must never move");
    }

    #[test]
    fn wind_accel_is_stronger_at_the_tip_than_the_root() {
        let s = test_settings();
        let mut s = s.clone();
        s.wind_strength = 1.0;
        let root = wind_accel(0.3, 0, 1.0, &s).length();
        let tip = wind_accel(0.3, 5, 1.0, &s).length();
        assert!(tip >= root, "tip (free end) should sway at least as much as the root, got root={root} tip={tip}");
    }
}
