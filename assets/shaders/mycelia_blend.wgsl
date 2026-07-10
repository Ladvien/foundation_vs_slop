// MYCELIA temporal interpolation — the one pass that runs every RENDERED frame.
//
// The simulation advances on its own slow clock (`sim_hz`, 1.5 Hz), which is what keeps the mold's growth
// below the human motion-detection threshold. But a slow clock is not the same as a slow *image*. Writing
// each tick's fields straight to the texture the materials sample made the mold jump once per period:
//
//   - The margin itself creeps at 2.92 mm/s, comfortably under the 3.33 mm/s object-relative motion
//     threshold (Leibowitz 1955, 10.1364/josa.45.000829) — so the *field* was never moving too fast.
//   - But the materials resolve that field through `smoothstep`. The rendered iso-contour sits wherever the
//     field crosses a threshold, and its displacement per tick is `Δfield / |∇field|`. Along a shallow
//     gradient — which is most of an advancing colony margin — a small step in `V` slides the visible edge a
//     long way. The edge hopped, several pixels at a time, 1.5 times a second.
//   - A step is also broadband in temporal frequency: it carries energy right through the band the eye is
//     most sensitive to (Kelly 1979, "Motion and vision II", 10.1364/josa.69.001340), regardless of how
//     little the mean advances.
//
// So the fix is not a slower clock (already slow enough) nor a rate limiter (which would only smear the
// step). It is to reconstruct the continuous signal the speed limit was derived for: keep the last two tick
// snapshots and linearly interpolate between them by the phase through the current period. The eye then
// integrates a smooth 2.92 mm/s creep and sees nothing at all.
//
// Costs one tick of latency: the blend reaches snapshot `k` exactly as snapshot `k+1` is computed. That is
// the standard price of temporal interpolation, and 667 ms of lag on an ambience layer is invisible.
//
// This shader has its own bind group (group 0 of its own layout, see `pipeline.rs`) rather than sharing the
// simulation's: a texture may not be bound as a write-only storage target and a sampled texture in the same
// bind group, and `snap_write` is exactly that — written by the sim's `field` pass, read here.

struct BlendParams {
    /// Phase through the current sim period, `0..1`. `0` = show the older snapshot, `1` = the newer.
    alpha: f32,
};

@group(0) @binding(0) var<uniform> blend: BlendParams;
// The tick BEFORE last. Parity means this is whichever snapshot the next tick will overwrite.
@group(0) @binding(1) var snap_old: texture_2d<f32>;
// The most recent tick, just written by the sim's `field` pass.
@group(0) @binding(2) var snap_new: texture_2d<f32>;
// What the floor / wall / fruit materials sample: `R` trail · `G` biomass V · `B` wall contact.
@group(0) @binding(3) var display: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn blend_snapshots(@builtin(global_invocation_id) id: vec3<u32>) {
    let dim = textureDimensions(snap_new);
    if (id.x >= dim.x || id.y >= dim.y) {
        return;
    }
    let p = vec2<i32>(i32(id.x), i32(id.y));
    let a = textureLoad(snap_old, p, 0);
    let b = textureLoad(snap_new, p, 0);
    // Componentwise: trail, biomass and wall contact all interpolate linearly. `A` is unused (see
    // `mycelia_sim.wgsl`); mixing it costs nothing and keeps this a single vector op.
    textureStore(display, p, mix(a, b, blend.alpha));
}
