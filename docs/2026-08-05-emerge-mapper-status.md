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

**`--workspace` is load-bearing and was missing.** This workspace has a root package, so its default
members are the root package *alone* — a bare `cargo test` compiles no test target under `crates/`.
That is how extracting the animation layer into `crates/emerge-anim` dropped its 21 unit tests and the
rigs-manifest drift guard out of the CI gate with nothing going red: the tests were not failing, they
were not running. Every plain `cargo test` in `ci.yml`, `TESTING.md` and `CLAUDE.md` now carries it.

The non-harness suite is **fully green**. Two of `tests/replay.rs` still fail under `--features
test-harness` (`authored_world_config_override_is_a_noop`,
`deterministic_core_is_bit_identical_across_many_builds`) — both predate this branch. The gates that
matter, `deterministic_core_is_bit_identical` and the `a0_fvs_j6_mutant3_on_world_0x5c09191` golden, are
green, so `snapshot_hash` has not moved.

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

Editor: screen-aligned WASD panning; the map vocabulary under one hand (`X` removes, `V` aims straight);
`Cmd`/`Ctrl` places freely off the grid (XZ only — Y comes from `mount`); the aim and piece-turn keys
repeat while held at 150 ms; candidates scan themselves on selection; hover-and-click **ray picks** a
lattice cell and names the face you are looking through.

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
- **`align.scale`.** `stack::covers` reserves the raw `extent.footprint`, ignoring scale, and
  `divisions` deliberately matches it so the lattice and the reservation at least agree with each
  other. `site/books` (`scale: Some(0.6)`) renders at 0.6x while reserving its full footprint. This is
  the same measured-vs-placed question already answered for the vertical axis; answering it here
  touches `stack`, `fill` and clearance, which the **game** uses, so it needs the replay gates.
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
