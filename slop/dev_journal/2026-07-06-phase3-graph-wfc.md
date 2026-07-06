# Dev Journal: 2026-07-06 — Phase 3: graph-based WFC (hand-rolled, organic non-lattice dungeon)

**Session Duration:** ~one focused session (plan verification → 6 staged commits → in-engine devshot)
**Walkthrough:** None
**Plan:** `slop/research/2026-07-06-phase3-graph-wfc-plan.md` (roadmap) + the approved execution plan.
**Branch:** `dungeon-wfc-improvements` (commits `b54d7d8` → `57f3144`)

## What We Did

Finished the last piece of the WFC dungeon work: a **config-selected alternative topology**. Instead of
the fixed `coarse_w × coarse_h` lattice, `Topology::Graph` scatters room sites by Poisson-disk sampling,
connects them with a hand-rolled Delaunay graph, and collapses *which edges are corridors* with a
graph-generalized WFC. `Topology::Grid` stays the default and byte-identical — this is first-class config
routing, not a fallback (one path).

Shipped as six independently-tested commits:

- **Step 0 — golden gates.** Before touching anything, captured golden fingerprints of the *current*
  behaviour: an FNV hash of the full `Dungeon` output (walkable mask, spawn, every region's
  rect/tags/adjacency/openings) at liminality 1.0 **and** 0.0, plus an exact-output golden on
  `collapse_grid`. These are the byte-identical gate for the refactor.
- **Step 1 — the seam.** Extracted `Dungeon::generate`'s fine carve into `CoarseLayout { sites,
  adjacency, spawn_site }` + `expand_to_fine`, a topology-agnostic carver both front-ends converge on.
  The grid front-end (`grid_layout`) restates the old coarse phase; `expand_to_fine` derives per-edge
  direction from `sign(center_b − center_a)`, which for grid block-centres is exactly the old cardinal.
- **Step 2 — shared collapse core.** Factored `observe_min_entropy` + `collapse_one` out of
  `collapse_grid` so the graph collapse can reuse them, preserving the exact `below → unit` RNG draw order.
- **Step 3 — `collapse_graph`.** Variable-degree graph WFC: a degree-`d` node's `u32` domain is its `2^d`
  **port-link patterns** (bit `i` = port `i` is a corridor). Socket rule off-grid: across edge `(a.p ↔ b.q)`,
  `bit_p(a) == bit_q(b)`. Degree capped at 5 (`2^5 = 32` = the mask width).
- **Step 4 — `src/geom.rs`.** New engine-free module: Bridson Poisson-disk, Bowyer–Watson Delaunay, and a
  longest-edge degree prune, all deterministic from `DetRng`.
- **Step 5 — wiring.** `Topology` enum on `DungeonConfig` (`#[serde(default)] = Grid`), `graph_layout`
  front-end, `Dungeon::generate` routing, validation, and integration tests. Removed the interim
  `#[allow(dead_code)]`s once the chain went live.

68 tests green throughout; grid stayed byte-identical at every step; verified in-engine via devshot.

## Bugs & Challenges

### Four byte-identical traps in the refactor (caught before writing code)

**Symptom:** The `CoarseLayout` + `expand_to_fine` extraction *looked* trivially behaviour-neutral, but a
Plan-agent pressure-test surfaced four places where a naive port would silently drift the grid output.

**Root Cause + Solution (each):**
1. **Spawn selection.** The old spawn rule is `argmin` over **slot coords** vs `(cw/2, ch/2)`. Re-deriving
   it from fine `site.center` shifts the reference point by `(block/2, block/2)` and flips near-ties →
   different `spawn`. Fix: `CoarseLayout.spawn_site` is **front-end-computed**; `expand_to_fine` just
   returns `sites[spawn_site].center`.
2. **Corridor as a zero-length L.** Composing every corridor as "horizontal leg + vertical leg" is *not*
   byte-identical for axis-aligned (grid) edges: the zero-length second leg still stacks its perpendicular
   lanes at the corner, carving cells the old straight carve never touched. Fix: `carve_corridor` branches
   on colinearity — grid edges take the single-straight-run path and never execute the L code.
3. **The `wfc_weights` rock trap.** In the grid, `rock = 6.0` dominates *because rock is discarded as
   negative space*. In the graph every site is a room, so a 0-link pattern is an isolated room the
   largest-component prune throws away — reusing rock weight makes isolation the heaviest outcome and
   collapses connectivity. Fix: a dedicated `link_weights` profile (0-link a small epsilon, 1–2 links
   dominant), **not** `wfc_weights`.
4. **Degree cap after symmetrization.** Delaunay's average degree is ~6, so degree > 5 is the *common*
   case, not an edge case. "Keep 5 nearest, then union for symmetry" can rebound a node to degree 6 →
   overflows the `u32` mask. Fix: prune to ≤5 on the **final symmetric graph**, removing longest edges
   (deterministic tie-break) from over-degree endpoints.

**Lesson:** "Behaviour-neutral refactor" deserves an adversarial read *and* a mechanical gate. The golden
snapshot (Step 0) turned every "is this still identical?" question into a `cargo test`, and caught nothing
only because the four traps were designed out up front.

### The `never contradicts` proof was right-conclusion, wrong-reason

**Symptom:** My first justification for `collapse_graph` never returning `None` was "pattern 0 (all-walls)
always exists, so a global solution exists." That's a non-sequitur — greedy WFC can dead-end even when a
solution exists (that's *why* the grid has a retry loop).

**Root Cause:** The real guarantee is the **independent-bits invariant**: after any propagation, each port
forces its bit to 0, to 1, or leaves it free — never both-forbidden (each neighbour always keeps ≥1
survivor), and distinct ports are distinct bit positions, so the pattern "set each forced bit, clear the
rest" always survives. No domain can empty, under any weights or collapse order.

**Solution:** Wrote the invariant into the `collapse_graph` doc comment; kept the `Option` return anyway to
defensively surface a malformed (degree > 5) table, mirroring `collapse_grid`.

**Lesson:** "It works in the tests" isn't a proof. The fuzz sweep (150 collapses across ring sizes/seeds)
*confirmed* never-contradicts, but the invariant is what makes it true — and it's stronger than I first thought.

### RON encodes `[f64; 6]` as a tuple, not a list

**Symptom:** `graph_config_validation` failed with `Expected opening '('` at the `link_weights` field. The
"valid" config that should parse+generate wouldn't parse.

**Investigation:** Ran the failing test with `--nocapture` to get the real RON error (the initial
grep-filtered output truncated it to just the test name).

**Root Cause:** serde deserializes a fixed-size array `[T; N]` via `deserialize_tuple`, not
`deserialize_seq`. So in RON a `[f64; 6]` field must be written `(0.05, 1.2, …)`, **not** `[0.05, 1.2, …]`.
My test (and any real `dungeon.ron`) used list syntax.

**Solution:** Wrote `link_weights` with tuple parens. A side-effect worth noting: the `is_err()` cases were
previously passing for the *wrong* reason (parse error, not validation rejection) — fixing the syntax made
them actually exercise the validation path.

**Lesson:** When a serde array field won't parse, check tuple-vs-list. And an `is_err()` assertion that
passes doesn't tell you *why* it erred — make the happy-path parse succeed first.

### Environment friction: foreground `sleep` blocked; hidden-window screenshots

**Symptom:** Couldn't `sleep` in the foreground to wait for the game to boot / the devshot PNG to land.

**Solution:** Used background `until`-loops (`run_in_background`) that exit on the boot signal *or* a panic,
so a crash wouldn't look like "still booting." The devshot came back at 14.5 MB (a real frame; a
hidden-window capture would be ~57 KB black), showing organic non-lattice rooms + a branching corridor.

**Lesson:** When watching a process for an outcome, the wait condition must match the failure signatures too
— silence is not success.

## Code Changes Summary

- `src/dungeon.rs`: `Site`/`CoarseLayout`/`Topology`; `grid_layout` + `expand_to_fine` + `carve_corridor`
  (the shared carver, with the L-route for diagonal graph edges); `graph_layout` + helpers
  (`port_neighbors`, `corridor_edges`, `largest_graph_component`, `build_graph_layout`);
  `Dungeon::generate` routes on `config.topology`; `parse_config` validates the Graph variant; golden +
  graph integration tests.
- `src/wfc.rs`: extracted `observe_min_entropy` + `collapse_one`; added `collapse_graph` + `propagate_graph`
  + `MAX_DEGREE`; a `collapse_grid` draw-order golden. `collapse_grid`'s signature + `propagate` untouched.
- `src/geom.rs` (NEW): `poisson_disk`, `delaunay_edges`, `prune_to_max_degree` + in-circle/edge helpers.
- `src/main.rs`: `mod geom;`.

## Patterns Learned

- **Golden-snapshot gate for behaviour-neutral refactors.** Capture a hash of the full output *before*
  refactoring (from the old code — a post-refactor snapshot only proves code equals itself), bake it into a
  test, and every step becomes a `cargo test` away from "did I drift?". FNV-1a over the fields (not
  `DefaultHasher`, whose per-process seed isn't reproducible).
- **Per-degree local bit-indexing.** A graph WFC can't use one global prototype table (63 patterns across
  degrees 0–5 overflows `u32`). Instead each node's domain is over its *own* `2^degree` patterns, and the
  pattern index literally *is* the port-link bitmask — the index carries the semantics.
- **One general operation that degenerates to the special case, not two paths.** `carve_corridor` handles
  both axis-aligned and diagonal corridors; the colinear branch is the exact old carve. That's one path
  (honours the one-path rule) as long as the degenerate case is bit-exact — which the golden verifies.
- **Provable non-overlap via Chebyshev.** Per-site square bounds of half-side `½·min-neighbour-Chebyshev`
  guarantee `h_i + h_j ≤ Cheb(i,j)` for adjacent pairs, so rooms never overlap regardless of
  expansion-to-touch (the room stays ⊆ its bounds).

## Open Questions

- **Degree > 5.** The `u32` mask caps node degree at 5. Higher-degree hubs would need a `u64`/`Vec` domain
  — deliberately out of scope; the prune enforces the cap today.
- **Graph corridor aesthetics.** Diagonal L-routes keep both mouths axis-aligned, but two graph neighbours
  can map to the same cardinal wall; the opening cell uses the corridor's lane-0 ∩ wall to separate them.
  Worth an in-engine look at low `site_spacing` (dense sites) to check doorway crowding on short walls.
- **Shipped Graph preset.** No `Topology::Graph` config ships (Grid stays default). If we want it selectable
  at runtime, that's a menu/config-swap feature, not a generator change.

## Next Session

Phase 3 closes the WFC dungeon roadmap (Phases 1–3 + the two review bug-fixes all landed). Candidates for
next: the **reported-not-fixed review bugs** (`resolve_move` NaN/overflow guard; `wall_mesh` silent-UV
degradation; `is_camera_facing` float-`==`; `unreachable!()` direction arms; collision-sampler band-aid),
or exercising the Graph topology in real gameplay (furnish/nav/fog over organic rooms) to surface anything
the tests + one devshot didn't.
