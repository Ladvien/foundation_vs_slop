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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FieldId(pub usize);

impl FieldId {
    /// Food/blood trail — creatures deposit as they feed/die; foragers climb its gradient.
    pub const SCENT: FieldId = FieldId(0);
    /// Danger **emitted by the squad's weapons** — a firing unit and the point its bolts land. Read by
    /// crabs and the boss so they scatter from a shooter. Deliberately NOT read by units: an agent that
    /// feared its own muzzle would flee from itself (this channel used to be a single undifferentiated
    /// `THREAT` that every `Drives` carrier tracked, which pinned a firing squad into `Mode::Flee`).
    pub const THREAT_GUN: FieldId = FieldId(1);
    /// Local creature density — recruitment/crowding substrate (positive feedback + dispersal).
    pub const CRAB_DENSITY: FieldId = FieldId(2);
    /// Meat trail — carryable gibs emit it; foraging crabs climb its gradient toward food.
    pub const MEAT: FieldId = FieldId(3);
    /// Alarm — a **wounded crab** floods this locally; nearby crabs read it and muster (converge on the
    /// squad) instead of fleeing. The nest floods this *same local* channel when hit (`nest::nest_alarm`):
    /// a nest hit → a stronger, wider bloom, a crab hit → a one-room bloom. Models alarm-pheromone
    /// recruitment to defense in social insects — a stigmergic "warning cry" (Heylighen, "Stigmergy as a
    /// universal coordination mechanism", Cognitive Systems Research 2016). Deposited by
    /// `crab::crab_alarm_on_damage`; read by the brain as `Fact::AlarmHere` (gates Muster on, Flee off).
    pub const ALARM: FieldId = FieldId(4);
    /// Danger **emitted by crabs** — the menace a swarm radiates. Read by units (never by crabs, which
    /// would otherwise fear the swarm they belong to). Kept distinct from [`Self::CRAB_DENSITY`]: density
    /// is the crabs' own *coordination* substrate (crowding → dispersal, recruitment), whereas this is a
    /// *fear* signal for the other faction, and the two want different radii and decay rates.
    pub const THREAT_CRAB: FieldId = FieldId(5);
    /// Danger **emitted by the watcher** — its standing anomaly aura, deposited every tick while it lives.
    /// Read by units; it is what the Psionic's field-sight renders and what `PsiScan` reacts to.
    pub const THREAT_ANOMALY: FieldId = FieldId(6);

    // --- Acoustic stimulus channels: sound as a perception field (not a one-way cosmetic output). The
    // gameplay sites that emit an `audio::Sfx` also deposit into these, so the *audible din* of a fight
    // propagates through the dungeon and creatures react to it. Faction-partitioned exactly like
    // THREAT_GUN vs THREAT_CRAB, so the "nothing fears a channel it emits" invariant holds by
    // construction. Propagation/salience/perception knobs live in the `audio:` config slice
    // (`crate::audio_tuning::AudioTuning`) so the offline audio search can evolve them. Deliberately
    // NOT in [`UNIT_THREAT_CHANNELS`]: audible din is a distinct category from creature menace, and the
    // Psionic's field-sight should render dread from monsters, not the squad's own muzzle echoes.

    /// Audible din **emitted by the squad** — muzzle fire, bolt impacts, a unit's death. Read by crabs
    /// (fear and/or investigate), never by units. Kept distinct from [`Self::THREAT_GUN`]: same emit
    /// sites, but THREAT_GUN is an abstract danger a crab flees, whereas this is the *sound* of the fight,
    /// which the swarm may be drawn toward — a different radius/decay and an evolvable perception sign.
    pub const NOISE_SQUAD: FieldId = FieldId(7);
    /// Audible din **emitted by crabs** — a crab's death squelch. Read by units, never by crabs (which
    /// would otherwise react to the sound of their own dying kin).
    pub const NOISE_SWARM: FieldId = FieldId(8);

    /// **Observation** — how heavily a cell is being *watched* right now, deposited by every gaze
    /// (squad vision cones, the Researcher's flashlight, and — windowed-only — the player's camera) and
    /// evaporating fast so it is a live, decaying "where the eyes are" field, not a permanent memory.
    /// Observation as stigmergy: the watcher writes attention into the environment and other systems
    /// read it, so gaze coordinates behaviour *through the world* (Grassé 1959; Heylighen, "Stigmergy as
    /// a universal coordination mechanism", Cognitive Systems Research 2016). Read by threats with
    /// **opposite signs** — the mould recoils from it (grows in the inattention shadow, SCP-173's
    /// freeze-when-watched pole) while a marked predator is *drawn* to it (aggros on being seen, SCP-096's
    /// pole). Deliberately NOT in [`UNIT_THREAT_CHANNELS`]: attention is not faction-fear, and a unit that
    /// feared the attention it emits would flee its own gaze.
    pub const ATTENTION: FieldId = FieldId(9);
    // NOTE: the rally beacon is NOT a scalar channel — it's a *vectorial* pheromone (see [`RallyField`]
    // below), which stores a direction toward the moving prey rather than a scalar concentration.
}

/// Number of channels. Bump when adding a [`FieldId`].
pub const CHANNEL_COUNT: usize = 10;

/// The danger channels a *unit* reads. One per hostile creature type, so nothing ever fears its own
/// emissions. Ordered, but consumed by an order-independent `max` (see `DriveRule::TrackMaxFields`).
pub const UNIT_THREAT_CHANNELS: [FieldId; 2] = [FieldId::THREAT_CRAB, FieldId::THREAT_ANOMALY];

/// Per-channel behaviour, filled from the `ai_tuning:` slice of `assets/config/config.ron` at startup.
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

/// Stable ordering for a batch of deposits before they are queued. `drain_deposits` applies each with a
/// non-associative `f32 +=`, so two deposits landing on overlapping cells in different iteration order
/// (unstable across App instances — async GLB load + entity-id reuse) would smear the channel to a
/// different sum. A site that emits deposits in raw ECS-query order sorts its batch through this first, so
/// the drained field is a pure function of the deposits, not of query order. (Sites that already sort
/// their source rows by a stable key before pushing — e.g. `crab_despawn_dead` by `Seed` — do not need it.)
pub fn sort_deposits(batch: &mut [Deposit]) {
    // VALUE-CANONICAL, not total: two deposits with the same position AND amount contribute the same term
    // to the same sum, so permuting them cannot change the drained field. Ties here are genuinely harmless —
    // that is the claim `sort_value_canonical` makes, and it is why this is not `sort_total!`.
    crate::util::sort_value_canonical(batch, |d| {
        (d.pos.x.to_bits(), d.pos.y.to_bits(), d.pos.z.to_bits(), d.amount.to_bits())
    });
}

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
    /// The floor cells (the only cells that ever carry value), precomputed once so the per-tick
    /// evaporation/diffusion/hotspot passes skip the rock cells. See [`floor_cells_of`].
    floor_cells: Vec<FloorCell>,
}

/// Walk the floor cells within `radius` (in cells) of `pos`, calling `emit(cell_index, falloff)` with the
/// linear falloff `1 - dist/radius` (1.0 when `radius == 0`). The shared deposit kernel — the disc walk,
/// the `in_grid && is_floor` wall mask, and the falloff math — used by both the scalar [`Stig`] and the
/// vectorial [`RallyField`] deposit paths, which differ only in what they accumulate per cell.
fn deposit_disc(
    width: usize,
    height: usize,
    dungeon: &Dungeon,
    pos: Vec3,
    radius: f32,
    mut emit: impl FnMut(usize, f32),
) {
    let radius = radius.max(0.0);
    let center = dungeon.world_to_cell(pos);
    let r = radius.ceil() as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            let cell = center + IVec2::new(dx, dy);
            if !crate::util::in_grid(cell, width, height) || !dungeon.is_floor(cell) {
                continue;
            }
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            if dist > radius {
                continue;
            }
            let falloff = if radius > 0.0 { 1.0 - dist / radius } else { 1.0 };
            emit(crate::util::row_major(cell, width), falloff);
        }
    }
}

/// A precomputed floor cell: its row-major grid index and its `(x, y)` coordinates, carried together so the
/// per-tick passes need neither an `is_floor` test nor an `i % w` / `i / w` recompute. `idx == row_major(pos)`.
#[derive(Clone, Copy)]
struct FloorCell {
    idx: usize,
    pos: IVec2,
}

/// Every floor cell of the (static) dungeon, precomputed once in ascending row-major order. The per-tick
/// field passes iterate only these: a rock cell never carries field value — deposits are floor-masked
/// (`deposit_disc`), evaporation of 0 is 0, and the diffusion double-buffer's rock cells stay 0 across the
/// swap — so skipping the ~half-to-two-thirds of the grid that is rock is **bit-identical** to scanning the
/// whole grid, not an approximation. The floor set comes from the shared [`Dungeon::floor_cells`] so it can
/// never drift from the harness coverage denominator or the habitat mask.
fn floor_cells_of(dungeon: &Dungeon) -> Vec<FloorCell> {
    let mut cells = Vec::with_capacity(dungeon.width * dungeon.height);
    cells.extend(
        dungeon
            .floor_cells()
            .map(|c| FloorCell { idx: crate::util::row_major(c, dungeon.width), pos: c }),
    );
    cells
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
            floor_cells: floor_cells_of(dungeon),
        }
    }

    #[inline]
    fn index(&self, c: IVec2) -> usize {
        crate::util::row_major(c, self.width)
    }

    #[inline]
    fn in_grid(&self, c: IVec2) -> bool {
        crate::util::in_grid(c, self.width, self.height)
    }

    /// Point read at a world position (query). Off-grid reads as 0.
    pub fn sample(&self, field: FieldId, dungeon: &Dungeon, pos: Vec3) -> f32 {
        let c = dungeon.world_to_cell(pos);
        if self.in_grid(c) {
            self.channels[field.0][self.index(c)]
        } else {
            0.0
        }
    }

    /// Direction (world XZ) of *increasing* value, magnitude ≈ the local slope. Central differences on
    /// the 4-neighbour cells; `FollowGradient` uses `+`, `FleeGradient` uses `-`.
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
        let (w, h) = (self.width, self.height);
        let channel = &mut self.channels[field.0];
        deposit_disc(w, h, dungeon, pos, self.defs[field.0].deposit_radius, |i, falloff| {
            channel[i] += amount * falloff;
        });
    }

    /// One evaporation + diffusion step for every channel (Ch.29 diffusion, ACO evaporation). `dt` in
    /// seconds. Diffusion blends only between floor cells so influence doesn't seep through walls.
    fn evaporate_diffuse(&mut self, dungeon: &Dungeon, dt: f32) {
        // Only floor cells ever hold value (rock cells are invariantly 0), so both passes iterate
        // `floor_cells` rather than the whole grid. This is bit-identical, not an approximation: evaporating
        // a rock cell (0·retain) and diffusion's old `scratch[rock] = grid[rock]` (0) were both no-ops, and
        // the double-buffer's rock cells stay 0 across the swap. The neighbour sum keeps its fixed
        // E/W/S/N order — float add is non-associative, so the order is load-bearing.
        let w = self.width;
        let h = self.height;
        for ch in 0..CHANNEL_COUNT {
            let def = self.defs[ch];
            let retain = (1.0 - def.evaporate * dt).clamp(0.0, 1.0);
            {
                let grid = &mut self.channels[ch];
                for fc in &self.floor_cells {
                    grid[fc.idx] *= retain;
                }
            }
            if def.diffuse <= 0.0 {
                continue;
            }
            // Blend each floor cell toward the average of its floor neighbours (double-buffered).
            let diffuse = def.diffuse;
            let grid = &self.channels[ch];
            let scratch = &mut self.scratch;
            for fc in &self.floor_cells {
                let (x, y) = (fc.pos.x, fc.pos.y);
                let mut sum = 0.0;
                let mut n = 0.0;
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let nb = IVec2::new(x + dx, y + dy);
                    if nb.x >= 0 && nb.y >= 0 && (nb.x as usize) < w && (nb.y as usize) < h && dungeon.is_floor(nb) {
                        sum += grid[(nb.y as usize) * w + nb.x as usize];
                        n += 1.0;
                    }
                }
                let avg = if n > 0.0 { sum / n } else { grid[fc.idx] };
                scratch[fc.idx] = grid[fc.idx] * (1.0 - diffuse) + avg * diffuse;
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
        // `floor_cells` is ascending-index order, and rock cells are 0 (can never beat `best` under the
        // strict `>`), so this yields the identical first-max-wins result as scanning the whole grid.
        for fc in &self.floor_cells {
            let v = grid[fc.idx];
            if v > best {
                best = v;
                best_cell = fc.pos;
            }
        }
        (dungeon.cell_center(best_cell), best)
    }

    /// Field-degeneracy stats for the offline search's field-sanity gate: `(peak, flatness)` where `peak`
    /// is the largest value over every channel and floor cell, and `flatness` is the fraction of floor
    /// cells whose strongest channel is at least **half** that peak. A healthy field has a sharp peak over
    /// sparse activity (low flatness); a saturated field (evaporation ≈ 0) has a runaway peak, and a
    /// whole-map smear (huge radius/diffusion) is high *and* uniform (flatness → 1), so agents cannot
    /// navigate its gradient. Read-only and order-independent (`max`/count), so it never perturbs the
    /// pinned sim — it is sampled from `squad_ai::evaluate::rollout`, not a system.
    pub fn saturation_stats(&self) -> (f32, f32) {
        let per_cell_max =
            |i: usize| (0..CHANNEL_COUNT).map(|ch| self.channels[ch][i]).fold(0.0f32, f32::max);
        let floor = self.floor_cells.len();
        let peak = self.floor_cells.iter().map(|fc| per_cell_max(fc.idx)).fold(0.0f32, f32::max);
        if floor == 0 || peak <= 0.0 {
            return (peak, 0.0);
        }
        let thresh = 0.5 * peak;
        let hot = self.floor_cells.iter().filter(|fc| per_cell_max(fc.idx) >= thresh).count();
        (peak, hot as f32 / floor as f32)
    }

    /// FNV-1a-fold the exact bit pattern of every channel cell (the **full** grid, so the rock-cells-stay-0
    /// invariant is pinned too) plus the derived `saturation_stats`, into `hash`. The determinism oracle for
    /// the field passes: `snapshot_hash` hashes only actor Transform+Health, so without this a reordered
    /// neighbour sum or broken floor mask that doesn't happen to move an agent would ship silently. Test-only.
    #[cfg(feature = "test-harness")]
    pub fn fold_fingerprint(&self, hash: &mut u64) {
        for ch in &self.channels {
            for &v in ch {
                fnv1a_fold(&v.to_bits().to_le_bytes(), hash);
            }
        }
        let (peak, flatness) = self.saturation_stats();
        fnv1a_fold(&peak.to_bits().to_le_bytes(), hash);
        fnv1a_fold(&flatness.to_bits().to_le_bytes(), hash);
    }
}

/// FNV-1a byte fold — the same mix `snapshot_hash` uses, shared by the field fingerprints.
#[cfg(feature = "test-harness")]
fn fnv1a_fold(bytes: &[u8], hash: &mut u64) {
    for &b in bytes {
        *hash ^= b as u64;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
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

/// Per-second attention a squad unit lays on every cell it can currently **see**. Deposited as `RATE·dt`
/// each fixed tick, so a continuously-watched cell settles at the timestep-independent steady state
/// `RATE / evaporate` (the `crab_density` rate idiom — a cell's value tracks how long/heavily it is
/// watched) and a cell just out of sight decays from there. This is the negative-feedback + accumulation
/// that turns the binary "in line of sight" bit into a graded, smoothly-fading gaze signal.
pub const ATTENTION_RATE: f32 = 1.0;

/// Deposit [`FieldId::ATTENTION`] over the squad's current line-of-sight set (`crate::fog::FogGrid`).
///
/// Observation as stigmergy: a watcher writes attention *into the environment*, and other systems (the
/// mould's recoil, a marked predator's aggro) read it — gaze coordinates behaviour through the world
/// rather than by messaging (Grassé 1959; Heylighen, "Stigmergy as a universal coordination mechanism",
/// Cognitive Systems Research 2016).
///
/// **Determinism.** Fog visibility is a pure function of unit *cell positions* + integer line-of-sight
/// (`fog::update_los` — no rotation, no transcendentals), so this channel folds into the cross-arch replay
/// fingerprint like every other one. The Researcher's flashlight cone is deliberately NOT a source here:
/// its `forward` comes from `Transform.rotation`, whose glam slerp is not bit-identical across
/// architectures — folding a rotation-derived channel would re-open the #46 cross-arch hash hazard (the
/// same reason `LightField::fold_fingerprint` folds `base`, not the moving cone). Deposits go through the
/// [`StigDeposits`] queue (drained + globally sorted), so batch order can't perturb the field.
pub fn deposit_attention(
    dungeon: Res<Dungeon>,
    fog: Res<crate::fog::FogGrid>,
    time: Res<Time>,
    mut deposits: ResMut<StigDeposits>,
) {
    let amount = ATTENTION_RATE * time.delta_secs();
    if !(amount > 0.0) {
        return;
    }
    for c in dungeon.floor_cells() {
        if fog.visible_at(c) {
            deposits.0.push(Deposit { pos: dungeon.cell_center(c), field: FieldId::ATTENTION, amount });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Vectorial rally pheromone — Tang, Xu, Yu, Zhang & Zhang, "Dynamic target searching and tracking with
// swarm robots based on stigmergy", Robotics & Autonomous Systems 2019 (DOI 10.1016/j.robot.2019.103251).
// ---------------------------------------------------------------------------------------------------

/// Per-field tuning for the vectorial rally pheromone (mirrors [`ChannelDef`], but for the vector store).
#[derive(Clone, Copy)]
pub struct RallyDef {
    /// Decay coefficient `c_d` (fraction lost per second). Drives both per-frame evaporation and the
    /// `(1 - c_d)` term of the accumulation recurrence — evaporation is the automatic "call off the attack".
    pub decay: f32,
    /// Accumulation gain `c_a` applied to each deposited intermediate-vector.
    pub accumulate: f32,
    /// World-unit radius a single deposit smears over (placement kernel, linear falloff).
    pub deposit_radius: f32,
}

impl Default for RallyDef {
    fn default() -> Self {
        Self {
            decay: 0.3,
            accumulate: 0.5,
            deposit_radius: 2.0,
        }
    }
}

/// One vectorial-pheromone deposit request (a scout's intermediate-vector `s`, pointing toward the prey).
pub struct RallyDeposit {
    pub pos: Vec3,
    pub vec: Vec2,
}

/// Decoupling queue for rally writes (mirrors [`StigDeposits`]). Drained by `drain_rally_deposits`.
#[derive(Resource, Default)]
pub struct RallyDeposits(pub Vec<RallyDeposit>);

/// [`sort_deposits`]'s twin for the **vectorial** queue. Same contract, same reason — and this one is the
/// reason the rule below exists.
///
/// **This helper did not exist, and that was the whole bug.** The determinism campaign canonicalised the
/// *scalar* [`Deposit`]/[`StigDeposits`] path — every producer (`nest_alarm`, `crab_alarm_on_damage`,
/// `deposit_crab_fields`, `deposit_meat_scent`, `deposit_manca_dread`, …) batches and calls
/// [`sort_deposits`]. `RallyDeposits` is a **separate** path and [`sort_deposits`] is typed `&mut [Deposit]`,
/// so it never type-checked here; the sole producer (`crab::scout_mark_prey`) therefore pushed bare, in raw
/// ECS query order, into [`RallyField::deposit`]'s non-associative `grid[i] += s * (accumulate * falloff)`.
///
/// Two properties made it survive every previous sweep:
///  * **Auditing sort sites could not find it** — there was no sort to audit.
///  * **`sort_total!` could not fire on it** — same reason. The runtime tie-check only guards code that
///    already decided to sort.
///
/// And it is invisible to `snapshot_hash` (which folds only `(Transform, Health)`) until a perturbed cell
/// flips a threshold — `re_role_crabs`' `rally.sample(..).length() > bc.rally_live`, which the *authored*
/// config keeps at 0.15 but the genome may push to **0.02**, right onto the field's noise floor. Hence
/// green for the authored genome, divergent for a mutant.
///
/// **The rule this buys, stated so the next queue type inherits it:** *every deposit queue owns a
/// canonicalising helper next to its type, and a new queue type must add one.* A queue whose producers push
/// in query order and whose consumer accumulates non-associatively is not reproducible, and no lint in this
/// repo can tell you so.
pub fn sort_rally_deposits(batch: &mut [RallyDeposit]) {
    // VALUE-CANONICAL, not total (same judgement as `sort_deposits`): two rally deposits with the same
    // position AND the same vector contribute the identical term to the identical cells, so permuting them
    // cannot change the drained field. The key is the WHOLE value — never a prefix of it, which is how the
    // ORCA / drink-contention / boss-cull ties happened.
    crate::util::sort_value_canonical(batch, |d| {
        (d.pos.x.to_bits(), d.pos.y.to_bits(), d.pos.z.to_bits(), d.vec.x.to_bits(), d.vec.y.to_bits())
    });
}

/// The vectorial rally pheromone map (Tang et al. 2019). Each floor cell stores a 2D **direction vector**
/// — the bearing toward the (moving) prey — not a scalar concentration like the [`Stig`] channels. A
/// scout that senses prey deposits an intermediate-vector `s` pointing at it; the map accumulates
/// deposits with decay (`pher = (1 - c_d)·pher + c_a·s`, the paper's `pher_N^m` recurrence) and
/// evaporates each frame. Crabs read the vector **locally** and steer straight along it, so the swarm
/// tracks the prey's live motion — and a crab far from any arrow reads ≈0, so it never has its flight
/// suppressed by a distant beacon (the locality the old global-peak scalar lacked).
#[derive(Resource)]
pub struct RallyField {
    width: usize,
    height: usize,
    grid: Vec<Vec2>,
    decay: f32,
    accumulate: f32,
    deposit_radius: f32,
    /// The floor cells (only floor cells receive value), so evaporation skips the rock cells. See
    /// [`floor_cells_of`].
    floor_cells: Vec<FloorCell>,
}

impl RallyField {
    /// Allocate an empty vector grid sized to the dungeon. `def` comes from tuning.
    pub fn new(dungeon: &Dungeon, def: RallyDef) -> Self {
        let cells = dungeon.width * dungeon.height;
        Self {
            width: dungeon.width,
            height: dungeon.height,
            grid: vec![Vec2::ZERO; cells],
            decay: def.decay,
            accumulate: def.accumulate,
            deposit_radius: def.deposit_radius,
            floor_cells: floor_cells_of(dungeon),
        }
    }

    #[inline]
    fn index(&self, c: IVec2) -> usize {
        crate::util::row_major(c, self.width)
    }

    #[inline]
    fn in_grid(&self, c: IVec2) -> bool {
        crate::util::in_grid(c, self.width, self.height)
    }

    /// Local vectorial read at a world position (query). Off-grid reads as `Vec2::ZERO`. Magnitude ≈ the
    /// local beacon strength (gate on it); direction ≈ the bearing to the prey (steer along it).
    pub fn sample(&self, dungeon: &Dungeon, pos: Vec3) -> Vec2 {
        let c = dungeon.world_to_cell(pos);
        if self.in_grid(c) {
            self.grid[self.index(c)]
        } else {
            Vec2::ZERO
        }
    }

    /// Accumulate a deposited intermediate-vector `s` (Tang's `c_a·s` term), smeared over `deposit_radius`
    /// with linear falloff. Only floor cells receive value (deposits don't bleed into rock).
    fn deposit(&mut self, dungeon: &Dungeon, pos: Vec3, s: Vec2) {
        let (w, h) = (self.width, self.height);
        let accumulate = self.accumulate;
        let grid = &mut self.grid;
        deposit_disc(w, h, dungeon, pos, self.deposit_radius, |i, falloff| {
            grid[i] += s * (accumulate * falloff);
        });
    }

    /// One evaporation step: decay every cell toward zero (the `(1 - c_d)` term / the automatic call-off).
    fn evaporate(&mut self, dt: f32) {
        let retain = (1.0 - self.decay * dt).clamp(0.0, 1.0);
        // Only floor cells ever hold a vector (deposits are floor-masked), so scaling the rock cells is a
        // no-op — iterate floor cells only (bit-identical).
        for fc in &self.floor_cells {
            self.grid[fc.idx] *= retain;
        }
    }

    /// FNV-1a-fold the exact bit pattern of every cell's direction vector (full grid) into `hash`. The
    /// vectorial-field half of the determinism oracle — see [`Stig::fold_fingerprint`]. Test-only.
    #[cfg(feature = "test-harness")]
    pub fn fold_fingerprint(&self, hash: &mut u64) {
        for v in &self.grid {
            fnv1a_fold(&v.x.to_bits().to_le_bytes(), hash);
            fnv1a_fold(&v.y.to_bits().to_le_bytes(), hash);
        }
    }
}

/// Drain queued rally deposits into the vector map (placement).
pub fn drain_rally_deposits(
    mut rally: ResMut<RallyField>,
    dungeon: Res<Dungeon>,
    mut deposits: ResMut<RallyDeposits>,
) {
    for d in deposits.0.drain(..) {
        rally.deposit(&dungeon, d.pos, d.vec);
    }
}

/// Evaporate the rally map once per frame.
pub fn evaporate_rally(mut rally: ResMut<RallyField>, time: Res<Time>) {
    rally.evaporate(time.delta_secs());
}
