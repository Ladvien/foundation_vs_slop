// The ASYNC door's aperture (FVS-G-5) — the volume inside the frame that visibly *is not a room*.
//
// This is the game's signature image, and the brief is precise: it must read as a stable anomalous
// opening onto the Backrooms, not as a hole, a screen, or a portal effect pasted on a wall.
//
// THREE DECISIONS, each load-bearing:
//
// 1. OPAQUE, not blended. §7 of the design doc says the aperture must "visibly be-not-a-room", which
//    means it has to OCCLUDE — a translucent quad reads as a window onto the wall behind it, which is
//    exactly the wrong thing. `AlphaMode::Opaque` on the Rust side.
//
// 2. The vanishing point DOES NOT TRACK THE CAMERA. A corridor whose perspective follows you reads as
//    a painting; one whose perspective disagrees with the room reads as *wrong*, which is the whole
//    point. Parallax that contradicts its frame is the uncanny-valley mechanism the backlog cites
//    ([UV-REV], Kätsyri et al. 2015: it is the MISMATCH between cues, not ugliness, that unsettles).
//    So the corridor is computed purely in the quad's own UV space and never sees the view matrix.
//
// 3. THE COLOUR IS FLOORED WELL ABOVE BLACK, and `nest.wgsl` paid for this lesson already (see its
//    note): a vignette to black reads as a hole punched in the geometry, not as somewhere else. The
//    Backrooms palette — sodium-yellow wallpaper over damp carpet — keeps it legibly a PLACE, and ties
//    the door to the expedition levels it opens onto (assets/textures/, almond_water_backrooms).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals
#import foundation::noise::{vnoise, fbm3_organic}

struct ApertureSettings {
    // How far down the false corridor the walls recede. Higher = deeper.
    depth: f32,
    // Domain-warp strength. 0 = a clean corridor; high = the geometry stops agreeing with itself.
    warp: f32,
    // [0,1] — rises while an operative stands in the trigger. The door noticing you.
    charge: f32,
    _pad: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: ApertureSettings;

// Sodium-lit wallpaper and damp carpet: the Backrooms palette the expedition levels already use.
const WALL_TINT: vec3<f32> = vec3<f32>(0.82, 0.74, 0.42);
const FLOOR_TINT: vec3<f32> = vec3<f32>(0.34, 0.30, 0.20);
// The floor referenced above. Below this the aperture stops being a place and becomes a hole.
const FLOOR_LEVEL: f32 = 0.055;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // Quad UVs are [0,1]; centre to [-1,1] so the corridor's vanishing point sits at the middle of the
    // frame regardless of where the player is standing (decision 2).
    let uv = (mesh.uv - vec2<f32>(0.5)) * 2.0;
    let t = globals.time;

    // A false corridor by inverse projection: distance along the corridor is 1/|height|, so the walls
    // converge on the horizon without any camera involvement. `q.y` is the receding coordinate.
    let h = max(abs(uv.y), 0.0015);
    let march = material.depth / h;
    // Sideways coordinate widens with distance — the perspective of a hallway seen head-on.
    let across = uv.x * march;

    // Domain warp. This is what stops it being a tunnel and starts it being a place that is wrong: the
    // corridor's own geometry drifts, slowly, and never resolves.
    let drift = t * 0.06;
    let w = fbm3_organic(vec2<f32>(across * 0.35, march * 0.12 - drift)) - 0.5;
    let warped = vec2<f32>(across + w * material.warp, march + w * material.warp * 0.5);

    // Wallpaper banding down the corridor, and a carpet grain that runs the other way, so the two
    // surfaces never share a rhythm.
    let paper = vnoise(vec2<f32>(warped.x * 1.6, warped.y * 0.55 - drift * 2.0));
    let grain = vnoise(vec2<f32>(warped.x * 4.0, warped.y * 2.2));

    // Blend wall into floor across the horizon line, softly enough that the seam is not a hard edge.
    let floorness = smoothstep(0.0, 0.55, abs(uv.y));
    var col = mix(FLOOR_TINT * (0.75 + 0.5 * grain), WALL_TINT * (0.68 + 0.5 * paper), floorness);

    // Depth falloff — dimmer far down the corridor, but FLOORED (decision 3). Never black.
    let far = 1.0 / (1.0 + march * 0.05);
    col = col * mix(FLOOR_LEVEL / 0.35, 1.0, far);
    col = max(col, vec3<f32>(FLOOR_LEVEL));

    // The door noticing you: charge pushes a slow sodium bloom up out of the vanishing point and
    // stiffens the warp's contrast. Deliberately NOT a colour shift to red or green — the anomaly is
    // not hostile, it is indifferent, and the Foundation's containment of it is what makes it safe.
    let core = 1.0 - smoothstep(0.0, 0.45, length(uv * vec2<f32>(0.6, 1.0)));

    // 4. IT IS NEVER INERT, AND IT IS AN HDR LIGHT SOURCE (both added 2026-08-02).
    //
    //    The core term used to be multiplied by `charge` alone, so an aperture nobody was standing in
    //    contributed exactly nothing and the door rendered as a flat sheet of sodium — the game's
    //    signature image was the dullest surface in the hub. A resting breath fixes that: slow enough
    //    (~7 s) to read as respiration rather than a pulse, which is the difference between "alive"
    //    and "an effect".
    //
    //    And nothing in this shader previously exceeded 1.0, so despite the camera carrying `Hdr` +
    //    `Bloom` the aperture could not bloom AT ALL. Taking the core above 1 is what makes it read as
    //    a hole emitting light into the room rather than a panel painted on the wall. The Rust side
    //    puts a real `PointLight` at the same place so the spill lands on the hall floor too — a
    //    portal that lights nothing does not read as a portal.
    let breath = 0.5 + 0.5 * sin(t * 0.9);
    let rest = 1.15 + 0.55 * breath;
    col += WALL_TINT * core * (rest + material.charge * 2.6);

    // A single slow horizontal band, like a fluorescent tube passing somewhere it should not be.
    let sweep = exp(-pow((uv.y - sin(t * 0.13) * 0.7) * 6.0, 2.0));
    col += WALL_TINT * sweep * 0.06;

    return vec4<f32>(col, 1.0);
}
