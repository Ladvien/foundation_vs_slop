// MYCELIA floor material — samples the GPU-written mold field by WORLD XZ position (not mesh UV: every
// floor tile shares one Plane3d with UV 0..1, so world position is the only stable index) and composites
// its grimy bioluminescence over the floor. Phase A: standalone overlay proving the sample path.

#import bevy_pbr::forward_io::VertexOutput

// MUST byte-match `MoldMatParams` in `src/mycelia/material.rs`.
struct MoldMatParams {
    world_origin: vec2<f32>,
    world_extent: vec2<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> mat: MoldMatParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var mold_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var mold_samp: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // World XZ → field UV using the SAME mapping the compute pass writes with.
    let uv = (mesh.world_position.xz - mat.world_origin) / mat.world_extent;

    // Outside the field footprint → fully transparent (nothing to composite).
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        discard;
    }

    let mold = textureSample(mold_tex, mold_samp, uv);
    // RGB is the mold colour; alpha (Phase A: the vein mask in .a) fades the overlay so bare floor shows
    // through where there is no mold.
    return vec4<f32>(mold.rgb, mold.a);
}
