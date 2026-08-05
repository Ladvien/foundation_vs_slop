# emerge-mapper — where it stands (2026-08-05)

Supersedes the "do these first" list in `docs/2026-08-04-emerge-mapper-next.md`, all of which is done,
and its findings table, all eight of which are closed. That document's **§4 "Traps this work paid for"**
is still accurate and still worth your time; so is `docs/2026-08-04-emerge-mapper-handoff.md`, which
remains the no-prior-context orientation.

Branch **`feat/emerge-lattice`**, 33 commits off `bc7a92a` (which is where `main` sits), **pushed**.

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
| `cargo test --lib` | 977 passed, 1 ignored |
| `cargo test -p emerge-core` | 267 + 2 + 4 |
| `cargo test -p emerge-mapper` | 40 + 10 headless |
| `cargo test --tests` | **35 targets, none failing** |
| `cargo check -p emerge-core -p emerge-mapper -p fvs --all-targets` | **zero warnings** |

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
- **A review before this lands.** 33 commits touching schema (`Subgrid`, `front`, `rotate`), assets and
  gameplay-adjacent code. `/code-review ultra` is user-triggered and billed; the last one on a *smaller*
  version of this branch produced 66 verified findings.

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
