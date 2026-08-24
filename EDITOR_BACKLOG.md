# EDITOR_BACKLOG.md — `emerge-mapper` UI

> **This is the editor's backlog, not the game's.** `BACKLOG.md` holds the containment loop; this file
> holds the tool that authors its content. `emerge-mapper` is a standalone app and **not a game
> dependency**, so nothing here can move a determinism golden and nothing here waits on a game
> milestone. Completed items move to `BACKLOG_ARCHIVE.md` under their own heading. IDs are never reused.

IDs are `FVS-S-<n>`. An `FVS-R-*` item (Push 12's world-building series) and an S item touching the
same file is expected.

---

## 1. Where this stands

**The 2026-08-18 overhaul shipped.** Eleven items and one that had to be invented on the way, all
archived — see `BACKLOG_ARCHIVE.md` § *"`emerge-mapper` UI — the editor overhaul"*, which carries the
reasoning and the commit for each. In one paragraph: the editor had two unrelated fixed-pixel layout
models and neither filled the window; nothing at window level was navigation; there was no scrollbar
anywhere; six font sizes had no rule between them; and an agent could not see a panel without taking
the author's screen. All of that is closed.

**Four items were closed by measurement rather than by building** — `VirtualList`, Compose-as-rows,
and both halves of direct door switching. The reasons are in the archive; each names what would
re-open it. That is the same move FVS-N-25 made on FVS-N-23, and it is the reason this file is short.

### The gate, and what it is honestly measuring

`cargo test -p emerge-mapper` is **107 passing with 4 failures**, and those four are **pre-existing
and not the editor's**: they open the shipped `site` kit, deleted 2026-08-16 and still owed 45 pieces
(`FVS-R-39` in `BACKLOG.md`, which calls itself the only real gate in that file). **Read any claim of
green in this file as "the floor of 4 is unchanged"** — that is what every item below was verified
against.

`cargo test --workspace` is **106 binaries green and 6 red, 56 failures, all of them `site`**: 31 in
`foundation_vs_slop --lib` plus `site_descriptors` (8), `site_editor` (7), `mesh_measurement` (5),
`importer_against_real_meshes` (1) and the editor's 4. That is FVS-R-39's own accounting — it names
those four files by name — and it is worth stating in full here because **the first measurement of
this taken during the overhaul said 36 and was an undercount**, having captured only two of the six
red binaries. A floor quoted from a partial run is worse than no floor: it reads as a regression the
next time somebody measures properly.

### What the overhaul left behind, for whoever is next

Six ratchets that did not exist before, each of which found something or would have:

| Ratchet | What it stops |
|---|---|
| `no_system_writes_every_frame.rs` | a drawing system writing `Node`/colour/`Camera` unguarded — found `compass::follow_the_camera` on its first run |
| `every_resource_says_what_a_door_does_to_it.rs` | a resource arriving with nobody having said what a door change does to it |
| `chrome_census.rs::text_is_named_not_numbered` | a font size written as a number instead of a role |
| `the_frame_owns_position_and_carries_no_hover` | a panel positioning itself again, or a frame node claiming the pointer |
| `a_tab_is_not_a_button` | the `Enter`-steals-the-panel bug returning |
| `a_scrollbar_shows_exactly_when_there_is_somewhere_to_scroll` | a bar that never shows, or one that never hides |
| `the_sweep_is_finished.rs` | a hand-rolled scroll container, strip or panel — and a global observer demanding a door's resource, which is a crash |

---

## 2. Standing constraints

Unchanged, and every item below is judged against them.

1. **No system writes `ScrollPosition`, `Node` or a colour unconditionally per frame.** Now a test.
2. **`keys.rs` owns every binding.** The widget declares; the census owns. `Tab` was the case that
   proved it — see the archive.
3. **A missing `Res<T>` panics its system, and all run conditions are evaluated.** `and`/`or` are
   deprecated since 0.19.0; `src/` uses none of them.
4. **`add_plugins` tuples cap at 15.** The shared list nests past it already; the error names a
   `Plugins<_>` bound rather than the cap.
5. **One plugin list, two entry points** — `harness::add_editor_plugins`.
6. **Never take a real keyboard or display.** This is now cheaper than it was: the editor draws into
   one surface, so `bevy_debugger/screenshot` returns the interface with a region and a zoom.
7. **`cargo test --workspace` is the gate**, and `--workspace` is load-bearing. Kill any process on
   `BEVY_BRP_PORT` first.
8. **Tests do not read the shipped assets.** Fixtures are written by `tests/fixtures/mod.rs`, and
   each test names its own — three sharing one name is three processes writing one temp project,
   which surfaces as a briefly-missing resource rather than as the collision.

---

## 3. Open

### FVS-S-51 — a crowded kit can still stand a banded badge on ink

**Found by measurement, 2026-08-22, while fixing the leader crossings — and it is older than that
fix.** Pointing `no_badge_cluster_draws_through_another` at a populated project instead of the
one-descriptor fixture it has always used reports **fourteen** boxes standing on ink or on one another
with the pre-2026-08-22 placement, and **three** with the current one. So this is a real defect that
predates the crossing work, not a regression from it, and it is why that ratchet was left on its
original fixture rather than widened with `no_two_leaders_cross`: widening it would make one red suite
the record of two unrelated bugs.

Reproduce by swapping the fixture in `no_badge_cluster_draws_through_another`
(`crates/emerge-mapper/tests/headless.rs`) for `crowded_root("badgeoverlap")` and the surface list for
`CROWDED_SHAPES`. What comes back, at the time of writing:

```
MESHES at 1280x800: Legend and Control(CellGrid) overlap by 16x126 px
TILES  at 1280x800: Control(Pieces) and Control(Detail) overlap by 134x65 px
TILES  at 1280x800: Control(Pieces) and Control(Grid)   overlap by 35x31 px
MESHES: Control(Title) covers 57x16 px of ink at Vec2(1218.0, -5.0)      (x7, all tabs/shapes)
```

**Most of this is now closed** (2026-08-22, same day it was filed). Releasing the pane's ground —
a control inside a scrolling pane may put its badge in its own dock — and dropping the world-dodge
in favour of fading the envelope took the Tiles and Meshes tabs to zero overlaps on the real
furniture kit, verified in captures. What is left is the first of the two causes below.

Two distinct causes, and they wanted different answers:

- **The banded pass does no ground test at all.** `Control(Title)`'s box is placed level with its
  control at `a.at.min.x - size.x - reach`, falling back to the other side only if that leaves the
  window — `FreeGround` is never consulted, so a banded badge covers whatever is beside it. The
  negative `y` in the report is real: the ink it covers is partly above the window. Cheapest of the
  two to fix, and probably just the ladder's ground test applied to the banded branch.
- ~~**The stage genuinely runs out.**~~ **Closed.** It was true, and the cause was not the width of
  the boxes: badges were detouring around the box the *world* draws. On Tiles the tile envelope is a
  1 × 4 × 1 m box that projects to a tall rectangle down the middle of the stage; the filter's badge
  detoured 530 px below its own row to clear it, that dragged the side's floor with it, and the
  piece list's badge then had nowhere to stand but on its neighbour. One detour, three displaced
  boxes. Fixed by fading the envelope while the key is held (`chrome::WORLD_HELD`) instead of
  routing around it — the author's call: *"we should use fading of certain UI elements to ensure
  what we are visually communicating to users instead."* `WorldInk`, `WorldOnScreen`,
  `sense_world_ink`, `project_envelope`, `world_ink` and `world_ink_now` are deleted with it.

The `settle_past(..).clamp(top, bottom - size.y)` arm still exists and still knowingly pulls a box
back **up onto another** rather than let it leave the stage. It is now unreached on every tab that
has been measured, but it is the last "let the overlap show" fallback in the module and the module's
own header argues against exactly this shape. Removing it needs an answer to *what a badge does when
there is genuinely nowhere* — most likely sending its chords to the legend the way `badges::resolve`
already does for an off-screen control, which is a rebuild-time decision informed by a
placement-time fact, so it is not a small change.

---

Every other `FVS-S-*` item is closed — see `BACKLOG_ARCHIVE.md` § *"`emerge-mapper` UI — the
editor overhaul"*, which carries the reasoning and the commit for each, including the four closed by
measurement rather than by building.

The two that were last to go were decisions rather than work, and both are now enforced rather than
remembered:

- **Focus traversal stays off** (FVS-S-33). Routing is by `Context`, per `keys.rs`'s header, and
  `focus_traversal_stays_off_until_somebody_reopens_it` fails on a `TabIndex` without a
  `// FOCUS-DECISION-REOPENED:` line. The point is not to forbid it — it is that FVS-R-25's finding
  about click-to-focus is three documents deep, and nobody should meet it by accident.
- **Reference frames are not committed** (FVS-S-30). This repo has never had them, the argument
  against is the one that keeps derived assets out of git, and the captures are cheap to retake now
  that an agent can take them without asking for the screen — which is what made them expensive.
  `debug_screenshots/` stays gitignored.

**If you are picking this up:** the two things most likely to want attention next are not on this
list because nobody has asked for them. Compose still spaces itself with blank `Text` rows (its pane
measures 2417 px of content in an 833 px viewport, which the new scrollbar made visible for the first
time), and the widget layer's `TabView`/`VirtualList` were deliberately not built — the strip does
what a `TabView` would, and virtualization is unwarranted until a pack over ~200 rows ships.

## 4. Explicitly not in scope

Named so nobody re-opens them as oversights. The first four are archived with their measurements.

- **`VirtualList`** — lists fold into collapsed packs; the largest shipped pack is 145 against a
  ~200-row threshold. Re-open if a pack over ~200 ships.
- **Compose as rows** — selection is already a filled row and indents are already layout. What is
  left is monospace column alignment inside a generated report.
- **Direct door switching / `Door` as a `TabView`** — entering the MAP door needs a map name, which
  is a menu question. `docs/2026-08-17-one-application.md` §7 left it open and it stays open.
- **Warm project** (`one-application` §4.3) — the classification it depends on shipped; the
  behaviour change did not.
- **Actual browser ability** — no scroll anchoring, no overscroll rubber-banding, no compositor
  threading. **Wheel momentum** goes here too: Bevy 0.19 has no inertia, macOS trackpads already
  deliver OS momentum as a tail of `Pixel` events, and software inertia on top double-applies it.
- **Shift+wheel horizontal scroll** — `Pointer<Scroll>` carries no modifier state.
- **A workspace bump to bevy 0.19.1** — a root `Cargo.toml` change that touches the game and its
  goldens. Not this backlog's call.

---

## 5. Reading

- `BACKLOG_ARCHIVE.md` § *"`emerge-mapper` UI — the editor overhaul"* — what shipped and why.
- `docs/research/2026-08-18-reusable-scroll-and-tab-widgets.md` — the design. Note that the
  `ScrollView` was delivered through `chrome::scroll_list` instead of as a BSN widget, deliberately;
  the commit says why.
- `docs/2026-08-17-one-application.md` — the chrome bar and the resource classification came from
  here. §7's three questions are still open.
- `docs/2026-08-17-mapper-ui-audit.md` — the seven defects, all closed, and the palette leaks.
- `crates/emerge-mapper/CLAUDE.md` — the 0.19 pin, the vendored-source rule, and the corrected
  account of what an agent can now see.
