//! Render-world compute plumbing for MYCELIA — the repo's first `RenderApp` / render-graph / compute
//! pipeline. Verified against the Bevy 0.19.0 `compute_shader_game_of_life.rs` example: the pipeline is
//! created by a **`RenderStartup`** system (not `FromWorld`), bind groups are prepared in
//! `RenderSystems::PrepareBindGroups`, and the pass is a plain system added to the **`RenderGraph`
//! schedule** ordered `.before(camera_driver)` (the old `render_graph::Node` trait API is superseded).
//!
//! Phase A scope: a single `gradient` compute entry point writes an animated pattern into the shared
//! `mold_display` texture, proving the whole compute→texture→material path works before any Physarum /
//! reaction-diffusion is layered on.

use std::borrow::Cow;

use bevy::app::SubApp;
use bevy::core_pipeline::schedule::camera_driver;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{texture_storage_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    CachedComputePipelineId, CachedPipelineState, ComputePassDescriptor, ComputePipelineDescriptor,
    PipelineCache, ShaderStages, StorageTextureAccess, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderStartup, RenderSystems};
use bevy::shader::ShaderCacheError;

use super::{MoldImages, MoldParams, DISPLAY_FORMAT, FIELD_SIZE, WORKGROUP_SIZE};

/// WGSL source for the compute passes (runtime-loaded from `assets/`, like every other shader here).
const SHADER_ASSET_PATH: &str = "shaders/mycelia_gradient.wgsl";

/// Wire the render sub-app: pipeline creation at `RenderStartup`, per-frame bind-group prep + state
/// advance in the `Render` schedule, and the compute pass on the `RenderGraph` schedule. Called from
/// `MyceliaPlugin::build` only when a `RenderApp` sub-app exists.
pub(super) fn build_render_app(render_app: &mut SubApp) {
    render_app
        .init_resource::<MoldState>()
        .add_systems(RenderStartup, init_mold_pipeline)
        .add_systems(Render, prepare_bind_group.in_set(RenderSystems::PrepareBindGroups))
        .add_systems(Render, advance_state.in_set(RenderSystems::Prepare))
        .add_systems(RenderGraph, mold_compute.before(camera_driver));
}

/// The compute pipeline handle(s) + the layout descriptor. Held as a render-world resource, created once.
#[derive(Resource)]
struct MoldPipeline {
    layout: BindGroupLayoutDescriptor,
    gradient_pipeline: CachedComputePipelineId,
}

/// The prepared bind group for this frame (display storage texture + params uniform).
#[derive(Resource)]
struct MoldBindGroup(BindGroup);

/// Compute lifecycle: wait for the pipeline to compile, then dispatch every frame.
#[derive(Resource, Default)]
enum MoldState {
    #[default]
    Loading,
    Ready,
}

/// `RenderStartup` system — build the bind-group layout and queue the compute pipeline. Mirrors the
/// 0.19 game-of-life `init_*_pipeline`: the layout is a `BindGroupLayoutDescriptor` (resolved lazily by
/// the cache), and `queue_compute_pipeline` returns a `CachedComputePipelineId`.
fn init_mold_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "mycelia_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                // binding 0: the display texture, written by the compute pass.
                texture_storage_2d(DISPLAY_FORMAT, StorageTextureAccess::WriteOnly),
                // binding 1: shared sim params (world mapping + time).
                uniform_buffer::<MoldParams>(false),
            ),
        ),
    );
    let shader = asset_server.load(SHADER_ASSET_PATH);
    let gradient_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        layout: vec![layout.clone()],
        shader,
        entry_point: Some(Cow::from("gradient")),
        ..default()
    });
    commands.insert_resource(MoldPipeline { layout, gradient_pipeline });
}

/// `RenderSystems::PrepareBindGroups` — rebuild the bind group each frame (cheap; the underlying
/// textures/buffers persist). Writes the extracted `MoldParams` into a uniform buffer and binds it
/// alongside the display texture view. Silently returns until the GPU image exists.
fn prepare_bind_group(
    mut commands: Commands,
    pipeline: Res<MoldPipeline>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    mold_images: Res<MoldImages>,
    params: Res<MoldParams>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    queue: Res<RenderQueue>,
) {
    let Some(display) = gpu_images.get(&mold_images.display) else {
        return;
    };

    let mut uniform = UniformBuffer::from(params.into_inner().clone());
    uniform.write_buffer(&render_device, &queue);

    let bind_group = render_device.create_bind_group(
        None,
        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((&display.texture_view, &uniform)),
    );
    commands.insert_resource(MoldBindGroup(bind_group));
}

/// `RenderSystems::Prepare` — advance the lifecycle: stay `Loading` until the pipeline compiles (a
/// still-loading shader is not an error — just wait), then flip to `Ready`. A genuine compile error
/// fails loudly.
fn advance_state(
    pipeline: Res<MoldPipeline>,
    pipeline_cache: Res<PipelineCache>,
    mut state: ResMut<MoldState>,
) {
    if let MoldState::Loading = *state {
        match pipeline_cache.get_compute_pipeline_state(pipeline.gradient_pipeline) {
            CachedPipelineState::Ok(_) => *state = MoldState::Ready,
            CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(_)) => {}
            CachedPipelineState::Err(err) => {
                panic!("mycelia: compiling assets/{SHADER_ASSET_PATH}:\n{err}")
            }
            _ => {}
        }
    }
}

/// The `RenderGraph`-schedule compute pass. `RenderContext` is a system param in 0.19; we record a
/// compute pass into its encoder and dispatch one workgroup per 8×8 texel tile. Ordered
/// `.before(camera_driver)` so the write lands before the main 3D pass samples it.
fn mold_compute(
    mut render_context: RenderContext,
    bind_group: Option<Res<MoldBindGroup>>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<MoldPipeline>,
    state: Res<MoldState>,
) {
    // Nothing to do until the pipeline is ready and a bind group has been prepared.
    let (MoldState::Ready, Some(bind_group)) = (&*state, bind_group) else {
        return;
    };
    let Some(gradient) = pipeline_cache.get_compute_pipeline(pipeline.gradient_pipeline) else {
        return;
    };

    let mut pass = render_context
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor::default());
    pass.set_bind_group(0, &bind_group.0, &[]);
    pass.set_pipeline(gradient);
    let groups = FIELD_SIZE / WORKGROUP_SIZE;
    pass.dispatch_workgroups(groups, groups, 1);
}
