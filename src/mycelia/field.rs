//! Field-texture construction helpers shared by the mold's GPU resources.
//!
//! Every field texture is square, `Rgba16Float`, and usable as **both** a compute storage-write target and
//! a sampled/loaded texture (the trail ping-pong alternates between the two roles each frame), so they all
//! carry `STORAGE_BINDING | TEXTURE_BINDING | COPY_DST`. They are created **zero-filled** (not
//! `new_uninit`) so the very first simulation tick reads a clean field instead of undefined GPU memory.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureUsages};

use super::DISPLAY_FORMAT;

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
