# Tile authoring — handoff, 2026-08-11

**Read this first if you are picking up the editor work.** Three commits landed today on
`constraint-solver`: `54998e2` (the tile becomes authorable), `fe015af` (the project cleared to a
blank slate) and the tab split. **The next job is to author two or three tiles with the tool** — that
is the acceptance test the plan named, and the fastest way to find what is wrong with the loop.

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
- **The tile assembler** — `crates/emerge-mapper/src/build.rs`, panel, 3-D stage, gizmos.
- **`Project::commit_composition`** — the one door to `compositions.ron`.

Shipped in `fe015af`: every authored map and composition deleted; measurements, policy, vocabulary
and all 462 meshes kept.

**85 test binaries green.** The one red is `bevy_debugger_mcp::test_highlight_entities`, which asserts
*no* BRP server exists — it inverts whenever a game or editor is listening on 15702. Environmental,
not a regression; check `lsof -nP -iTCP:15702 -sTCP:LISTEN` before believing it.

## The split shipped

**Done 2026-08-11.** `MESHES` and `TILES` are two tabs; `Mode::ALL` is
`[Map, Meshes, Tiles, Compose, Anim]` — Map first because it is the job. `build::TileMode` and the `C`
flip are gone, which was the point: a tab strip is a mode you cannot forget. The right-hand library
list is still shared, because describing picks a mesh to edit and building picks a mesh to drop.

The argument was the **hierarchy** — `docs/research/2026-08-08-kitbashing-guidance.md`: *"A good kit is
hierarchical: parts → sub-assemblies → assemblies."* Crowding was already handled by giving each half its own key context.
The known cost stands: I10 of the editor-model guide argues for *fewer* contexts and this added one,
weighed and overridden. **The corpus could not settle it** — a search for information architecture
returns Ousterhout and Bass, and Raskin has never been ingested; the gap is recorded in
`docs/research/2026-08-10-snapping-corpus-vetting.md`.

**Two bugs the split introduced, and how they were caught.** Both are the same shape — something that
was safe only because of a fact the split changed:

- **`ProblemBanner` carries the tab it speaks for**, and the visibility pass `continue`d past every
  other banner. That was safe while each banner sat in a panel `apply_mode` hid for it. Meshes and
  Tiles now share one panel, so the Tiles tab had no banner at all, and once given one it kept
  showing a refusal after the author switched away. Fixed both ways: a second banner in the shared
  panel, and **hide** rather than skip the one that is not live.
- **`build_keys` shared `Phase::Act` with `toggle_mode` and `tab_shortcuts` and was unordered against
  them.** Arriving on the tab is what opens a tile, so whether that happened on the arrival frame or
  the next was Bevy's choice. The symptom was a test that passed alone and failed in the suite —
  which reads as an unrelated flake until you look. `build_keys` is now `.after` both.

*Verified the way this doc's own trap section demands:* `a_refusal_on_the_tiles_tab_is_visible_and_stays_there`
was checked against a revert of **each** fix separately, and the ordering fix was confirmed by six
consecutive green runs where the suite had been a coin flip.

## What is next

Rough order, and none of it is blocking:

1. **Author two or three tiles on the Tiles tab** — floor, wall, corner. This is the acceptance test
   the plan named, and the fastest way to find what is wrong with the loop. Nothing below is
   blocking; this is.
2. **`Member::paint` has no writer** (`composition_from_set` hard-codes `paint: 0`,
   `editor.rs:4878`), so decal ordering is not authorable. The author's *"a decal or two"*.
3. **Re-pin the solver against real tiles.** `grammar.rs`'s `site_kit()` now builds a synthetic
   four-tile fixture, which is correct — the crate's rule is that tests do not read shipped assets —
   but nothing checks the shipped kit any more. If that matters, it is a new asset-contract test, and
   it should say so in its doc comment the way the deleted ones did.
4. **`BEVY_GAME_INFO.md`** on the library share needs the tile-authoring model, for the 3-D artists.
5. **`scripts/mirror_crates.sh`** has not run; the crate mirrors are behind.

## Verified end to end, 2026-08-11 — and what it found

§7 of the plan asked for a round-trip and it had never been written. Writing it
(`a_tile_survives_a_save_and_a_reopen`) found **two bugs that made holes useless**, both invisible to
a suite where every individual piece was green:

- **A vocabulary token is not an id.** `place_slot` seeded the member id with its `accepts` token, and
  `fresh_id` assumes a legal id segment — true of a descriptor id, false of a token. `vocab.rs`
  documents `"uses-electricity"`; hyphens are correct there and illegal in an id. So `validate`
  refused the member and **no tile carrying a hole could be saved**. Both tokens the project declares
  are hyphenated, so this was every hole. `build::slot_id` converts and then checks its own output
  against `naming::is_id`, so it cannot drift from the rule.
- **Cell zero's corner is the tile edge.** A slot took a zero span, landing on its cell's corner,
  under a comment reasoning that "the last cell's corner is inside by less than a rung" — true of the
  last cell. `validate` wants a slot *strictly* inside, so a hole anywhere in row or column zero was
  refused, **including the cell the cursor opens on**. A hole now takes its cell as its span and lands
  at the centre. The test walks every cell, because "the first" and "the last" is exactly the pair the
  original reasoning got half right.

The loop now has a test from keystroke to stamp: author → save → reopen → `expand`, asserting two
rows and one hole rather than three rows. §7's attachment item was already covered by
`a_fixture_whose_host_is_gone_is_an_error_naming_both`. What is left of §7 needs a person: driving it
over BRP, timing one tile end to end, and re-authoring the kit.

**And one gap that is a design call, not a bug to fix quietly — FVS-R-24.** The author's own example,
*"wall mounted light fixture on the wall mesh"*, is not authorable: `Body::Descriptor` carries `on`,
the assembler always writes `None`, and a face-mounted piece with no host makes the **map refuse to
load**. The slot path is unaffected and works. See the backlog item for the two shapes a fix could
take and why the late-refusal half cannot be built independently.

## Traps paid for, so nobody pays twice

- **A comment describing a fix nobody wrote.** I left one in `keys.rs` claiming the Save handler
  "asks which mode is live" before writing it. This codebase has been bitten by that exact shape
  before (`site::pieces` and the header course). Caught and closed the same day.
- **A test that cannot fail reads as a guarantee.** The tile-panel test passed before the fix was in,
  because changing tab happened to touch `ImportState`. Verified by reverting the fix; it now
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
