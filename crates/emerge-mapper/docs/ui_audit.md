# emerge-mapper UI audit — 2026-09-03

The 2026-08-17 audit (`docs/2026-08-17-mapper-ui-audit.md`) named four tabs drifting into four dialects
and the 2026-08-18 overhaul closed its findings. This one starts from a fresh report at the keyboard:

> *"Overall readability of the UI. And inconsistency of UI, buttons work different in different areas,
> layouts different, colors are inconsistent, and the layout overall is muddy as hell."*

Method: a code inventory of every surface in `crates/emerge-mapper/src/`, a mechanical census of the
un-ratcheted axes (spacing, widget adoption, clickable shape), a WCAG contrast computation over the
whole palette, and **nine live captures** taken over BRP from a real windowed build at 3396×1356 — no
window raised, no screen taken.

**The one-line verdict, and it is not the same as last time.** The vocabulary is now good *and widely
adopted*; what is wrong is one layer below it. The palette separates **ink from ground** superbly
(`TEXT` on `PANEL_BG` measures 13.4:1) and separates **ground from ground** essentially not at all
(`PANEL_BG` against `VOID` measures **1.03:1**). Every surface boundary in this editor — panel edge,
row edge, section header, hover — is carried by a fill difference at or below 1.2:1, with no border
and no radius to help it. That is the literal, measurable meaning of *muddy*: the screen is one
undifferentiated dark field with high-contrast text scattered on it.

---

## Part 1 — What is there

### 1.1 Screens, doors, tabs

Two screens (`screen.rs:44,46`): `Menu` (the chooser) and `Editor`. Three doors
(`tiles.rs:94,97,99`) — `Kit` / `Map` / `Rigs` — carrying five tabs (`Mode`, `tiles.rs:42-53`), split
3/1/1 (`tiles.rs:113`).

| Door | Tabs | Content owner |
|---|---|---|
| `Kit` | Meshes, Tiles, Compose | `tiles.rs` (9,410 ln), `compose.rs` |
| `Map` | Map | `editor.rs` (9,986 ln), `build.rs`, `fill.rs` |
| `Rigs` | Anim | `anim_tab.rs`, `anim_stage.rs`, `anim_plots.rs` |

The tab strip is `tiles.rs:3487-3598`; it is deliberately not a `Button`
(`tiles.rs:3532`, `a_tab_is_not_a_button`) and has its own repainter `style_tabs` (`tiles.rs:3611`).

### 1.2 Panels — 13 persistent containers

Seven editor dock panels through `chrome::panel_root`, six menu panels that hand-roll their own.

| # | Panel | Spawner | Side | Width | `full_height` | Heading builder | Scrolls |
|---|---|---|---|---|---|---|---|
| 1 | Map controls | `editor.rs:1367` | L | `CONTROLS_W` 300 | **false** | `title("EMERGE MAPPER")` | **no** |
| 2 | Map PLACE palette | `editor.rs:1474` | R | `LIST_W` 264 | true | `list_heading("PLACE")` | yes |
| 3 | Meshes/Tiles controls | `tiles.rs:3777` | L | `TILES_CONTROLS_W` 380 | true | `title("MESHES AND TILES")` | yes |
| 4 | Meshes/Tiles list | `tiles.rs:3839` | R | `LIST_W` 264 | true | `title("TILES AND MESHES")` | yes |
| 5 | Anim slots | `anim_tab.rs:307` | L | `TILES_CONTROLS_W` 380 | true | `title("ANIMATION")` | yes |
| 6 | Anim RIGS list | `anim_tab.rs:346` | R | `LIST_W` 264 | true | `list_heading("RIGS")` | yes |
| 7 | Compose body | `compose.rs:326` | L | `TILES_CONTROLS_W` 380 | true | `title("COMPOSE")` | yes |
| 8–13 | PROJECTS / KITS / KIT INFO / POLICY / MAPS / MAP INFO | `chooser.rs:4906–4941` | — | `COL_MAX` 420 | — | `chooser::header()` | lists only |

Frame slots: `chrome_bar`, `door_strip`, `left`, `viewport`, `right`, `status` (`chrome.rs:625-760`).

### 1.3 Modals and overlays — eight

| Overlay | Trigger | Scrim | z | Buttons |
|---|---|---|---|---|
| Confirm dialog (`confirm.rs:222`) | delete / quit / leave-dirty | `SCRIM` .55 | 900 | 2 real |
| Token prompt (`token_prompt.rs:139`) | `Shift+T`, `+` chip | `SCRIM` .55 | 400 | none |
| Name box (`chrome.rs:1801`) | `M` on Map, `N` on Tiles | **inline `srgba(0,0,0,0.72)`** | 400 | none |
| Key badges (`badges.rs:462`) | hold `K` | `VEIL` .35 | 500 | n/a |
| Session journal (`chrome.rs:1157`) | `Cmd+E` | none | **none** | n/a |
| Problem toast (`chrome.rs:1093`) | any `Status::problem` | none | **none** | n/a |
| Compass (`compass.rs:92`) | always on | n/a | 100 | n/a |
| Guide card | agent, `--features debugger` | n/a | n/a | n/a |

### 1.4 Verbs

96 `Action` variants, 130 `Binding` rows, 7 `Context`s × 4 `Stance`s, 21 `ControlId` badge homes
(`keys.rs:410-664`, `886-2417`). Full per-door tables are in `agent://PanelInventory` §5; the census
is the single source and nothing here restates a chord.

### 1.5 Clickables — 33 kinds, six shape dialects

| Dialect | What it is | Sites |
|---|---|---|
| A | `chrome::list_row` — Button + `RowRest` + shared repainter | 5 |
| B | `chrome::quiet_row` — same shape, off the `Activate` bus | 3 |
| C | `chrome::chip` | 9 |
| D | `chrome::text_field` | 4 |
| E | `chrome::severity_rail` + Button through its marker | 1 |
| F | bare `UiButton` on a hand-rolled `Node` | 7 |
| G | no `Button` at all — per-entity `Pointer<Click>` | 3 |
| H | **drawn like a row, answers no pointer** | 1 (`KitRow`) |

26 global `Activate` observers plus 6 `Pointer<Click>` handlers.

---

## Part 2 — Findings

Ordered by what the captures show first.

### F1 — Surfaces do not separate. This is the "muddy" (measured)

Relative-luminance contrast over the shipped palette (`chrome.rs:30-207`), sRGB → WCAG 2.x:

| Boundary | Ratio |
|---|---|
| `VOID` → `PANEL_BG` (panel against the window) | **1.03 : 1** |
| `PANEL_BG` → `HEADER_BG` (a section band) | **1.03 : 1** |
| `PANEL_BG` → `ROW_BG` (a row against its panel) | **1.08 : 1** |
| `PANEL_BG` → `BAR_BG` (chrome bar against a panel) | 1.12 : 1 |
| `VOID` → `BAR_BG` | 1.16 : 1 |
| `ROW_BG` → `ROW_HOVER` (the entire hover signal) | **1.19 : 1** |
| `PANEL_BG` → `SLOT_BG` | 1.22 : 1 |
| `ROW_HOVER` → `ROW_SELECTED` | 1.64 : 1 |

Nothing else carries the boundary: `BorderRadius` appears **4 times in the crate** (2 in `chrome.rs`,
2 in `compass.rs`) and `BorderColor` 13 times, 6 of which are badges. A panel has no border, no
radius, and a 1.03:1 fill step. In the boot capture the three chooser columns are not visibly
distinct objects, and `PROJECTS` / `KITS` / `MAPS` read as text floating on the void.

Text-on-ground, by contrast, is mostly excellent — with three exceptions that are all **small type**:

| Ink | Ground | Ratio |
|---|---|---|
| `LABEL` (10 px, the label column of every label/value row) | `ROW_BG` | **3.65 : 1** — fails 4.5 |
| `LABEL` | `PANEL_BG` | **3.96 : 1** — fails 4.5 |
| `MUTED` (an excluded pack) | `PANEL_BG` | **2.49 : 1** |
| `SUGGEST` | `ROW_SELECTED` | **2.80 : 1** — `chrome.rs:141-146` already admits this |
| `GRID_LINE` | `VOID` | 2.02 : 1 |
| `TEXT` | `PANEL_BG` | 13.37 : 1 |
| `ACCENT` | `PANEL_BG` | 9.24 : 1 |

### F2 — The editor does not fill the window, and does not scale with it

`EDITOR_UI_SCALE` is a fixed 1.2 (`chrome.rs:244`), multiplied only by the display's scale factor
(`surface.rs:335`). On this 3396×1356 window that leaves the Meshes tab's two panels occupying
**470 px of 3396** — under 14 % of the width — with 11 px body text, and the chooser confined to a
centred island roughly 700 px wide with ~2,500 px of dead void either side. Panel widths are pure
pixel constants (300 / 380 / 264 / 420) with no relationship to the window.

### F3 — Opening the Map door crashes the application (reproduced 2/2)

`cargo run -p emerge-mapper --features debugger -- . <map>` dies inside two seconds:

```
Attachments have differing sizes: the depth attachment's texture view has extent (3396, 1356, 1)
but is followed by the color attachment at index 0's texture view which has (3440, 1440, 1)
Quitting the application due to Validation RenderError
```

`fit_surface_to_window` (`surface.rs:315`) resizes the surface image to the window's *physical* size
while `fit_viewport_to_frame` (`surface.rs:381`) hands the 3-D camera a viewport computed from the
previous frame's layout; on the frame the window grows, the two disagree and wgpu refuses the pass.
Booting through the chooser survives (the menu's own `fit_capture_to_window` has already sized the
image by the time the editor screen starts), so the crash is specific to entering a door directly.
**This makes the Map door — the editor's primary surface — unreachable from the command line.**

### F4 — `Enter` on `+ new project` is a silent no-op

The status band advertises `Enter new project` and the row is selectable and highlights. Pressing
`Enter` produces no prompt, no project, no refusal, and no status line (verified on a full-frame
capture). `crates/emerge-mapper/CLAUDE.md` §Data model already records that nothing can create a
project root — the row and its hint promise a verb that does not exist.

### F5 — Overlapping text in KIT INFO

The chooser's KIT INFO panel renders `new work lays here` with the value `yes` drawn **on top of**
the label (`chooser.rs` info rows, `INFO_LABEL_W = 76.0` at `chooser.rs:5103`). Two glyph runs
occupy the same pixels in every capture of the menu.

### F6 — A count column joins the sentence it labels

Right-dock list rows put the pack count inside the wrapping text flow, so a folder whose name wraps
renders as `kenney_prototype-kit/Models/GLB 145 format` and `low_poly_furniture/glb/ 8 Electronics`.
The number needs its own non-wrapping column.

### F7 — Chips overflow the panel and collide with the scrollbar

On the Meshes tab the KIND row's last chip (`terminal`) and the LOOKS row's `+` are clipped by the
panel's right edge, and the overlay scrollbar (`BAR_W` 5, `chrome.rs:1383`) is drawn over them.

### F8 — Spacing: 79 literals, 19 distinct values, against a three-step scale

The palette and the type scale are ratcheted (`tests/chrome_census.rs`); **spacing is not**. Outside
`chrome.rs`, in non-test code:

- **79 literal `Val::Px(..)` sites, 19 distinct values.**
- Gaps and paddings in use: 2, 3, 4, 6, 7, 8, 10, 12, 14, 16, 20 — **eleven values** where
  `GAP_TIGHT` 3 / `GAP_ROW` 5 / `GAP_GROUP` 16 declare three.
- `margin.top` above a heading is 3, 4, 6 or 8 depending on the file
  (`anim_stage.rs:751`, `editor.rs:1528`, `anim_tab.rs:323`, `tiles.rs:7325`).
- Worst files: `tiles.rs` 26, `anim_tab.rs` 13, `confirm.rs` 12, `editor.rs` 12.
- `CHIP_PAD` is restated by hand as `UiRect::axes(Val::Px(6.0), Val::Px(3.0))` at `tiles.rs:7531`,
  in a file that imports the constant.
- `panel_root`'s own `row_gap` is a literal `6.0` (`chrome.rs:492`) — the one gap every panel uses is
  the one gap that is not on the scale.
- Label columns are six unnamed literals: 76 (`chooser.rs:5103`), 62 (`editor.rs:1415`),
  56 (`editor.rs:1446`), 48 (`tiles.rs:8075`), 40 (`tiles.rs:8161`), 14 (`editor.rs:1440`).

### F9 — Widget adoption is roughly half

78 shared-builder call sites against **102 hand-rolled `Node {` literals** outside `chrome.rs`.
Five modules use **no** builder at all: `badges.rs` (6 nodes), `confirm.rs` (4), `compass.rs` (3),
`token_prompt.rs` (2), `filter.rs` (1) — i.e. **every modal and overlay is hand-built.**

### F10 — Six row dialects, three of them in one scroll area

The Tiles right panel draws `quiet_row` (`tiles.rs:7080`), a bare `Text` row (`tiles.rs:7142`) and
`list_row` (`tiles.rs:7464`, `7593`) in the same list. Row insets differ by dialect: `CHIP_PAD` (6,3)
for the shared rows, `axes(8,4)` for the Map's PLACE rows (`editor.rs:1569`), `axes(16,8)` for a tab
chip (`tiles.rs:3541`).

### F11 — Two clickables are dead or invisible

- `KitRow` (`tiles.rs:7142`, `7160`) is a bare `Text` with no `Node`, no `Button`, no `Hovered`, and
  **no observer anywhere** — the Tiles page of the right panel cannot be clicked, while the Meshes
  page of the same panel can. `tiles.rs:2195` quotes the mouse/keyboard parity rule at itself.
- `ShelfChip` (`tiles.rs:6957`) carries `Hovered` and nothing repaints it: a hover state that is
  sensed and never shown.
- `PaletteRow` (`editor.rs:1569`) carries no `RowRest`, so `chrome::style_list_rows` cannot see it and
  `editor.rs:2308` restates the same priority privately.

### F12 — Headings mean four different things

`title` (15 px `ACCENT`) heads the right dock on Tiles and `list_heading` (10 px `LABEL`) heads it on
Map and Anim. The Map's *left* panel is titled `EMERGE MAPPER` — the application's name in a slot
where the other three name a job. The menu adds a fourth: `chooser::header()` at `text::HEADING` in
`KEY` (`chooser.rs:4805`).

Two panels on the Kit door are titled `MESHES AND TILES` and `TILES AND MESHES` — the same three
words, reversed, side by side on the same screen.

### F13 — Four full-screen dimmers, three opacities

`SCRIM` .55 (confirm, token prompt), `VEIL` .35 (badges), and an inline `srgba(0,0,0,0.72)` for the
name box at `chrome.rs:1824` — legal only because the colour census exempts `chrome.rs`, and exactly
the drift `SCRIM`'s own doc says it exists to prevent.

### F14 — Two modals, two contracts

Confirm has real buttons, a `TabGroup`, z 900, click-eating, a border, `padding: 20`, `row_gap: 12`.
The token prompt has no buttons, no `TabGroup`, z 400, `Pickable::IGNORE`, no border,
`padding: PAD*1.5` = 18, `row_gap: GAP_ROW*2` = 10. The journal and the toast carry **no
`GlobalZIndex` at all** while every sibling overlay does.

### F15 — Layout is not uniform across tabs

- The Map's left panel is the only one that is content-sized (`full_height: false`,
  `editor.rs:1373`) and the only one with **no scroll area anywhere in it**.
- Compose has no right panel, so the right dock collapses and the viewport jumps 264 px wide when
  the author presses `3`.
- Left panels are 300 on Map and 380 on all three others, so the viewport also jumps 80 px between
  doors. Three tabs use a constant named `TILES_CONTROLS_W` whose doc argues only the Tiles case.

### F16 — Compose is still a different program

The pane renders structure as flat text with leading-space indentation, ASCII markers and blank
`Text` rows as spacers (`DERIVED INTERFACE`, the four `north/east/south/west` lines). The 2026-08-18
backlog closed "Compose as rows" by measurement; the capture shows what is left is not just
monospace column alignment — the whole block reads as terminal output pasted into a panel.

### F17 — The font-size ratchet has a laundering hole

`chrome_census.rs:135` matches only `from_font_size(<digit>`. `chrome::chip` and
`chrome::text_field` take `px: f32` and call `from_font_size` *inside* `chrome.rs`, which the scan
skips — so **20 call sites pass bare 9.0 / 10.0 / 11.0 through a function argument**
(`tiles.rs:7655, 7669, 7688, 7718, 7731, 7739, 7751, 7780, 7917, 8080, 8149, 8229, 8418, 8446,
8804, 8828`, `anim_stage.rs:739, 759`, `anim_tab.rs:1061`, `editor.rs:1448`).

### F18 — Nothing ratchets shape

`tests/chrome_census.rs` has three tests: colour literals, `from_font_size` literals, and the length
of the type scale. **Nothing covers padding, gaps, panel composition, row shape, clickable shape,
z-index or heading role** — every finding F8 through F17 is currently unenforced.

---

## Part 3 — Captures

Taken 2026-09-03 over `bevy_debugger/screenshot` at `BEVY_BRP_PORT=15703`, window 3396×1356.

| File | Surface |
|---|---|
| `a_boot.png` | Menu, whole window |
| `b_chooser_zoom.png` | Menu, columns at 1.6× |
| `c_kit_door.png` | Kit door, Meshes tab, whole window |
| `d_meshes_left.png` | Meshes controls panel at 2.2× |
| `e_meshes_right.png` | Meshes list panel at 2.2× |
| `f_tiles.png` | Kit door, Tiles tab |
| `g_compose.png` | Kit door, Compose tab |
| `h_compose_left.png` | Compose panel at 3.0× |
| `i_badges.png` | Meshes tab with `K` held — the badge overlay |
| `n_full_after_enter.png` | Menu after `Enter` on `+ new project` (F4) |

---

## Part 4 — The author's answers

Answered at the keyboard, 2026-09-03. **This section is the binding spec** — where it disagrees with
a finding above, it wins.

### D1 — Surfaces separate by a formal elevation model *(answers F1)*

Four named elevations — **void / panel / raised / overlay** — each a fixed luminance step apart, with
**border and radius derived from elevation**. Not "widen the fills a bit": the step, the edge and the
corner are one decision made once, so a surface reads as an object.

### D2 — Raise the base density **and** let the chooser fill the window *(answers F2)*

The editor's docks may keep pixel widths — a viewport wants the space — but the menu has no viewport
to protect and must stretch. Base scale goes up so 11 px body text is readable on a 3396 px display.

### D3 — Fix the Map-door crash, with a regression test *(answers F3)*

A headless test resizes the surface and asserts the viewport follows **in the same frame**.

### D4 — Three clickable shapes, five states *(answers F10, F11)*

One row, one chip, one button. Every clickable in the editor is one of them; hand-rolled `Button`s
are migrated. All three carry **rest / hover / pressed / selected / disabled** — today nothing
acknowledges a click at all.

### D5 — Shape carries kind, colour carries severity *(answers the chip/command collision)*

A *toggle* (a tag) and a *command* (`rescan mesh`, `clear`) must not be the same grey box. Shape tells
them apart; colour is then free to mean severity only — **red is destructive and nothing else**, and
amber stops meaning "expensive".

### D6 — Amber means one thing: a live edit *(answers the ACCENT overload)*

`ACCENT` currently does five jobs. It keeps exactly one: **a value being changed right now.** Panel
titles go to `TEXT`, selection is carried by the row, `rescan mesh` becomes an ordinary command.

### D7 — Selection is fill **plus a 2 px accent left rail** *(follows from D6)*

The same rail vocabulary `severity_rail` already uses, so selection gains a shape rather than
borrowing an ink.

### D8 — Normalise the docks, build a real inspector hierarchy, and unify the modals *(answers F15, F14, F13)*

- One left width, one right width; **every panel scrollable**; the right dock holds its width when a
  tab has nothing for it, so the viewport never jumps between tabs or doors.
- The left panel becomes **explicit collapsible sections** — IDENTITY / SIZE / TAGS / FINDINGS / GRID
  — instead of one flat 30-row scroll.
- Confirm, token prompt and name box share **one modal shell**: one scrim, one z-order, one button
  row. The inline `srgba(0,0,0,0.72)` goes.

### D9 — A panel is named for what it holds *(answers F12)*

Left = the thing being inspected; right = the list. **No panel is named for the application**, and no
two panels on one screen are anagrams of each other.

### D10 — Compose gets the full re-lay *(re-opens F16)*

Members and the derived interface become label/value rows and list rows like every other tab; the
blank-`Text` spacers become `GAP_GROUP`.

### D11 — Raise `LABEL` and `MUTED` to clear 4.5:1 *(answers F1's small-text half)*

Both. The label/value hierarchy is recovered by keeping the value at `TEXT`, not by keeping the label
illegible.

### D12 — All four capture defects are in scope *(answers F4–F7)*

- **F4 — `+ new project` is implemented**, not removed: a name prompt, then `<name>/assets/emerge/`
  with a byte-copied `vocab.ron`, an empty `kits.ron` and `maps/`.
- **F5** — the KIT INFO overlap.
- **F6** — a non-wrapping count column.
- **F7** — chip clipping and the scrollbar collision.

### D13 — Focus is visible, and the columns stop wrapping *(new)*

The panel holding the keyboard lights its border (using D1's edges), in the menu **and** in the
editor's docks. `←`/`→` clamp at the ends instead of carousel-wrapping.

### D14 — The ratchet covers spacing, the font hole, and shape *(answers F8, F17, F18)*

- A literal `Val::Px` outside `chrome.rs` fails unless marked `// CHROME-OK:`.
- `chip`/`text_field` take a **role type**, not `px: f32`, so a bare number cannot be passed.
- Every clickable must be one of the three builders; every overlay must declare a `GlobalZIndex`;
  every panel must come from `panel_root`.

---

## Part 5 — What changed

### The measurement, before and after

The complaint was *"muddy as hell"*, and this is what that was, in numbers. Surface separation is
**ΔL\***, not WCAG contrast — see `chrome.rs`'s elevation-ladder header for why the ratio is the
wrong instrument at near-black and how it let the old ladder pass review.

| Boundary | Before | After |
|---|---:|---:|
| `VOID` → `PANEL_BG` (a panel against the window) | ΔL\* **1.60** | ΔL\* **4.58** |
| `PANEL_BG` → `HEADER_BG` | 1.54 | 2.95 |
| `PANEL_BG` → `ROW_BG` | 4.13 | 5.84 |
| `ROW_BG` → `ROW_HOVER` (the whole hover signal) | 1.98 | 5.25 |
| `ROW_HOVER` → `ROW_SELECTED` | 15.07 | 3.89 *(plus a 2 px accent rail)* |
| A panel's border | **none existed** | ΔL\* 23.0 over its own ground |
| A panel's corner radius | **none existed** | 4 px |

| Ink on its ground | Before | After |
|---|---:|---:|
| `LABEL` on `ROW_BG` — the label column of every label/value row, at 10 px | **3.65:1** | 5.13:1 |
| `MUTED` on `PANEL_BG` | **2.49:1** | 5.14:1 |
| `SUGGEST` on `ROW_SELECTED` | **2.80:1** | 5.07:1 |

| Census | Before | After |
|---|---:|---:|
| Literal `Val::Px` outside `chrome.rs` | **79** over 19 distinct values | **0** unjustified |
| Font sizes laundered through a function argument | **20** | **0** — impossible, the parameter is a type |
| Overlays with no declared `GlobalZIndex` | **2** | 0 |
| Distinct z-values written at call sites | 5, in 4 files | 0 — all named in `chrome.rs` |
| Clickable shape dialects | **6**, plus one row that answered nothing | 3, ratcheted |
| Control states | rest / hover / selected | + **pressed** and **disabled**, from one repainter |
| Ratchets over the design system | 3 (colour, size, scale length) | **8** |

### The eight ratchets, and what each stops

`tests/chrome_census.rs`. Each was run against a planted violation before being trusted; each named
its own rule and nothing else.

| Test | Fails when |
|---|---|
| `panel_ink_comes_from_the_palette` | a colour literal appears outside `chrome.rs` unmarked *(pre-existing)* |
| `the_type_scale_is_a_type` | `from_font_size` appears outside `chrome.rs` — closes F17's laundering hole |
| `the_type_scale_stays_short` | an eighth type role arrives without a decision |
| `spacing_comes_from_the_scale` | a spacing literal appears outside `chrome.rs` unmarked — closes F8 |
| `the_ladder_is_a_ladder` | two surfaces that meet on screen close to within ΔL\* 2.5, or the edge stops reading — closes F1 |
| `the_ink_clears_its_grounds` | an ink drops below 4.5:1 on a ground it actually renders on |
| `a_clickable_is_one_of_the_three_shapes` | a bare `Button` is spawned outside `chrome.rs` — closes F10 |
| `every_overlay_declares_its_z` | a z-order is written as a number at a call site — closes F14's half |

### Findings, resolved

| | Finding | Outcome |
|---|---|---|
| F1 | Surfaces do not separate | **Fixed.** Elevation ladder, borders, radii; `the_ladder_is_a_ladder` holds it |
| F2 | Does not fill or scale with the window | **Fixed.** Menu columns flex to the window; `surface::ui_scale_for` grows density with width to a cap |
| F3 | Map door crashes on direct boot | **NOT FIXED — see below** |
| F4 | `+ new project` silently does nothing | **Fixed.** `create_project` existed and was unreachable; the bug was the keyboard wiring |
| F5 | KIT INFO draws its value over its label | **Fixed.** `row_label`'s column is a floor, not a cap |
| F6 | Count joins the sentence it labels | **Fixed.** `tiles::count_cell`, non-wrapping, its own column |
| F7 | Chips clip and collide with the scrollbar | **Fixed.** `chrome::SCROLL_GUTTER` reserves the bar's lane inside `scroll_list` |
| F8 | 79 spacing literals, 19 values | **Fixed** and ratcheted |
| F9 | Widget adoption ~half | **Fixed.** Every modal and every panel now shares the shell |
| F10 | Six row dialects | **Fixed.** Three shapes, ratcheted |
| F11 | Dead and invisible clickables | **Fixed.** `KitRow` clicks, `ShelfChip` lights, `PaletteRow` joined the shared repainter and `editor::style_rows` is deleted |
| F12 | Headings mean four things | **Fixed.** `INSPECTOR` / `PIECES` / `PLACE` / `RIGS`; no panel named for the application, no anagrams |
| F13 | Four dimmers, three opacities | **Fixed.** One `modal_card` |
| F14 | Two modals, two contracts | **Fixed.** One shell, one z ladder |
| F15 | Layout not uniform | **Fixed.** One `CONTROLS_W`, every left dock scrolls |
| F16 | Compose is a different program | **Fixed.** The derived interface is label/value rows; the blank-`Text` spacers are gone |
| F17 | Font ratchet laundering hole | **Fixed** by the type, not by a regex |
| F18 | Nothing ratchets shape | **Fixed.** Five new ratchets |

### F3 is open, and two fixes have been eliminated

**`emerge-mapper . <map>` still dies inside two seconds**, reproduced 4/4; booting through the
chooser never does, 0/4. `bevy_render`'s default handler treats the validation error as fatal, so the
application exits rather than dropping a frame.

```text
Attachments have differing sizes: the depth attachment's texture view has extent (3396, 1356, 1)
but is followed by the color attachment at index 0's texture view which has (3440, 1440, 1)
```

Eliminated, both by measurement rather than by argument:

1. **A stale viewport.** `fit_viewport_to_frame` runs after `UiSystems::Layout`, which `bevy_ui`
   chains *after* `CameraUpdateSystems` — so a viewport written there is unvalidated until the next
   frame. Clamping it to the image's extent is a real fix for a real latent invariant and **is
   kept**, but the crash is unchanged: the failing depth extent is the whole previous target, not a
   dock-hole rect.
2. **The resize landing after the cameras read it.** Moving `fit_surface_to_window` to `PostUpdate`
   `.before(CameraUpdateSystems)` makes the new extent visible to `camera_system` the same frame.
   The crash is byte-identical, so the cameras are not disagreeing about *when* they read the target.
   That change was reverted rather than kept, since it buys nothing measured.

What is left, written where the next person will hit it (`surface.rs`, `SurfacePlugin::build`):
**three cameras share one image** and only the 3-D one has a depth attachment.
`prepare_core_3d_depth_textures` allocates per *target*, so a depth sized for one camera's extracted
target info can meet a colour attachment allocated for another's — and the world camera is the one
spawned late, at `OnEnter(Editor)`, which fits the startup-path evidence exactly.

### Still open, named rather than quietly dropped

- **D8's collapsible inspector sections** were not built. The docks are normalised, every left panel
  scrolls, and the Meshes inspector is grouped by `section` headings — but IDENTITY / SIZE / TAGS /
  FINDINGS / GRID do not fold. It is one flat scroll with headings, not an accordion.
- **`Severity::Primary` is unexercised.** No block in the editor had a single obvious next action, so
  the value was left unspent rather than assigned arbitrarily.

### The gate

`cargo test --workspace`: **1,621 passing**, one red binary — `emerge-mapper`'s `headless` at
**152 passed / 4 failed**, which is *exactly* the pre-change baseline, measured on a `git worktree`
of `HEAD` before any of this landed. The four are `a_refit_on_another_tab_leaves_the_tile_history_alone`,
`a_tile_survives_a_save_and_a_reopen`, `undo_steps_back_through_the_meshes_brought_into_a_tile` and
`no_two_leaders_cross`. None is this work's.

Two badge-overlap tests *were* broken on the way through, by a first attempt at F2 that raised
`EDITOR_UI_SCALE` to a larger constant: that is the density a **small** window also gets, and at
1280 × 800 two docks scaled by 1.45 left the badge packer nowhere to put a legend. The fix is that
scale is now a function of window width rather than a constant, which is what D2 asked for in the
first place. Both tests are green.

### Contact sheet

Captured over `bevy_debugger/screenshot` at `BEVY_BRP_PORT=15703`, window 3396 × 1356. No window was
raised and no screen was taken. The files are gitignored — see `crates/emerge-mapper/.gitignore` for
why, and retake them by re-running the capture rather than by editing them.

| Surface | Before | After |
|---|---|---|
| Menu | ![](ui_audit/before_menu.png) | ![](ui_audit/after_menu.png) |
| Kit door — Meshes | ![](ui_audit/before_meshes.png) | ![](ui_audit/after_meshes.png) |
| Inspector, 2× | ![](ui_audit/before_inspector.png) | ![](ui_audit/after_inspector.png) |
| Piece list, 2× | ![](ui_audit/before_list.png) | ![](ui_audit/after_list.png) |
| Kit door — Tiles | ![](ui_audit/before_tiles.png) | ![](ui_audit/after_tiles.png) |
| Kit door — Compose | ![](ui_audit/before_compose.png) | ![](ui_audit/after_compose.png) |
| Key badges, `K` held | ![](ui_audit/before_badges.png) | ![](ui_audit/after_badges.png) |
| A question | *(no before — the confirm dialog was one of three shells)* | ![](ui_audit/after_modal.png) |

What to look for, surface by surface:

- **Menu** — the three columns reach the window edges instead of sitting in a ~700 px island with
  2,500 px of void either side; every panel has an edge and a corner; the KITS panel's border is lit
  because that is where the keyboard is; `new work lands here  yes` no longer overlaps itself.
- **Meshes** — `INSPECTOR` and `PIECES`, not `MESHES AND TILES` beside `TILES AND MESHES`; the last
  chip of a wrapped tag row is no longer cut off by the panel edge or drawn under the scrollbar; the
  pack counts sit in their own right-hand column instead of inside the wrapped folder name.
- **Compose** — the derived interface is a label column and a value column, not `{name:>5}` padded
  with `" ".repeat(7)`.
