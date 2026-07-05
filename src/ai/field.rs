//! Stigmergy substrate — decaying scalar influence fields agents **write and read**, so creatures
//! coordinate *through the environment* rather than by messaging each other (Holland & Melhuish,
//! "Stigmergy, self-organization, and sorting in collective robotics", 1999; Tang, Liu & Pan, ACO
//! review, IEEE/CAA JAS 2021 — deposit + evaporation + positive feedback). Each channel is a scalar
//! grid over the dungeon cells; the standard influence-map operations are **placement** (deposit),
//! **diffusion** (blur to neighbours), and **query** (sample/gradient) — Lewis, "Escaping the Grid",
//! Game AI Pro 2 Ch.29. The field is computed once and shared by every agent (Mark, "Modular Tactical
//! Influence Maps", Ch.30), which is where emergent *group* behaviour comes from.
//!
//! Extensibility: a channel is an index newtype ([`FieldId`]) over a fixed-width array — add a channel
//! by adding a const + bumping [`CHANNEL_COUNT`] + one tuning entry. Deposits are decoupled through a
//! [`StigDeposits`] queue (the project's `GoreQueue`/`ImpactQueue` idiom).

use bevy::prelude::*;

use crate::dungeon::Dungeon;

/// A stigmergy channel, addressed by a stable slot index. Extend by adding a const below.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FieldId(pub usize);

impl FieldId {
    /// Food/blood trail — creatures deposit as they feed/die; foragers climb its gradient.
    pub const SCENT: FieldId = FieldId(0);
    /// Danger — gunfire, the boss's aura, a unit's distress; drives fear and flight.
    pub const THREAT: FieldId = FieldId(1);
    /// Local creature density — recruitment/crowding substrate (positive feedback + dispersal).
    pub const CRAB_DENSITY: FieldId = FieldId(2);
    /// Meat trail — carryable gibs emit it; foraging crabs climb its gradient toward food.
    pub const MEAT: FieldId = FieldId(3);
}

/// Number of channels. Bump when adding a [`FieldId`].
pub const CHANNEL_COUNT: usize = 4;

/// SCENT deposited by a death — a strong, lingering feeding-site marker the swarm and boss home on.
pub const BLOOD_SCENT: f32 = 4.0;

/// Per-channel behaviour, filled from `ai_tuning.ron` at startup.
#[derive(Clone, Copy)]
pub struct ChannelDef {
    /// Fraction of value lost per second (ACO evaporation ρ).
    pub evaporate: f32,
    /// Blend weight [0,1] toward the 4-neighbour average each update (Ch.29 diffusion).
    pub diffuse: f32,
    /// World-unit radius a single deposit smears over (placement kernel).
    pub deposit_radius: f32,
}

impl Default for ChannelDef {
    fn default() -> Self {
        Self {
            evaporate: 0.4,
            diffuse: 0.1,
            deposit_radius: 1.5,
        }
    }
}

/// One deposit request; pushed by gameplay systems, drained into the grid by `drain_deposits`.
pub struct Deposit {
    pub pos: Vec3,
    pub field: FieldId,
    pub amount: f32,
}

/// Decoupling queue for field writes (mirrors `GoreQueue`). A single owner (`drain_deposits`) drains it.
#[derive(Resource, Default)]
pub struct StigDeposits(pub Vec<Deposit>);

/// The shared field grids. One `Vec<f32>` per channel over the (fixed) dungeon cell grid, row-major
/// `y*width + x` — the same indexing every other grid in the project uses.
#[derive(Resource)]
pub struct Stig {
    width: usize,
    height: usize,
    channels: [Vec<f32>; CHANNEL_COUNT],
    defs: [ChannelDef; CHANNEL_COUNT],
    /// Reused double-buffer for the diffusion pass (avoids per-frame allocation).
    scratch: Vec<f32>,
}

impl Stig {
    /// Allocate empty grids sized to the dungeon. `defs` come from tuning.
    pub fn new(dungeon: &Dungeon, defs: [ChannelDef; CHANNEL_COUNT]) -> Self {
        let cells = dungeon.width * dungeon.height;
        Self {
            width: dungeon.width,
            height: dungeon.height,
            channels: std::array::from_fn(|_| vec![0.0; cells]),
            defs,
            scratch: vec![0.0; cells],
        }
    }

    #[inline]
    fn index(&self, c: IVec2) -> usize {
        c.y as usize * self.width + c.x as usize
    }

    #[inline]
    fn in_grid(&self, c: IVec2) -> bool {
        c.x >= 0 && c.y >= 0 && (c.x as usize) < self.width && (c.y as usize) < self.height
    }

    /// Point read at a world position (query). Off-grid reads as 0. (Consumed by steering in Phase 3.)
    #[allow(dead_code)]
    pub fn sample(&self, field: FieldId, dungeon: &Dungeon, pos: Vec3) -> f32 {
        let c = dungeon.world_to_cell(pos);
        if self.in_grid(c) {
            self.channels[field.0][self.index(c)]
        } else {
            0.0
        }
    }

    /// Direction (world XZ) of *increasing* value, magnitude ≈ the local slope. Central differences on
    /// the 4-neighbour cells; `FollowGradient` uses `+`, `FleeGradient` uses `-`. (Used in Phase 3.)
    #[allow(dead_code)]
    pub fn gradient(&self, field: FieldId, dungeon: &Dungeon, pos: Vec3) -> Vec2 {
        let c = dungeon.world_to_cell(pos);
        let at = |dx: i32, dy: i32| -> f32 {
            let n = c + IVec2::new(dx, dy);
            if self.in_grid(n) {
                self.channels[field.0][self.index(n)]
            } else {
                0.0
            }
        };
        Vec2::new(at(1, 0) - at(-1, 0), at(0, 1) - at(0, -1))
    }

    /// Add `amount` at `pos`, smeared over the channel's `deposit_radius` with linear falloff. Only
    /// floor cells receive value (deposits don't bleed into rock).
    fn deposit(&mut self, field: FieldId, dungeon: &Dungeon, pos: Vec3, amount: f32) {
        let radius = self.defs[field.0].deposit_radius.max(0.0);
        let center = dungeon.world_to_cell(pos);
        let r = radius.ceil() as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                let cell = center + IVec2::new(dx, dy);
                if !self.in_grid(cell) || !dungeon.is_floor(cell) {
                    continue;
                }
                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                if dist > radius {
                    continue;
                }
                let falloff = if radius > 0.0 {
                    1.0 - dist / radius
                } else {
                    1.0
                };
                let i = self.index(cell);
                self.channels[field.0][i] += amount * falloff;
            }
        }
    }

    /// One evaporation + diffusion step for every channel (Ch.29 diffusion, ACO evaporation). `dt` in
    /// seconds. Diffusion blends only between floor cells so influence doesn't seep through walls.
    fn evaporate_diffuse(&mut self, dungeon: &Dungeon, dt: f32) {
        for ch in 0..CHANNEL_COUNT {
            let def = self.defs[ch];
            let retain = (1.0 - def.evaporate * dt).clamp(0.0, 1.0);
            let grid = &mut self.channels[ch];
            for v in grid.iter_mut() {
                *v *= retain;
            }
            if def.diffuse <= 0.0 {
                continue;
            }
            // Blend each floor cell toward the average of its floor neighbours (double-buffered).
            let w = self.width as i32;
            let h = self.height as i32;
            for y in 0..h {
                for x in 0..w {
                    let cell = IVec2::new(x, y);
                    let i = (y as usize) * self.width + x as usize;
                    if !dungeon.is_floor(cell) {
                        self.scratch[i] = grid[i];
                        continue;
                    }
                    let mut sum = 0.0;
                    let mut n = 0.0;
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let nb = IVec2::new(x + dx, y + dy);
                        if nb.x >= 0 && nb.y >= 0 && nb.x < w && nb.y < h && dungeon.is_floor(nb) {
                            sum += grid[(nb.y as usize) * self.width + nb.x as usize];
                            n += 1.0;
                        }
                    }
                    let avg = if n > 0.0 { sum / n } else { grid[i] };
                    self.scratch[i] = grid[i] * (1.0 - def.diffuse) + avg * def.diffuse;
                }
            }
            std::mem::swap(&mut self.channels[ch], &mut self.scratch);
        }
    }

    /// The peak `(cell, value)` in a channel — used by the boss's "drawn to the biggest frenzy" hunt
    /// and by diagnostics.
    pub fn hotspot(&self, field: FieldId, dungeon: &Dungeon) -> (Vec3, f32) {
        let grid = &self.channels[field.0];
        let mut best = 0.0f32;
        let mut best_cell = dungeon.spawn;
        for (i, &v) in grid.iter().enumerate() {
            if v > best {
                best = v;
                best_cell = IVec2::new((i % self.width) as i32, (i / self.width) as i32);
            }
        }
        (dungeon.cell_center(best_cell), best)
    }
}

/// Drain queued deposits into the grid (placement).
pub fn drain_deposits(mut stig: ResMut<Stig>, dungeon: Res<Dungeon>, mut deposits: ResMut<StigDeposits>) {
    for d in deposits.0.drain(..) {
        stig.deposit(d.field, &dungeon, d.pos, d.amount);
    }
}

/// Evaporate + diffuse every channel once per frame.
pub fn evaporate_diffuse(mut stig: ResMut<Stig>, dungeon: Res<Dungeon>, time: Res<Time>) {
    let dt = time.delta_secs();
    stig.evaporate_diffuse(&dungeon, dt);
}
