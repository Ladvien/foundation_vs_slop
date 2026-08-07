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
/// Defined in `bevy_stigmergy`; re-exported here so `impl From<ChannelTuning> for ChannelDef` in
/// `ai::tuning` still resolves (a local type onto a foreign one is legal, which is why the conversion
/// did not have to move).
pub use bevy_stigmergy::ChannelDef;

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

/// The shared field grids — a thin **facade** over `bevy_stigmergy::StigGrid`.
///
/// The mechanism (deposit / evaporate / diffuse / sample / gradient) lives in that crate, which knows
/// nothing about a dungeon and works in CELL space. This type is what turns it back into the game's
/// vocabulary: it owns the `Dungeon` (world↔cell conversion, the floor set) and keeps the exact method
/// signatures every call site already uses, so the extraction moved no caller.
///
/// **A newtype rather than a trait, deliberately.** Callers write `stig.sample(f, &dungeon, pos)` where
/// `dungeon: Res<Dungeon>`; against a generic `&impl CellMap` inference would pick `Res<Dungeon>`, and
/// the orphan rule forbids implementing a foreign trait for it — every one of the ~40 call sites would
/// have needed `&*dungeon`. A `&dyn` would instead put a virtual call inside the diffusion inner loop.
#[derive(Resource)]
pub struct Stig(bevy_stigmergy::StigGrid<CHANNEL_COUNT>);

impl Stig {
    /// Allocate empty grids sized to the dungeon. `defs` come from tuning.
    pub fn new(dungeon: &Dungeon, defs: [ChannelDef; CHANNEL_COUNT]) -> Self {
        Self(bevy_stigmergy::StigGrid::new(
            dungeon.width,
            dungeon.height,
            dungeon.floor_cells(),
            defs,
        ))
    }

    /// Point read at a world position (query). Off-grid reads as 0.
    #[inline]
    pub fn sample(&self, field: FieldId, dungeon: &Dungeon, pos: Vec3) -> f32 {
        self.0.sample_cell(field.0, dungeon.world_to_cell(pos))
    }

    /// Direction (world XZ) of *increasing* value, magnitude ≈ the local slope. Central differences on
    /// the 4-neighbour cells; `FollowGradient` uses `+`, `FleeGradient` uses `-`.
    #[inline]
    pub fn gradient(&self, field: FieldId, dungeon: &Dungeon, pos: Vec3) -> Vec2 {
        self.0.gradient_cell(field.0, dungeon.world_to_cell(pos))
    }

    /// Add `amount` at `pos`, smeared over the channel's `deposit_radius` with linear falloff. Only
    /// floor cells receive value (deposits don't bleed into rock).
    fn deposit(&mut self, field: FieldId, dungeon: &Dungeon, pos: Vec3, amount: f32) {
        self.0.deposit(field.0, dungeon.world_to_cell(pos), amount);
    }

    /// One evaporation + diffusion step for every channel. `dt` in seconds.
    fn evaporate_diffuse(&mut self, dt: f32) {
        self.0.evaporate_diffuse(dt);
    }

    /// The peak `(world position, value)` in a channel — used by the boss's "drawn to the biggest
    /// frenzy" hunt and by diagnostics.
    ///
    /// An empty channel reports the dungeon spawn at value 0, which is the behaviour callers were
    /// written against: the crate returns `None` for "nothing above zero", and the seed lives here
    /// because `dungeon.spawn` is a game fact.
    pub fn hotspot(&self, field: FieldId, dungeon: &Dungeon) -> (Vec3, f32) {
        let (cell, v) = self.0.hotspot_cell(field.0);
        (dungeon.cell_center(cell.unwrap_or(dungeon.spawn)), v)
    }

    /// Field-degeneracy stats for the offline search's field-sanity gate: `(peak, flatness)`. Read-only
    /// and order-independent, so it never perturbs the pinned sim — it is sampled from
    /// `squad_ai::evaluate::rollout`, not a system.
    pub fn saturation_stats(&self) -> (f32, f32) {
        self.0.saturation_stats()
    }

    /// FNV-1a-fold the exact bit pattern of every channel cell (the **full** grid, so the
    /// rock-cells-stay-0 invariant is pinned too) plus the derived `saturation_stats`, into `hash`. The
    /// determinism oracle for the field passes: `snapshot_hash` hashes only actor Transform+Health, so
    /// without this a reordered neighbour sum or broken floor mask that doesn't happen to move an agent
    /// would ship silently. Test-only.
    #[cfg(feature = "test-harness")]
    pub fn fold_fingerprint(&self, hash: &mut u64) {
        for ch in self.0.channels() {
            for &v in ch {
                fnv1a_fold(&v.to_bits().to_le_bytes(), hash);
            }
        }
        let (peak, flatness) = self.0.saturation_stats();
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
pub fn evaporate_diffuse(mut stig: ResMut<Stig>, time: Res<Time>) {
    // Profiling span: read the per-system cost under `--features bevy/trace_tracy` (see `perf_hud`).
    let _span = info_span!("stig_evaporate_diffuse").entered();
    let dt = time.delta_secs();
    stig.evaporate_diffuse(dt);
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
/// [`StigDeposits`] queue, which [`drain_deposits`] applies in RAW ARRIVAL ORDER — there is no global
/// sort (see `laser.rs`: the batch is applied unsorted, so every PRODUCER must push in a deterministic
/// order). This producer qualifies because it emits from `dungeon.floor_cells()` — a fixed grid raster,
/// never an ECS query.
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

/// Per-field tuning for the vectorial rally pheromone (mirrors [`ChannelDef`], but for the vector
/// store). Defined in `bevy_stigmergy`; re-exported here so `impl From<RallyTuning> for RallyDef` in
/// `ai::tuning` still resolves — a local type onto a foreign one is legal, which is why the conversion
/// did not have to move with it.
pub use bevy_stigmergy::RallyDef;

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

/// The vectorial rally pheromone map (Tang et al. 2019) — a **facade** over
/// `bevy_stigmergy::RallyGrid`, on the same argument as [`Stig`] above.
///
/// Each floor cell stores a 2D **direction vector** — the bearing toward the (moving) prey — not a
/// scalar concentration like the [`Stig`] channels. A scout that senses prey deposits an
/// intermediate-vector `s` pointing at it; the map accumulates deposits with decay (`pher =
/// (1 - c_d)·pher + c_a·s`, the paper's `pher_N^m` recurrence) and evaporates each frame. Crabs read
/// the vector **locally** and steer straight along it, so the swarm tracks the prey's live motion — and
/// a crab far from any arrow reads ≈0, so it never has its flight suppressed by a distant beacon (the
/// locality the old global-peak scalar lacked).
#[derive(Resource)]
pub struct RallyField(bevy_stigmergy::RallyGrid);

impl RallyField {
    /// Allocate an empty vector grid sized to the dungeon. `def` comes from tuning.
    pub fn new(dungeon: &Dungeon, def: RallyDef) -> Self {
        Self(bevy_stigmergy::RallyGrid::new(dungeon.width, dungeon.height, dungeon.floor_cells(), def))
    }

    /// Local vectorial read at a world position (query). Off-grid reads as `Vec2::ZERO`. Magnitude ≈ the
    /// local beacon strength (gate on it); direction ≈ the bearing to the prey (steer along it).
    #[inline]
    pub fn sample(&self, dungeon: &Dungeon, pos: Vec3) -> Vec2 {
        self.0.sample_cell(dungeon.world_to_cell(pos))
    }

    /// Accumulate a deposited intermediate-vector `s` (Tang's `c_a·s` term), smeared over
    /// `deposit_radius` with linear falloff. Only floor cells receive value.
    fn deposit(&mut self, dungeon: &Dungeon, pos: Vec3, s: Vec2) {
        self.0.deposit(dungeon.world_to_cell(pos), s);
    }

    /// One evaporation step: decay every cell toward zero (the `(1 - c_d)` term / the automatic
    /// call-off).
    fn evaporate(&mut self, dt: f32) {
        self.0.evaporate(dt);
    }

    /// FNV-1a-fold the exact bit pattern of every cell's direction vector (full grid) into `hash`. The
    /// vectorial-field half of the determinism oracle — see [`Stig::fold_fingerprint`]. Test-only.
    #[cfg(feature = "test-harness")]
    pub fn fold_fingerprint(&self, hash: &mut u64) {
        for v in self.0.cells() {
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
