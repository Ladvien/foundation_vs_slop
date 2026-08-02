//! Engine-free placement grammar IR — the solver-agnostic constraint problem a grammar compiles to.
//!
//! Karth & Smith ("WaveFunctionCollapse is Constraint Solving in the Wild", FDG 2017) established
//! that placement *is* finite-domain constraint solving; this module is that observation made into
//! types. Nothing here imports `bevy::` — a `PlacementProblem` is pure data any backend can consume,
//! and every type is `Serialize`/`Deserialize` so a solver could one day run as an external process
//! (see the implementation plan's "future seam"). The Bevy boundary lives only in `furnish.rs`.
//!
//! Types beyond `Region` are consumed by later stages (Solver backends, the manifest, the furnish
//! pass); `Region` alone is wired into `Dungeon` at Stage 0.
#![allow(dead_code)] // Staged scaffolding: solver/constraint types land ahead of their Stage 1–4 consumers.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Stable handle for a region within one generation. Index into `Dungeon::regions`.
pub type RegionId = u32;
/// Stable handle for a compiled constraint, so an `Outcome::Partial` can name what it dropped.
pub type ConstraintId = u32;
/// Index into a `PlacementProblem::candidates`.
pub type CandidateIx = usize;
/// Direction index, matching `crate::wfc::{N, E, S, W}`.
pub type Dir = usize;
/// Manifest key for an asset — opaque to the solver stack; resolved to a GLB only in `furnish.rs`.
pub type AssetKey = String;

/// Axis-aligned rectangle in fine-grid tile coordinates (inclusive `min`, exclusive `max`) — the same
/// integer grid as `Dungeon`'s walkability mask, so a `Region` addresses exactly the cells it owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect2 {
    pub min: [i32; 2],
    pub max: [i32; 2],
}

impl Rect2 {
    #[inline]
    pub fn width(&self) -> i32 {
        self.max[0] - self.min[0]
    }
    #[inline]
    pub fn height(&self) -> i32 {
        self.max[1] - self.min[1]
    }
    /// The integer cell nearest the rectangle's centre (for a marker / delivery point).
    #[inline]
    pub fn center_cell(&self) -> [i32; 2] {
        [
            (self.min[0] + self.max[0]) / 2,
            (self.min[1] + self.max[1]) / 2,
        ]
    }
    #[inline]
    pub fn contains(&self, c: [i32; 2]) -> bool {
        c[0] >= self.min[0] && c[0] < self.max[0] && c[1] >= self.min[1] && c[1] < self.max[1]
    }
}

/// One placed object's axis-aligned floor footprint: where it is, which way it faces, and how much
/// room it takes.
///
/// # Why this lives in the IR rather than in a solver
///
/// The two rules that stop furniture being nonsense — *pieces do not interpenetrate* and *a piece
/// stays inside the room it was placed in* — were, until 2026-08-02, private to
/// `solvers::metropolis`: an `Obj` struct plus `aabb_overlap_area`, scored as built-in energy terms
/// rather than as [`Constraint`]s. That worked for the dungeon, whose furniture is *solved*.
///
/// It left the **Site** unprotected, because Site-67 is hand-authored: `site67.ron` types positions
/// directly and never enters a solver, so nothing checked them. The first pass at furnishing the
/// living half put three bunks 15 cm through a wall, ran a 3.68 m control desk a metre out into a
/// corridor, and sat three bedside tables inside their own bunks — none of it visible in a
/// screenshot at play zoom.
///
/// So the geometry moved here, where both callers can reach it, and there is exactly **one**
/// definition of "do these two overlap" and "is this inside its room". A second copy for the Site
/// would have been a second answer to the same question, which is the failure mode this codebase
/// spends most of its comments avoiding.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    pub x: f32,
    pub z: f32,
    /// Yaw about +Y, radians.
    pub yaw: f32,
    /// Native half-extents BEFORE yaw. The axis-aligned pair is [`Self::half_extents`].
    pub hw: f32,
    pub hd: f32,
}

impl Footprint {
    /// Axis-aligned half-extents at the current yaw.
    ///
    /// **Quarter-turn furniture:** at 90°/270° width and depth swap, and nothing between is modelled.
    /// That is a real limit of this representation and it is the right one here — every placement in
    /// this game is authored or solved on quarter turns, and an oriented box would make the overlap
    /// test a separating-axis problem for no gain anyone can see.
    #[inline]
    pub fn half_extents(&self) -> (f32, f32) {
        if self.yaw.sin().abs() > 0.5 {
            (self.hd, self.hw)
        } else {
            (self.hw, self.hd)
        }
    }
}

/// Area (m²) by which two footprints interpenetrate. `0.0` when they are clear of each other.
#[inline]
pub fn overlap_area(a: &Footprint, b: &Footprint) -> f32 {
    let (ahw, ahd) = a.half_extents();
    let (bhw, bhd) = b.half_extents();
    let ox = (ahw + bhw) - (a.x - b.x).abs();
    let oz = (ahd + bhd) - (a.z - b.z).abs();
    if ox > 0.0 && oz > 0.0 { ox * oz } else { 0.0 }
}

/// How far (metres, summed over the axes it breaches) a footprint sticks out past `bounds`, given as
/// the interior extents `(x0, x1, z0, z1)`. `0.0` when the whole footprint is inside.
///
/// A hinge sum rather than a boolean so a solver can descend it and a checker can report *how far*
/// out the offender is — "0.15 m through the west wall" is actionable; "invalid" is not.
#[inline]
pub fn escapes_bounds(f: &Footprint, bounds: (f32, f32, f32, f32)) -> f32 {
    let (ahw, ahd) = f.half_extents();
    let (x0, x1, z0, z1) = bounds;
    let (bx0, bx1, bz0, bz1) = (x0 + ahw, x1 - ahw, z0 + ahd, z1 - ahd);
    let over = |v: f32| v.max(0.0);
    over(bx0 - f.x) + over(f.x - bx1) + over(bz0 - f.z) + over(f.z - bz1)
}

/// A doorway / corridor mouth on a region's boundary — derived from the coarse `CellData.open`
/// links + the corridor carve. `cell` is the interior floor cell at lane 0 of the opening, `dir` the
/// wall it pierces (N/E/S/W), and `width` the number of open lanes (≥1) — the doorway necks down from
/// its corridor's carved width to `width` lanes, stacked perpendicular to `dir` from `cell`. Anchors
/// like doors and header lintels dispatch onto these and span all `width` lanes.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Opening {
    pub dir: Dir,
    pub cell: [i32; 2],
    /// Open lanes at the mouth (≥1). `#[serde(default = ...)]` → a hand-written `Opening` with no
    /// `width` (older fixtures/tests) reads as a 1-tile doorway, the historical behaviour.
    #[serde(default = "one")]
    pub width: usize,
}

/// serde default for [`Opening::width`] — a 1-tile doorway when the field is absent.
fn one() -> usize {
    1
}

/// Opaque token bag on a region (room type, style tags). The code *matches* these tokens but never
/// interprets them, which is what keeps rules portable across asset kits and domains (see the
/// affordance rationale in the vetting doc, §3.2). Interiors set a room-type tag; a domain swap sets
/// whatever it likes.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PropertyBag {
    pub tags: Vec<String>,
}

impl PropertyBag {
    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

/// A generic **bounded container** — the domain-agnostic placement region (vetting §3.3). Interiors
/// map it to rooms, urban to parcels/lots, dungeon to cells; the orchestrator and grammar never
/// change, only how a `Region` is filled. The adjacency edges make cross-region rules first-class (R5).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub id: RegionId,
    pub rect: Rect2,
    pub openings: Vec<Opening>,
    pub adjacency: Vec<RegionId>,
    pub props: PropertyBag,
}

/// Where an `Anchor` role attaches. Kept small and explicit; new hosts extend it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Host {
    Ceiling,
    Floor,
    Wall,
    Opening,
}

/// The dispatch key for how a candidate is placed — an **open** set (risk R4). The built-ins cover
/// the four roles the plan names; `Custom` carries an opaque token so a new kit or domain can add a
/// role without every consumer needing an exhaustive rewrite.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Role {
    /// Deterministically attached to a host surface (light→ceiling, door→opening, curtain→wall).
    Anchor { host: Host },
    /// Fills a surface on a grid — the WFC / model-synthesis case.
    Tiled,
    /// Placed in open floor space by a soft/relational solver (MCMC).
    Freestanding,
    /// A small prop scattered on a named support surface (books on a shelf).
    Scatter { surface: String },
    /// An opaque, kit- or domain-specific role.
    Custom(String),
}

/// Which degrees of freedom a solver may vary for a candidate.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Dof {
    pub translate: bool,
    /// Snap yaw to the four cardinal directions.
    pub rotate_quarter: bool,
    /// Free continuous yaw.
    pub rotate_free: bool,
}

/// One placeable asset instance plus its placement degrees of freedom.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub asset: AssetKey,
    pub role: Role,
    /// Footprint (width, depth) in world units — for clearance / collision reasoning.
    pub footprint: [f32; 2],
    pub dof: Dof,
    /// Opaque affordance tokens ("sit", "sleep", "emit"…) copied from the manifest so relational
    /// rules can target what an object *affords* rather than its kit-specific name (Fisher 2012; Qi
    /// 2018). Surface classes a piece OFFERS are a separate manifest axis (`ManifestItem::surfaces`),
    /// deliberately not mirrored here — a support relation pairs a `Role::Scatter` requirement with a
    /// provider's `surfaces`, never with an affordance.
    pub affordances: Vec<String>,
}

/// What a constraint ranges over. `Group` is first-class (R4): an articulated set (dining table +
/// chairs) is solved as a unit, not faked with a role.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Scope {
    Object(CandidateIx),
    Pair(CandidateIx, CandidateIx),
    Group(Vec<CandidateIx>),
    /// The whole region — cardinality / global rules.
    Region,
}

/// The relation a constraint asserts. Extensible via `Custom`; the named variants are the ones the
/// staged backends implement (clearance/wall/facing/distance = MCMC; count = cardinality solver;
/// aligned = the domain-swap predicate).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Predicate {
    /// Keep at least this much empty space (metres) around the object on its `clearance` sides.
    Clearance(f32),
    /// Back should sit against a boundary wall.
    AgainstWall,
    /// Face another candidate across the region (the long-range relation of risk R2).
    Facing(CandidateIx),
    /// Keep at least this far (metres) from the paired candidate.
    MinDistance(f32),
    /// Keep the paired candidates *within* this distance (metres) of each other — a soft grouping
    /// band that draws related pieces together (e.g. a bathroom's toilet + sink hugging the same
    /// wall). The inverse of `MinDistance`; overlap is already prevented by the layout's overlap term.
    /// Merrell et al. 2011 formulate pairwise grouping as a distance band; this is its near side.
    Near(f32),
    /// Exactly `count` placed candidates carrying `tag` (cardinality — risk R2).
    Count { tag: String, count: usize },
    /// Aligned to a named region feature — the one-predicate domain-swap test (`aligned(a,"road")`).
    Aligned(String),
    Custom(String),
}

/// Hard = must hold (a solution violating it is rejected); Soft = a weighted cost term.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Modality {
    Hard,
    Soft(f64),
}

/// An optional applicability condition on a constraint (an opaque token the compiler evaluates).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Guard(pub String);

/// A compiled rule: scope + predicate + modality + guard (the grammar decomposition, vetting §1/§7).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Constraint {
    pub id: ConstraintId,
    pub scope: Scope,
    pub predicate: Predicate,
    pub modality: Modality,
    pub guard: Option<Guard>,
}

/// A solved placement: a candidate positioned in the world with a yaw about +Y.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Placement {
    pub candidate: CandidateIx,
    /// World position (x, y, z).
    pub pos: [f32; 3],
    /// Yaw about +Y, radians.
    pub yaw: f32,
}

/// Grammar IR compiled for one bounded region — engine-free, the sole input to a `Solver` (vetting §2).
pub struct PlacementProblem<'a> {
    pub region: &'a Region,
    /// Shared, immutable candidate set. `Arc<[_]>` so the furnish pass hands the same tiled catalogue to
    /// every region's problem with a refcount bump instead of a per-region deep clone of owned strings.
    pub candidates: Arc<[Candidate]>,
    pub constraints: Vec<Constraint>,
}

/// A solver's result. `Partial` is the graceful-degradation path (risk R3): a contradiction or
/// timeout yields the consistent subset plus the constraints it could not satisfy — never a panic,
/// and never a silent substitute written as if it were a full solve.
pub enum Outcome {
    /// Hard solve: one consistent assignment.
    Assignment(Vec<Placement>),
    /// Soft solve: cost-ranked samples (lower cost first).
    Ranked(Vec<(f64, Vec<Placement>)>),
    /// Best-effort: what was placed, plus the constraints left unsatisfied.
    Partial {
        placed: Vec<Placement>,
        unsatisfied: Vec<ConstraintId>,
    },
}

/// Whether a backend enforces hard constraints, soft costs, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hardness {
    Hard,
    Soft,
    Both,
}

/// How far a backend can see: only immediate neighbours (WFC), pairwise/relational (MCMC), or the
/// whole region at once (cardinality / global). Ordered `Local < Relational < Global` for routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Locality {
    Local,
    Relational,
    Global,
}

/// A backend's declared reach — the orchestrator routes a constraint group to the first solver whose
/// capabilities cover the group's needs (vetting §2).
#[derive(Clone, Copy, Debug)]
pub struct Capabilities {
    pub hardness: Hardness,
    pub locality: Locality,
    pub cardinality: bool,
    pub deterministic: bool,
    pub needs_training_data: bool,
}

/// Why a solve could not complete. Carried by `Result`, never panicked (the project mandates one path
/// with loud failure at the door, not mid-solve aborts — see `CLAUDE.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolveError {
    /// No consistent assignment exists for the hard constraints.
    Contradiction,
    /// The iteration/restart budget was exhausted.
    Timeout,
    /// No registered backend covers this problem's required capabilities.
    Unsupported,
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::Contradiction => write!(f, "no consistent assignment for the hard constraints"),
            SolveError::Timeout => write!(f, "solve budget exhausted before convergence"),
            SolveError::Unsupported => write!(f, "no registered solver covers the required capabilities"),
        }
    }
}

impl std::error::Error for SolveError {}
