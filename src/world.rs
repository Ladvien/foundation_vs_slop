//! Environment lighting for the dungeon. No ground plane — the WFC dungeon lays down its own floor
//! tiles. Fog still hides *unexplored* tiles entirely (black void), which is the eerie part — see `fog`.
//!
//! # The ambient path is an environment map, not a flat fill
//!
//! This used to be a single `GlobalAmbientLight` at brightness 500, then 200. That is a *uniform* term:
//! Bevy adds it to every surface identically, regardless of which way the surface faces. So it could not
//! shade anything — a wall, a floor and an operative's shoulder all received the same fill, and the
//! result read as untextured clay. `mycelia_floor.wgsl` had already resorted to hand-authoring a cavity-AO
//! term to claw some of that back.
//!
//! The fill is now an **irradiance environment map** ([`GeneratedEnvironmentMapLight`], filtered on the
//! GPU into a 32² diffuse cubemap plus a GGX specular chain), built here as a vertical gradient: warm
//! low-CRI fluorescent above, dark bounce below. Because the lookup depends on the surface normal, an
//! upward-facing surface now catches the ceiling and a downward-facing one does not — which is the entire
//! source of "this object has form".
//!
//! There is **no HDRI anywhere in the asset library** (verified across `/mnt/codex_fs/game_assets`), and
//! Bevy's `hdr` image-loader feature is off. That is not a problem worth solving with a download:
//! Ramamoorthi & Hanrahan, "An efficient representation for irradiance environment maps", SIGGRAPH 2001
//! (`10.1145/383259.383317`) show that the irradiance from *any* environment is captured almost entirely
//! by a low-order spherical-harmonic expansion — 9 coefficients to within ~1% for diffuse surfaces. A
//! smooth 64² gradient therefore carries essentially the same ambient signal a full-resolution HDRI
//! would, at 192 KB built in-process with no asset dependency.
//!
//! [`LightingConfig::ambient_brightness`] survives only as a small black-crush floor. It is not a second
//! ambient path kept "just in case" — run both at strength and the uniform term re-flattens everything
//! the environment map just shaped.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};

use crate::config::GameConfig;
use crate::light::LightingConfig;

/// Edge length of the generated environment cubemap, in texels. Must be a power of two — Bevy's
/// realtime filter panics otherwise, which the `const` assertion below makes a build error instead.
///
/// 64 is not a compromise: per Ramamoorthi & Hanrahan (module docs) the diffuse irradiance signal is
/// band-limited to a handful of spherical-harmonic coefficients, and Bevy convolves this down to a 32²
/// diffuse cubemap regardless. A larger source would only sharpen specular reflections, and every
/// surface in this game is `perceptual_roughness` 0.95-ish concrete, carpet and wallpaper.
const ENV_CUBEMAP_SIZE: u32 = 64;

/// Compile-time, not `debug_assert!`. Bevy's realtime filter *panics* on a non-power-of-two source
/// cubemap, and a `debug_assert` is compiled out of exactly the release build where that panic would
/// reach a player. The value is a `const`, so there is no reason to check it at runtime at all.
const _: () = assert!(
    ENV_CUBEMAP_SIZE.is_power_of_two(),
    "ENV_CUBEMAP_SIZE must be a power of two — bevy_pbr's environment filter rejects anything else"
);

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        // Pull the environment-fill values from the one `lighting:` config slice (shared with
        // `light::LightingPlugin`, which owns the fixtures) so there is a single source of truth for
        // every light knob — one path, no hardcoded second copy. `ConfigPlugin` runs first, so
        // `GameConfig` exists here at build time (same seam every consumer plugin uses).
        let cfg = app.world().resource::<GameConfig>().lighting.clone();
        app.insert_resource(GlobalAmbientLight {
            // Residual only — the fill itself is the environment map (see module docs).
            color: Color::srgb(cfg.ambient_color[0], cfg.ambient_color[1], cfg.ambient_color[2]),
            brightness: cfg.ambient_brightness,
            ..default()
        })
        .add_systems(Startup, setup_lighting);
    }
}

/// Encode a finite, non-negative `f32` as IEEE-754 binary16 bits.
///
/// The environment cubemap is `Rgba16Float` because that is what Bevy's filter chain reads (its copy
/// pass binds the source as a filterable `texture_2d_array<f32>` and writes `rgba16float`). Rust has no
/// stable `f16`, and pulling in a dependency to write 24 KB of gradient would be the larger cost — so
/// the conversion is here, truncating rather than round-to-nearest, which is inconsequential for a
/// smooth ramp.
fn f16_bits(v: f32) -> u16 {
    /// Largest finite binary16 value.
    const F16_MAX: f32 = 65504.0;
    /// Smallest positive binary16 subnormal, 2⁻²⁴. Below this the encoding is zero anyway, and bounding
    /// here is what keeps the subnormal shift below its overflow point.
    const F16_MIN_SUBNORMAL: f32 = 5.960_464_5e-8;

    // Non-finite input would otherwise encode as a garbage exponent; the callers clamp too, but this is
    // the function's own contract and it fails to zero rather than to a NaN texel.
    let v = if v.is_finite() { v.clamp(0.0, F16_MAX) } else { 0.0 };
    if v < F16_MIN_SUBNORMAL {
        return 0;
    }
    let bits = v.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32 - 127;
    let mantissa = bits & 0x007f_ffff;
    if exp < -14 {
        // Subnormal half: restore the implicit leading 1 and denormalise. `v >= 2⁻²⁴` bounds `exp >= -24`,
        // so `shift <= 10` and `shift + 13 <= 23` — the shift can never reach the width of the operand.
        let shift = (-14 - exp) as u32;
        return ((mantissa | 0x0080_0000) >> (shift + 13)) as u16;
    }
    ((((exp + 15) as u32) << 10) | (mantissa >> 13)) as u16
}

/// Direction of the texel at normalised face coordinates `(u, v) ∈ [-1, 1]²` on cubemap `face`.
///
/// Face order is the wgpu/Vulkan cubemap layer convention: +X, −X, +Y, −Y, +Z, −Z. Only the `y`
/// component actually matters for a vertical gradient, but the full vector is built so the mapping is
/// checkable against the convention rather than being a `y`-only shortcut nobody can verify.
fn cubemap_direction(face: u32, u: f32, v: f32) -> Vec3 {
    let d = match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        // The caller iterates `0..6`; this arm is the sixth face, not a fallback for a bad index.
        _ => Vec3::new(-u, -v, -1.0),
    };
    d.normalize_or_zero()
}

/// Build the interior environment cubemap: a vertical gradient from `env_ground_color` at −Y to
/// `env_sky_color` at +Y.
///
/// Near-monochrome and slightly warm, per `docs/lore/2026-07-12-scp-color-language.md` §6 ("Desaturation
/// = reality. Saturation = anomaly") and the `docs/ui.md` §1.3 restatement — the ambient light is
/// Foundation infrastructure, so it carries no hue of its own. What it carries is a *value* difference
/// between up and down, and that difference is the whole shading signal: collapse the two colours toward
/// each other and the clay look returns even with the environment map still installed.
///
/// `pub` because the component it feeds belongs on the camera entity (Bevy's requirement), which
/// `crate::camera` owns — this module owns what the light *is*, `camera` owns what rides the camera.
pub fn interior_env_cubemap(cfg: &LightingConfig) -> Image {
    let size = ENV_CUBEMAP_SIZE;
    let sky = Vec3::from_array(cfg.env_sky_color);
    let ground = Vec3::from_array(cfg.env_ground_color);

    // 6 faces × size² texels × 4 channels × 2 bytes.
    let mut data: Vec<u8> = Vec::with_capacity((6 * size * size * 8) as usize);
    for face in 0..6u32 {
        for y in 0..size {
            for x in 0..size {
                // Texel centres, mapped to [-1, 1].
                let u = 2.0 * (x as f32 + 0.5) / size as f32 - 1.0;
                let v = 2.0 * (y as f32 + 0.5) / size as f32 - 1.0;
                let dir = cubemap_direction(face, u, v);
                // Hemispherical blend on the vertical axis. `smoothstep` rather than a raw lerp so the
                // horizon is a soft band instead of a hard equator, which otherwise shows up as a seam in
                // the specular chain on the few glossy surfaces (almond-water puddles, TV glass).
                let t = (dir.y * 0.5 + 0.5).clamp(0.0, 1.0);
                let t = t * t * (3.0 - 2.0 * t);
                let c = ground.lerp(sky, t);
                for ch in [c.x, c.y, c.z, 1.0] {
                    data.extend_from_slice(&f16_bits(ch).to_le_bytes());
                }
            }
        }
    }

    Image {
        texture_view_descriptor: Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..default()
        }),
        ..Image::new(
            Extent3d { width: size, height: size, depth_or_array_layers: 6 },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba16Float,
            // Render-world only: nothing on the CPU ever reads this back, and keeping a main-world copy
            // would just hold 192 KB for no reader.
            RenderAssetUsages::RENDER_WORLD,
        )
    }
}

fn setup_lighting(mut commands: Commands, config: Res<GameConfig>) {
    let cfg = &config.lighting;
    // The key light, and the game's ONLY shadow caster among the environment lights.
    //
    // That is a measured choice, not a perf compromise that happens to be cheap. Hubona, Wheeler, Shirah
    // & Brandt, "The relative contributions of stereo, lighting, and background scenes in promoting 3D
    // depth visualization", ACM ToCHI 6(3), 1999 (`10.1145/329693.329695`) found cast shadows improve
    // object-*positioning* accuracy — the exact judgement this game asks of a player choosing where to
    // send a squad — but also that "task performances degrade [...] when the number of shadowing light
    // sources increases from one to two." So the fixtures stay shadowless (`light::spawn_fixture_lights`)
    // and this one casts. It is also the cheap choice, which is a happy coincidence and not the argument.
    //
    // # What must NOT cast into it — `NotShadowCaster`, ~22 sites
    //
    // Turning this on made **every** rendered mesh a shadow caster, and roughly a fifth of what this
    // game draws is not geometry at all: camera-facing billboards (health bars, speech bubbles, AI
    // labels, the smiley's face and true form, SCP-999's eyes, blood spray, impact sparks), flat
    // decals (blood pools and wall splatter, the chestburster wound), unlit floor overlays (the psi
    // wash, the mycelia mat, almond-water puddles), emissive projectiles (laser bolts, the boss's
    // lightning) and the hair rig's alpha-masked ribbons.
    //
    // Left alone, each of those casts a shadow of a shape that does not exist in the world: a health
    // bar throws a rectangle on the floor, a floor overlay shadows the floor it is lying on, and a
    // camera-facing quad throws a shadow that *swings as the camera rotates*. Do not assume alpha
    // sorts this out — Bevy 0.19's shadow queue does not skip blended materials, it only tags them
    // `MeshPipelineKey::MAY_DISCARD` (`bevy_pbr/src/render/light.rs`), so they still reach the shadow
    // map. Every such spawn site carries `NotShadowCaster` and a comment pointing here.
    //
    // Real geometry keeps casting: architecture, furniture, characters, mushrooms, gibs, the crab
    // nest dome, and the gestation lump (a genuine bulge on a host, not a billboard).
    //
    // Selection rings need no marker — they are `Gizmos` (`selection::draw_selection_rings`), which
    // never enter the shadow pass. Neither does `bevy_ui`; only *worldspace* UI was ever at risk.
    commands.spawn((
        DirectionalLight {
            illuminance: cfg.key_illuminance,
            shadow_maps_enabled: true,
            ..default()
        },
        // Bevy's default cascade split is sized for outdoor draw distances; at this game's interior scale
        // it would spend nearly the whole shadow map on space no dungeon occupies. Both bounds are
        // validated in `light::validate_config`, so an inverted range is a startup error, not a silently
        // empty shadow map.
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: cfg.shadow_first_cascade,
            maximum_distance: cfg.shadow_max_distance,
            ..default()
        }
        .build(),
        Transform::from_xyz(6.0, 14.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Saturation in the *reality* layer must come from materials, never from the light.
    ///
    /// This is the world-lighting counterpart to `ui::theme::the_foundation_has_no_house_palette`, and
    /// it exists because the first version of this relight failed it. Reusing the old warm ambient
    /// colour `(1.0, 0.98, 0.9)` for the environment gradient multiplied an already-yellow wallpaper
    /// (`backrooms-wall-diffuse` averages srgb 0.59/0.58/0.36) and the whole scene came back olive —
    /// exactly what `docs/ui.md` §1.3 and `docs/lore/2026-07-12-scp-color-language.md` §6 reserve for
    /// anomalies. The textures are authored warm on purpose and are not in scope here; the *light* is,
    /// and it gets no hue of its own.
    ///
    /// Same chroma metric and ceiling as the UI test, so the two cannot drift apart.
    #[test]
    fn the_environment_light_carries_no_hue_of_its_own() {
        let cfg = crate::config::load_game_config().expect("shipped game config must load");
        let l = &cfg.lighting;
        for (name, c) in [
            ("env_sky_color", l.env_sky_color),
            ("env_ground_color", l.env_ground_color),
            ("ambient_color", l.ambient_color),
        ] {
            let (hi, lo) = (
                c[0].max(c[1]).max(c[2]),
                c[0].min(c[1]).min(c[2]),
            );
            let chroma = hi - lo;
            assert!(
                chroma <= crate::ui::theme::MAX_UI_CHROMA,
                "lighting.{name} has chroma {chroma:.3} — the environment light must stay near-neutral \
                 (max {}); reality's warmth belongs to the materials, and doubling it in the light is \
                 what drove the scene olive",
                crate::ui::theme::MAX_UI_CHROMA
            );
        }
    }
}
