// MYCELIA simulation — the Physarum transport network (Phase B). Three compute entry points sharing one
// bind group, dispatched in order each tick: clear_deposit → agents → diffuse. Agents sense the trail
// field, steer by Jones' three-sensor rule, step, and deposit scent into an atomic accumulator; diffuse
// blurs+decays the trail, folds in the deposits, and composites the grimy-bioluminescent display.
//
// Ref: Jones (2010), "Characteristics of pattern formation and evolution in approximations of Physarum
// transport networks," Artificial Life (arXiv 1503.06579). The sense→rotate→move→deposit→diffuse→decay
// loop is the standard real-time GPU formulation (Jenson; Lague).

// MUST byte-match `MoldParams` in `src/mycelia/mod.rs` (field order + types).
struct MoldParams {
    world_origin: vec2<f32>,
    world_extent: vec2<f32>,
    field_res: vec2<f32>,
    time: f32,
    agent_count: u32,
    sense_angle: f32,
    sense_dist: f32,
    rotate_angle: f32,
    step_size: f32,
    deposit_amount: f32,
    decay: f32,
    trail_max: f32,
    deposit_scale: f32,
};

// MUST byte-match the `u32` encoding in `src/mycelia/agents.rs` (std430 stride 16).
struct Agent {
    pos: vec2<f32>,
    heading: f32,
    _pad: f32,
};

@group(0) @binding(0) var<storage, read_write> agents: array<Agent>;
@group(0) @binding(1) var<storage, read_write> deposit: array<atomic<u32>>;
@group(0) @binding(2) var trail_read: texture_2d<f32>;
@group(0) @binding(3) var trail_write: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var display: texture_storage_2d<rgba16float, write>;
@group(0) @binding(5) var<uniform> params: MoldParams;

// Integer hash → uniform f32 in [0,1). Cheap per-agent randomness for the "turn away" case.
fn hash_u32(x: u32) -> u32 {
    var s = x;
    s ^= 2747636419u;
    s *= 2654435769u;
    s ^= s >> 16u;
    s *= 2654435769u;
    s ^= s >> 16u;
    s *= 2654435769u;
    return s;
}
fn rand01(seed: u32) -> f32 {
    return f32(hash_u32(seed)) * (1.0 / 4294967296.0);
}

// Wrap an integer texel coordinate into [0, dim) (toroidal field).
fn wrap_i(v: i32, dim: i32) -> i32 {
    return ((v % dim) + dim) % dim;
}

// Sample the trail R channel at a float field position, wrapped toroidally.
fn trail_at(p: vec2<f32>, dim: i32) -> f32 {
    let x = wrap_i(i32(floor(p.x)), dim);
    let y = wrap_i(i32(floor(p.y)), dim);
    return textureLoad(trail_read, vec2<i32>(x, y), 0).r;
}

// ── Pass 1: zero the deposit accumulator ─────────────────────────────────────────────────────────────
@compute @workgroup_size(256, 1, 1)
fn clear_deposit(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= arrayLength(&deposit)) {
        return;
    }
    atomicStore(&deposit[id.x], 0u);
}

// ── Pass 2: agents sense, steer, step, deposit ───────────────────────────────────────────────────────
// (Named `agent_step`, not `agents`, so the entry point doesn't collide with the `agents` storage binding
// — WGSL identifiers are unique module-wide.)
@compute @workgroup_size(64, 1, 1)
fn agent_step(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= params.agent_count) {
        return;
    }
    let dim = i32(params.field_res.x);
    let dimf = params.field_res.x;

    var a = agents[i];
    let sa = params.sense_angle;
    let sd = params.sense_dist;

    // Three sensors: centre, ahead-left, ahead-right.
    let dir_c = vec2<f32>(cos(a.heading), sin(a.heading));
    let dir_l = vec2<f32>(cos(a.heading + sa), sin(a.heading + sa));
    let dir_r = vec2<f32>(cos(a.heading - sa), sin(a.heading - sa));
    let c = trail_at(a.pos + dir_c * sd, dim);
    let l = trail_at(a.pos + dir_l * sd, dim);
    let r = trail_at(a.pos + dir_r * sd, dim);

    // Jones steering rule.
    var heading = a.heading;
    if (c > l && c > r) {
        // strongest ahead — hold course.
    } else if (c < l && c < r) {
        // weakest ahead — turn away randomly to break symmetry.
        let rnd = rand01(i * 1099087573u + u32(params.time * 60.0));
        if (rnd < 0.5) {
            heading += params.rotate_angle;
        } else {
            heading -= params.rotate_angle;
        }
    } else if (l < r) {
        heading -= params.rotate_angle;
    } else if (r < l) {
        heading += params.rotate_angle;
    }

    // Step forward, wrapping toroidally into [0, dim).
    var np = a.pos + vec2<f32>(cos(heading), sin(heading)) * params.step_size;
    np.x -= floor(np.x / dimf) * dimf;
    np.y -= floor(np.y / dimf) * dimf;

    a.pos = np;
    a.heading = heading;
    agents[i] = a;

    // Deposit fixed-point scent at the new texel.
    let dx = wrap_i(i32(floor(np.x)), dim);
    let dy = wrap_i(i32(floor(np.y)), dim);
    let idx = u32(dy) * u32(dim) + u32(dx);
    atomicAdd(&deposit[idx], u32(params.deposit_amount * params.deposit_scale));
}

// ── Pass 3: diffuse + decay the trail, fold in deposits, composite the display ───────────────────────
@compute @workgroup_size(8, 8, 1)
fn diffuse(@builtin(global_invocation_id) id: vec3<u32>) {
    let dim = i32(params.field_res.x);
    if (i32(id.x) >= dim || i32(id.y) >= dim) {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);

    // 3x3 box blur of the read trail (simulates diffusion of the scent — Jenson/Lague mean filter).
    var sum = 0.0;
    for (var oy = -1; oy <= 1; oy++) {
        for (var ox = -1; ox <= 1; ox++) {
            sum += textureLoad(trail_read, vec2<i32>(wrap_i(x + ox, dim), wrap_i(y + oy, dim)), 0).r;
        }
    }
    let blur = sum * (1.0 / 9.0);

    // This tick's deposits at this texel (fixed-point → float).
    let idx = u32(y) * u32(dim) + u32(x);
    let dep = f32(atomicLoad(&deposit[idx])) / params.deposit_scale;

    // Diffuse + decay, then reinforce with fresh scent. Clamp guards transient spikes.
    let v = clamp(blur * params.decay + dep, 0.0, params.trail_max);
    textureStore(trail_write, vec2<i32>(x, y), vec4<f32>(v, 0.0, 0.0, 1.0));

    // Grimy-bioluminescent composite: sickly green/cyan veins glowing out of a faint dark biomass film.
    // The vein threshold sits well above the wandering-agent noise floor so only reinforced channels light
    // up; the film is a dim wet sheen that hints at biomass where any scent lingers. Alpha stays < 1 so the
    // mold reads as a translucent coating over the carpet, not opaque paint.
    let veins = smoothstep(2.5, 8.0, v);
    let film = smoothstep(0.3, 2.5, v);
    let glow = vec3<f32>(0.10, 0.42, 0.24) * veins;
    let grime = vec3<f32>(0.015, 0.035, 0.028) * film;
    let alpha = max(veins * 0.85, film * 0.22);
    textureStore(display, vec2<i32>(x, y), vec4<f32>(glow + grime, alpha));
}
