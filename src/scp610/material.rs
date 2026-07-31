//! **SCP-610's two materials** — the flesh↔scar blend and the deliberately mismatched eye.
//!
//! The design argument lives in the shaders (`assets/shaders/scp610_flesh.wgsl`,
//! `assets/shaders/scp610_eye.wgsl`); this module is the Bevy side of it. Both are
//! [`ExtendedMaterial`]s over the glTF's own `StandardMaterial`, following
//! [`crate::mycelia::MoldFruitMaterial`] — the codebase's existing answer to "a per-vertex value has
//! to reach a shader without Bevy's vertex-colour handling corrupting it".
//!
//! # Why there are two, rather than one with a flag
//!
//! The body and the eye are not two configurations of one material; they are two *jobs*. The body
//! cross-fades two photographed PBR sets by the mask. The eye must ignore the mask entirely and stay
//! flat, because a flat feature on a photoreal body is the uncanny-valley lever the asset already
//! has — Kätsyri et al. 2015's H4a, supported by 4 of 4 studies it reviewed
//! (10.3389/fpsyg.2015.00390). A `is_eye: u32` branch inside one shader would be one path pretending
//! to be two; two materials with two purposes is the honest shape.
//!
//! # Why the swap is needed at all
//!
//! The glTF ships both slots as plain `StandardMaterial`s and the mesh now carries `COLOR_0`. Bevy's
//! `pbr_fragment.wgsl` **assigns** `base_color = in.color` when a mesh has vertex colours, so left
//! alone the body renders red-and-black and the eye renders pure black (the mask is 0.0 across all 82
//! of its verts). Neither slot can be left on the stock material — see `assets/scp610/README.md` §6.

use std::collections::HashMap;

use bevy::asset::{Assets, Handle};
use bevy::gltf::GltfMaterialName;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use super::{Scp610, Scp610Mutation};
use crate::containment::{Containment, Phase};

/// The infected body: two gore sets cross-faded by `COLOR_0.r`.
pub type Scp610FleshMaterial = ExtendedMaterial<StandardMaterial, Scp610FleshExt>;
/// The eye: flat, unblended, and mismatched on purpose.
pub type Scp610EyeMaterial = ExtendedMaterial<StandardMaterial, Scp610EyeExt>;

/// glTF material names, from the builder (`monsters/infected.py::_build_mesh` appends them in this
/// order). Matched against [`GltfMaterialName`] rather than guessed from primitive order, because
/// primitive order is an exporter detail and the name is authored.
const BODY_MATERIAL: &str = "scp610_mesh_body";
const EYE_MATERIAL: &str = "scp610_mesh_eye";

/// Scar-set tiling relative to the body's own UVs. The two gore sets were photographed at different
/// native densities; the Blender-side preview material makes the same split.
const SCAR_UV_SCALE: f32 = 2.0;
/// How far the scar normal map may pull the shading normal. Below 1.0 because the flesh normal map is
/// describing the same surface — the scar is a stage of it, not a different object.
const SCAR_NORMAL_STRENGTH: f32 = 0.8;

/// The eye's flat colour, linear RGB. Near-black with a cold cast: it must read as *not tissue*
/// against a body made of photographed gore. Deliberately hueless-adjacent — the colour doc reserves
/// saturation for anomaly signalling, not for creature paint.
const EYE_COLOR: Vec3 = Vec3::new(0.02, 0.023, 0.03);

/// ACS Disruption while an anomaly is loose. Vlam — a candle: present, not yet spreading.
const DISRUPTION_LOOSE: f32 = 0.35;
/// Disruption the instant a cordon breaks. Keneq — the fire got bigger, and the player must notice
/// without reading the HUD.
const DISRUPTION_BREACH: f32 = 1.0;
/// Seconds a breach flare takes to fall back to [`DISRUPTION_LOOSE`].
const BREACH_DECAY_SECS: f32 = 2.5;

/// MUST byte-match `Scp610FleshParams` in `assets/shaders/scp610_flesh.wgsl`.
#[derive(Clone, Copy, ShaderType)]
pub struct Scp610FleshParams {
    scar_uv_scale: f32,
    disruption: f32,
    normal_strength: f32,
    mutation: f32,
}

/// MUST byte-match `Scp610EyeParams` in `assets/shaders/scp610_eye.wgsl`. `Vec3` first: it aligns to
/// 16 bytes, so the scalar after it lands in the padding that alignment would have wasted anyway.
#[derive(Clone, Copy, ShaderType)]
pub struct Scp610EyeParams {
    color: Vec3,
    disruption: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct Scp610FleshExt {
    #[uniform(100)]
    params: Scp610FleshParams,
    #[texture(101)]
    #[sampler(102)]
    scar_color: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    scar_roughness: Handle<Image>,
    #[texture(105)]
    #[sampler(106)]
    scar_normal: Handle<Image>,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct Scp610EyeExt {
    #[uniform(100)]
    params: Scp610EyeParams,
}

impl MaterialExtension for Scp610FleshExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/scp610_flesh.wgsl".into()
    }
}

impl MaterialExtension for Scp610EyeExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/scp610_eye.wgsl".into()
    }
}

/// The loose scar/guts texture set, loaded once at `Startup`.
///
/// Loose rather than embedded because glTF has exactly one base-colour texture per primitive — there
/// is no format-level way to ship a second set for a shader to blend against. Same split the dungeon
/// floor/wall textures already use.
#[derive(Resource)]
pub struct Scp610Textures {
    color: Handle<Image>,
    roughness: Handle<Image>,
    normal: Handle<Image>,
}

/// Per-bloom handles to the two minted materials, so the per-frame uniform push has somewhere to
/// write without re-walking the scene graph.
#[derive(Component)]
pub struct Scp610Materials {
    flesh: Handle<Scp610FleshMaterial>,
    eye: Handle<Scp610EyeMaterial>,
}

/// Marks a primitive whose material has already been swapped, so the pass is once-per-entity.
#[derive(Component)]
pub struct Scp610Coated;

/// The bloom's live ACS Disruption, in its own component so [`drive_disruption`] can hold a breach
/// flare across frames without a side table.
#[derive(Component)]
pub struct Scp610Disruption {
    /// What the shader is showing.
    pub current: f32,
    /// Seconds left on a breach flare.
    flare_secs: f32,
    /// Last phase seen, to detect the `BeingContained → Uncontained` edge that IS the breach.
    was_containing: bool,
}

impl Default for Scp610Disruption {
    fn default() -> Self {
        Self { current: DISRUPTION_LOOSE, flare_secs: 0.0, was_containing: false }
    }
}

pub(super) fn load_textures(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Scp610Textures {
        color: assets.load("scp610/textures/scar_color.jpg"),
        roughness: assets.load("scp610/textures/scar_roughness.jpg"),
        normal: assets.load("scp610/textures/scar_normal.png"),
    });
}

/// Swap both glTF slots for the real materials, once each, as the scene's primitives arrive.
///
/// The `minted` map is load-bearing rather than an optimisation, and `mycelia::fruit::coat_fruit_bodies`
/// documents why: `commands.insert` is deferred to the end of the schedule, so within one frame the
/// `Option<&Scp610Materials>` lookup still reads `None` for every later descendant. 610 has **two**
/// primitives, so minting per descendant would leave the second material untracked and
/// [`drive_disruption`] would then update only one of them.
pub(super) fn coat_blooms(
    mut commands: Commands,
    textures: Option<Res<Scp610Textures>>,
    blooms: Query<(Entity, Option<&Scp610Materials>), With<Scp610>>,
    children: Query<&Children>,
    painted: Query<(&MeshMaterial3d<StandardMaterial>, &GltfMaterialName), Without<Scp610Coated>>,
    std_materials: Res<Assets<StandardMaterial>>,
    mut flesh_materials: ResMut<Assets<Scp610FleshMaterial>>,
    mut eye_materials: ResMut<Assets<Scp610EyeMaterial>>,
) {
    // The scar set is loaded at `Startup`; if the resource is not up yet there is nothing to coat
    // with, and the scene is still streaming anyway.
    let Some(textures) = textures else { return };

    let mut minted: HashMap<Entity, (Handle<Scp610FleshMaterial>, Handle<Scp610EyeMaterial>)> =
        HashMap::new();

    for (root, existing) in &blooms {
        for descendant in children.iter_descendants(root) {
            let Ok((mat, name)) = painted.get(descendant) else { continue };
            // The glTF material may not have finished loading; try again next frame.
            let Some(base) = std_materials.get(&mat.0) else { continue };

            let pair = match existing
                .map(|m| (m.flesh.clone(), m.eye.clone()))
                .or_else(|| minted.get(&root).cloned())
            {
                Some(pair) => pair,
                None => {
                    let flesh = flesh_materials.add(Scp610FleshMaterial {
                        base: base.clone(),
                        extension: Scp610FleshExt {
                            params: Scp610FleshParams {
                                scar_uv_scale: SCAR_UV_SCALE,
                                disruption: DISRUPTION_LOOSE,
                                normal_strength: SCAR_NORMAL_STRENGTH,
                                mutation: 0.0,
                            },
                            scar_color: textures.color.clone(),
                            scar_roughness: textures.roughness.clone(),
                            scar_normal: textures.normal.clone(),
                        },
                    });
                    let eye = eye_materials.add(Scp610EyeMaterial {
                        base: base.clone(),
                        extension: Scp610EyeExt {
                            params: Scp610EyeParams {
                                color: EYE_COLOR,
                                disruption: DISRUPTION_LOOSE,
                            },
                        },
                    });
                    let pair = (flesh, eye);
                    commands
                        .entity(root)
                        .insert(Scp610Materials { flesh: pair.0.clone(), eye: pair.1.clone() });
                    minted.insert(root, pair.clone());
                    pair
                }
            };

            // Matched on the AUTHORED material name, not primitive order. An unrecognised slot is
            // reported and left alone rather than guessed at: a third slot would mean the asset
            // gained a part this module has not been told how to shade, and silently painting it as
            // flesh would hide that.
            let mut entity = commands.entity(descendant);
            match name.0.as_str() {
                BODY_MATERIAL => {
                    entity
                        .remove::<MeshMaterial3d<StandardMaterial>>()
                        .insert((MeshMaterial3d(pair.0), Scp610Coated));
                }
                EYE_MATERIAL => {
                    entity
                        .remove::<MeshMaterial3d<StandardMaterial>>()
                        .insert((MeshMaterial3d(pair.1), Scp610Coated));
                }
                other => {
                    warn_once!(
                        "scp610: unknown glTF material slot {other:?} — left on the stock \
                         StandardMaterial, which will render it black because the mesh carries a \
                         COLOR_0 mask. Teach `scp610::material` about it."
                    );
                    entity.insert(Scp610Coated);
                }
            }
        }
    }
}

/// Advance each bloom's ACS Disruption and publish it, with the mutation weight, into both uniforms.
///
/// **Containment is darkness** (`docs/lore/2026-07-12-scp-color-language.md` §1): an anomaly under an
/// active cordon dims as the hold completes, a contained one is dark, and a *breach* — the
/// `BeingContained → Uncontained` edge — flares. That edge is the one containment event with no
/// surface at all today: the HUD panel simply vanishes, because it only renders `BeingContained`.
pub(super) fn drive_disruption(
    time: Res<Time>,
    mut blooms: Query<(
        &mut Scp610Disruption,
        &Containment,
        &Scp610Mutation,
        &Scp610Materials,
    )>,
    mut flesh_materials: ResMut<Assets<Scp610FleshMaterial>>,
    mut eye_materials: ResMut<Assets<Scp610EyeMaterial>>,
) {
    let dt = time.delta_secs();
    for (mut disruption, containment, mutation, handles) in &mut blooms {
        let phase = containment.phase();
        let containing = phase == Phase::BeingContained;

        // The breach edge. `tick_quarantine` cancels the attempt when the bloom leaves the cordon,
        // which lands here as BeingContained → Uncontained.
        if disruption.was_containing && phase == Phase::Uncontained {
            disruption.flare_secs = BREACH_DECAY_SECS;
        }
        disruption.was_containing = containing;

        let settled = match phase {
            // Darkness is containment: fade out over the hold rather than snapping at completion, so
            // the player can see the capture working on the creature itself.
            Phase::BeingContained => DISRUPTION_LOOSE * (1.0 - containment.progress()),
            Phase::Contained => 0.0,
            Phase::Uncontained => DISRUPTION_LOOSE,
        };

        let target = if disruption.flare_secs > 0.0 {
            disruption.flare_secs = (disruption.flare_secs - dt).max(0.0);
            let t = disruption.flare_secs / BREACH_DECAY_SECS;
            settled.max(DISRUPTION_BREACH * t)
        } else {
            settled
        };
        disruption.current = target;

        // `Assets::get_mut` emits `AssetEvent::Modified` and re-uploads the uniform, so skip a
        // write that would change nothing — a contained bloom and a fully-turned one both settle,
        // and there may be three of them on screen.
        let flesh_synced = flesh_materials.get(&handles.flesh).is_some_and(|m| {
            (m.extension.params.disruption - target).abs() < 1e-4
                && (m.extension.params.mutation - mutation.current).abs() < 1e-4
        });
        if !flesh_synced && let Some(mut m) = flesh_materials.get_mut(&handles.flesh) {
            m.extension.params.disruption = target;
            m.extension.params.mutation = mutation.current;
        }

        let eye_synced = eye_materials
            .get(&handles.eye)
            .is_some_and(|m| (m.extension.params.disruption - target).abs() < 1e-4);
        if !eye_synced && let Some(mut m) = eye_materials.get_mut(&handles.eye) {
            m.extension.params.disruption = target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The luminosity ladder must be monotone in "how contained is it" — that IS the readout.
    /// A player learns "darker = winning" only if it is never violated.
    #[test]
    fn containment_is_darkness() {
        assert!(
            DISRUPTION_BREACH > DISRUPTION_LOOSE,
            "a breach must be brighter than an anomaly merely being loose"
        );
        // The BeingContained ramp is `DISRUPTION_LOOSE * (1 - progress)`, so it starts at loose and
        // reaches zero — never brighter than loose, never darker than contained.
        for step in 0..=10 {
            let progress = step as f32 / 10.0;
            let value = DISRUPTION_LOOSE * (1.0 - progress);
            assert!(
                (0.0..=DISRUPTION_LOOSE).contains(&value),
                "containment ramp escaped [contained, loose] at progress {progress}: {value}"
            );
        }
    }

    /// The emissive is hueless by rule — `docs/lore/2026-07-12-scp-color-language.md` §7 ("colour
    /// means deviation, never danger") and the standing instruction that light added to this scene
    /// carries no hue of its own, or it fights the dungeon's authored palette.
    ///
    /// The flesh shader multiplies `vec3(1.0)`, which cannot carry hue by construction. The eye
    /// multiplies its own colour, so that colour is what this pins.
    #[test]
    fn the_eye_ink_is_near_neutral() {
        let max = EYE_COLOR.x.max(EYE_COLOR.y).max(EYE_COLOR.z);
        let min = EYE_COLOR.x.min(EYE_COLOR.y).min(EYE_COLOR.z);
        assert!(
            max - min <= 0.02,
            "the eye emissive would tint the scene: spread {} across {EYE_COLOR:?}",
            max - min
        );
    }
}
