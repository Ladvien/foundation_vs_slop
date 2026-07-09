//! The floor compositing material — samples the GPU-written mold field by **world XZ** and blends it
//! over the floor. In Phase A this is a standalone `Material` overlay (de-risking the compute→texture→
//! material path); later phases graduate it to `ExtendedMaterial<StandardMaterial, _>` so the mold layer
//! sits on the real PBR carpet.

use bevy::asset::Asset;
use bevy::pbr::Material;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use super::{WORLD_EXTENT, WORLD_ORIGIN};

/// GPU uniform — must byte-match `MoldMatParams` in `mycelia_floor.wgsl`. Carries the same world↔UV
/// mapping the compute pass uses, so the floor reads exactly where the sim wrote.
#[derive(Clone, ShaderType)]
pub struct MoldMatParams {
    world_origin: Vec2,
    world_extent: Vec2,
}

/// Samples the shared `mold_display` texture (written by the mycelia compute pass) and composites its
/// grimy bioluminescence over the floor. Bindings deliberately start at 0 (this is a standalone
/// `Material`, its own bind group) — the `ExtendedMaterial` variant in a later phase will move these to
/// high indices to avoid clashing with `StandardMaterial`'s bindings.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct MoldFloorMaterial {
    #[uniform(0)]
    params: MoldMatParams,
    /// The compute-written mold field. `Rgba16Float` is filterable, so a linear sampler is valid.
    #[texture(1)]
    #[sampler(2)]
    display: Handle<Image>,
}

impl MoldFloorMaterial {
    /// Build the material bound to the shared display texture, seeding the world↔UV mapping from the
    /// module constants (single source of truth with the compute pass).
    pub fn new(display: Handle<Image>) -> Self {
        Self {
            params: MoldMatParams { world_origin: WORLD_ORIGIN, world_extent: WORLD_EXTENT },
            display,
        }
    }
}

impl Material for MoldFloorMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/mycelia_floor.wgsl".into()
    }

    /// Translucent overlay: the mold composites over the underlying floor, transparent where there is no
    /// biomass. `Blend` (not `Opaque`) so bare carpet shows through.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}
