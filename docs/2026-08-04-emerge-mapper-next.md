# emerge-mapper — what to do next (handoff, 2026-08-04, second session)

Read `docs/2026-08-04-emerge-mapper-handoff.md` first: it is the no-prior-context document and its
§4 "Traps this work paid for" is still accurate and still worth your time. This one is narrower — it
is the *recommendation list*, written at the end of a long session by the agent that made the mess it
describes.

Everything below is on branch **`feat/emerge-lattice`**, nine commits off `bc7a92a`.

---

## 1. Where it stands

| suite | result |
|---|---|
| `cargo test --lib` (game) | 975 passed, 1 ignored |
| `cargo test -p emerge-core` | 220 + 2 + 4 |
| `cargo test -p emerge-mapper` | 31 |
| `cargo check -p emerge-core -p emerge-mapper` | **zero warnings** |

Two failures are **not yours** and were broken before any of this: `tests/lore_canon.rs` (fails on the
user's untracked `docs/lore/2026-08-01-scp-gear.md` — leave it alone) and two of `tests/replay.rs`
(`authored_world_config_override_is_a_noop`, `deterministic_core_is_bit_identical_across_many_builds`).
The hash gates that matter — `deterministic_core_is_bit_identical` and the
`a0_fvs_j6_mutant3_on_world_0x5c09191_reproduces` golden — are green.

**The one number that should shape your priorities: there are still zero authored `SubCell`s in the
repo.** The lattice is now authorable (that was this session's work) and nothing has been authored with
it. Every design argument below is therefore still unvalidated against real data.

---

## 2. Do these first, in this order

### 2.1 Project-level subgrid divisions (the user's `#6`) — **start here**

The user's own formulation, which is better than the one I proposed and is the one to build:

> "If it's 1 metre per tile, then each subgrid unit should measure 1m/X, where X = the number of
> divisions in a tile. Oh, hey, I guess that means we only need 1 number."

So: the **project** declares divisions-per-tile `X`; a subunit is `grid::SNAP / X` (SNAP is 0.5 m
today, `crates/emerge-core/src/grid.rs:12`); a piece spanning N tiles gets `N·X` divisions on that
axis, derived from `grid::cells(footprint)`.

**Why this is first:** it is the only change that makes edge tokens *comparable*. Today `div` is
per-descriptor and defaults to `(3,3,3)`, so a 3 m wall has 1 m cells and a 1 m chair has 0.33 m cells
— two tiles whose faces can never mean the same thing. Everything downstream (2.2, the adjacency
geometry fix in 3.5, the face picking in 2.4) is cheaper after this and has to be redone if it lands
first.

**Shape.** `Subgrid` loses `div` and keeps `cells`. Add something like
`descriptor::divisions(extent: &Extent, per_tile: u32) -> (u32, u32, u32)`. Then `holds`, `volume`,
`validate`, `rotated`, and the three mutators all take divisions as a parameter — `at()` does not,
it only searches. Call sites: `crates/emerge-core/src/{descriptor,adjacency,library}.rs` and
`crates/emerge-mapper/src/tiles.rs`.

**Where X lives:** `policy::Policy` (the `project.ron` struct) is the natural home — it is already the
per-project layer and `Project::open` already reads it. It is `deny_unknown_fields`, so add the field
with a default and old files keep parsing.

**Gotchas.**
- `Subgrid::validate` currently refuses a zero axis; with derived divisions a zero can only come from
  `X = 0`, so validate `X` at the project boundary instead and the per-lattice check gets simpler.
- `adjacency::may_abut`'s "faces of different lengths never agree" test stops being reachable for
  same-size tiles, which is the point — but keep it, because differently-sized tiles still differ.
- Re-pin nothing: no `FixedUpdate` system reads this, so `snapshot_hash` cannot move.

### 2.2 Auto-mark `solid` from the mesh (the user's last request)

The user asked for this and it is the thing that makes 2.1 pay off: nobody hand-marks 27 cells, let
alone `N·X³`.

**Bounding boxes will not work** and the user half-suspected it — one box per mesh is by definition the
whole extent, so every cell comes out solid. Three options that do:

1. **Per-primitive boxes.** POSITION accessors carry `min`/`max` per primitive, already in the JSON
   `emerge_core::glb` reads. Free. Useful for kitbashed multi-part meshes, useless for a
   single-primitive chair.
2. **Vertex occupancy.** Read POSITION, mark the cell each vertex falls in. Cheap enough, honest, and
   its failure mode — a large flat face spanning a cell with no vertex in it, e.g. a tabletop's middle
   — is *visible* in the grid rather than silent.
3. **Triangle rasterisation.** Mark every cell a triangle passes through. Correct, most work.

**Recommendation: build 2**, and put the marked-cell count in the status line so under-marking is
obvious. `emerge_core::import::triangles` already reads accessors without vertex data; this is the
first thing that needs the buffer, so expect to extend `glb.rs`.

Keep it a **button**, not automatic-on-import: it overwrites hand-authored cells, and an author who
tuned a lattice should not lose it to a rescan.

### 2.3 `write_library` corrupts the kit (review finding — most serious unfixed)

`Project::open` builds `library` through `policy::layered_library` (measurements ⊕ `project.ron`
patches). `tiles::write_library` serializes *that* back over `library_path`. So under `--kit site`,
toggling one lattice cell writes one facility's stretched wall heights into the measurements file the
kit is meant to share — and the next load applies the patches again, on top.

**Shape.** Keep the unlayered `Library` beside the layered one in `Project` (`measured` and `library`,
say), edit and write `measured`, and re-layer after every write. It is not hard; it is *delicate*, and
it silently damages an asset file, which is why I did not start it thin.

### 2.4 Then the rest of the user's list

- **`#5`/`#8` orientation.** The user chose **two separate things**: an authored XYZ default rotation
  (snapped to 90°) *and* `front` naming a **subgrid face** rather than an angle. Today `align.front` is
  a single yaw in degrees, *derived* by `glb::derive_front` from bbox asymmetry, with no UI. A 90° turn
  about X or Z swaps height and depth, so `extent` must be re-derived — that is the trap.
- **`#9` ray picking.** The user chose **cell picking plus face picking**: hover reports the cell and
  which of its six faces the ray entered, click selects it, and typing lands an edge token on that
  face. Do it after 2.1 so the lattice it targets is stable.

---

## 3. The eight unfixed review findings

A max-effort review ran over this branch (67 agents, 66 verified findings collapsing to 15). Seven are
fixed in `e30f96f`. These eight are not, ranked by how much damage they do:

| # | What | Where | Why it was skipped |
|---|---|---|---|
| 1 | `write_library` bakes the policy layer into the measurements file | `tiles.rs:792` | See 2.3 — delicate, damages an asset file |
| 2 | A rig missing from `rigs.ron` panics an unrelated system at Startup | `src/crab/setup.rs:32` + 5 more | Needs a decision about where "the rigs the game requires" is written down |
| 3 | Arrow keys select filtered-out library tiles; `Delete` then removes one the author never saw | `tiles.rs:1601` | `anim_tab::keep_selection_visible` is the pattern to copy; wanted GUI verification |
| 4 | `faults` pairs by centre cell, so touching pieces are unchecked and non-touching ones are compared | `adjacency.rs:230` | Interacts directly with 2.1; doing it first means doing it twice |
| 5 | Focus can move between opening a text field and committing it, redirecting the edit | `tiles.rs:298` | Fix is to capture the target at field-open; touches all four fields |
| 6 | `PreviewOf` keyed on a non-unique id, so two candidates sharing a GLB stem show one mesh | `tiles.rs:1749` | Four real collisions in the tree (`wall`, `column`, `pipe`, `crate`) |
| 7 | `check_edges` is O(n²) with a `Descriptor` clone per placement, rerun on every placement | `editor.rs:912` | ~7.8M comparisons after a 1,400-piece flood fill |
| 8 | `patched_with` uses `Subgrid::default()` as an "unset" sentinel, which is also the commonest legal value | `descriptor.rs:584` | 2.1 changes `Subgrid`'s shape; fix it in the same pass |

Full text with failure scenarios: the workflow output under
`/private/tmp/claude-501/.../tasks/wnatfhh55.output` if it survives, otherwise re-run
`/code-review max`.

---

## 4. Traps paid for this session (beyond the first handoff's §4)

- **The user works on this machine.** `scripts/macinput.py` steals the keyboard and mouse. **Tell them
  before driving the GUI**, every time. They are fine with it; they are not fine with being surprised.
- **Verify the window is frontmost before believing an input test.** I "reproduced" a placement
  regression that did not exist because VS Code had focus and my clicks went there:
  `osascript -e 'tell application "System Events" to get name of first process whose frontmost is true'`
  and loop until it says `emerge-mapper`.
- **`Esc` does not reach Bevy in borderless fullscreen** — the OS appears to take it. This is why
  removal mode felt inescapable, and why arming a piece from the palette now also exits it. Do not
  bind anything important to `Esc` alone.
- **Measure, do not squint.** I nearly reported that the shortcuts scrim failed to dim the side
  panels; sampling pixels showed every region dimming by the same ratio. Twice in one session a
  pixel comparison overturned what the screenshot "looked like".
- **`BEVY_ASSET_ROOT`.** Running the binary directly resolves assets against the *executable's*
  directory, not the cwd. Set `BEVY_ASSET_ROOT=<project>` or every mesh 404s.
- **home-still's distill service (`192.168.1.110:7434`) is down**, so `distill_search` fails. Grep
  `~/mnt/home-still/markdown` instead — but note it only contains *converted* papers, so a held PDF
  looks missing. `paper_download` is idempotent and cheap; check with it before concluding anything is
  absent. Two papers `grammar.rs` cites looked missing that way and were not.

---

## 5. Decisions still owed by the user

- **`id: "is"`** in `assets/emerge/library.ron`, pointing at `characters/cipher_field.glb` — a
  character mesh in the furniture library, almost certainly a survivor of the accidental-import
  incident. `naming::is_id` will not catch it (`is` is valid snake_case). Delete, or rename to
  `cipher_field` and move it out of that library.
- **`FVS-Q-10`** — should authored `edge` tokens feed `grammar`'s `support` table, or only check it?
  Deliberately deferred until real tokens exist. Karth & Smith 2017 (now converted in the corpus at
  `markdown/10/10.1145_3102071.3110566.md`) names both as WFC's own modes: *simple tiled* (explicit
  tile constraints) and *overlapping* (inferred from the source). `grammar.rs` is the inferred half.
- **`FVS-Q-9`** — `solid` refining `stack::covers`. Blocked on the same authoring problem; much
  cheaper to judge once 2.1 and 2.2 have put real lattices in the library.

---

## 6. What was verified in the running app, and what was not

Trust these — they were driven and screenshotted:

- `Enter` while typing commits the field and leaves `library.ron` at 42 entries; `Enter` with no field
  open still adds (42 → 43). `W A S D` typed into the map-name field leave the world **pixel-identical**.
- Clicking a library tile and pressing `Z` wrote the first authored `SubCell` in this repo's history.
- A conflicting `seam` token produced the EDGES readout naming both pieces and both faces, with a red
  outline on exactly those two placements.
- `--kit site` shows `IN LIBRARY (45)` listing the architecture.
- The shortcuts overlay titles itself per tab and dims panels, tab strip and world alike.
- The description round trip reaches disk (`note: Some("tall lamp")`).
- Three lattice layers render side by side with their headers, and a selection of `1,2,1` highlights in
  the **top** grid.

**Not verified, and worth your eye first:**

- **Clicking a row/column/layer header** (`>`, `v`, `*`). The code shares `apply_verb_to` with the
  chips, which *is* verified — but I never landed a clean click on a header.
- **Tag chips after the fix** in `f47afd2`.
- The default (non-`--kit`) project still showing 42 entries after the `--kit` change — argued by
  construction (the `None` branch resolves to the identical path) and by the suite, not observed.
