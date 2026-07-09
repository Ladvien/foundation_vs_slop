//! Field-texture construction helpers shared by the mold's GPU resources.
//!
//! Every field texture is square, `Rgba16Float`, and usable as **both** a compute storage-write target and
//! a sampled/loaded texture (the trail ping-pong alternates between the two roles each frame), so they all
//! carry `STORAGE_BINDING | TEXTURE_BINDING | COPY_DST`. They are created **zero-filled** (not
//! `new_uninit`) so the very first simulation tick reads a clean field instead of undefined GPU memory.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureUsages};

use super::{CONTROL_FORMAT, DISPLAY_FORMAT};

/// Bytes per texel of [`DISPLAY_FORMAT`] (`Rgba16Float` = 4 channels × 2 bytes). Used to size the
/// zero-fill backing so the CPU upload matches the texture exactly.
const BYTES_PER_TEXEL: usize = 8;

/// Build one zero-filled square field texture at `size`×`size`, wired for compute storage writes **and**
/// sampled reads. Used for the trail ping-pong pair and the composited display texture.
pub(super) fn field_texture(size: u32) -> Image {
    let extent = Extent3d { width: size, height: size, depth_or_array_layers: 1 };
    let bytes = (size as usize) * (size as usize) * BYTES_PER_TEXEL;
    let mut image = Image::new(
        extent,
        TextureDimension::D2,
        vec![0u8; bytes],
        DISPLAY_FORMAT,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::COPY_DST | TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING;
    image
}

/// Build the CPU-written control texture (one texel per dungeon cell). Unlike the field textures this one
/// is *not* storage-written — it is rewritten from the main world each `Update` and only sampled by the
/// compute chain, so it needs `MAIN_WORLD` asset usage (to keep the CPU-side data around for mutation)
/// plus `COPY_DST | TEXTURE_BINDING`.
pub(super) fn control_texture(size: u32) -> Image {
    let extent = Extent3d { width: size, height: size, depth_or_array_layers: 1 };
    let bytes = (size as usize) * (size as usize) * CONTROL_BYTES_PER_TEXEL;
    let mut image = Image::new(
        extent,
        TextureDimension::D2,
        vec![0u8; bytes],
        CONTROL_FORMAT,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage = TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING;
    image
}

/// Bytes per texel of [`CONTROL_FORMAT`] (`Rgba8Unorm`).
pub(super) const CONTROL_BYTES_PER_TEXEL: usize = 4;
