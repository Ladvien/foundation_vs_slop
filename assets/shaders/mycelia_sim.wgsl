// MYCELIA simulation — a two-layer sentient-mold field. Four compute entry points share one bind group and
// are dispatched in order each tick: clear_deposit → agent_step → diffuse → field.
//
//   Transport layer (the "mind"): agents sense the trail, steer by Jones' three-sensor rule, step, and
//   deposit scent into an atomic accumulator; `diffuse` blurs+decays the trail and folds the deposits in.
//   Ref: Jones (2010), "Characteristics of pattern formation and evolution in approximations of Physarum
//   transport networks," Artificial Life (10.1162/artl.2010.16.2.16202; arXiv 1503.06579). The
//   sense→rotate→move→deposit→diffuse→decay loop is the standard real-time GPU formulation (Jenson; Lague).
//
//   Field layer (the "flesh"): `field` runs one Gray-Scott reaction-diffusion step whose biomass is
//   nucleated by the veins, then composites veins + biomass into the grimy-bioluminescent display.
//   Ref: Turk (1991), "Generating textures on arbitrary surfaces using reaction-diffusion," SIGGRAPH
//   (10.1145/122718.122749); Pearson (1993), Science 261.

// MUST byte-match `MoldParams` in `src/mycelia/mod.rs` (field order + types).
struct MoldParams {
    world_origin: vec2<f32>,
    world_extent: vec2<f32>,
    field_res: vec2<f32>,
    control_res: vec2<f32>,
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
    dt: f32,
    feed: f32,
    kill: f32,
    d_u: f32,
    d_v: f32,
    bloom_seed: f32,
    diffuse_weight: f32,
    photophobia: f32,
    chemo_gain: f32,
    disturbance_gain: f32,
    wall_repel: f32,
    glow_gain: f32,
    intensity: f32,
    vein_lo: f32,
    vein_hi: f32,
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
@group(0) @binding(6) var biomass_read: texture_2d<f32>;
@group(0) @binding(7) var biomass_write: texture_storage_2d<rgba16float, write>;
// CPU-written world state, one texel per dungeon cell.
//   R = chemoattractant (blood, nests) · G = light/gaze · B = disturbance (squad)
//   A = substrate mask: 0 = void · 0.5 = floor never seen · 1 = floor explored
// The mold GROWS on any floor (A >= 0.25) but is only DRAWN on explored floor (A >= 0.75) — otherwise the
// coating would trace the corridor layout through the fog of war and leak the map.
@group(0) @binding(8) var control: texture_2d<f32>;

/// Is this cell floor the mold may grow on? (Seen or not.)
fn is_walkable(a: f32) -> f32 {
    return step(0.25, a);
}
/// Has the player explored this cell, so the mold may be rendered here?
fn is_explored(a: f32) -> f32 {
    return step(0.75, a);
}

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

// Sample the control texture at a float FIELD position. The control texture spans the same world footprint
// at one texel per dungeon cell, so the field UV maps straight onto it.
fn control_at(p: vec2<f32>, dim: i32) -> vec4<f32> {
    let uv = vec2<f32>(f32(wrap_i(i32(floor(p.x)), dim)), f32(wrap_i(i32(floor(p.y)), dim)))
           / params.field_res;
    let c = uv * params.control_res;
    let cx = clamp(i32(c.x), 0, i32(params.control_res.x) - 1);
    let cy = clamp(i32(c.y), 0, i32(params.control_res.y) - 1);
    return textureLoad(control, vec2<i32>(cx, cy), 0);
}

// What an agent's sensor actually perceives at `p`: the scent trail, pulled toward food and pushed away
// from gaze, footsteps, and the void. All gains are in trail units so they compete directly with scent.
//
// This single expression is where the "sentience" lives — foraging (chemo), photophobia + habituation
// (light, already habituation-attenuated on the CPU), flinching from the squad (disturbance), and staying
// on the floor (wall_repel).
fn sense(p: vec2<f32>, dim: i32) -> f32 {
    let ctl = control_at(p, dim);
    let attract = ctl.r * params.chemo_gain;
    let repel = ctl.g * params.photophobia
              + ctl.b * params.disturbance_gain
              + (1.0 - is_walkable(ctl.a)) * params.wall_repel;
    return trail_at(p, dim) + attract - repel;
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

    // Three sensors: centre, ahead-left, ahead-right. They perceive scent *and* the world (food, gaze,
    // footsteps, walls), so the same Jones steering rule now produces foraging and flinching.
    let dir_c = vec2<f32>(cos(a.heading), sin(a.heading));
    let dir_l = vec2<f32>(cos(a.heading + sa), sin(a.heading + sa));
    let dir_r = vec2<f32>(cos(a.heading - sa), sin(a.heading - sa));
    let c = sense(a.pos + dir_c * sd, dim);
    let l = sense(a.pos + dir_l * sd, dim);
    let r = sense(a.pos + dir_r * sd, dim);

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

    // Deposit fixed-point scent at the new texel — but only on walkable floor, so no trail ever accumulates
    // out in the void (agents that seeded there leave no mark while they steer themselves back to ground).
    let dx = wrap_i(i32(floor(np.x)), dim);
    let dy = wrap_i(i32(floor(np.y)), dim);
    if (is_walkable(control_at(np, dim).a) > 0.5) {
        let idx = u32(dy) * u32(dim) + u32(dx);
        atomicAdd(&deposit[idx], u32(params.deposit_amount * params.deposit_scale));
    }
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

    // 3x3 mean of the read trail (Jenson/Lague mean filter).
    var sum = 0.0;
    for (var oy = -1; oy <= 1; oy++) {
        for (var ox = -1; ox <= 1; ox++) {
            sum += textureLoad(trail_read, vec2<i32>(wrap_i(x + ox, dim), wrap_i(y + oy, dim)), 0).r;
        }
    }
    let blur = sum * (1.0 / 9.0);
    let here = textureLoad(trail_read, vec2<i32>(x, y), 0).r;

    // This tick's deposits at this texel (fixed-point → float).
    let idx = u32(y) * u32(dim) + u32(x);
    let dep = f32(atomicLoad(&deposit[idx])) / params.deposit_scale;

    // Diffuse by lerping *toward* the mean (not replacing with it — a full replacement divides every
    // deposit spike by 9 each tick, so channels could never accumulate), then decay, then reinforce with
    // this tick's fresh scent. Clamp guards transient spikes.
    let spread = mix(here, blur, params.diffuse_weight);
    let v = clamp(spread * params.decay + dep, 0.0, params.trail_max);
    textureStore(trail_write, vec2<i32>(x, y), vec4<f32>(v, 0.0, 0.0, 1.0));
}

// ── Pass 4: Gray-Scott biomass step + final display composite ────────────────────────────────────────
// U (substrate) and V (biomass) diffuse at unequal rates and react via the autocatalytic U + 2V -> 3V.
// Strong veins nucleate V beneath them, so the blooms grow *along* the transport network. The 9-point
// Laplacian stencil (orthogonal 0.2, diagonal 0.05, centre -1) is the standard discretization.
//
// Refs: Turk (1991) SIGGRAPH 10.1145/122718.122749 (RD as surface texture synthesis); Pearson (1993),
// Science 261 (the (F,k) regime map); Leppänen et al. 10.1590/S0103-97332004000300006.

fn bio_at(x: i32, y: i32, dim: i32) -> vec2<f32> {
    return textureLoad(biomass_read, vec2<i32>(wrap_i(x, dim), wrap_i(y, dim)), 0).rg;
}

@compute @workgroup_size(8, 8, 1)
fn field(@builtin(global_invocation_id) id: vec3<u32>) {
    let dim = i32(params.field_res.x);
    if (i32(id.x) >= dim || i32(id.y) >= dim) {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);

    let c = bio_at(x, y, dim);
    var u = c.x;
    var v = c.y;

    // 9-point Laplacian of (U, V).
    var lap = (bio_at(x - 1, y, dim) + bio_at(x + 1, y, dim)
             + bio_at(x, y - 1, dim) + bio_at(x, y + 1, dim)) * 0.2;
    lap += (bio_at(x - 1, y - 1, dim) + bio_at(x + 1, y - 1, dim)
          + bio_at(x - 1, y + 1, dim) + bio_at(x + 1, y + 1, dim)) * 0.05;
    lap -= c;

    // Gray-Scott reaction.
    let uvv = u * v * v;
    u += (params.d_u * lap.x - uvv + params.feed * (1.0 - u)) * params.dt;
    v += (params.d_v * lap.y + uvv - (params.feed + params.kill) * v) * params.dt;

    // The transport network feeds the flesh: a strong vein seeds biomass beneath itself. `trail_read` is
    // this tick's source field (the freshly-diffused trail lands in `trail_write`), so the bloom trails the
    // veins by exactly one tick — imperceptible, and it avoids a read-after-write on the same texture.
    let trail = textureLoad(trail_read, vec2<i32>(x, y), 0).r;
    let ctl = control_at(vec2<f32>(f32(x), f32(y)), dim);
    let vein = smoothstep(params.vein_lo, params.vein_hi, trail);

    // Blooms swell in the unseen dark and shrink under a live gaze — the room you just left goes ripe.
    let dark = 1.0 - ctl.g;
    v += vein * params.bloom_seed * dark * params.dt;

    u = clamp(u, 0.0, 1.0);
    v = clamp(v, 0.0, 1.0);
    textureStore(biomass_write, vec2<i32>(x, y), vec4<f32>(u, v, 0.0, 1.0));

    // Grimy-bioluminescent composite. Sickly green/cyan veins glow out of a dark wet biomass film; a faint
    // scent sheen hints at growth even where no vein has established. Alpha stays < 1 throughout so the
    // mold reads as a translucent coating over the carpet, not opaque paint.
    // The vein window is config-driven (`vein_lo`..`vein_hi`) — reuse the mask computed for nucleation, so
    // "what nucleates a bloom" and "what lights up" can never drift apart. The sheen sits just under the
    // window so a faint scent still hints at growth where no channel has established.
    let veins = vein;
    let sheen = smoothstep(params.vein_lo * 0.17, params.vein_lo, trail);
    let bio = smoothstep(0.10, 0.35, v);

    // The mold conceals its bioluminescence under a direct gaze — glow is strongest in the dark. (Slower
    // structural retreat comes from the agents themselves fleeing the light; this is the instant flinch.)
    let conceal = 1.0 - 0.7 * ctl.g;

    // Kept deliberately dim: the game's bloom post-process blows out anything brighter, turning the veins
    // into neon tubes rather than a sickly phosphorescence.
    let glow = vec3<f32>(0.05, 0.22, 0.13) * veins * conceal * params.glow_gain;
    let flesh = vec3<f32>(0.045, 0.075, 0.055) * bio;
    let grime = vec3<f32>(0.015, 0.035, 0.028) * sheen;

    // Masked to *explored* floor: never over the void, and never on ground the player has not yet seen (the
    // mold still grows there — it just isn't drawn, so it cannot leak the map through the fog). The film
    // terms are held well under the veins so the network stays legible instead of drowning in a flat wash.
    let alpha = max(max(veins * 0.80, sheen * 0.12), bio * 0.34)
              * is_explored(ctl.a) * params.intensity;
    textureStore(display, vec2<i32>(x, y), vec4<f32>(glow + flesh + grime, alpha));
}
