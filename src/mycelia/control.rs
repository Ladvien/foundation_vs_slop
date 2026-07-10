//! The mold's senses — a small CPU-written control texture (one texel per dungeon cell) that the compute
//! chain reads to steer on world state. This is the ONLY channel by which the world reaches the mold, and
//! it is strictly one-way: nothing here reads mold state back into gameplay (see the `mod.rs` firewall).
//!
//! # Channels (`Rgba8Unorm`)
//! | Ch | Meaning | Source |
//! |----|---------|--------|
//! | `R` | chemoattractant | blood, nests, and **meat chunks** — carnage to forage on and feed from |
//! | `G` | light / gaze repellent | cells a squad unit currently sees, attenuated by habituation, rate-limited |
//! | `B` | disturbance repellent | squad unit proximity — footsteps scatter the mold |
//! | `A` | substrate mask | `0` void · `0.33` floor never seen · `0.67` remembered · `1` visible |
//!
//! # Why `A` is four-state, not a bool
//! The mold *grows* on any floor, seen or not — a room you have never entered is exactly where it should be
//! ripest. But it must not be *drawn* on floor the player has never explored, or the coating would trace
//! the corridor layout straight through the fog of war and leak the map. And it must not be *lit* on floor
//! the squad cannot currently see, or a remembered room's mold glows through the dark while the floor under
//! it is dimmed. So growth keys off "is floor", rendering off "is explored", and brightness off "is
//! visible". One channel, three thresholds.
//!
//! # Habituation
//! `G` is not simply "is this cell watched". Each watched cell accumulates habituation and its repellent
//! fades; once the gaze leaves, habituation decays and the cell becomes frightening again. This gives the
//! mold a memory: a corridor the squad keeps staring down stops scaring it, and re-scares it after they
//! leave. Grounded in Boisseau, Vogel & Dussutour (2016), 10.1098/rspb.2016.0446 — *P. polycephalum*
//! habituates to a repeatedly-presented harmless repellent and shows spontaneous recovery when it is
//! withheld.
//!
//! # Why `G` is rate-limited
//! Habituation sets where `G` is *going*; [`perceptual::slew`] bounds how fast it may *get there*. The
//! shaders turn `G` into `conceal`, a 2.75x swing on the mat's vein glow, and habituation crosses its whole
//! range in ~3 s — so an unlimited `G` made the mat visibly pulse as the squad milled around a room, well
//! inside the band the eye is most sensitive to. The mold's glow is an *autonomous* signal and so is bound
//! by the slow-change window (`MIN_APPEARANCE_RAMP_SECS`); the fog reveal in `A` is *player-caused* and
//! stays instantaneous, because the mat must appear the moment its floor tile does.
//!
//! `G` therefore has two consumers and one shape: the shaders' `conceal`, and the compute chain's
//! `photophobia` steering plus its `dark` bloom term. Slewing the single signal means the agents' flight
//! from a gaze is now as unhurried as the glow's — which is what a fungus does anyway, and it keeps the two
//! from disagreeing about how brightly a cell is lit.

use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;

use crate::dungeon::Dungeon;
use crate::fog::FogGrid;
use crate::gore::{BloodPool, GibChunk};
use crate::nest::Nest;
use crate::squad::Unit;

use super::{field, perceptual, MyceliaConfig, CONTROL_SIZE};

/// Chemoattractant splat radius (cells) around a nest's floor position.
const NEST_RADIUS_CELLS: f32 = 2.5;
/// Disturbance splat radius (cells) around each squad unit.
const UNIT_RADIUS_CELLS: f32 = 2.0;
/// Floor on a blood pool's splat radius (cells), so even a small stain is smelled.
const BLOOD_MIN_RADIUS_CELLS: f32 = 1.0;
/// Chemoattractant splat radius (cells) around a meat chunk. Tight: a gib is a point source of food, and a
/// wide splat would smear the bloom instead of erupting from the chunk itself.
const MEAT_RADIUS_CELLS: f32 = 1.6;

/// The control texture handles, extracted so the render world can bind them.
#[derive(Resource, Clone, ExtractResource)]
pub struct MoldControlImage {
    /// Rewritten every `Update` (chemo / light / disturbance / substrate).
    pub dynamic: Handle<Image>,
    /// Written **once**, when the dungeon first exists. At **field** resolution, not cell resolution:
    /// `R` = wall proximity (1 hard against a wall surface, falling to 0 over `wall_reach` world units).
    /// The dungeon never regenerates, so this never changes.
    pub wall: Handle<Image>,
}

/// Main-world scratch: the CPU-side pixel buffers we rewrite each `Update`, plus the two per-cell
/// accumulators (which are *state*, so they must persist between frames).
#[derive(Resource)]
pub struct MoldControl {
    dynamic: Handle<Image>,
    wall: Handle<Image>,
    cpu: Vec<u8>,
    habituation: Vec<f32>,
    /// The gaze signal actually written to `G`, rate-limited toward its instantaneous target so the mat's
    /// glow can never swing faster than the slow-change window. See [`write_control`].
    light: Vec<f32>,
    /// The static wall field is uploaded exactly once, the first `Update` that sees a `Dungeon`.
    wall_written: bool,
}

impl MoldControl {
    fn cells() -> usize {
        (CONTROL_SIZE * CONTROL_SIZE) as usize
    }
}

/// Create the control textures and their CPU mirrors. Runs once at `Startup`.
///
/// The two textures are deliberately different sizes: `dynamic` is one texel per dungeon cell (world state
/// is per-cell), while `wall` is at **field** resolution so the mold's contact ridge can land on the wall
/// surface rather than the cell centre.
pub(super) fn setup_control(
    mut commands: Commands,
    cfg: Res<MyceliaConfig>,
    mut images: ResMut<Assets<Image>>,
) {
    let dynamic = images.add(field::control_texture(CONTROL_SIZE));
    let wall = images.add(field::control_texture(cfg.field_size));
    commands.insert_resource(MoldControlImage { dynamic: dynamic.clone(), wall: wall.clone() });
    commands.insert_resource(MoldControl {
        dynamic,
        wall,
        cpu: vec![0u8; MoldControl::cells() * field::CONTROL_BYTES_PER_TEXEL],
        habituation: vec![0.0; MoldControl::cells()],
        light: vec![0.0; MoldControl::cells()],
        wall_written: false,
    });
}

/// Stand-in for infinity in the distance transform. A true `f32::INFINITY` makes the parabola-intersection
/// arithmetic below evaluate `inf - inf` and produce `NaN`; a large finite value keeps it well-defined.
/// Far larger than any squared distance on a 1024² grid (max ≈ 2·1024² ≈ 2.1e6).
const DT_FAR: f32 = 1.0e20;

/// Exact 1-D squared distance transform of a sampled function, in place over `f` → `d`.
///
/// Felzenszwalb & Huttenlocher (2012), "Distance Transforms of Sampled Functions", *Theory of Computing*
/// 8(19):415–428, doi:10.4086/toc.2012.v008a019. O(n): sweeps the lower envelope of the parabolas rooted at
/// each sample. `v` and `z` are caller-owned scratch (length `n` and `n+1`) so the 2-D driver can reuse them
/// across every row and column instead of reallocating 2048 times.
fn dt_1d(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    // Index of the rightmost parabola in the lower envelope so far.
    let mut k: usize = 0;
    v[0] = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;

    for q in 1..n {
        let qf = q as f32;
        // Intersection of the parabola from `q` with the one currently rightmost. Walk left while `q`'s
        // parabola dominates. `z[0]` is -inf and `s` is always finite, so `k` can never underflow past 0.
        let mut s;
        loop {
            let vk = v[k] as f32;
            s = ((f[q] + qf * qf) - (f[v[k]] + vk * vk)) / (2.0 * qf - 2.0 * vk);
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }

    k = 0;
    for (q, slot) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let delta = q as f32 - v[k] as f32;
        *slot = delta * delta + f[v[k]];
    }
}

/// Is the FIELD texel at (`tx`, `ty`) inside solid matter — rock, or the band a wall slab occupies?
fn texel_is_solid(dungeon: &Dungeon, tx: u32, ty: u32, texel_world: f32) -> bool {
    // World XZ of this texel's centre. `super::WORLD_ORIGIN` is the world position of the field's corner.
    let wx = super::WORLD_ORIGIN.x + (tx as f32 + 0.5) * texel_world;
    let wz = super::WORLD_ORIGIN.y + (ty as f32 + 0.5) * texel_world;
    solid_at_world(dungeon, Vec2::new(wx, wz))
}

/// Is this world XZ point inside solid matter — rock, or the band a wall slab occupies?
///
/// Walls are thin cuboids standing on cell *edges*, inset so their outer face is flush with the boundary.
/// Testing that band (not merely the cell boundary) is what puts the mold's contact ridge on the surface
/// the player actually sees — and, for `fruit::wall_escape`, what lets a mushroom know exactly how far the
/// slab it must lean away from actually protrudes.
pub(super) fn solid_at_world(dungeon: &Dungeon, p: Vec2) -> bool {
    // Cells are centred on integers and span ±0.5, so this maps a world point to its cell.
    let cell = IVec2::new((p.x + 0.5).floor() as i32, (p.y + 0.5).floor() as i32);
    if !dungeon.is_floor(cell) {
        return true;
    }

    // Offset within the cell, in [-0.5, 0.5).
    let lx = p.x - cell.x as f32;
    let lz = p.y - cell.y as f32;
    let band = 0.5 - crate::dungeon::WALL_THICKNESS;

    // A wall stands on an edge exactly when the neighbour across it is not floor.
    let walled = |dx: i32, dz: i32| !dungeon.is_floor(cell + IVec2::new(dx, dz));
    (lx >= band && walled(1, 0))
        || (lx <= -band && walled(-1, 0))
        || (lz >= band && walled(0, 1))
        || (lz <= -band && walled(0, -1))
}

/// Compute, once, how close each **field texel** sits to a wall surface.
///
/// Previously this ran a cell-resolution BFS, so `wall_prox` was uniform across a whole wall-adjacent cell;
/// bilinearly sampled, its ridge landed at the *cell centre* — half a tile away from the wall. That reads as
/// a stripe hovering beside the wall rather than mold hugging it. At field resolution (≈5.3 texels per tile)
/// the ridge sits on the slab face.
///
/// Written as `R` bytes: `1.0` immediately against a wall surface, falling linearly to `0.0` at `wall_reach`
/// **world units** away. Solid texels are `0` — rock is not somewhere the mold pools.
fn compute_wall_proximity(dungeon: &Dungeon, field_size: u32, wall_reach: f32, cpu: &mut [u8]) {
    let n = (field_size * field_size) as usize;
    let texel_world = super::WORLD_EXTENT.x / field_size as f32;

    // Seed: 0 inside solids, "infinitely far" everywhere else.
    let mut sq: Vec<f32> = Vec::with_capacity(n);
    for ty in 0..field_size {
        for tx in 0..field_size {
            sq.push(if texel_is_solid(dungeon, tx, ty, texel_world) { 0.0 } else { DT_FAR });
        }
    }

    let dim = field_size as usize;
    let mut col = vec![0.0f32; dim];
    let mut out = vec![0.0f32; dim];
    let mut v = vec![0usize; dim];
    let mut z = vec![0.0f32; dim + 1];

    // Separable exact EDT: transform down every column, then across every row.
    for x in 0..dim {
        for (y, slot) in col.iter_mut().enumerate() {
            *slot = sq[y * dim + x];
        }
        dt_1d(&col, &mut out, &mut v, &mut z);
        for (y, &d) in out.iter().enumerate() {
            sq[y * dim + x] = d;
        }
    }
    for y in 0..dim {
        let row = &sq[y * dim..(y + 1) * dim];
        col.copy_from_slice(row);
        dt_1d(&col, &mut out, &mut v, &mut z);
        sq[y * dim..(y + 1) * dim].copy_from_slice(&out);
    }

    // Squared texel distance → world distance → proximity ramp.
    //
    // The transform measures centre-to-centre, so the free texel hard against a wall reports a full texel of
    // separation. Half a texel of that is the solid texel's own radius: subtracting it turns "distance to
    // the nearest solid sample" into "distance to the solid's surface", which is what the shading wants.
    // Solid texels themselves have `d2 == 0`, so they clamp to 0 and stay distinguishable — rock is not
    // somewhere the mold pools.
    let reach = wall_reach.max(texel_world);
    for (i, &d2) in sq.iter().enumerate() {
        let dist = (d2.max(0.0).sqrt() - 0.5).max(0.0) * texel_world;
        let prox = if dist <= 0.0 { 0.0 } else { (1.0 - dist / reach).clamp(0.0, 1.0) };
        cpu[i * field::CONTROL_BYTES_PER_TEXEL] = to_u8(prox);
    }
}

/// Rasterize world state into the control texture, once per `Update`.
///
/// Cosmetic and read-only with respect to gameplay: it queries `Transform`s and the fog/dungeon grids and
/// mutates nothing but its own buffers.
///
/// Runs on `Update` and re-uploads **every frame**, whatever the clock is doing: the substrate mask in `A` is
/// the fog reveal, and it must land on the same frame the floor tile swaps. Only the *accumulators* —
/// habituation, and the gaze slew below — read `Time<Virtual>`, so they scale with `GameSpeed` and freeze
/// with a pause, exactly as the growth they modulate does.
pub(super) fn write_control(
    mut control: ResMut<MoldControl>,
    cfg: Res<MyceliaConfig>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time<Virtual>>,
    dungeon: Option<Res<Dungeon>>,
    fog: Option<Res<FogGrid>>,
    pools: Query<&Transform, With<BloodPool>>,
    gibs: Query<&Transform, With<GibChunk>>,
    nests: Query<&Nest>,
    units: Query<&Transform, With<Unit>>,
) {
    // The dungeon is the substrate; without it there is nothing to coat. (Menu//loading frames.)
    let Some(dungeon) = dungeon else {
        return;
    };
    let dt = time.delta_secs();
    let size = CONTROL_SIZE as i32;

    // Split the borrow so we can read `cpu` while mutating `habituation` (and vice versa).
    let MoldControl { dynamic, wall, cpu, habituation, light, wall_written } = &mut *control;

    // ── Once: the static wall-proximity field ─────────────────────────────────────────────────────────
    // The dungeon is generated once and never regenerates, so this is computed and uploaded a single time.
    if !*wall_written {
        let texels = (cfg.field_size as usize) * (cfg.field_size as usize);
        let mut wall_cpu = vec![0u8; texels * field::CONTROL_BYTES_PER_TEXEL];
        compute_wall_proximity(&dungeon, cfg.field_size, cfg.wall_reach, &mut wall_cpu);
        if let Some(mut gpu) = images.get_mut(&*wall) {
            gpu.data = Some(wall_cpu);
            *wall_written = true;
        }
    }

    // ── Pass 1: per-cell fog (light + habituation) and the walkable mask ──────────────────────────────
    for y in 0..size {
        for x in 0..size {
            let cell = IVec2::new(x, y);
            let i = (y * size + x) as usize;

            let watched = fog.as_ref().is_some_and(|f| f.visible_at(cell));
            let hab = &mut habituation[i];
            if watched {
                *hab = (*hab + cfg.hab_rate * dt).min(1.0);
            } else {
                *hab = (*hab - cfg.hab_recover * dt).max(0.0);
            }

            // Only a *currently seen* cell repels. Explored-but-dark and never-seen cells are equally safe
            // — which is exactly the ambience we want: the mold blooms wherever nobody is looking.
            let target = if watched { 1.0 - *hab * cfg.hab_strength } else { 0.0 };

            // ...but the mat may not *reach* that target any faster than a human can notice a luminance
            // change. `G` drives `conceal` in the floor/wall/fruit shaders, a 2.75x swing on the vein glow;
            // written instantaneously it flickered visibly as the squad milled about a room, because
            // `hab_rate` crosses the whole range in ~3 s and a cell flips `watched` the moment a unit
            // turns. Rate-limiting it here — rather than in three shaders — keeps the gaze a single CPU
            // signal, and one that `photophobia` (agent flight) and the `dark` bloom term already share.
            //
            // This is the mold's *autonomous* half of the signal, so it is bound by the slow-change window
            // (Simons, Franconeri & Reimer 2000). Fog reveal is NOT: that is caused by the player walking
            // into a room, and the mat must appear the instant its floor tile does. See `A` below.
            light[i] = perceptual::slew(light[i], target, dt, perceptual::MIN_APPEARANCE_RAMP_SECS);

            // A: three-state substrate mask. Agents grow on any floor; only *explored* floor is drawn.
            // Four-state substrate. Growth keys off "is floor"; RENDERING keys off explored; and the
            // mold's brightness keys off *currently visible*, so a remembered room's mold dims exactly like
            // the floor under it instead of glowing at full strength through the fog.
            let substrate = if !dungeon.is_floor(cell) {
                0
            } else if fog.as_ref().is_some_and(|f| f.visible_at(cell)) {
                255
            } else if fog.as_ref().is_none_or(|f| f.seen_at(cell)) {
                170
            } else {
                85
            };

            let base = i * field::CONTROL_BYTES_PER_TEXEL;
            cpu[base] = 0; // R: chemo — accumulated in pass 2
            cpu[base + 1] = to_u8(light[i]); // G: light/gaze, rate-limited above
            cpu[base + 2] = 0; // B: disturbance — accumulated in pass 2
            cpu[base + 3] = substrate; // A: 0 void / 85 unseen / 170 remembered / 255 visible
        }
    }

    // ── Pass 2: splat the point sources ───────────────────────────────────────────────────────────────
    // Blood pools and nests attract (R); squad units disturb (B).
    for t in &pools {
        // A pool's footprint lives only in its Transform scale (there is no radius field); the quad spans
        // [-1,1] in local space, so the half-extent in world units is `scale * 0.5`.
        let radius = (t.scale.x * 0.5).max(BLOOD_MIN_RADIUS_CELLS);
        splat(cpu, dungeon.world_to_cell(t.translation), radius, 0);
    }
    for nest in &nests {
        splat(cpu, dungeon.world_to_cell(nest.pos), NEST_RADIUS_CELLS, 0);
    }
    // Meat chunks are the mold's food, not merely its scent. The `R` splat both steers agents here (via
    // `chemo_gain`) and nucleates biomass directly (via `carrion_bloom` in the `field` pass), so a fresh gib
    // erupts. Gibs tumble and settle in 3D; splat at their resting XZ.
    for t in &gibs {
        splat(cpu, dungeon.world_to_cell(t.translation), MEAT_RADIUS_CELLS, 0);
    }
    for t in &units {
        splat(cpu, dungeon.world_to_cell(t.translation), UNIT_RADIUS_CELLS, 2);
    }

    // ── Upload ────────────────────────────────────────────────────────────────────────────────────────
    // Mutating through `Assets<Image>` marks the asset changed, so Bevy re-uploads it to the GPU.
    // `get_mut` hands back an `AssetMut` guard; touching it through `DerefMut` is what flags the asset as
    // changed, which is precisely what triggers Bevy's re-upload of the texture to the GPU.
    let Some(mut gpu_image) = images.get_mut(&*dynamic) else {
        return;
    };
    match gpu_image.data.as_mut() {
        Some(data) if data.len() == cpu.len() => data.copy_from_slice(cpu),
        _ => gpu_image.data = Some(cpu.clone()),
    }
}

/// Additively splat a radial falloff into one channel of the control buffer, saturating at 255.
fn splat(cpu: &mut [u8], center: IVec2, radius_cells: f32, channel: usize) {
    let size = CONTROL_SIZE as i32;
    let r = radius_cells.max(0.5);
    let ri = r.ceil() as i32;
    for dy in -ri..=ri {
        for dx in -ri..=ri {
            let cell = center + IVec2::new(dx, dy);
            if cell.x < 0 || cell.y < 0 || cell.x >= size || cell.y >= size {
                continue;
            }
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            if dist > r {
                continue;
            }
            // Smooth falloff to the rim so the gradient is sensible to steer on.
            let strength = 1.0 - (dist / r);
            let idx = (cell.y * size + cell.x) as usize * field::CONTROL_BYTES_PER_TEXEL + channel;
            cpu[idx] = cpu[idx].saturating_add(to_u8(strength));
        }
    }
}

/// Map a `0..=1` strength to a `Rgba8Unorm` byte, clamping out-of-range inputs.
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dungeon must be `CONTROL_SIZE`-square for the world↔cell mapping to hold; carve a floor block
    /// out of solid rock.
    fn dungeon_with_floor_block(lo: i32, hi: i32) -> Dungeon {
        let size = CONTROL_SIZE as usize;
        let mut walkable = vec![false; size * size];
        for y in lo..=hi {
            for x in lo..=hi {
                walkable[y as usize * size + x as usize] = true;
            }
        }
        Dungeon::from_walkable(size, size, walkable)
    }

    /// Read back the `R` byte of field texel (tx, ty) as a `0..=1` proximity.
    fn prox_at(cpu: &[u8], field_size: u32, tx: u32, ty: u32) -> f32 {
        let i = (ty * field_size + tx) as usize;
        f32::from(cpu[i * field::CONTROL_BYTES_PER_TEXEL]) / 255.0
    }

    /// Textbook check of the 1-D transform: distance-squared from the single zero at index 0.
    #[test]
    fn dt_1d_matches_squared_distance() {
        let f = vec![0.0, DT_FAR, DT_FAR, DT_FAR];
        let mut d = vec![0.0; 4];
        let mut v = vec![0usize; 4];
        let mut z = vec![0.0f32; 5];
        dt_1d(&f, &mut d, &mut v, &mut z);
        assert_eq!(d, vec![0.0, 1.0, 4.0, 9.0]);
    }

    /// Two sources: every sample takes the nearer one.
    #[test]
    fn dt_1d_takes_the_nearest_source() {
        let f = vec![0.0, DT_FAR, DT_FAR, DT_FAR, 0.0];
        let mut d = vec![0.0; 5];
        let mut v = vec![0usize; 5];
        let mut z = vec![0.0f32; 6];
        dt_1d(&f, &mut d, &mut v, &mut z);
        assert_eq!(d, vec![0.0, 1.0, 4.0, 1.0, 0.0]);
    }

    /// Rock is never a place the mold pools, and the deep interior of a room is out of the wall's reach.
    #[test]
    fn wall_proximity_is_zero_in_rock_and_in_open_floor() {
        let field_size = 768; // 4 texels per tile
        let dungeon = dungeon_with_floor_block(90, 100);
        let mut cpu = vec![0u8; (field_size * field_size) as usize * field::CONTROL_BYTES_PER_TEXEL];
        compute_wall_proximity(&dungeon, field_size, 0.6, &mut cpu);

        // Texel deep inside rock (cell ~40).
        assert_eq!(prox_at(&cpu, field_size, 160, 160), 0.0, "rock must not attract mold");
        // Texel at the centre of the 11-tile room (cell 95), ~5 tiles from any wall.
        assert_eq!(prox_at(&cpu, field_size, 382, 382), 0.0, "open floor is out of reach");
    }

    /// The ridge must sit *against the wall*, and fall off monotonically into the room.
    #[test]
    fn wall_proximity_peaks_against_the_wall_and_decays_inward() {
        let field_size = 768;
        let dungeon = dungeon_with_floor_block(90, 100);
        let mut cpu = vec![0u8; (field_size * field_size) as usize * field::CONTROL_BYTES_PER_TEXEL];
        compute_wall_proximity(&dungeon, field_size, 0.6, &mut cpu);

        // Walk east along the middle row of the room, starting from the west wall.
        // Cell 90 spans world x in [89.5, 90.5]; texel centres are 0.25 apart.
        let row = 382; // a texel row inside cell 95
        let series: Vec<f32> = (359..=366).map(|tx| prox_at(&cpu, field_size, tx, row)).collect();

        // 359 is rock, 360 lies inside the 0.14-thick slab band — both solid, so both zero.
        assert_eq!(series[0], 0.0, "rock texel");
        assert_eq!(series[1], 0.0, "texel inside the wall slab");

        // The first free texel is hard against the slab: proximity must be near its maximum.
        assert!(series[2] > 0.7, "first free texel should hug the wall, got {}", series[2]);

        // ...and it must decay monotonically inward from there.
        for w in series[2..].windows(2) {
            assert!(w[0] >= w[1], "proximity must not increase away from the wall: {series:?}");
        }
        assert_eq!(*series.last().unwrap_or(&1.0), 0.0, "reach must run out: {series:?}");
    }

    /// A room one tile wider has its ridge one tile further east — i.e. the ridge tracks the *surface*,
    /// not the cell grid. This is the regression the cell-resolution BFS could not express.
    #[test]
    fn the_ridge_tracks_the_wall_surface_not_the_cell_centre() {
        let field_size = 768;
        let mut cpu = vec![0u8; (field_size * field_size) as usize * field::CONTROL_BYTES_PER_TEXEL];
        compute_wall_proximity(&dungeon_with_floor_block(90, 100), field_size, 0.6, &mut cpu);

        // Within cell 90 (world x in [89.5, 90.5] -> texels 360..=363), proximity must be strictly
        // greatest at the texel nearest the west slab, not at the cell's centre texel (361/362).
        let row = 382;
        let west = prox_at(&cpu, field_size, 361, row); // nearest free texel to the slab
        let centre = prox_at(&cpu, field_size, 362, row); // cell interior
        assert!(west > centre, "ridge sits at the wall face: west={west} centre={centre}");
    }
}
