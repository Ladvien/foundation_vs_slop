//! Render-world compute plumbing for MYCELIA — the repo's first `RenderApp` / render-graph / compute
//! pipeline. Verified against the Bevy 0.19.0 `compute_shader_game_of_life.rs` example: the pipeline is
//! created by a **`RenderStartup`** system (not `FromWorld`), bind groups are prepared in
//! `RenderSystems::PrepareBindGroups`, and the passes are plain systems added to the **`RenderGraph`
//! schedule** ordered `.before(camera_driver)` (the old `render_graph::Node` trait API is superseded).
//!
//! # The simulation chain
//! One shader (`mycelia_sim.wgsl`) exposes four entry points that share a single bind group, dispatched
//! in order each frame as four separate compute passes (so the GPU inserts a memory barrier between them
//! and each sees the previous one's writes):
//!   1. `clear_deposit` — zero the per-texel scent accumulator.
//!   2. `agent_step`    — each walker senses the trail, steers (Jones three-sensor rule), steps, and
//!                        `atomicAdd`s scent into the accumulator.
//!   3. `diffuse`       — blur+decay the trail and fold in this tick's deposits (the transport network).
//!   4. `field`         — one Gray-Scott reaction-diffusion step of the biomass, nucleated by the veins,
//!                        then composite veins + biomass into the shared `display` the material samples.
//! The trail and biomass ping-pongs swap read/write each frame by the same parity, so neither stencil ever
//! reads the texture it is concurrently writing.

use std::borrow::Cow;

use bevy::app::SubApp;
use bevy::core_pipeline::schedule::camera_driver;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_sized, texture_2d, texture_storage_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    CachedComputePipelineId, CachedPipelineState, ComputePassDescriptor, ComputePipelineDescriptor,
    PipelineCache, ShaderStages, StorageTextureAccess, TextureSampleType, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph, RenderQueue};
use bevy::render::storage::GpuShaderBuffer;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderStartup, RenderSystems};
use bevy::shader::ShaderCacheError;

use super::{agents, MoldBuffers, MoldImages, MoldParams, DISPLAY_FORMAT, FIELD_SIZE, WORKGROUP_SIZE};

/// WGSL source for the compute chain (runtime-loaded from `assets/`, like every other shader here).
const SHADER_ASSET_PATH: &str = "shaders/mycelia_sim.wgsl";

/// 1D workgroup width for the agent pass. Must match `@workgroup_size` in the shader.
const LINEAR_WORKGROUP: u32 = 64;
/// `clear_deposit` zeroes one deposit slot per thread; a wider group than the agent pass.
const CLEAR_WORKGROUP: u32 = 256;

/// Wire the render sub-app: pipeline creation at `RenderStartup`, per-frame bind-group prep + lifecycle
/// advance in the `Render` schedule, and the compute chain on the `RenderGraph` schedule. Called from
/// `MyceliaPlugin::build` only when a `RenderApp` sub-app exists.
pub(super) fn build_render_app(render_app: &mut SubApp) {
    render_app
        .init_resource::<MoldState>()
        .add_systems(RenderStartup, init_mold_pipeline)
        .add_systems(Render, advance_state.in_set(RenderSystems::Prepare))
        .add_systems(Render, prepare_bind_group.in_set(RenderSystems::PrepareBindGroups))
        .add_systems(RenderGraph, mold_compute.before(camera_driver));
}

/// The three compute pipelines + the shared bind-group layout. Held as a render-world resource, created
/// once at `RenderStartup`.
#[derive(Resource)]
struct MoldPipeline {
    layout: BindGroupLayoutDescriptor,
    clear: CachedComputePipelineId,
    agents: CachedComputePipelineId,
    diffuse: CachedComputePipelineId,
    field: CachedComputePipelineId,
}

/// This frame's prepared bind group (the six mold resources, with the trail read/write pair selected by
/// the current parity).
#[derive(Resource)]
struct MoldBindGroup(BindGroup);

/// Compute lifecycle + ping-pong parity. `frame` advances once per rendered frame (only once the pipelines
/// are ready); `frame & 1` selects which trail texture is read vs written this tick.
#[derive(Resource, Default)]
struct MoldState {
    ready: bool,
    frame: u64,
}

/// `RenderStartup` — build the shared bind-group layout and queue the three compute pipelines. The layout
/// is a superset used by all three (each entry point statically uses only the bindings it needs, which is
/// permitted), so one bind group serves every pass.
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
                // 0: agent population (read_write — agents update their own pos/heading in place).
                storage_buffer_sized(false, None),
                // 1: deposit accumulator (read_write — atomic add / load / store).
                storage_buffer_sized(false, None),
                // 2: trail READ (sampled/loaded — this tick's source field).
                texture_2d(TextureSampleType::Float { filterable: false }),
                // 3: trail WRITE (storage — this tick's diffused target field).
                texture_storage_2d(DISPLAY_FORMAT, StorageTextureAccess::WriteOnly),
                // 4: display (storage — the composited output the floor material samples).
                texture_storage_2d(DISPLAY_FORMAT, StorageTextureAccess::WriteOnly),
                // 5: shared sim params.
                uniform_buffer::<MoldParams>(false),
                // 6: biomass READ (Gray-Scott `U`,`V` source field for this tick's stencil).
                texture_2d(TextureSampleType::Float { filterable: false }),
                // 7: biomass WRITE (storage — this tick's reacted field).
                texture_storage_2d(DISPLAY_FORMAT, StorageTextureAccess::WriteOnly),
            ),
        ),
    );

    let shader = asset_server.load(SHADER_ASSET_PATH);
    let queue = |entry: &str| {
        pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            layout: vec![layout.clone()],
            shader: shader.clone(),
            entry_point: Some(Cow::from(entry.to_owned())),
            ..default()
        })
    };
    let clear = queue("clear_deposit");
    let agents = queue("agent_step");
    let diffuse = queue("diffuse");
    let field = queue("field");

    commands.insert_resource(MoldPipeline { layout, clear, agents, diffuse, field });
}

/// `RenderSystems::Prepare` — advance the lifecycle. Stay not-ready until ALL three pipelines compile (a
/// still-loading shader is not an error, just wait); a genuine compile error fails loudly. Once ready,
/// advance the frame counter so the ping-pong parity flips each tick.
fn advance_state(
    pipeline: Res<MoldPipeline>,
    pipeline_cache: Res<PipelineCache>,
    mut state: ResMut<MoldState>,
) {
    if !state.ready {
        let all_ok = [pipeline.clear, pipeline.agents, pipeline.diffuse, pipeline.field].iter().all(|id| {
            match pipeline_cache.get_compute_pipeline_state(*id) {
                CachedPipelineState::Ok(_) => true,
                CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(_)) => false,
                CachedPipelineState::Err(err) => {
                    panic!("mycelia: compiling assets/{SHADER_ASSET_PATH}:\n{err}")
                }
                _ => false,
            }
        });
        state.ready = all_ok;
    }
    if state.ready {
        state.frame = state.frame.wrapping_add(1);
    }
}

/// `RenderSystems::PrepareBindGroups` — rebuild the bind group each frame (cheap; the underlying
/// textures/buffers persist). Selects the trail read/write pair by parity and writes the params uniform.
/// Silently returns until every GPU resource exists.
fn prepare_bind_group(
    mut commands: Commands,
    pipeline: Res<MoldPipeline>,
    state: Res<MoldState>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    gpu_buffers: Res<RenderAssets<GpuShaderBuffer>>,
    mold_images: Res<MoldImages>,
    mold_buffers: Res<MoldBuffers>,
    params: Res<MoldParams>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    queue: Res<RenderQueue>,
) {
    let (Some(display), Some(trail_a), Some(trail_b), Some(bio_a), Some(bio_b)) = (
        gpu_images.get(&mold_images.display),
        gpu_images.get(&mold_images.trail_a),
        gpu_images.get(&mold_images.trail_b),
        gpu_images.get(&mold_images.biomass_a),
        gpu_images.get(&mold_images.biomass_b),
    ) else {
        return;
    };
    let (Some(agent_buf), Some(deposit_buf)) =
        (gpu_buffers.get(&mold_buffers.agents), gpu_buffers.get(&mold_buffers.deposit))
    else {
        return;
    };

    // Parity: even frames read A / write B, odd frames read B / write A. What we write this tick becomes
    // next tick's read (the parity flips), giving a correct ping-pong. Trail and biomass share the parity.
    let even = state.frame & 1 == 0;
    let (read, write) = if even { (trail_a, trail_b) } else { (trail_b, trail_a) };
    let (bio_read, bio_write) = if even { (bio_a, bio_b) } else { (bio_b, bio_a) };

    let mut uniform = UniformBuffer::from(params.into_inner().clone());
    uniform.write_buffer(&render_device, &queue);

    let bind_group = render_device.create_bind_group(
        None,
        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            agent_buf.buffer.as_entire_buffer_binding(),
            deposit_buf.buffer.as_entire_buffer_binding(),
            &read.texture_view,
            &write.texture_view,
            &display.texture_view,
            &uniform,
            &bio_read.texture_view,
            &bio_write.texture_view,
        )),
    );
    commands.insert_resource(MoldBindGroup(bind_group));
}

/// The `RenderGraph`-schedule compute chain, ordered `.before(camera_driver)` so its writes land before the
/// main 3D pass samples the display texture. Dispatches the three passes as three separate compute passes
/// so the GPU barriers between them (clear → deposit → diffuse each observe the prior pass's writes).
fn mold_compute(
    mut render_context: RenderContext,
    bind_group: Option<Res<MoldBindGroup>>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<MoldPipeline>,
    state: Res<MoldState>,
) {
    let Some(bind_group) = bind_group.filter(|_| state.ready) else {
        return;
    };
    let (Some(clear), Some(agents_pl), Some(diffuse), Some(field)) = (
        pipeline_cache.get_compute_pipeline(pipeline.clear),
        pipeline_cache.get_compute_pipeline(pipeline.agents),
        pipeline_cache.get_compute_pipeline(pipeline.diffuse),
        pipeline_cache.get_compute_pipeline(pipeline.field),
    ) else {
        return;
    };

    let deposit_slots = FIELD_SIZE * FIELD_SIZE;
    let field_groups = FIELD_SIZE / WORKGROUP_SIZE;
    let bg = &bind_group.0;

    let mut dispatch = |pipeline: &bevy::render::render_resource::ComputePipeline, x: u32, y: u32| {
        let mut pass = render_context
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor::default());
        pass.set_bind_group(0, bg, &[]);
        pass.set_pipeline(pipeline);
        pass.dispatch_workgroups(x, y, 1);
    };

    dispatch(clear, deposit_slots.div_ceil(CLEAR_WORKGROUP), 1);
    dispatch(agents_pl, agents::AGENT_COUNT.div_ceil(LINEAR_WORKGROUP), 1);
    dispatch(diffuse, field_groups, field_groups);
    dispatch(field, field_groups, field_groups);
}
