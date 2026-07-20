//! Registers the shared `foundation::noise` WGSL library (`assets/shaders/noise.wgsl`) so every
//! asset-loaded consumer shader can `#import foundation::noise::{hash21, vnoise, fbm, rand_dir}` instead
//! of copy-pasting the Dave-Hoskins hash → value-noise → fbm chain across ~7 files (2026-07-19 review
//! Finding E; deferred from 2026-07-05 [9]).
//!
//! `load_shader_library!` embeds the file into the binary and force-loads it permanently — this "works
//! around a limitation of the shader loader not properly loading dependencies of shaders" (its own doc):
//! an asset-loaded library that is only ever `#import`ed (never a material's `ShaderRef`) is otherwise
//! not reliably registered with the shader composer. Windowed-only: the headless harness renders nothing,
//! so no material shader compiles there and no import needs resolving.

use bevy::prelude::*;
use bevy::shader::load_shader_library;

/// Adds the `foundation::noise` shader import module. Registered after `DefaultPlugins` (so the
/// `EmbeddedAssetRegistry` that `AssetPlugin` inserts already exists) and before any material shader
/// specializes.
pub struct ShaderLibraryPlugin;

impl Plugin for ShaderLibraryPlugin {
    fn build(&self, app: &mut App) {
        // Path is relative to THIS file (`src/shader_lib.rs`) for `include_bytes!`, so `../assets/...`
        // reaches the repo's `assets/shaders/` where every other shader lives.
        load_shader_library!(app, "../assets/shaders/noise.wgsl");
    }
}
