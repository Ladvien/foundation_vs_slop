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
(`FVS-R-39` in `BACKLOG.md`, which calls itself the only real gate in that file). `cargo test
--workspace` adds 31 more of the same cause on the game side. **Read any claim of green in this file
as "the floor of 4 is unchanged"** — that is what every item below was verified against.

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

**FVS-S-32 · There is no focus model, and the design assumes one** · M
The design's §3.5 routes keyboard paging to *"the scroll view that currently owns focus"*. Two halves
of that do not exist here: `keys::just_pressed` is a **pull** helper each consumer polls, with no
`Action` event to route, and `InputFocus`/`TabIndex` appear **zero times** in `src/` — the editor
routes by `Live(Context, Stance)`, an ambient pair, and has never had a focused widget.
**The ground moved in this overhaul's favour and the decision is still owed:** `Tab` is now free (the
census retired `NextTab`), and `acquire_focus`/`click_to_focus` are live because `FeathersPlugins` is
in the graph. So (a) introducing focus is cheaper than it was. But (b) — routing by `Context` like
every other key here — remains the crate's grain, and **picking silently means shipping half of
each**. *Blocks:* keyboard paging, and any roving-tabindex widget. · *Reading:* design §3.5, §5.3

**FVS-S-33 · Focus-by-click is undrivable by an agent, and the reason is already written down** · S
`TabNavigationPlugin` registers `click_to_focus` globally, so a `Pointer<Press>` moves `InputFocus`.
FVS-R-25 measured why that path is broken for agents: `bevy_picking` writes `Hovered` from the
**window's** cursor, which `view::sense_pointer` deliberately never moves. **This overhaul narrowed
it** — `surface::retarget_pointer` and `surface::inject_clicks` mean an injected cursor now reaches
`Hovered` and an injected button now reaches picking — so the remaining question is only whether
`InputFocus` follows. Settle it with FVS-S-32, and whatever that picks must be reachable **without a
click**. *Do not attempt to fix the `Hovered` split here* — FVS-R-25 records that replacing it was
tried and reverted. · *Deps:* FVS-S-32

**FVS-S-34 · `chooser.rs` has never had a full audit** · M
Partly answered: the screen was re-laid onto the frame, its parallel sizing system deleted, its rows
given a real value column, and its stale header rewritten. What has **not** happened is the treatment
the four tabs got — a code catalog cross-checked against `chrome.rs`. It is 5,616 lines and it is the
first thing an author sees. · *Touches:* `chooser.rs` (audit only)

**FVS-S-22b · The inline MESHES/KIT strip is keyboard-only** · S
`tiles::tab_strip` (`tiles.rs:6292`) is two words and a hint inside the candidate list, switching
which list is shown. The **door** strip answers a click now; this one does not, which is the same
parity defect (`docs/ui.md` §4.2) one level down. Left alone deliberately: its state is
`Build::browsing`, an `Option` whose entry index has invariants worth reading before a second writer
is added. · *Touches:* `tiles.rs`, `build.rs`

**FVS-S-28 · Finish the sweep** · S
Mostly done by construction — `scroll_list` is the one scroll container and the frame is the one
layout — but the claim should be a ratchet rather than a belief. Extend the FVS-S-12 test to tab
strips, so a hand-rolled one fails rather than being noticed in a capture. · *Deps:* FVS-S-22b

**FVS-S-30 · Devshot baselines, kept rather than taken** · S
A full after-set exists at `debug_screenshots/after_{menu,map,kit_meshes,kit_tiles,kit_compose}.png`,
captured through BRP with nobody's screen taken. They are **gitignored**, so they are a working set
and not a baseline anybody else can diff against. Decide whether this repo wants committed reference
frames at all — it has never had them, and the argument against is the same one that keeps derived
assets out of git. · *Touches:* `debug_screenshots/`, `.gitignore`

---

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
