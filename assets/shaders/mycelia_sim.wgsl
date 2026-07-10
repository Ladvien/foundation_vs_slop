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
//   nucleated by the veins, then composites veins + biomass into this tick's field snapshot.
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
    wall_affinity: f32,
    carrion_bloom: f32,
    vein_lo: f32,
    vein_hi: f32,
    coarse_res: u32,
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
// Per-tick SNAPSHOT target. `blend` (mycelia_blend.wgsl) interpolates the last two snapshots into the
// `display` texture the materials sample, so the mold advances continuously between ticks.
@group(0) @binding(4) var snap_write: texture_storage_2d<rgba16float, write>;
@group(0) @binding(5) var<uniform> params: MoldParams;
@group(0) @binding(6) var biomass_read: texture_2d<f32>;
@group(0) @binding(7) var biomass_write: texture_storage_2d<rgba16float, write>;
// CPU-written world state, one texel per dungeon cell.
//   R = chemoattractant (blood, nests) · G = light/gaze · B = disturbance (squad)
//   A = substrate: 0 = void · 0.33 = floor never seen · 0.67 = remembered · 1.0 = currently visible
// The mold GROWS on any floor — that is all this pass needs from `A`. It is only DRAWN on explored floor
// (else the coating would trace the corridor layout through the fog and leak the map) and only LIT where a
// unit can currently see, but both of those are decided per-frame by the material shaders, which sample this
// same texture directly.
@group(0) @binding(8) var control: texture_2d<f32>;
// Static wall-proximity field at FIELD resolution, written once: R = 1 on the mold's side of a wall surface,
// falling to 0 over `wall_reach` world units. Built by an exact Euclidean distance transform against the
// real slab geometry, so its ridge sits on the surface the player sees rather than at the cell centre.
@group(0) @binding(9) var wall_prox: texture_2d<f32>;
// The mold's ONLY channel back to the CPU: a `coarse_res²` reduction of the biomass field, one `vec4` per
// cell = (max V in the block, U at that same texel, that texel's x, that texel's y). Read back by
// `src/mycelia/fruit.rs` to decide where mushrooms erupt. Write-only from the shader's point of view.
@group(0) @binding(10) var<storage, read_write> coarse: array<vec4<f32>>;

const TAU: f32 = 6.2831853;

// Substrate mask `A` is four-state: 0 void · 0.33 floor never seen · 0.67 remembered · 1.0 currently visible.
/// Is this cell floor the mold may grow on? (Seen or not.) The only threshold the SIM needs: growth keys off
/// "is floor" alone. The explored/visible thresholds are a *rendering* concern and live in the three material
/// shaders, which read `control.a` per frame rather than through this pass's 1.5 Hz output.
fn is_walkable(a: f32) -> f32 {
    return step(0.2, a);
}

/// Is the dungeon cell under FIELD texel (x, y) floor? Out-of-range texels are non-floor: the field covers
/// the dungeon exactly, so its border *is* the world boundary and must behave like rock. (Returning 0 here
/// is what lets the stencils below treat the border as a no-flux wall instead of wrapping toroidally.)
fn texel_walkable(x: i32, y: i32, dim: i32) -> f32 {
    if (x < 0 || y < 0 || x >= dim || y >= dim) {
        return 0.0;
    }
    let c = vec2<f32>(f32(x), f32(y)) / params.field_res * params.control_res;
    let cx = clamp(i32(c.x), 0, i32(params.control_res.x) - 1);
    let cy = clamp(i32(c.y), 0, i32(params.control_res.y) - 1);
    return is_walkable(textureLoad(control, vec2<i32>(cx, cy), 0).a);
}

/// May an agent stand at this float FIELD position?
fn pos_walkable(p: vec2<f32>, dim: i32) -> bool {
    return texel_walkable(i32(floor(p.x)), i32(floor(p.y)), dim) > 0.5;
}

/// Bilinearly sample the static wall-proximity field at a float FIELD position. The field is now stored at
/// field resolution (1:1 with this texture), so this is a plain bilinear tap — no cell-grid remap. Bilinear
/// (not nearest) so agents get a smooth gradient to climb toward the wall instead of a blocky step.
fn wall_prox_at(p: vec2<f32>, dim: i32) -> f32 {
    let c = p - vec2<f32>(0.5, 0.5);
    let base = floor(c);
    let f = c - base;
    let hi = params.field_res - vec2<f32>(1.0, 1.0);

    var acc = 0.0;
    for (var j = 0; j < 2; j++) {
        for (var i = 0; i < 2; i++) {
            let q = clamp(base + vec2<f32>(f32(i), f32(j)), vec2<f32>(0.0, 0.0), hi);
            let wx = select(1.0 - f.x, f.x, i == 1);
            let wy = select(1.0 - f.y, f.y, j == 1);
            acc += textureLoad(wall_prox, vec2<i32>(i32(q.x), i32(q.y)), 0).r * wx * wy;
        }
    }
    return acc;
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

// What an agent's sensor actually perceives at `p`: the scent trail, pulled toward food and the damp
// shelter of walls, pushed away from gaze, footsteps, and solid rock. All gains are in trail units so they
// compete directly with scent.
//
// This single expression is where the "sentience" lives — foraging (chemo), nestling into corners
// (wall_affinity), photophobia + habituation (light, already habituation-attenuated on the CPU), flinching
// from the squad (disturbance), and shying from the rock (wall_repel).
//
// `wall_repel` is a STEERING term, not a movement guard, and the two are not interchangeable. Movement is
// hard-blocked in `agent_step`, but a sensor reaches `sense_dist` (≈1.7 cells) ahead — well into rock the
// agent will never occupy. `wall_repel` is what lets an agent *anticipate* a wall and turn early instead of
// driving into the face and relying on the collision bounce. Without it agents pile against every wall.
//
// `wall_affinity` is its counterpart in floor space: it pulls agents toward the floor *beside* a wall. The
// pair is what parks the mold hard against the wall face — dark, sheltered, damp — without cramming it there.
//
// Note the trail term is self-limiting at walls now: with no-flux boundaries (see `diffuse`/`field`) rock
// texels hold exactly zero trail, so rock is intrinsically scentless as well as explicitly repellent.
fn sense(p: vec2<f32>, dim: i32) -> f32 {
    let ctl = control_at(p, dim);
    let attract = ctl.r * params.chemo_gain
                + wall_prox_at(p, dim) * params.wall_affinity;
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

    // ── Step forward, confined to the floor ──────────────────────────────────────────────────────────
    // Agents are HARD-BLOCKED from rock. Seeding places every agent on floor (see `agents.rs`), and this
    // rule preserves that, so "an agent stands on walkable floor" is an invariant by induction — which is
    // why nothing downstream needs a walkability guard.
    //
    // The two axes resolve SEQUENTIALLY, not in parallel. Testing both against the *starting* cell tunnels
    // through concave corners: with N and E floor but NE rock, each axis test passes on its own while the
    // combined diagonal step lands in the rock. Committing X first and testing Z from the post-X position
    // tests the true destination, and yields wall-sliding for free.
    //
    // `step_size` is 1 texel ≈ 0.19 cells, so a step can never skip a cell boundary — one test per axis is
    // sufficient. Out-of-range positions are non-floor (`texel_walkable`), so the world border is a wall and
    // the old toroidal wrap — which could teleport an agent across the map — is gone.
    let delta = vec2<f32>(cos(heading), sin(heading)) * params.step_size;
    var pos = a.pos;
    var blocked_x = false;
    var blocked_y = false;

    let try_x = vec2<f32>(pos.x + delta.x, pos.y);
    if (pos_walkable(try_x, dim)) { pos = try_x; } else { blocked_x = true; }

    let try_y = vec2<f32>(pos.x, pos.y + delta.y);
    if (pos_walkable(try_y, dim)) { pos = try_y; } else { blocked_y = true; }

    // Collision response. One axis blocked = slide along the wall: the mold creeps along the surface it
    // touched, which is thigmotropism (contact guidance) and is how real hyphae follow topography —
    // Perera et al. (1997), "Contact-sensing by hyphae of dermatophytic and saprophytic fungi",
    // 10.1080/02681219780001301. Both axes blocked = wedged in a corner, so take Jones' collision rule and
    // pick a fresh random heading; that is also what breaks corner-pinning oscillation.
    if (blocked_x && blocked_y) {
        heading = rand01(i * 2246822519u + u32(params.time * 60.0)) * TAU;
    }

    a.pos = pos;
    a.heading = heading;
    agents[i] = a;

    // Deposit fixed-point scent at the new texel. No walkability guard: `pos` is floor by the invariant
    // above, and a redundant second check would be a silent second execution path.
    let dx = i32(floor(pos.x));
    let dy = i32(floor(pos.y));
    let idx = u32(dy) * u32(dim) + u32(dx);
    atomicAdd(&deposit[idx], u32(params.deposit_amount * params.deposit_scale));
}

// ── Pass 3: diffuse + decay the trail, fold in deposits ─────────────────────────────────────────────
@compute @workgroup_size(8, 8, 1)
fn diffuse(@builtin(global_invocation_id) id: vec3<u32>) {
    let dim = i32(params.field_res.x);
    if (i32(id.x) >= dim || i32(id.y) >= dim) {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);

    // Rock carries no scent. Storing zero (rather than letting trail bleed in and rot) is what makes rock
    // intrinsically unattractive to `sense()`, and it is the read side of the no-flux boundary below.
    if (texel_walkable(x, y, dim) < 0.5) {
        textureStore(trail_write, vec2<i32>(x, y), vec4<f32>(0.0, 0.0, 0.0, 1.0));
        return;
    }

    // 3x3 mean of the read trail (Jenson/Lague mean filter), restricted to floor.
    //
    // This is an AVERAGING filter, so masked neighbours must be dropped *and the divisor reduced to match*.
    // Dividing by a fixed 9 while summing only the floor neighbours would bias every wall-adjacent texel
    // toward zero — the very drain we are removing. Contrast the Gray-Scott Laplacian in `field`, which is a
    // flux operator and must NOT be renormalised. Same masking, opposite correction.
    var sum = 0.0;
    var wsum = 0.0;
    for (var oy = -1; oy <= 1; oy++) {
        for (var ox = -1; ox <= 1; ox++) {
            let nx = x + ox;
            let ny = y + oy;
            if (texel_walkable(nx, ny, dim) > 0.5) {
                sum += textureLoad(trail_read, vec2<i32>(nx, ny), 0).r;
                wsum += 1.0;
            }
        }
    }
    // `wsum >= 1` always: the centre texel is walkable (we returned above otherwise).
    let blur = sum / wsum;
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

// ── Pass 4: Gray-Scott biomass step + this tick's field snapshot ─────────────────────────────────────
// U (substrate) and V (biomass) diffuse at unequal rates and react via the autocatalytic U + 2V -> 3V.
// Strong veins nucleate V beneath them, so the blooms grow *along* the transport network. The 9-point
// Laplacian stencil (orthogonal 0.2, diagonal 0.05, centre -1) is the standard discretization.
//
// Refs: Turk (1991) SIGGRAPH 10.1145/122718.122749 (RD as surface texture synthesis); Pearson (1993),
// Science 261 (the (F,k) regime map); Leppänen et al. 10.1590/S0103-97332004000300006.

fn bio_at(x: i32, y: i32, dim: i32) -> vec2<f32> {
    return textureLoad(biomass_read, vec2<i32>(wrap_i(x, dim), wrap_i(y, dim)), 0).rg;
}

// One neighbour's contribution to the no-flux (Neumann) Laplacian, in FLUX form: `w * (u_k - u_c)`.
//
// A masked (rock) neighbour contributes exactly zero. That is the discrete ghost-node mirror condition
// `u_ghost := u_c` ⇒ zero gradient ⇒ zero flux through that face — mold piles up against the wall instead
// of leaking through it.
//
// This must NOT be renormalised back to `sum(w) == 1`. The previous form, `sum(w*u_k) - u_c`, is
// algebraically identical in the interior (the eight weights sum to 1) but at a wall it keeps the full
// `-u_c` while losing the neighbour's positive term — a Dirichlet `u -> 0` node. Rock was an absorbing sink
// that drained the mold near every wall. Renormalising the surviving weights would be a different bug: it
// would speed tangential diffusion to compensate for the blocked normal direction. Lowering the effective
// diffusivity next to a wall is the physically correct consequence of a smaller domain.
//
// The Turing condition `d_v < d_u` survives: U and V share this mask, so both diffusivities scale by the
// same near-wall factor and the ratio is preserved.
fn bio_flux(x: i32, y: i32, dim: i32, w: f32, c: vec2<f32>) -> vec2<f32> {
    if (texel_walkable(x, y, dim) < 0.5) {
        return vec2<f32>(0.0, 0.0);
    }
    return (bio_at(x, y, dim) - c) * w;
}

@compute @workgroup_size(8, 8, 1)
fn field(@builtin(global_invocation_id) id: vec3<u32>) {
    let dim = i32(params.field_res.x);
    if (i32(id.x) >= dim || i32(id.y) >= dim) {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);

    // Nothing grows in rock. Zeroing here (instead of letting the reaction term drift `u -> 1` in cells
    // nothing ever reads) keeps the field honest and matches the trail's treatment in `diffuse`.
    if (texel_walkable(x, y, dim) < 0.5) {
        textureStore(biomass_write, vec2<i32>(x, y), vec4<f32>(0.0, 0.0, 0.0, 1.0));
        textureStore(snap_write, vec2<i32>(x, y), vec4<f32>(0.0, 0.0, 0.0, 0.0));
        return;
    }

    let c = bio_at(x, y, dim);
    var u = c.x;
    var v = c.y;

    // 9-point no-flux Laplacian of (U, V): orthogonal 0.2, diagonal 0.05. See `bio_flux`.
    var lap = bio_flux(x - 1, y, dim, 0.2, c) + bio_flux(x + 1, y, dim, 0.2, c)
            + bio_flux(x, y - 1, dim, 0.2, c) + bio_flux(x, y + 1, dim, 0.2, c);
    lap += bio_flux(x - 1, y - 1, dim, 0.05, c) + bio_flux(x + 1, y - 1, dim, 0.05, c)
         + bio_flux(x - 1, y + 1, dim, 0.05, c) + bio_flux(x + 1, y + 1, dim, 0.05, c);

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

    // Carrion is FOOD, not merely a scent. Blood and nests only steer agents (`chemo_gain`); meat nucleates
    // flesh directly, without waiting for a vein to establish, so a fresh gib erupts into biomass.
    v += ctl.r * params.carrion_bloom * params.dt;

    u = clamp(u, 0.0, 1.0);
    v = clamp(v, 0.0, 1.0);
    textureStore(biomass_write, vec2<i32>(x, y), vec4<f32>(u, v, 0.0, 1.0));

    // Grimy-bioluminescent composite. Sickly green/cyan veins glow out of a dark wet biomass film; a faint
    // scent sheen hints at growth even where no vein has established. Alpha stays < 1 throughout so the
    // mold reads as a translucent coating over the carpet, not opaque paint.
    // The snapshot carries raw SIMULATION FIELDS, not a colour. Shading (lighting, normals, wetness, glow) is
    // the material's job — that separation is what lets the mold be a lit PBR surface rather than a flat
    // decal, and lets the wall material reuse the exact same field.
    //
    //   R = trail (raw, 0..trail_max)   G = biomass V (0..1)
    //   B = wall contact (0..1)         A = unused (see below)
    //
    // `A` once carried coverage — the explored/visible fog mask, baked in from `control.a`. It no longer
    // does. This texture is only rewritten on a sim tick, so a fog state routed through it reached the screen
    // a whole tick period after the fog itself, and the mat visibly arrived *after* the floor tile beneath it.
    // The materials now read `control.a` directly, which `write_control` rewrites every frame. Nothing samples
    // this channel; it is written as 1.0 rather than repurposed, because a second meaning for one channel is
    // exactly how the first one got lost.
    //
    // This is a per-tick SNAPSHOT, not the texture the materials sample: `blend` interpolates the two most
    // recent snapshots into `display` every rendered frame. Writing straight to `display` made the mold's
    // rendered contour hop once per period.
    let contact = wall_prox_at(vec2<f32>(f32(x), f32(y)), dim);
    textureStore(snap_write, vec2<i32>(x, y), vec4<f32>(trail, v, contact, 1.0));
}

// ── Pass 5: reduce the biomass field for the CPU ─────────────────────────────────────────────────────
//
// The mold's only reading back to gameplay. Each thread owns one coarse cell, max-pools the biomass `V`
// over its `field_res / coarse_res` block, and reports the winning texel's `(V, U)` together with that
// texel's exact field coordinates. So the *search* is coarse (1.5 world units per cell at 1024²→128²) while
// the *answer* is at full field precision (0.19 world units) — a mushroom lands where the mat is actually
// thickest, not at a cell centre.
//
// One thread per output slot: no atomics, no clear pass, and a deterministic readback order. An
// `atomicAdd`-appended candidate list would have given neither.
//
// Reads `biomass_read`, which is this tick's *source* field (the `field` pass writes `biomass_write`), so
// the reduction trails the reaction by exactly one tick. At `sim_hz` that is a fraction of a second, and it
// avoids a read-after-write hazard on the same texture.
//
// Rock is skipped, so a block that is entirely wall reports `V = 0` and the CPU's fruiting conjunction
// (thick mat AND spent substrate) rejects it on the `V` term. Nothing fruits inside a wall.
@compute @workgroup_size(8, 8, 1)
fn pin_scan(@builtin(global_invocation_id) id: vec3<u32>) {
    let cres = i32(params.coarse_res);
    if (i32(id.x) >= cres || i32(id.y) >= cres) {
        return;
    }
    let dim = i32(params.field_res.x);
    let block = dim / cres;
    let x0 = i32(id.x) * block;
    let y0 = i32(id.y) * block;

    var best_v = 0.0;
    var best_u = 0.0;
    var best_x = f32(x0);
    var best_y = f32(y0);

    for (var j = 0; j < block; j++) {
        for (var i = 0; i < block; i++) {
            let x = x0 + i;
            let y = y0 + j;
            if (texel_walkable(x, y, dim) < 0.5) {
                continue;
            }
            let b = bio_at(x, y, dim);
            if (b.y > best_v) {
                best_v = b.y;
                best_u = b.x;
                best_x = f32(x);
                best_y = f32(y);
            }
        }
    }

    coarse[u32(id.y) * params.coarse_res + u32(id.x)] = vec4<f32>(best_v, best_u, best_x, best_y);
}
