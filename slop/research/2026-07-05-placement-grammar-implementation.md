# Placement Grammar — Codebase-Specific Implementation Plan

The engineering companion to `2026-07-05-placement-grammar-research-vetting.md` (the vetting +
architecture) and the grammar doc. This maps the vetted design onto **this** repo — actual files,
line numbers, types, and the spawn idioms already in use — as an agent-executable sequence of stages.

Scope decision (2026-07-05): **all Stages 0–5**, built as sequential increments.
Dependency decision: **add `rand_chacha` + `bevy_rand` + `rayon`** (portable reproducible RNG,
ECS-safe randomness, parallel per-region MCMC).

---

## 0. Ground truth — what the repo looks like today

| Fact | Location | Consequence for this plan |
|---|---|---|
| Coarse room-graph WFC, engine-free | `src/wfc.rs` | Wrap as `WfcSolver`; keep it Bevy-free. |
| `wfc::generate()` **panics** on non-convergence | `src/wfc.rs:143`, panic at `:169` | Room-graph pass keeps loud panic (a bug should surface). Furniture pass returns `Outcome::Partial`. |
| In-repo xorshift64 `Rng` | `src/wfc.rs:113` | Replaced/augmented by `rand_chacha::ChaCha8Rng` per the dep decision; keep `wfc.rs` internal RNG usage working during migration. |
| `Dungeon` resource = `width, height, walkable, spawn` | `src/dungeon.rs:68` | Add `pub regions: Vec<Region>`. |
| One room carved per kept coarse slot | `src/dungeon.rs:103–118` | This exact loop yields each `Region` rect for free. |
| Corridor carving per Link edge | `src/dungeon.rs:122–143` | Source of `Region.adjacency` + `openings`. |
| GLB spawn idiom | `src/crab.rs:384,571` — `WorldAssetRoot(GltfAssetLabel::Scene(0).from_asset(path))` | `furnish` system spawns furniture the same way. |
| Sub-mesh load idiom | `src/gore.rs:513` — `"...glb#Mesh0/Primitive0"` | For per-mesh props if needed. |
| Avian collider spawn | `src/nest.rs:118–137` | Mirror for furniture colliders. |
| RON asset precedent | `ai_tuning.ron` (loaded via `ron` crate, already a dep) | Rules + manifest ship as RON, hot-reloadable. |
| **Furniture assets are `.fbx`** | `assets/low_poly_furniture/**/*.fbx` | Bevy's glTF loader can't read FBX → Stage 2 has a real FBX→GLB conversion prerequisite. |
| Kenney kit present | `assets/kenney_prototype-kit/Models/` | The Stage-5 asset-swap acceptance target. |
| Dungeon constants | `src/dungeon.rs:14–43` (`TILE_SIZE=1.0`, `COARSE_W/H=9`, `BLOCK=7`, `ROOM_MIN/MAX=4/5`, `WALL_THICKNESS`, `WALL_HEIGHT=1.0`) | Region rects and clearance math are in these units. |

---

## 1. Dependencies to add (`Cargo.toml`)

```toml
rand = "0.9"
rand_chacha = "0.9"          # ChaCha8Rng — portable, reproducible, well-tested stream
bevy_rand = "0.11"           # ECS-side randomness that doesn't depend on system order
rayon = "1"                  # per-region-independent MCMC parallelism
```

Pin to versions compatible with Bevy 0.19 at implementation time (verify `bevy_rand`'s Bevy support
matrix before adding). **Determinism rule:** thread one seeded `ChaCha8Rng` from orchestrator →
solvers; derive per-region sub-seeds `ChaCha8Rng::seed_from_u64(base_seed ^ splitmix64(region_id))`
so regions solve independently and reproducibly regardless of iteration/thread order. Any ECS-side
randomness uses `bevy_rand` (Bevy system execution order is nondeterministic — RNG must not depend
on it). Migrate `wfc.rs`'s internal `Rng` to `ChaCha8Rng` as part of Stage 1 so there is **one**
RNG implementation, per the one-path rule.

---

## 2. Module layout (new)

```
src/placement/
  mod.rs          // plugin, re-exports, PlacementPlugin
  ir.rs           // Region, Candidate, Constraint, Scope/Predicate/Modality/Guard,
                  //   PlacementProblem, Outcome, Capabilities, SolveError  (ZERO bevy types)
  solver.rs       // Solver trait + Orchestrator (partition → capability profile → route)
  manifest.rs     // RON manifest schema + loader (affordance-annotated)
  solvers/
    wfc.rs        // WfcSolver  { Hard, Local }   — wraps crate::wfc
    anchor.rs     // AnchorSolver — deterministic ceiling/opening/wall attach pass
    metropolis.rs // MetropolisSolver { Soft, Relational }  — Merrell 2011
    constraint.rs // ConstraintSolver { Hard, Global, Cardinality } — finite-domain (GameAIPro2 Ch.26)
  furnish.rs      // Bevy system: consumes Outcome → spawns WorldAssetRoot + Avian collider
```

`ir.rs`, `solver.rs`, `solvers/*` stay **engine-free** (pure Rust, no `bevy::` imports), exactly as
`wfc.rs` is today. Only `furnish.rs` and `mod.rs` touch Bevy. This is the generation↔runtime boundary
the repo already respects — extend it, don't break it.

---

## 3. Core IR (Stage 0) — from vetting §2, made concrete

```rust
// ir.rs — engine-free. Serialize/Deserialize so a solver could be an external process.

/// A generic bounded container. Interiors=rooms, urban=parcels, dungeon=cells.
/// Populated for interiors from src/dungeon.rs's kept-slot loop.
pub struct Region {
    pub id: RegionId,
    pub rect: Rect2,                     // fine-grid tiles (dungeon units)
    pub openings: Vec<Opening>,          // from coarse CellData.open[dir] + corridor carve
    pub adjacency: Vec<RegionId>,        // kept 4-neighbours (first-class graph, R5)
    pub props: PropertyBag,              // opaque tokens: room type, tags
}

pub struct Candidate { pub asset: AssetKey, pub role: Role, pub dof: Dof /* pos/rot ranges */ }

pub struct Constraint { pub scope: Scope, pub predicate: Predicate,
                        pub modality: Modality /* Hard|Soft(weight) */, pub guard: Option<Guard> }

pub enum Outcome {
    Assignment(Vec<Placement>),
    Ranked(Vec<(f64, Vec<Placement>)>),
    Partial { placed: Vec<Placement>, unsatisfied: Vec<ConstraintId> }, // graceful degradation
}

pub struct Capabilities { pub hardness: Hardness, pub locality: Locality,
                          pub cardinality: bool, pub deterministic: bool, pub needs_training_data: bool }

pub trait Solver: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    fn solve(&self, p: &PlacementProblem, rng: &mut ChaCha8Rng) -> Result<Outcome, SolveError>;
}
```

**Role is an OPEN set** (R4): `Anchor{host}`, `Tiled`, `Freestanding`, `Scatter{surface}` are the
built-ins, but new roles must be addable without editing an exhaustive match — model as a trait or a
`#[non_exhaustive]` enum with a dispatch registry. **Groups are first-class scopes** (R4): a dining
set places as a unit via `Scope::Group`, not as a role.

---

## Stage 0 — Region plumbing + IR skeleton (no solving)
- `Cargo.toml`: add deps (§1).
- New `src/placement/{mod.rs,ir.rs,solver.rs}` with the full IR + `Solver` trait (§3). No solving yet.
- `src/dungeon.rs`: add `pub regions: Vec<Region>` to `Dungeon` (`:68`); build them in the kept-slot
  loop (`:103–118`) — rect from `(ox,oy,rw,rh)`, openings from `coarse_open(cx,cy,dir)`, adjacency
  from kept 4-neighbours.
- Debug system spawns one cube per `region` centroid.
- **AC:** IR + trait compile; `Dungeon.regions` populated + adjacency-linked; debug cubes render.
  Verify via `touch screenshot.request; sleep 1.5` then read `screenshot.png`.

## Stage 1 — `Solver` trait live + wrap WFC + RNG unification
- `solvers/wfc.rs`: `WfcSolver: Solver { Hard, Local }` wrapping `crate::wfc`.
- `solver.rs`: **Orchestrator** — partition compiled constraints by region/scope → compute each
  group's required `Capabilities` → route to first registered `Solver` covering it. Solvers in a
  `Vec<Box<dyn Solver>>` Bevy resource.
- Regenerate the room graph *through* the trait (prove the seam).
- Migrate `wfc.rs` internal `Rng` → `ChaCha8Rng` (one RNG impl).
- **AC:** dungeon generates identically; capability dispatch selects `WfcSolver` for local-hard groups.
- Grounding: Gumin 2016; Merrell 2011; Karth & Smith 2017.

## Stage 2 — Manifest adapter + FBX→GLB + anchor pass + first furnished frame
- **Prereq — FBX→GLB conversion:** headless Blender export of the chosen `Low Poly Furniture` set to
  `assets/low_poly_furniture/*.glb`. Scriptable, one-off. (Kenney kit already has usable models.)
- `manifest.rs`: affordance-annotated RON (vetting §3.2):
  `( glb, category, tags, footprint:(w,d), front:<Anchor>, support_surfaces:[<Rect>],`
  `  attach_points:[<Anchor>], affordances:["sit","support",...], clearance:[(Side,m)] )`.
  Roles are the dispatch key; affordances/categories are opaque tokens matched, never interpreted.
- `solvers/anchor.rs`: deterministic `Anchor` placement (lights→ceiling, doors→openings,
  curtains→windows) — trivial solver / direct pass.
- `furnish.rs`: consume `Outcome` → spawn `WorldAssetRoot(GltfAssetLabel::Scene(0).from_asset(glb))`
  + Avian collider, mirroring `src/crab.rs:571` / `src/nest.rs:118`.
- **AC:** first furnished frame renders; editing the manifest changes output with **zero code diff**.

## Stage 3 — `MetropolisSolver` (soft/relational)
- `solvers/metropolis.rs`: port Merrell et al. 2011 §2–3 (`[home-still: furnitureLayout2]`) — cost
  terms (clearance, pairwise distance/angle, wall-alignment, balance, symmetry) + Metropolis–Hastings.
  Per-region, seeded `ChaCha8Rng` sub-stream, weights in RON, `rayon` across regions, `Partial` on
  failure. Budget iterations/region; early-stop on <0.1% cost improvement past ~10k iters.
- **Cite the paper in-code** (per `CLAUDE.md`).
- **AC:** rooms look designed; term ablation reproduces the paper's Fig. 6 behavior.
- Grounding: Merrell 2011; Yu 2011.

## Stage 4 — Global/cardinality backend (closes R2)
- `solvers/constraint.rs`: `ConstraintSolver: Solver { Hard, Global, Cardinality }` — small
  finite-domain solver templated on *Game AI Pro 2* Ch. 26 (`[home-still]`) for counts + global rules.
  Long-range "facing": high-weight soft term or graph edge.
- **AC:** "exactly one door per room" + one long-range facing constraint satisfied deterministically.
- Grounding: Smith & Mateas 2011; Karth & Smith; Kim 2020; GameAIPro Ch.26.

## Stage 5 — Extensibility acceptance tests (falsifiable)
1. **Backend swap** — register an alternate `Solver` for a rule group; grammar unchanged.
2. **Asset swap** — repoint manifest at `assets/kenney_prototype-kit/`; **zero code diff**.
3. **Domain swap** — instantiate `Region` for an exterior/parcel layout + one new predicate
   `aligned(a, road)`; reuse the same orchestrator + grammar.
- **AC:** all three pass → the three extensibility axes hold.

**Future seam (unscheduled):** `LearnedSolver`/`LlmSolver` implementing the same trait — a drop-in if
a dataset/LLM budget ever appears (Infinigen Indoors 2024 shows pure-procedural already reaches SOTA).

---

## 4. Cross-cutting invariants (hold in every stage)
- **One path (`CLAUDE.md`):** no fallbacks/legacy/stubs. Room-graph WFC fails **loudly** (panic on a
  real bug); furniture solve degrades to `Outcome::Partial` (a legitimate design outcome, not a hidden
  substitute) — these are different passes, each with exactly one path.
- **No panics on the solve path:** `Solver::solve` returns `Result`; contradictions/timeouts →
  bounded restarts → `Partial`. Fix `wfc.rs`'s furniture-path convergence (R3).
- **Engine-free solving:** `ir.rs`/`solver.rs`/`solvers/*` import zero `bevy::` types.
- **Determinism:** one `ChaCha8Rng` stream; per-region sub-seeds; `bevy_rand` for any ECS randomness.
- **No `unwrap()`** on fallible paths (`CLAUDE.md`); handle errors.
- **Cite papers in comments** where their algorithm is implemented (`CLAUDE.md`).
- **Serializable IR** so a solver can become an external process later.

## 5. Risk → mitigation → where (from vetting §5)
| Risk | Mitigation | Stage |
|---|---|---|
| R1 hand-tuned MCMC weights | weights in RON per-profile; future LearnedSolver fits them | 3 |
| R2 cardinality + long-range hard constraints gap | `ConstraintSolver` (Hard/Global/Cardinality) | 4 |
| R3 WFC NP-hard contradiction → panic | bounded restarts → `Partial` | 1–2 |
| R4 roles leak on groups/stacking | open role set + first-class group scopes | 0, 3 |
| R5 cross-room constraints | region adjacency graph first-class | 0 |
