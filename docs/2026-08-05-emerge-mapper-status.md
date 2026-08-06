# emerge-mapper — where it stands (2026-08-05)

Supersedes the "do these first" list in `docs/2026-08-04-emerge-mapper-next.md`, all of which is done,
and its findings table, all eight of which are closed. That document's **§4 "Traps this work paid for"**
is still accurate and still worth your time; so is `docs/2026-08-04-emerge-mapper-handoff.md`, which
remains the no-prior-context orientation.

Branch **`feat/emerge-lattice`**, 37 commits off `bc7a92a` (which is where `main` sits), **pushed** as
PR #90. A `/code-review max 90` pass over it produced **15 confirmed findings, all fixed** — §7 below.

---

## 0. Do not drive the GUI

`scripts/macinput.py` takes the keyboard and `EMERGE_FULLSCREEN=1` takes the display, on a machine
somebody is working on. Asked directly, 2026-08-05: *"Can you please stop capturing my keyboard and
monitor? I'm trying to work."*

**Use `harness::build_headless(root, map, kit)`.** It returns the real editor — the same plugin graph
`main.rs` uses, via the shared `add_editor_plugins` — with no window, no wgpu device
(`WgpuSettings { backends: None }`) and no audio. Step it with `app.update()`.
`crates/emerge-mapper/tests/headless.rs` boots it on both the default project and the Site kit, ten
frames each, in **0.2 s with no GPU**.

That covers the class that has actually broken this editor: in Bevy 0.19 a missing `Res<T>` **panics its
system** rather than skipping it, and every run condition is evaluated with no short-circuit. Only
*rendering* is out of scope, deliberately — "does the highlight land on the right cell" is a question
for `descriptor::pick_cell`, answered in arithmetic.

---

## 1. Where it stands

| suite | result |
|---|---|
| `cargo test --workspace` | **1443 passed, 0 failed** |
| `cargo check -p emerge-core -p emerge-mapper -p fvs --all-targets` | **zero warnings** |

**Re-measured 2026-08-05 after the `align.scale` and move-tool work**, since both touch `emerge-core`,
which the game links: `cargo test --workspace` green; `cargo check` still zero warnings; the harness
lane run with **`ci.yml`'s exact command and skip list** gives **38 suites, 0 failed**; and
`tests/replay.rs` on its own gives **21 passed, 0 failed**. `snapshot_hash` has not moved —
`deterministic_core_is_bit_identical`, `..._across_many_builds` and the
`a0_fvs_j6_mutant3_on_world_0x5c09191` golden all pass.

That is the expected result rather than a lucky one, and the reason is worth stating: `placed_height`
computes exactly what `stack::drawn_height` computed, `emerge-bevy` reaches `emerge-core` only through
`resolve_y`, and no shipped map places the one scaled piece. **`emerge_map` also adds nothing at all
unless `FVS_EMERGE_MAP` is set**, so none of this sits on a shipped path.

One red was met and confirmed pre-existing rather than assumed:
`containment::watching_the_feed_makes_it_generate_and_ignoring_it_stops` fails identically from a clean
stash of this work. It is already `ci.yml` skip #1, with its own `BACKLOG.md` entry and a
`tests/skip_debt.rs` guard. It surfaced only because the first local run omitted the skip list — which
is the lesson worth keeping: **run the lane with `ci.yml`'s command, or the result is not comparable
to CI's.**

**`--workspace` is load-bearing and was missing.** This workspace has a root package, so its default
members are the root package *alone* — a bare `cargo test` compiles no test target under `crates/`.
That is how extracting the animation layer into `crates/emerge-anim` dropped its 21 unit tests and the
rigs-manifest drift guard out of the CI gate with nothing going red: the tests were not failing, they
were not running. Every plain `cargo test` in `ci.yml`, `TESTING.md` and `CLAUDE.md` now carries it.

The non-harness suite is **fully green**, and `snapshot_hash` has not moved: both
`deterministic_core_is_bit_identical` and the `a0_fvs_j6_mutant3_on_world_0x5c09191` golden pass.

### The harness lane on this machine — measured against the baseline, not assumed

`tests/replay.rs` (minus the two nightly-only `search_rollouts` tests) gives **17 passed, 4 failed**.
Run again from a worktree at `aea728b`, the commit before this session's work: **the identical 17 and
4**. So none of them is this branch's, which is worth stating with the evidence rather than by
inspection of the diff. Two earlier claims in this doc were wrong and are corrected here:

| test | what it actually is |
|---|---|
| `deterministic_core_is_bit_identical_across_many_builds` | **Not a failure.** It aborts with `thread 'IO Task Pool (0)' has overflowed its stack` unless `RUST_MIN_STACK=33554432` is set — which `ci.yml` sets in `env:` and a local shell does not. With it, it passes. `BACKLOG.md`'s FVS-J entry already describes this; I had been calling it a determinism failure. |
| `field_passes_are_bit_identical` | **Would pass in CI.** *"no field golden is pinned for this architecture yet (goldens are PER-PLATFORM). This run measured `0xe090401cb48e2ae3`."* This is an M-series Mac; the goldens are pinned for `x86_64`. |
| `migrated_defaults_reproduce_the_shipped_golden_hash` | Same — no `aarch64` golden. Measured `0xac8196c4a1bfb0d0`. Its own message says to pin it in the `cfg(not(target_arch = "x86_64"))` arm once the `determinism-arm` lane reproduces it across builds. |
| `photophobia_pulls_crabs_into_shadow` | **A real pre-existing failure**, and it fails in isolation: *"photophobic crabs (gain>0) should occupy darker cells than gain=0 crabs: on=0.195 off=0.114"* — the photophobic group is in **brighter** cells than the control, which is the oracle inverted, not merely unmet. |
| `authored_world_config_override_is_a_noop` | **A real pre-existing failure**, in isolation too: *"installing the authored world config changed the sim — the override seam or encode/decode is lossy"*. |

`BACKLOG.md`'s claim that *"`deterministic_core_is_bit_identical` and `field_passes_are_bit_identical`
both pass at HEAD in isolation"* is stale for the second one: it fails in isolation, for the
per-platform reason above. The harness lane is `continue-on-error` in CI, so neither real failure is
gating — which is exactly how both stayed unnoticed.

**35 authored `SubCell`s** in `assets/emerge/site/library.ron` — the first in this repo. `wall`,
`wall_corner`, `wall_window` and `column` carry `wall` on their run-faces, and a run of them reports no
faults.

---

## 2. What the lattice is now

`Subgrid` lost `div`. Divisions are **derived** from a piece's own size times one project number,
`policy::divisions` (shipped at 1, a 0.5 m subunit). Merrell & Manocha 2009 §4.4–4.5 is the source and
names this exact remedy; `descriptor::Subgrid`'s doc carries the argument. An edge token on a 3 m wall
now means the same thing as one on a 0.5 m chair.

The height is `extent.height * align.stretch_y` — **the piece as placed, not the mesh as measured**. Two
kits that build the same facility derive the same layers, which is pinned as an invariant.

`align.front` names a `Face` (N/E/S/W) rather than an arbitrary yaw. `align.rotate` is an authored
quarter-turn for a mesh exported the wrong way up, baked into `extent` at import so no reader of
`extent` learns it exists. `import::occupancy` rasterises triangles (Akenine-Möller 2001 SAT), so a
wall is solid all the way up.

Editor: screen-aligned WASD panning; the map vocabulary under one hand (`X` removes, `B` moves, `V`
aims straight); `Cmd`/`Ctrl` places freely off the grid (XZ only — Y comes from `mount`); the aim and
piece-turn keys repeat while held at 150 ms; candidates scan themselves on selection; hover-and-click
**ray picks** a lattice cell and names the face you are looking through.

Added 2026-08-05, and the reason each one exists:

- **`B` moves a piece.** Click to pick up, click to put down, `Esc` to put back. Everything resting on
  it comes too, at its own offset — the promise `Placed::on`'s doc has always made (*"move the table
  and the lamp moves with it"*) and which nothing had kept, because nothing could move a table. The
  whole group is re-seated or the move is refused; `emerge_core::stack::move_placement` is the one
  path, and it is a pure function so it is tested without a window. **The Map context is now at its
  twelve-row ceiling** — the next verb there has to share a row or take a key.
- **`EditorState::removing: bool` is now `tool: Tool { Place, Remove, Move }`.** A second bool would
  have made "both armed" and "neither armed" expressible and meaningless.
- **A dragged box places**, the twin of the removal tool's box, at the brush's own cell pitch and as
  one undo entry. Occupied cells are skipped rather than refusing the drag. Note that a plain click
  now places on **release** rather than on press — that is what tells a click from a drag, and it is
  the threshold (`CLICK_EPS`) removal already used.

  **A box never fills while the modifier is held.** Snapped, the two corners are multiples of `SNAP`,
  so `CLICK_EPS` is really asking *did the cell change*. Free, they are continuous and confined to one
  0.5 m cell — so an ordinary hand tremor clears 0.2 m without leaving the cell, and the box path would
  then quantise the result back to that cell's centre, silently discarding the fine placement the
  modifier exists for. Holding it means *place one, exactly here*.
- **Fine placement stays in its cell.** Holding the modifier used to switch the grid off entirely, so
  a small hand movement walked the piece a cell or two over. The cell under the cursor is captured when
  the modifier goes down (`FineAnchor`) and the free position is clamped to it. Every position is still
  reachable — in two gestures rather than one.
- **Arming a palette row blurs the filter box.** `blur_on_world_click` covered a click on the world;
  the click on the row you just filtered for is the other half of the same trap, and did not. §6 below
  records what that trap costs: `drive_place` is gated on `not_typing`, so searching for a piece was
  the way to stop being able to place it, silently.
- **The findings block is laid out.** Its remedy line was indented with three literal spaces, which
  indents only the first line of a wrapped paragraph — so every remedy ran into the finding below it.
  Each finding now has a coloured rail, a severity word, and prose in plain `TEXT`. Two of the finding
  *strings* were also malformed in `import.rs` (*"a re-export that forgets to can silently return"*)
  and are rewritten.

---

## 3. Three things the assets said that the code did not expect

1. **A wall and a doorway cannot meet.** The kit reaches 2.40 m by stacking a 2.00 m doorway under a
   0.40 m header, so their faces are 5 rows and 4. Authored the obvious way, the fault reads
   `[wall wall wall wall]` against `[wall wall wall wall wall]` — same token, one row short.
2. **The same problem exists horizontally.** A 3 m wall meeting a 1 m doorway presents faces of
   different lengths. **One question, both axes** — see §5.
3. **`solid` has no room to be useful.** See §4.

---

## 4. FVS-Q-9 is closed (*no*)

Built, measured, discarded. At `divisions: 1` a lattice-aware `stack::covers` agrees with the bounding
box **96% of the time** (451 solid cells of 469); shape needs `divisions: 3`, which is 53,000 lines of
RON and makes a wall 810 cells. And `divisions` cannot be raised for this alone, because it is one
project-wide number *precisely so that two faces are comparable* — coarse-for-matching and
fine-for-clearance cannot be the same number. Full table in `BACKLOG_ARCHIVE.md`.

`SubCell::solid` therefore has **no gameplay consumer**, deliberately. It is authored and drawn as the
author's confirmation that the lattice lines up with the mesh; `Descriptor::clearance` is the field that
decides anything about space. `SubCell::solid`'s own doc says so.

---

## 5. Decisions owed

- **The seam rule, both axes.** Should `may_abut` compare the *overlapping part* of two faces rather
  than requiring equal lengths? **The price is measured now:** `faults` pairs on `(x, z)` alone, so a
  header directly above a doorway shares its cells and is never compared to it — both are compared
  against the wall. Overlap comparison needs `stack::resolve_y` threaded in, which changes `faults`'
  signature and its cost. Current behaviour is pinned in `tests/site_descriptors.rs` as *current, not
  desired*.
- ~~**`align.scale`.**~~ **Answered, 2026-08-05** — the way the vertical axis already had it. There are
  now two helpers in `descriptor.rs`, `placed_footprint` (footprint × scale) and `placed_height`
  (height × scale × stretch_y), and every reader that asks *how much room does this take* goes through
  them: `divisions`, `stack::covers`, `adjacency::faults`, `fill::cell_extents`, `pick_at`'s area
  tiebreak, and the Tiles tab's lattice box. `stack::drawn_height` is gone — it was `placed_height`
  written in one place while `divisions` and `adjacency` wrote `height * stretch_y` by hand and
  dropped the scale, which is exactly how the two axes came to disagree.

  The editor can now set it: a **`SIZE (m)`** field on the Tiles tab takes the width in metres and
  stores `scale = typed ÷ measured`, which is what `Align::scale`'s own doc says the field is.

  `site/books` (`scale: Some(0.6)`) is the only non-unity scale in the repo and now reserves
  0.184 × 0.064 m rather than its full 0.306 × 0.106 m mesh. Its 1×1 lattice is unchanged — both spans
  were already under half a cell. **No shipped map is affected**: `assets/emerge/break_room.map.ron` is
  the only map that ships and holds no `books`, and `emerge_map::install_if_requested` adds nothing at
  all unless `FVS_EMERGE_MAP` is set.
- **`FVS-Q-10`** — should authored `edge` tokens feed `grammar`'s support table, or only check it? Still
  deferred, and still correctly: `grammar` learns adjacency from the map and `edge` is the declared
  half. Settle the seam rule first.
- ~~**A review before this lands.**~~ Done — `/code-review max 90`, 15 confirmed findings, all fixed.
  See §7.

---

## 6. Traps this session paid for, beyond the earlier doc's §4

- **Measure before building.** Twice a plan that reasoned well died on contact with a number.
  Vertex occupancy looked fine and marked a wall 4 cells of 10. FVS-Q-9's blocker was genuinely gone
  and the feature was still worthless at 96% solid. Both were recommendations I had already given.
- **Establish a baseline; do not assume one.** A key-repeat measurement read as "3 steps" and was 6,
  from an unverified starting yaw of 315°. Pressing `X` to zero the aim first made it trivial.
- **A shared epsilon cancels itself.** Nudging both edges of a span the same way made every piece a
  cell too wide, so everything abutted everything. Near edge rounds up, far edge rounds down.
- **A silent no-op is the worst failure mode this editor had.** Filtering the palette left the box
  holding the keyboard, and `place_on_click` is gated on `not_typing` — so the most natural way to find
  a piece was the way to break placing it, with no message. Fixed in `Phase::Sense` so one click blurs
  *and* places.
- **`fvs` is the launcher.** `cargo fvs edit <map> --kit site --fullscreen`, or a global `fvs` after
  `cargo install --path crates/fvs` — it only shells out to `cargo run`, so the editor is still rebuilt
  from live source. The `cargo fvs` alias is opt-in per machine (`.cargo/config.toml` is deliberately
  uncommitted: a machine-specific `target-dir` in it once broke CI).

---

## 7. What the review found (15 confirmed, all fixed)

`/code-review max 90` — 46 agents, 41 verified candidates, 0 refuted. Ranked as reported. The pattern
worth noticing: **eleven of the fifteen were a program lying about success**, not a crash.

| # | Where | What |
|---|---|---|
| 1 | `tiles.rs` `commit_candidate` | Accept pushed the import onto `project.library` — the **derived** layer — then `write_library` serialized `measured` and rebuilt `library` from it. The file was rewritten byte-identical, the palette never gained the piece, and the status line said *"it is in the palette now"*. `remove_tile` had it too. Both now go through `commit_measured`, which proposes a library and adopts it only once it is on disk. |
| 2 | `tiles.rs` five readers | The editor derived a piece's lattice from `measured` while `validate_lattices`, `adjacency::faults` and the game all read the layered library. Under `site_greybox` an author saw 2 rows and authored into row 1 of 5, with the other three unreachable — not refused, absent. `ImportState::placed` is now the one accessor for shape. |
| 3, 9, 10 | `adjacency.rs` | One parameter (`cell`) was doing three incompatible jobs and was wrong at all three: its grid was anchored at `-bounds/2` so an odd map width unpaired **every** seam; it erased every piece smaller than the step, exempting most furniture; and it set [`seam`]'s sampling rate to 1.0 m when a lattice cell is 0.5 m, so a 2.40 m wall's five rows were read at two places. The grid is gone — neighbours are decided in world metres, which is what `seam` already did for the tokens. |
| 4 | `src/rigs.rs` | The previous review's fix replaced `insert_resource` with `error!`-and-return, but the seven spawners take a bare `Res<CrabAnim>` — and in Bevy 0.19 a missing `Res<T>` **panics its system**. The fix moved the panic and the doc claimed a bind-pose creature that never existed. `required()` is now production code checked at plugin build, so the precondition holds. |
| 5 | `Cargo.toml` / CI | See §1. |
| 6, 7, 8, 11 | `align.rotate` | `occupancy` rasterised into the raw bbox with rotated divisions (transposed lattice); `rotate_mesh` re-measured the extent and left the cells where they were, so `write_library` then refused **every** later edit in the session; `remeasure_rotated` zeroed authored `y_offset`s — measured, all six shipped ones are *entirely* authored, so `site/floor`'s sink and three decals' anti-z-fight lifts died on one keypress; and the preview ignored `mesh_rotation`, in the one tab that has the rotate chips. |
| 12, 13 | `tiles.rs` | The candidate arrows walked the *unfiltered* list — the library branch had been fixed and candidates left — so one Down and one Enter could import a mesh the author never saw. And a cell write was range-checked against the live selection while landing on the captured target. |
| 14, 15 | `library.rs`, `headless.rs` | Two guards that could not fire: `MAX_LATTICE_CELLS` skipped `subgrid: None`, which is most pieces; and `every_action_resolves_to_a_binding` asserted the returned row had a non-empty chord and description, which `BINDINGS[0]` satisfies — so a new `Action` bound itself to Tab and the test stayed green. |

### The rotate rule, as decided

Refuse by default, with an override where one makes sense — the author's call, 2026-08-05:

* An authored **lift or sink is preserved exactly** across a turn. It is measured out before and put
  back after, so no refusal is needed and nothing is lost.
* A turn about **Y maps the cells** with `Subgrid::rotated` + `rotate_div`, the pair used everywhere
  else. No refusal needed there either.
* A turn about **X or Z** has no lattice mapping — the lattice's Y axis becomes a floor axis. With
  authored cells it refuses and names the count; **Shift** turns it anyway and reports how many cells
  were cleared.

### Two traps this pass paid for

* **A test can pass for the wrong reason.** `a_mismatched_pair_is_reported` placed two 0.5 m pieces
  1.0 m apart — a genuine 0.5 m gap — and only ever paired them *because* of the quantisation finding
  #3 describes. Fixing the code broke the test, and the test was what was wrong.
* **Measure before choosing.** Whether `remeasure_rotated` destroyed anything real turned on whether
  any shipped `y_offset` differs from `-base_y`. Six meshes, one throwaway probe: every one sits at
  `base_y = 0`, so every authored offset was the whole value. That turned a judgement call into an
  arithmetic one.
