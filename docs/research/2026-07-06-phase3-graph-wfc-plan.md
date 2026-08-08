# Phase 3 plan — graph-based WFC (hand-rolled), non-lattice organic dungeon

**Read first (fresh session):** `~/.claude/projects/-Users-ladvien-foundation-vs-slop/memory/dungeon-wfc-improvements-status.md`
(current status) and the original plan `~/.claude/plans/make-a-plan-to-composed-bentley.md`. Work is on
branch **`dungeon-wfc-improvements`** (5 commits). The pre-existing uncommitted `ai/*`, `crab`, `enemy`,
`nest`, `util` changes in the working tree are **not ours** — never stage them.

## Context (what already exists after Phase 1 + 2)

- `src/wfc.rs` — `collapse_grid(width, height, weights, support: &[Vec<u32>;4], initial: &[u32], seed)
  -> Option<Vec<usize>>`: grid WFC. `propagate()` is a shared arc-consistency helper. `generate()` builds
  the coarse room alphabet (6 prototypes × rotations) + a boundary `initial` domain. **Two callers of
  `collapse_grid` must keep working: the dungeon grid path and `placement/solvers/wfc.rs` (furniture).**
- `src/dungeon.rs` — `Dungeon::generate(&DungeonConfig) -> Result<Self, String>` runs the coarse grid
  WFC (with a zero-room retry), then carves the fine grid: per-block room (type-driven size via
  `pick_room`, `jitter_origin` offset, expansion toward `linked` edges), straight center-to-center
  corridors, doorway necking, `Region` openings/adjacency. `largest_room_component` keeps the largest
  connected slot component. `DungeonConfig` (RON) holds `coarse_w/h`, `block`, `corridor_width`, `seed`,
  `max_attempts`, `liminality`, `wfc_weights`, `room_types`.
- Determinism: one seeded ChaCha8 RNG (`src/rng.rs`, `seeded`/`DetRng`). **`Date::now`/`Math::random` are
  unavailable/forbidden** — everything flows from `config.seed`.

## Goal

Add a **config-selected alternative topology**: instead of the fixed `coarse_w × coarse_h` grid, place
room sites irregularly (Poisson-disk) and connect them by a Delaunay graph, collapsed by a
graph-generalized WFC (Kim et al. 2020, DOI 10.1587/transinf.2019edp7295). Produces non-lattice, organic
room layouts. **`Topology::Grid` stays the default and is unchanged; `Topology::Graph` is the new path.**
This is first-class config routing, **not** a fallback (one-path rule).

## The key seam: refactor to `CoarseLayout` + `expand_to_fine` FIRST

Both topologies must converge on one fine-grid carver so furnish/nav/fog/occlusion never change. Do this
refactor before writing any graph code — it is behavior-neutral for the grid and de-risks everything:

```rust
struct CoarseLayout {
    // A room "slot": its site (fine-grid centre) and per-slot block extent for sizing/jitter/expansion.
    sites: Vec<Site>,                 // Site { center: IVec2, bounds: Rect2 }  (bounds = the slot's block)
    adjacency: Vec<(usize, usize)>,   // undirected corridor links between kept sites (already trimmed to
}                                     // the largest connected component)

fn expand_to_fine(layout: &CoarseLayout, config: &DungeonConfig, rng: &mut impl DetRng)
    -> (Vec<bool> /*walkable*/, Vec<Region>, IVec2 /*spawn*/)
```

`expand_to_fine` is the current `dungeon.rs` carve, generalized to take sites+adjacency instead of the
grid loop: per-site room (reuse `pick_room`/`jitter_origin`/expansion, bounded by `site.bounds`), carve a
corridor per `adjacency` edge between the two site centres (reuse the current straight carve + necking),
record openings/adjacency. **Grid front-end:** build `CoarseLayout` from today's kept grid (sites = block
centres, bounds = blocks, adjacency = kept Link edges) and call `expand_to_fine`. Assert the existing
tests still pass **byte-identical** at `liminality 1.0` (this is the correctness gate for the refactor).

## Graph front-end (the new work)

### 1. Sites — Poisson-disk sampling (`src/dungeon.rs`)
Bridson's algorithm over the level rect (`coarse_w*block × coarse_h*block`), min-distance `~block`,
seeded from `config.seed`. Gives ~`coarse_w*coarse_h` irregularly-spaced sites. Each site's `bounds` =
its Voronoi cell bbox clamped to a max (so room sizing/expansion still has a bounded block-like extent).
Config: `Topology::Graph { site_spacing, ... }`.

### 2. Delaunay — hand-rolled Bowyer–Watson (`src/dungeon.rs`, or a new `src/geom.rs`)
Incremental Bowyer–Watson triangulation of the sites → triangle set → undirected edge set. This is the
substantial hand-rolled piece; keep it engine-free + unit-tested. **Determinism:** insert sites in a
fixed order; break geometric ties deterministically. **Robustness:** use a large super-triangle and
tolerance-based in-circle tests (watch for collinear/cocircular degeneracies — Poisson spacing helps).
Then **prune each node to its K nearest neighbours (K ≤ 5)** — see the mask cap below.

### 3. `collapse_graph` (`src/wfc.rs`)
Generalize the grid collapse to a variable-degree graph:
```rust
pub fn collapse_graph(
    n_nodes: usize,
    weights: &[f64],
    neighbors: &[Vec<usize>],   // neighbors[node] = adjacent node indices (its ports, in order)
    support: &[Vec<u32>],       // support[?] — see the compatibility model below
    initial: &[u32],            // per-node starting domain (encodes the degree-restricted alphabet)
    seed: u64,
) -> Option<Vec<usize>>
```
Reuse the observe(min-entropy)/collapse(weighted)/propagate loop from `collapse_grid`; the only change is
iterating `neighbors[node]` instead of the 4 fixed offsets. **`collapse_grid` can then become a thin
adapter** over `collapse_graph` (build the 4-regular in-bounds neighbour lists, delegate) — verify it
stays byte-identical for the grid, or keep `collapse_grid` standalone if the adapter risks drift.

**Compatibility model (the crux — resolve this first):** a node of degree `d` collapses to a
**port-link pattern** (which of its `d` incident edges are corridors). Across edge `(a,b)` where `b` is
`a`'s port `i` and `a` is `b`'s port `j`, compatibility requires `a`'s bit `i == b`'s bit `j` (a Link
meets a Link — the socket rule, generalized off the grid). Two sub-decisions:
- **Domain representation / `u32` mask cap.** `2^d` patterns per degree-`d` node; the `u32` mask caps the
  per-node alphabet at 32 ⇒ **degree ≤ 5** (`2^5 = 32`). Enforce by pruning Delaunay to the 5 nearest
  neighbours (step 2). If higher degree is wanted later, widen the domain to `u64`/`Vec` — out of scope
  now. `initial[node]` selects that node's degree-`d` alphabet slice.
- **Weights.** Bias toward 1–2 links per node (mirror the grid intent: `dead_end`/`corridor`/`corner`
  favored, `cross` rare) so the graph reads as rooms-with-a-few-doors, not a mesh.

Because the alphabet is per-degree, either (a) build one global prototype table indexed by (degree,
pattern) and set each node's `initial` to its degree's slice, or (b) build per-node weights/support on
the fly. Option (a) is cleaner and matches `collapse_grid`'s shape.

### 4. Connectivity + `CoarseLayout`
Generalize `largest_room_component` to the adjacency list (flood-fill over linked edges, keep largest
component). Emit `CoarseLayout { sites (kept), adjacency (linked edges within the component) }`, then
`expand_to_fine`. Corridors between arbitrary site centres are non-axis-aligned — the corridor carve must
handle diagonal routes (carve an L: horizontal then vertical between centres, keeping the room-mouth
segment axis-aligned so necking/openings still work). This L-route is also the natural home for the
**bent corridors** deferred in Phase 2.

### 5. Config routing (`src/dungeon.rs`, `assets/dungeon.ron`)
```rust
#[derive(Debug, Clone, Deserialize)]
enum Topology { Grid, Graph { site_count: usize } }   // add `pub topology: Topology` (#[serde(default)] = Grid)
```
`Dungeon::generate` routes on `config.topology`: `Grid` → today's grid front-end; `Graph` → sites →
Delaunay → `collapse_graph` → `CoarseLayout`. Both → `expand_to_fine`. Each topology fails loud (same
retry-then-`Err` contract) if it can't yield ≥1 room. `#[serde(default)]` so the shipped `dungeon.ron`
(no `topology` field) stays `Grid` — no behavior change until opted in.

## Suggested sequencing (each step builds, tests, commits independently)

1. **Refactor** current carve → `CoarseLayout` + `expand_to_fine` + Grid front-end. Behavior-neutral;
   all existing tests pass byte-identical. *Commit.*
2. **`collapse_graph`** + tests: determinism; on a 4-regular grid-shaped graph it matches `collapse_grid`
   (equivalence test); contradiction handling; `initial` degree-slice restriction. *Commit.*
3. **Bowyer–Watson** (+ Poisson-disk) in `geom.rs` + tests: every site is a triangulation vertex, no
   overlapping triangles, deterministic for a seed, degree-prune to ≤5. *Commit.*
4. **Wire Graph front-end** + `Topology` config + route + diagonal L-corridor carve. Tests: Graph
   topology generates a non-empty, connected, non-overlapping dungeon; Grid unchanged. **devshot** at
   `Topology::Graph` (temporarily set it in `dungeon.ron`, screenshot, revert). *Commit.*

## Verification
- `cargo test` green throughout; Grid path byte-identical after step 1 (the refactor gate).
- Reuse test patterns already in `dungeon.rs`/`wfc.rs` `#[cfg(test)] mod tests`.
- devshot per CLAUDE.md: `touch screenshot.request; sleep 1.5;` read `screenshot.png` (>150 KB = real
  frame). Keystroke injection is blocked; the squad-follow camera zooms in, so read the crab boot log
  line (`surface graph built — N patches`) as the layout-scale signal, like Phase 2 did.

## Guardrails
- **One path:** `Topology` is config-selected routing, not a fallback; each topology fails loud.
- **Determinism:** all randomness from `config.seed` via `DetRng`; fixed insertion/tie-break order in
  Delaunay. No `Date`/`Math.random`.
- **No panics** in gameplay code (`Result` + `?`); config-load/gen invariant failures panic loud at
  startup only (the sanctioned door).
- **Don't break `collapse_grid`'s two callers** (dungeon grid + furniture solver).
- **Scope:** degree ≤ 5 (u32 mask); higher-degree domain widening is a later follow-up.
