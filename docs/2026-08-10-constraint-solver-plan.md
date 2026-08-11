# Plan: replace the greedy collapse with a constraint solver

**Written 2026-08-10.** Durable on purpose — this is the handoff for a fresh context.

---

## 0. Why, in one paragraph you should not skip

FVS-R-9 measured the composition grammar: **128 solves, zero enclosed regions** (`docs/research/2026-08-10-expressive-range.md`). The first diagnosis was `Empty`'s weight — it asks for 20% of cells and takes 37.58%. **That was falsified the same day** (§7 of that doc): sweeping the weight to **0.0** removes `Empty` from the output entirely and buys *two* enclosed regions in 128, with median enclosure never leaving 0.000. Row 3 never fires at any weight, so the alphabet is not over-constrained either. And a hand-laid room is **legal** under the learned support (`the_kit_can_build_a_room_the_metric_calls_enclosed`).

So it is **not vocabulary** (FVS-R-5's nine missing kinds), **not weight**, and **not over-constraint**. It is the solver: WFC is a greedy, non-backtracking approximation of constraint solving (Karth & Smith), and a closed boundary is a property *over distance* that local pairwise support cannot express. No reweighting of local choices produces one — which is what the sweep measures.

`docs/research/2026-08-09-composition-grammar-decisions.md` §4.3 already anticipated this: *"If the solver is ever swapped, this row is the first thing to revisit."*

## 1. The target

Cooper's **Sturgeon** (`10.1609/aiide.v18i1.21944`, in the corpus, already cited by this project for tags). Its mid-level constraint API is *"only a few functions"* over Boolean variables, and is implemented against SAT, Answer Set **and** SMT back-ends — so the architecture is solver-agnostic by construction.

Four rule families, mapping onto what exists here:

| Sturgeon rule | Here |
|---|---|
| tile rules | exactly one prototype per cell — implicit in `collapse_grid` today |
| pattern rules | the support relation `grammar::from_compositions` already learns |
| distribution rules | the weight knob, done as a **constraint** not a sampling bias |
| **reachability rules** | **the closure problem** — the thing that does not work today |

Enclosure *is* a reachability property: `range::measure` already flood-fills from the border to find cells unreachable from outside. Same graph, moved from measurement to constraint.

## 2. Layers

**L1 — `crates/emerge-core/src/constraints.rs`.** Five functions, Sturgeon's API:

```rust
pub struct Var(u32);
pub struct Lit { var: Var, positive: bool }
pub struct Problem { /* clauses, soft weights */ }

impl Problem {
    pub fn var(&mut self) -> Var;
    pub fn conj(&mut self, lits: &[Lit]) -> Lit;
    /// `weight: None` is hard, `Some(w)` soft.
    pub fn count(&mut self, vs: &[Lit], lo: u32, hi: u32, weight: Option<u32>);
    pub fn implies_any(&mut self, l: Lit, ms: &[Lit], weight: Option<u32>);
    pub fn solve(&mut self, seed: u64) -> Result<Solution, String>;
}
pub struct Solution { /* ... */ }
impl Solution { pub fn get(&self, v: Var) -> bool; pub fn unmet(&self) -> u64; }
```

`unmet()` is the objective — the weight of soft constraints it could not meet. That is what turns today's flat refusal into *"everything except the north corridor"*.

**L2 — rules over `Problem`.**

```rust
pub fn tile_rules(p: &mut Problem, cells: usize, protos: usize) -> Vec<Vec<Var>>;
pub fn pattern_rules(p: &mut Problem, place: &[Vec<Var>], support: &[Vec<u32>; 4], w: usize, h: usize);
pub fn distribution_rules(p: &mut Problem, place: &[Vec<Var>], target: &[f64], weight: u32);
pub fn enclosure_rules(p: &mut Problem, place: &[Vec<Var>], faces: &Faces, w: usize, h: usize, want: f32);
```

**L3 — enclosure, the hard part.** Needs `outside[c]` = "reachable from the border across seams no wall blocks", pinned to exactly the flood-fill answer:

```
border(c)                    -> outside[c]
outside[n] & seam_open(c,n)  -> outside[c]
outside[c]                   -> border(c) | ⋁ₙ (outside[n] & seam_open(c,n))    // justification
```

The third line admits **circular justification** — two cells vouching for each other with no path to the border — unless foundedness is enforced. **ASP's minimal-model semantics gives this free; that is why Cooper used clingo.** In SAT it needs an explicit level/rank encoding (each reached cell carries an integer one greater than its justifier, ~8 bits at 144 cells). This is the single most likely place to get it subtly wrong.

**L4 — the seam.**

```rust
pub struct Wishes { pub min_rooms: u32, pub enclosure: f32, pub keep_distribution: u32 }

pub fn solve_constrained(
    map: &Map, g: &Grammar, faces: &[Option<Interface>],
    wishes: &Wishes, cell: f32, seed: u64, next_id: impl FnMut() -> String,
) -> Result<Solved, String>;
```

**Returns the same `Solved`**, so `generate_from`, the stamp write-back, `Undo::Stamped` and `redraw_stamps` are untouched. That is what makes this a solver swap rather than an editor rewrite.

`wfc::collapse_grid` **stays** — the dungeon uses it. Only the composition path moves, so there is still one path per feature.

## 3. The checkpoint that de-risks everything

**Run L1 + tile rules + pattern rules only, and reproduce today's behaviour through the new machinery** before any global constraint exists. Same kit, same region, same seeds; the output should be the same *kind* of thing `expressive_range` reports now (wall confetti, zero enclosed regions). If that does not hold, the encoding is wrong and no amount of reachability work will save it.

Only then add enclosure and re-measure.

## 4. How it gets judged

**The measurement already exists and does not change.** `cargo run -p emerge-core --example expressive_range` reads the same four rows committed in `2026-08-09-composition-grammar-decisions.md` §4.2 and §4.5, before any of this was built:

- median enclosure `< 0.15` → wall confetti
- median enclosure `> 0.95` with opening density `< 0.5` → sealed boxes
- `H / ln 36 < 0.25` → one hot spot
- max-bin share `> 50%` → the same, seen the other way
- **gate:** `> 20%` non-convergence → nothing else is interpretable

Success is the histogram no longer being empty, judged against those numbers. **Do not edit them.**

## 5. Open questions the research agents are answering

Written to `docs/research/2026-08-10-*.md` so they survive a context clear:

1. **Which solver.** Pure-Rust SAT keeps `emerge-core`'s `ALLOWED_DEPS` ratchet plausible and keeps determinism auditable; clingo is the paper's default and gets foundedness free but is a C++ dependency. → `-solver-choice.md`
2. **The encodings.** Cardinality (`count`) and founded reachability, concretely, with the failure modes. → `-constraint-encodings.md`
3. **What the corpus already holds** on SAT/ASP for PCG — Sturgeon cites Aloul et al. for SAT pathfinding. → `-pcg-solver-corpus.md`

## 6. Non-negotiables carried from the repo

- **Determinism is the load-bearing invariant.** The solver is deterministic given identical input, so *variety must come from varying the problem per seed* (seeded soft weights via `det_rng`), never from solver randomness. Encoding order becomes load-bearing in exactly the way `sort_total!` exists to police.
- **`emerge-core` is engine-free and ratcheted** — `tests/engine_free.rs` `ALLOWED_DEPS` is `serde, serde_json, ron, rand, rand_chacha, det_rng`. Widening it is a deliberate edit with an argument, or the solver lives in a new crate.
- **No `unwrap()`, no fallbacks, one path per feature.** A solver failure is a named refusal, not a degraded result.
- **Mutation-test every new assertion.** Three coverage gaps were found that way today.
- **`cargo test --workspace`** is the gate; a live editor on BRP 15702 inverts `bevy_debugger_mcp`'s `test_highlight_entities`, so kill it by PID first.
