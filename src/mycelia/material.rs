//! The mold's *shading*. The compute chain produces raw simulation fields; everything about how the mold
//! looks — lighting, surface normal, wetness, bioluminescence — lives here.
//!
//! Both materials are `ExtendedMaterial<StandardMaterial, _>`, so the mold is a genuinely **lit PBR
//! surface** rather than a flat unlit decal. It picks up the scene's directional key and ambient fill, its
//! normal is perturbed by the biomass thickness (so it reads as a lumpy wet film), and its roughness drops
//! where it is thickest (a wet sheen). That is what stops it looking like paint.
//!
//! Two surfaces, one field:
//! - [`MoldFloorMaterial`] on the translucent floor overlay, sampling the field by world XZ.
//! - [`MoldWallMaterial`] on the wall slabs, sampling the *same* field at the wall's footprint XZ and
//!   fading with height, so the coating visibly creeps up out of the floor/wall corner.
//!
//! Only `fragment_shader()` is overridden, so the prepass keeps `StandardMaterial`'s default — no
//! `PREPASS_PIPELINE` branching is needed in the WGSL.

use bevy::asset::Asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    StandardMaterial,
};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use super::{MyceliaConfig, WORLD_EXTENT, WORLD_ORIGIN};

/// The lit floor coating.
pub type MoldFloorMaterial = ExtendedMaterial<StandardMaterial, MoldFloorExt>;
/// The lit coating creeping up the walls.
pub type MoldWallMaterial = ExtendedMaterial<StandardMaterial, MoldWallExt>;
/// The fruit body itself — the same organism, wearing flesh instead of film.
pub type MoldFruitMaterial = ExtendedMaterial<StandardMaterial, MoldFruitExt>;

/// GPU uniform shared by both surface shaders — must byte-match `MoldSurfaceParams` in
/// `mycelia_floor.wgsl` / `mycelia_wall.wgsl`. `vec2`s first so every following scalar is naturally aligned.
#[derive(Clone, ShaderType)]
pub struct MoldSurfaceParams {
    /// World XZ of field texel (0,0) — the same mapping the compute chain writes with.
    world_origin: Vec2,
    /// World-space span the field covers.
    world_extent: Vec2,
    /// Field resolution in texels (for the finite-difference normal).
    field_res: Vec2,
    /// Multiplier on the veins' emissive. The camera is LDR with no bloom, so this stays near 1.
    glow_gain: f32,
    /// Master opacity dial.
    intensity: f32,
    /// Trail value at which a vein begins to show.
    vein_lo: f32,
    /// Trail value at which a vein is fully lit.
    vein_hi: f32,
    /// Strength of the thickness-derived normal perturbation. `0` = flat.
    normal_strength: f32,
    /// Perceptual roughness in the **vein cores only**. The body of the mat stays matte.
    wet_roughness: f32,
    /// How far up a wall the mold climbs before fading out (world units).
    climb_height: f32,
    /// Spatial frequency of the hyphal filament noise, in cycles per world unit.
    fiber_scale: f32,
    /// How hard the filaments carve the surface normal.
    fiber_strength: f32,
    /// How much fbm breaks up the colony's outer contour. `0` = a smooth iso-contour meniscus (reads as
    /// liquid); higher = a feathery, dendritic advancing margin (reads as a fungus).
    margin_roughness: f32,
    /// Strength of the grazing-angle fuzz rim that stands in for a sheen lobe.
    sheen_strength: f32,
    /// Strength of the cavity ambient occlusion written into `diffuse_occlusion`. Without this the scene's
    /// bright uniform ambient fills every crevice and the filaments render flat regardless of the normal.
    ao_strength: f32,
    /// UV distance the reveal/coverage tap is domain-warped by world-space fbm, so the coat's edge stops
    /// snapping to the per-cell control-texture grid (the "tiled" read). `0` = off.
    reveal_warp_amp: f32,
    /// Frequency (cycles per world unit) of that warp noise.
    reveal_warp_scale: f32,
}

impl MoldSurfaceParams {
    fn new(cfg: &MyceliaConfig) -> Self {
        Self {
            world_origin: WORLD_ORIGIN,
            world_extent: WORLD_EXTENT,
            field_res: Vec2::splat(cfg.field_size as f32),
            glow_gain: cfg.glow_gain,
            intensity: cfg.intensity,
            vein_lo: cfg.vein_lo,
            vein_hi: cfg.vein_hi,
            normal_strength: cfg.normal_strength,
            wet_roughness: cfg.wet_roughness,
            climb_height: cfg.climb_height,
            fiber_scale: cfg.fiber_scale,
            fiber_strength: cfg.fiber_strength,
            margin_roughness: cfg.margin_roughness,
            sheen_strength: cfg.sheen_strength,
            ao_strength: cfg.ao_strength,
            reveal_warp_amp: cfg.reveal_warp_amp,
            reveal_warp_scale: cfg.reveal_warp_scale,
        }
    }
}

/// Floor extension. Bindings start at 100 so they cannot collide with `StandardMaterial`'s own bindings in
/// the shared material bind group.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct MoldFloorExt {
    #[uniform(100)]
    params: MoldSurfaceParams,
    /// Simulation fields: `R` trail · `G` biomass · `B` wall contact · `A` coverage.
    #[texture(101)]
    #[sampler(102)]
    display: Handle<Image>,
    /// World state; the material reads `G` (light/gaze) so the mold can conceal its glow when watched.
    #[texture(103)]
    #[sampler(104)]
    control: Handle<Image>,
}

/// Wall extension — identical bindings, different fragment shader.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct MoldWallExt {
    #[uniform(100)]
    params: MoldSurfaceParams,
    #[texture(101)]
    #[sampler(102)]
    display: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    control: Handle<Image>,
}

/// Per-body state for the fruit shader. Separate from [`MoldSurfaceParams`] because every mushroom needs
/// its own copy while the surface dials are shared. `vec2` first, so the following scalar is naturally
/// aligned and the WGSL struct in `mycelia_fruit.wgsl` matches byte for byte.
#[derive(Clone, ShaderType)]
pub struct MoldFruitParams {
    /// Apex deflection of the stipe, in the body's **object space**, in native-scale metres. Fixed at spawn:
    /// thigmotropic escape from a wall the cap would otherwise pass through, plus a random lean. The vertex
    /// shader applies it over the stipe's upper 30% and lets the cap ride rigid and level on top.
    bend: Vec2,
    /// The body's growth angle, object space, as a slope. Applied linearly in `y`, so it leans the whole
    /// stem while leaving the volva seated. Fixed at spawn.
    tilt: Vec2,
    /// This body's Oklab `(a, b)` chroma offset for the cap, fixed at spawn: its cluster's shade plus its own
    /// small deviation from it (`perceptual::cap_ab_for`). Applied in Oklab so it moves hue and chroma while
    /// leaving *lightness* exactly alone — the mushroom recolours without relighting. Third `vec2` in a row,
    /// so `tint` after it is still naturally aligned.
    cap_ab: Vec2,
    /// Rate-limited maturity. `0` = the pale universal veil stretched over a fresh primordium, `1` = the
    /// mat's own deep flesh showing through an expanded pileus. Not `growth`: the albedo shift is throttled
    /// so it can never complete faster than the slow-change-blindness window (see
    /// `perceptual::MIN_APPEARANCE_RAMP_SECS`).
    tint: f32,
    /// Per-species flat part colours (linear RGB), from the `mycelia.species` table. The cap mixes
    /// `young → old` with `tint`; the stipe/volva/substrate tint the other `COLOR_0` parts. The death cap
    /// carries the values previously hard-coded in the shader, so it renders byte-identical.
    cap_young: Vec3,
    cap_old: Vec3,
    stipe: Vec3,
    volva: Vec3,
    substrate: Vec3,
    /// This species' stipe bending zone (native-scale metres). Per-species so a short mushroom bends over
    /// its own upper stipe rather than a zone that sits above its whole height. Feeds the vertex shader.
    bend_lo: f32,
    bend_hi: f32,
}

/// The fruit body. Reuses [`MoldSurfaceParams`] so a mushroom inherits the mat's palette, hyphal fibre
/// noise, margin mottle, matte felt roughness, sheen and cavity AO — retuning the mat retunes the mushroom,
/// and the two visibly read as the *same organism*.
///
/// The mesh's `COLOR_0` is a **part mask**, not artwork: `R` = cap (pileus), `G` = flesh (stipe, gills,
/// annulus), `B` = volva. Bevy's `StandardMaterial` multiplies base colour by the vertex colour when the
/// mesh carries `COLOR_0`, which would tint the cap pure red — so this shader overwrites `base_color`
/// outright and reads the mask itself. There are no textures on this asset; the parts *are* the mask.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct MoldFruitExt {
    #[uniform(100)]
    params: MoldSurfaceParams,
    #[texture(101)]
    #[sampler(102)]
    display: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    control: Handle<Image>,
    #[uniform(105)]
    fruit: MoldFruitParams,
}

impl MoldFruitExt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: &MyceliaConfig,
        display: Handle<Image>,
        control: Handle<Image>,
        tint: f32,
        bend: Vec2,
        tilt: Vec2,
        cap_ab: Vec2,
        colors: &super::species::SpeciesColors,
        bend_lo: f32,
        bend_hi: f32,
    ) -> Self {
        Self {
            params: MoldSurfaceParams::new(cfg),
            display,
            control,
            fruit: MoldFruitParams {
                bend,
                tilt,
                cap_ab,
                tint,
                cap_young: Vec3::from_array(colors.cap_young),
                cap_old: Vec3::from_array(colors.cap_old),
                stipe: Vec3::from_array(colors.stipe),
                volva: Vec3::from_array(colors.volva),
                substrate: Vec3::from_array(colors.substrate),
                bend_lo,
                bend_hi,
            },
        }
    }

    /// Publish this body's maturity to its shader. Called by `fruit::tint_fruit_bodies` when it changes.
    pub fn set_tint(&mut self, tint: f32) {
        self.fruit.tint = tint;
    }

    /// The tint currently uploaded, so the caller can skip a no-op write (which would otherwise emit an
    /// `AssetEvent::Modified` and re-upload the uniform every frame for every mature mushroom).
    pub fn tint(&self) -> f32 {
        self.fruit.tint
    }

    /// Publish this body's stem deflection to its shader. Fixed at spawn for most bodies, but a
    /// phototropic species leans toward lamp light as it grows (`fruit::bend_toward_light`), so its
    /// bend is re-uploaded when it moves.
    pub fn set_bend(&mut self, bend: Vec2) {
        self.fruit.bend = bend;
    }

    /// The bend currently uploaded, so the caller can skip a no-op re-upload.
    pub fn bend(&self) -> Vec2 {
        self.fruit.bend
    }
}

impl MaterialExtension for MoldFruitExt {
    /// The body's stem is bent on the GPU, so this overrides the **vertex** stage as well as the fragment.
    /// Both entry points live in one file: `vertex` re-applies the glTF morph (which the default mesh vertex
    /// shader would otherwise have done for us) and then curves the stipe.
    ///
    /// `prepass_vertex_shader` is deliberately **not** overridden, and that is safe here rather than
    /// merely convenient: the scene's directional light sets `shadow_maps_enabled: false` (`world.rs`) and
    /// no camera carries a `DepthPrepass`/`NormalPrepass`, so the main pass is the only pipeline that ever
    /// rasterizes this mesh. Turn shadows on and a bent mushroom would cast a straight silhouette — at which
    /// point this needs a matching prepass vertex shader, not a `NotShadowCaster`.
    fn vertex_shader() -> ShaderRef {
        "shaders/mycelia_fruit.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/mycelia_fruit.wgsl".into()
    }
}

impl MoldFloorExt {
    pub fn new(cfg: &MyceliaConfig, display: Handle<Image>, control: Handle<Image>) -> Self {
        Self { params: MoldSurfaceParams::new(cfg), display, control }
    }
}

impl MoldWallExt {
    pub fn new(cfg: &MyceliaConfig, display: Handle<Image>, control: Handle<Image>) -> Self {
        Self { params: MoldSurfaceParams::new(cfg), display, control }
    }
}

impl MaterialExtension for MoldFloorExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/mycelia_floor.wgsl".into()
    }

    /// Strip the *rasterizer* half of [`FLOOR_DEPTH_BIAS`], keeping only the sort-order half.
    ///
    /// `StandardMaterial::depth_bias` drives two unrelated things, and we want exactly one of them: the
    /// transparent sort distance (yes) and `depth_stencil.bias.constant` (no). At the magnitude the sort
    /// needs, that constant would bias every fragment's depth far enough back that the opaque carpet
    /// 0.02 below would reject the overlay outright — the mold would simply stop drawing.
    ///
    /// `MaterialExtension::specialize` is documented to run *after* the base material's, so the
    /// `bias.constant` `StandardMaterial` just wrote is still ours to clear. The overlay is a flat plane
    /// with nothing coplanar to z-fight against, so zero is the value it always wanted.
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth_stencil) = descriptor.depth_stencil.as_mut() {
            depth_stencil.bias.constant = 0;
        }
        Ok(())
    }
}

impl MaterialExtension for MoldWallExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/mycelia_wall.wgsl".into()
    }
}

/// Sort offset that pins the floor overlay behind every other transparent mesh.
///
/// The overlay is `AlphaMode::Blend`, so it never writes depth: Bevy resolves it against other
/// transparent meshes purely by a painter's-algorithm sort on *one* key per mesh — the AABB centre,
/// projected onto the camera axis (`Transparent3d::sort_distance` = `rangefinder.distance(centre) +
/// depth_bias`; values increase toward the camera and are sorted ascending, i.e. back to front). The
/// overlay is a single `WORLD_EXTENT`-wide quad whose centre sits at the middle of the map, so any
/// transparent object standing *farther* from the camera than that midpoint — the Smiley's blended
/// billboard, a dialogue balloon — sorted behind the overlay and was painted over by the mold.
///
/// A negative bias lowers the overlay's sort distance, i.e. pushes it away from the camera
/// (`StandardMaterial::depth_bias`: "negative values cause the material to render behind other objects").
/// The magnitude has to exceed the largest sort-distance gap the overlay's centre can have with any other
/// mesh. That projection is 1-Lipschitz — `|d(a) - d(b)| <= |a - b|` — so the world's diagonal
/// (`|WORLD_EXTENT| ≈ 271`) bounds it, and the Manhattan sum below clears that bound with room to spare.
///
/// This is correct rather than merely convenient: the overlay lies on the ground at `Y = 0.02`, and every
/// transparent thing in this game — faces, balloons, health bars — is *above* the ground. There is no
/// camera angle from which the mold film should occlude one, so "always draw the overlay first" is the
/// order the scene actually has.
///
/// **The field is overloaded.** Bevy also casts `depth_bias` to `i32` and writes it into
/// `depth_stencil.bias.constant` (`pbr_material.rs`), a genuine per-fragment rasterizer bias. At this
/// magnitude that would shift the overlay's *tested* depth behind the carpet it floats above and cull it.
/// [`MoldFloorExt::specialize`] zeroes that half back out; only the sort offset survives.
const FLOOR_DEPTH_BIAS: f32 = -(WORLD_EXTENT.x + WORLD_EXTENT.y);

/// The `StandardMaterial` under the floor overlay: translucent (the carpet shows through where there is no
/// mold) and non-metallic. The extension fragment supplies base colour, normal, roughness and emissive.
pub fn floor_base() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        alpha_mode: AlphaMode::Blend,
        depth_bias: FLOOR_DEPTH_BIAS,
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overlay quad's centre — kept in step with the spawn in `mycelia::spawn_floor_overlay`.
    fn overlay_centre() -> Vec3 {
        Vec3::new(
            WORLD_ORIGIN.x + WORLD_EXTENT.x * 0.5,
            0.02,
            WORLD_ORIGIN.y + WORLD_EXTENT.y * 0.5,
        )
    }

    /// Bevy's transparent sort key is `rangefinder.distance(aabb_centre) + depth_bias`, and
    /// `ViewRangefinder3d::distance` is a row of the view matrix dotted with the point — an affine map
    /// whose linear part is a unit vector. Any unit `axis` therefore models it exactly (the constant
    /// term is shared by both meshes and cancels in the comparison).
    fn sort_key(p: Vec3, axis: Vec3, depth_bias: f32) -> f32 {
        axis.dot(p) + depth_bias
    }

    /// The invariant the Smiley-occlusion fix rests on: the mold overlay must sort strictly *before*
    /// (behind) every other transparent mesh, from every camera angle. Bevy sorts `Transparent3d`
    /// ascending, and values increase toward the camera — so "behind" means a strictly smaller key.
    ///
    /// `sort_key` is affine in `p`, so over the convex world AABB its minimum is attained at a corner.
    /// Checking the eight corners is therefore exhaustive, not a sample.
    #[test]
    fn floor_overlay_sorts_behind_every_transparent_mesh_in_the_world() {
        let centre = overlay_centre();
        let (x0, z0) = (WORLD_ORIGIN.x, WORLD_ORIGIN.y);
        let (x1, z1) = (x0 + WORLD_EXTENT.x, z0 + WORLD_EXTENT.y);
        // Ground level, and the highest a transparent mesh rides (a dialogue balloon sits ~2.5 up).
        let corners = [x0, x1]
            .into_iter()
            .flat_map(|x| [z0, z1].map(move |z| (x, z)))
            .flat_map(|(x, z)| [0.0_f32, 4.0].map(move |y| Vec3::new(x, y, z)));

        // A spread of view directions, including straight-down and the game's isometric key angle.
        let axes = [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::NEG_X,
            Vec3::NEG_Y,
            Vec3::NEG_Z,
            Vec3::new(1.0, -1.0, 1.0).normalize(),
            Vec3::new(-1.0, -1.0, -1.0).normalize(),
            Vec3::new(1.0, -1.0, -1.0).normalize(),
        ];

        for axis in axes {
            let overlay = sort_key(centre, axis, FLOOR_DEPTH_BIAS);
            for p in corners.clone() {
                let other = sort_key(p, axis, 0.0);
                assert!(
                    overlay < other,
                    "mold overlay ({overlay}) would draw over a transparent mesh at {p} \
                     ({other}) for view axis {axis} — the Smiley-occlusion bug",
                );
            }
        }
    }

    /// Guards the bias magnitude against someone growing the world: the offset must beat the largest
    /// gap the overlay's centre can have with any point in it (bounded by the half-diagonal, since the
    /// projection is 1-Lipschitz).
    #[test]
    fn depth_bias_magnitude_covers_the_world_half_diagonal() {
        let half_diagonal = (WORLD_EXTENT * 0.5).length();
        assert!(
            FLOOR_DEPTH_BIAS < -half_diagonal,
            "bias {FLOOR_DEPTH_BIAS} no longer clears the half-diagonal {half_diagonal}",
        );
    }
}
