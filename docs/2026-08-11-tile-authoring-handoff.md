# Tile authoring — handoff, 2026-08-11

**Read this first if you are picking up the editor work.** Two commits landed today on
`constraint-solver`: `54998e2` (the tile becomes authorable) and `fe015af` (the project cleared to a
blank slate). The next job is agreed and not started: **split the Tiles tab in two.**

## Where it started

Two complaints from authoring: *"tiles are awkward to place as they are center align, but my eye
wants to place a tile that falls wholly in the grid"*, and *"placing walls over tiles, they don't line
up with where I'm actually placing it."*

They were one regression with two symptoms, and chasing them surfaced a larger thing — the author's
own account of what the editor was supposed to be:

> The "Tiles" tab allows a developer to bring in meshes and arrange them based on where they'd like to
> see them when that tile is placed on the map… floor mesh at the lowest, a wall mesh over it, a wall
> mounted light fixture on the wall mesh… Then "Compose" is meant to be similar, but using multiple
> tiles to make a mini scene. **I want this process to be fast.** … **This should be done by the
> keyboard, as key strokes are faster.** Somewhere, we lost this vision.

Where it went is on the record: `docs/2026-08-09-unified-composition.md` §3 states the design almost
verbatim and ends *"What is missing is the authoring surface and the decision to make it primary."*
Step 3 of the compose-authoring plan **shipped that surface** on 2026-08-09 — and **FVS-R-15 cut it
the same day** (commit `345cede`, 21 actions, 804 lines), on a cost-of-duplication argument.

## What is true now

**The Map places tiles; a tile is a `Bounded` composition of one cell.** Under that model the wall
complaint dissolves — a 0.1 m wall is never placed alone, it is a member inside a tile.

Shipped in `54998e2`:

- **`editor::brush_at`** — one expression for where a brush lands, asked by both the preview and the
  commit. They were on different lattices; `drive_ghost` passed a hardcoded `(0.0, 0.0)` span.
  `tests/the_ghost_is_the_contract.rs` is the ratchet.
- **One ladder.** `seating_divisions` retired. It divided `grid::SNAP` while the Map's divided
  `grid::TILE`, so "divide a tile into 4" gave eight squares. A lift, a nudge and a seat now move by
  one rung of `grid::SnapLevel` over `policy.snap_divisor`.
- **`fill.rs` on the click's lattice**, stepping by however many rungs the piece occupies.
- **`Body::Slot`** — a declared hole. Interior-only, refused at the boundary. Costs the solver
  nothing, and there is a test that says so.
- **`Mount::OnFace` + `Offers::faces`** — a fixture attaches to a wall rather than to a number.
- **`SubCell::anchor` retired**; its vocabulary axis inherited by slots.
- **BUILD mode** — `crates/emerge-mapper/src/build.rs`, `Context::Build`, panel, 3-D stage, gizmos.
- **`Project::commit_composition`** — the one door to `compositions.ron`.

Shipped in `fe015af`: every authored map and composition deleted; measurements, policy, vocabulary
and all 462 meshes kept.

**85 test binaries green.** The one red is `bevy_debugger_mcp::test_highlight_entities`, which asserts
*no* BRP server exists — it inverts whenever a game or editor is listening on 15702. Environmental,
not a regression; check `lsof -nP -iTCP:15702 -sTCP:LISTEN` before believing it.

## The next job: split the Tiles tab

**Decided 2026-08-11.** `MESHES → TILES → COMPOSE → MAP → ANIM`.

The argument is the **hierarchy**, not crowding. `docs/research/2026-08-08-kitbashing-guidance.md`:
*"A good kit is hierarchical: parts → sub-assemblies → assemblies."* The editor already gives a tab
per level — Compose is scenes, Map is worlds — and Tiles is the only tab carrying two: a mesh (part)
and a tile (sub-assembly). Different object, different file, different frequency: a mesh is a
measurement written to `library.ron` and described once; a tile is an arrangement written to
`compositions.ron` and built constantly.

The crowding argument is already spent — `Context::Build` gave each half its own twelve-row budget.
What remains is conceptual.

**The shape:**

- `tiles::Mode` gains `Meshes`; `Mode::ALL` becomes five; number keys go to 5.
- `Context::Tiles` becomes the **mesh** tab's context. `Context::Build` becomes the **Tiles** tab's
  only context.
- **`build::TileMode` and the `C` flip disappear** — that is the point. A tab strip is the most
  visible mode indicator there is, so the mode stops being something to indicate.
- `rebuild_detail`'s BUILD branch (`tiles::build_detail`) moves to the Tiles tab's own panel; the
  mesh inspector keeps the rest.
- The right-hand library list is needed by **both** — describing picks a mesh to edit, building picks
  a mesh to drop. It stays a shared piece, not two.

**Known cost.** I10 of `docs/research/2026-08-08-editor-model-design-guide.md` argued for *fewer*
contexts — *"a single Compose key context… rather than three near-identical contexts that drift."* A
fifth tab cuts against that. Weighed and overridden, because a tab is a mode you cannot forget.

**The corpus cannot settle this**, and that is worth knowing before anyone re-litigates it: a search
for information architecture returns Ousterhout and Bass — software module design, not UI. The gap is
already recorded in `docs/research/2026-08-10-snapping-corpus-vetting.md`: *"a graphics/PCG corpus
with no HCI direct-manipulation holdings."* Raskin is cited throughout and has never been ingested.

## After the split

Rough order, and none of it is blocking:

1. **Author two or three tiles with BUILD** — floor, wall, corner. This is the acceptance test the
   plan named, and the fastest way to find what is wrong with the loop.
2. **`Member::paint` has no writer** (`composition_from_set` hard-codes `paint: 0`,
   `editor.rs:4878`), so decal ordering is not authorable. The author's *"a decal or two"*.
3. **Re-pin the solver against real tiles.** `grammar.rs`'s `site_kit()` now builds a synthetic
   four-tile fixture, which is correct — the crate's rule is that tests do not read shipped assets —
   but nothing checks the shipped kit any more. If that matters, it is a new asset-contract test, and
   it should say so in its doc comment the way the deleted ones did.
4. **`BEVY_GAME_INFO.md`** on the library share needs the tile-authoring model, for the 3-D artists.
5. **`scripts/mirror_crates.sh`** has not run; the crate mirrors are behind.

## Traps paid for, so nobody pays twice

- **A comment describing a fix nobody wrote.** I left one in `keys.rs` claiming the Save handler
  "asks which mode is live" before writing it. This codebase has been bitten by that exact shape
  before (`site::pieces` and the header course). Caught and closed the same day.
- **A test that cannot fail reads as a guarantee.** The BUILD panel test passed before the fix was
  in, because entering BUILD happened to touch `ImportState`. Verified by reverting the fix; it now
  fails on the *cursor*, which is the freeze rather than the flip. Do this for every ratchet.
- **The headless press idiom fires once.** *"Pressing an already-pressed key does not re-arm
  `just_pressed`."* Two keystrokes need one-shot systems that `release_all()` first — see
  `a_dropped_piece_is_staged_and_takes_the_focus`.
- **`grep`-shaped edits over a large file are dangerous.** A `str.index()` on a doc-comment line
  matched the first of eight occurrences and deleted 36 KB of `headless.rs`. Restored from the
  commit; use line ranges with asserted boundaries.
- **The key census catches real collisions.** Three, all correct: `Cmd+S` was already Global, `B` was
  taken by mesh-rescan, and a bare `b(...)` binding is *indifferent* to Shift so it swallows the
  shifted chord — state both with `bs(...)`, the `RemoveTile`/`DemoteTile` precedent.

## Reading, in the order it helps

- `docs/2026-08-09-unified-composition.md` §3 — the design, stated before it was built.
- `docs/research/2026-08-08-kitbashing-guidance.md` — the hierarchy, and the kit contract.
- `docs/research/2026-08-08-editor-model-design-guide.md` — I1 (one recursive type), I10 (modes).
- `docs/research/2026-08-10-snapping-corpus-vetting.md` — the corner rule, FVS-R-19/20, corpus gaps.
- Lai, Latham & Leymarie, *Three Pillars of Industry*, `10.1145/3402942.3402946` — **now in the
  corpus** (indexed 2026-08-11). Pillar 2 is the author's "fast", and Compton's *grokloop* is the
  phrase for it: *"the speed of learning depends on how short the loop is."*
