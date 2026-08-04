# emerge-mapper — state and remaining work (handoff, 2026-08-04)

Written for an agent picking this up with **no prior context**. Everything below was measured, not
remembered. Read `CLAUDE.md`, `TESTING.md`, `docs/ui.md` and `docs/animation.md` before touching the
relevant area; this document does not repeat them, it records what *this* work changed and what is
left.

Nothing is committed. The whole change is in the working tree.

**Updated 2026-08-04 (second session):** §3 is rewritten — T1–T5 are done. §1's baselines have moved: `emerge-core` 216 (was 206), `emerge-mapper` 30 (was 28).

---

## 1. How to run and verify

```bash
cargo run -p fvs -- edit break_room --fullscreen   # the editor
cargo test                                         # CI hard gate (deterministic core)
cargo test -p emerge-core -p emerge-mapper         # the two crates this work touches
cargo test --features test-harness --test replay -- --test-threads=1   # GPU; SERIAL, see below
```

**Current green baseline** (reproduce this before changing anything, so you can attribute a failure):

| suite | result |
|---|---|
| `cargo test` game lib | 975 passed, 1 ignored |
| `emerge-core` | 206 passed |
| `emerge-core` integration (`rigs_match_assets`, `engine_free`) | 4 + 2 passed |
| `emerge-mapper` | 28 passed |

### Two failures that are NOT yours

Both verified pre-existing by stashing all work and re-running at clean `HEAD`:

1. **`tests/lore_canon.rs::the_lore_research_is_kept_but_marked_deprecated`** — fails on the
   untracked `docs/lore/2026-08-01-scp-gear.md`, which needs an FVS-K-4 deprecation banner in its
   first 12 lines. It is the user's document; leave it alone unless asked.
2. **`tests/replay.rs`** has two broken tests on `main`:
   `authored_world_config_override_is_a_noop` (asserts `left: 12430382217790730448, right: 0` — the
   config-override seam) and `deterministic_core_is_bit_identical_across_many_builds` (stack overflow
   in the IO task pool, SIGABRT). **The hash gates themselves pass** —
   `deterministic_core_is_bit_identical` and the golden `a0_fvs_j6_mutant3_on_world_0x5c09191_reproduces`
   are both green, which is what proves the `src/anim` crate move (§2.3) was safe.

---

## 2. What was done

### 2.1 Editor fixes (all verified in the running app)

| Fix | Note |
|---|---|
| Flood-fill undo | Was never broken — the platform modifier was. See below. |
| **Cmd on macOS** | `keys.rs` had `ControlLeft/Right` hardcoded, so `Cmd+Z` did nothing and undo looked broken. One `MOD_KEYS`/`MOD_NAME` pair per platform; the panel renders the name from the same constant. |
| Removal mode | `Delete` (Backspace on macOS) arms a tool rather than deleting. Red translucent marker on the hovered piece; click removes one, drag paints a box and removes all inside as **one** undo entry (`Undo::RemovedMany`). `Esc` leaves. |
| `Z`/`C` aim, `X` reset | Bare `Z` is safe beside `Cmd+Z` only because the modifier check keeps them apart — there is a test pinning both halves. |
| Map Size integer fields | Replaced `-`/`+` nudges. Digits filtered at the keystroke. |
| Wheel over panels | `view::drive` already skipped zoom when "over UI", but only *rows* carried `Hovered`; panel **roots** now do. `Hovered` is true for an entity **or any descendant**. |
| Font tofu | The mapper used Bevy's 95-codepoint default. It now installs `assets/fonts/FiraMono-Regular.ttf` over `AssetId::default()` at startup (`crates/emerge-mapper/src/main.rs`). Deliberate deviation from `docs/ui.md` §5's `FontAssets` rule — rationale is in the code comment. |
| **Flood fill wrote unplaceable pieces** | Found while testing: filling with a surface-mounted piece wrote one row per cell that `stack` refuses and nothing can draw — **4,089 invisible lamps** in one measured run. `fill::flood` now refuses up front. |

### 2.2 New editor structure

- **`crates/emerge-mapper/src/chrome.rs`** (253 lines) — the shared palette, the spacing scale
  (`GAP_TIGHT`/`GAP_ROW`/`GAP_GROUP`, used as *ratios*), and `panel_root` / `title` / `key_census` /
  `scroll_list` / `section`. Previously the colour table was declared twice with two names for the
  same value, and the census key-row block was copied byte-for-byte into both tabs **and rendered in
  different colours**.
- **`crates/emerge-mapper/src/filter.rs`** (203 lines) — per-list filter boxes. Text persists when
  focus leaves. Narrows, never reorders.
- **`crates/emerge-mapper/src/anim_tab.rs`** (546 lines) — the third tab.
- One typing guard: `editor::not_typing` reads **all six** fields (map name, pin reason, map size,
  candidate id, filter, subgrid div, subgrid cell token). **Adding a field means adding a line
  there** — a system gated on the wrong guard fires while you type.

### 2.3 Animation: measurement → manifest → game

This closes the gap `docs/animation.md` names: measuring a gait's
`(duration, phase_offset, cycle_distance)` was *"a manual offline step, not a repo tool"*.

- **`crates/emerge-core/src/clips.rs`** (591 lines) — engine-free GLB animation analysis: `clips`,
  `root_motion`, `cycle_distance`, `phase_offset`, `world_track`. FK is hand-rolled (quaternion →
  matrix → chain) because the `engine_free` allowlist has no math crate.
  Validated against the shipped Valkyrie vs `docs/artist_guide.md` §4: durations exact, root motion
  zero, **walk 1.373 vs 1.388 declared (1.1%)**, **run 2.106 vs 2.135 (1.4%)**, walk→walk_back phase
  −0.133 vs −0.141.
- **`assets/emerge/rigs.ron`** + `crates/emerge-core/src/rigs.rs` — the manifest. **All sixteen rigs**
  (valkyrie, crab, manca, scp610, 8 staff, 4 SCP-1048 variants) read from it; every `GAIT_*`,
  `CLIP_*`, `STAFF_CLIPS` and `ClipSpec` table is deleted.
- **`src/rigs.rs`** — `RigManifest` + `rigs::build`, one builder for all six creature systems.
- **`crates/emerge-core/tests/rigs_match_assets.rs`** — the drift guard. Re-measures every gait from
  the GLB the manifest names.
- **`crates/emerge-anim/`** — `src/anim/` moved here so the editor can drive the *real* `PoseBlender`.
  The game re-exports at `crate::anim`, so no call site moved. `smoothstep` went to `emerge-core`.

### 2.4 Subgrid (the tile lattice)

`Descriptor.subgrid: Subgrid { div: (3,3,3), cells: Vec<SubCell> }`, sparse. Each `SubCell` carries
**all three facets at once** — this was an explicit user decision:

- `solid` — occupancy (clearance, flood fill can respect shape not bounding box)
- `edge` — what the cell presents to the neighbour, for WFC face matching
- `anchor` — a role an interacting item may occupy (the regular-grid sibling of `offers.sockets`)

Mutations (`toggle_solid`, `set_edge`, `set_anchor`, `clear`) live **on `Subgrid` in emerge-core**, so
the schema owns its own edits and keeps the sparse invariant. No `LIBRARY_VERSION` bump was needed —
`Descriptor` is `#[serde(deny_unknown_fields, default)]`, so old files just omit the field.

Editor: Tiles previews on an isolated **stage** at `(-4096, 0, 4096)` (map out of shot, camera
restored on leaving), the lattice draws over it, `SUBGRID x/y/z` fields edit divisions, and a layer
picker + x·z cell grid + `[solid][edge][anchor][clear]` chips edit cells.

---

## 3. What is left

**T1–T5 are done** (2026-08-04, second session). What that session found, changed and left open:

### The reason T1 could not be "verified in the GUI"

It was not safe to try. `tiles::cell_keys` and `tiles::commit_candidate` sat in one **unordered**
system tuple and both took `ResMut<ImportState>`, so Bevy could run the text field first: it cleared
its own typing flag, the `not_typing` run condition re-evaluated to *true* in the same frame, `Enter`
was still `just_pressed`, and finishing a token **also** imported the candidate into `library.ron`.
That — not stray automation — is what put six descriptors in the file. §4 below still tells you to run
against a copy of `assets/`, and you still should, but the mechanism is gone.

The fix and **T4 are the same change**. `keys::Context::Typing` documented itself as *"overlaps
everything, and suppresses everything"* and was wired to nothing, which is why it and
`Context::overlaps` were the two dead-code warnings. Now:

- `keys::Live` — who owns the keyboard, decided **once** per frame in `keys::Phase::Sense`, so no
  system can see a keyboard that changed owner mid-frame.
- `keys::just_pressed(keys, live, action)` takes the live context and refuses there. The five
  `if *mode != Mode::Tiles` early returns and the `not_typing` conditions on key systems are **deleted**
  rather than kept beside it — that duplication is what this module exists to prevent. `not_typing` and
  `in_map_mode` survive only on the systems that read the **mouse**, which the census does not model.
- `Phase::Sense -> Text -> Act`. The fields go *before* the dispatchers, because the `X` that opens the
  edge field was otherwise still in that frame's `KeyboardInput` stream when the now-open field read
  it — the first token authored through the UI came out as `xseam`. Text systems also drain the reader
  **while shut**, so a keystroke from before the field opened cannot survive into it.

Verified in the running editor: `Enter` while typing commits the field and leaves `library.ron` at 42
entries; `Enter` with no field open still adds (42 -> 43); `W`/`A`/`S`/`D` typed into the map-name field
leave the world region **pixel-identical**.

### What else the lattice needed before it could be authored at all

- **Zero `SubCell` existed repo-wide** and `rebuild_detail` returned early unless an *import candidate*
  was selected — so an accepted tile's lattice was hand-edit-the-RON or nothing. `ImportState::editing`
  / `editing_mut` now follow one discriminant (`selected_library_id`), the detail pane, preview,
  gizmos, div fields, mount and tag chips all read it, and a library edit is written through
  `write_library` — the one writer `commit_candidate` and `remove_tile` now share.
- **Every lattice control was mouse-only**, against `docs/ui.md` §4.2. Three new census rows —
  `H, J, K, L` cursor, `[, ]` layer, `Z, X, C, V` verbs — put Tiles at 10 of its 12-row ceiling. The
  verbs go through `apply_verb`, the same function the chips call.
- `Action::CycleLayer` is now `CycleMount` and its label reads "mount": it cycles `Descriptor::mount`,
  and the panel said "layer" twice meaning two different things.
- The camera moved to `Context::Global`. `view::drive` never consulted a context, so pan and turn
  always worked on every tab; leaving them on `Map` would have silently taken the camera off the Tiles
  and Anim tabs. Only the typing half of the old behaviour is gone, and that half was a bug.

### T2 — `edge` now does something: it checks

`crates/emerge-core/src/adjacency.rs`. The three decisions the previous handoff flagged, and what was
chosen:

| Question | Answer |
|---|---|
| Does `None` match anything? | **Only another `None`.** Merrell & Manocha 2009 §4.3 defines adjacency as equality of the facing evaluation. A wildcard would make unauthored data permissive and authored data strict — one function, two behaviours. |
| Do faces rotate with yaw? | **Yes**, `Subgrid::rotated`. Non-90 yaws are refused **by name**. Not a literature call — Merrell §7 lists symmetry as something the method lacks — but a validator reading faces off an unrotated lattice is silently wrong for exactly the pieces that matter. |
| Equality or a compatibility table? | **Equality.** A table is a second artifact that can drift from the tokens it is about. |

And the fourth question, which the previous handoff did not ask: **who consumes it.** Not the solver.
`grammar.rs` already learns adjacency from the map and argues it should be the only way; `adjacency`
answers a different question — *does what you drew match what you declared* — and has exactly one
caller, the editor's `EDGES` readout plus a `DANGER` outline on both offending pieces. No RNG, no
placement change, `snapshot_hash` cannot move. Whether tokens should also feed `grammar`'s `support`
table is **FVS-Q-10**.

The two are not rivals. Karth & Smith 2017 — the paper `grammar.rs` already cites, converted during
this work — names them as WFC's own two modes: *"In the **simple tiled** version of the algorithm, the
patterns are specified as explicit tile constraint relationships. In the overlapping version, the
constraints are **inferred from the source image**."* `grammar.rs` is the inferred half; `edge` is the
explicit one.

Two scope rules that keep it quiet: a pair where **neither** tile declares an edge is not checked at
all (so `break_room`'s armchair at yaw 240 is nobody's problem until it carries a token), and each seam
is reported **once**, not once from each side.

### T3 — deferred, with the reason written down

`solid` refining `stack::covers` needs either all 42 lattices authored or an
`if the lattice is empty, use the bounding box` branch — the fallback `CLAUDE.md` forbids. Filed as
**FVS-Q-9** with the two honest routes.

### The key list is an overlay now

Held, not printed. `K` down shows this tab's rows over a scrim; releasing hides them. Each panel
carries one line — *"Hold K for shortcuts"* — where eighteen census rows used to be, which is what
closed **FVS-Q-11**: the whole subgrid section, chips included, now fits above the fold.

It reads `keys::rows(tab.context())`, so it is per-tab by construction rather than by three call
sites agreeing. That also makes the context model visible to the author for the first time: `W, A, S,
D` reads *"pan"* on the Map tab and *"move the cell cursor"* on Tiles.

Two knock-on rebindings, both recorded in the census beside the rows:

- **`K` was the lattice cursor's "back"**, so the cursor moved from `H J K L` to `W A S D` — moving
  one key of a cluster is worse than moving the cluster, and `W A S D` is what those keys mean.
- **Pan went back to `Context::Map`**, which is what freed `W A S D` in the Tiles tab. This partly
  reverses the earlier "the camera is Global" call: *turning* the view stays Global, because both
  other tabs stage a 3D subject worth looking round, but panning does not — the Tiles camera is parked
  on one tile by `stage_camera` and panning off it has no way back.

`scripts/macinput.py` gained `keydown`/`keyup`: a held key cannot be driven by `key()`, which sends
down-then-up, so the overlay would open and close inside one frame with nothing to capture.

### Still open

- **FVS-Q-9** — `solid` and clearance.
- **FVS-Q-10** — should `edge` feed the solver, or only check it.
- `BACKLOG.md` had **nothing to archive** — no open item related to the mapper, subgrid, WFC, rigs or
  clips, because this work was tracked only in `docs/2026-08-0*`. So T5's third bullet inverted: the
  three items above were *added*.
- **`docs/2026-08-03-emerge-mapper-plan.md` Stages 2–6b** (`emerge-bevy` spawn, site migration, F7
  parity, mesh importer, smart objects) are neither done nor mentioned by this document's first
  version. A reader of the handoff alone would not know they exist. They are still open.
- **Corpus.** The two papers `grammar.rs` cites — Karth & Smith 2017 (`10.1145/3102071.3110566`, line
  13) and Alvarez et al. 2018 (`10.1145/3235765.3235810`, line 26) — **are** held, as
  `papers/10/*.pdf`, but neither is converted to markdown, so nothing indexed or greppable exists for
  either. Conversion was started for the first. Merrell & Manocha 2009, the paper the `edge` matching
  rule comes from, is converted at `me/merrell09.md`.
  Worth knowing for the next session: the **distill vector-search service at `192.168.1.110:7434` is
  unhealthy**, so `distill_search` fails outright and the corpus has to be read by grepping the rclone
  mount at `~/mnt/home-still/markdown` — which only sees *converted* documents, which is exactly how
  two held papers looked missing.

### One flag chased down and closed

It looks like `SubCell::anchor` contradicts `plan.md:145` — *"Interactions are NOT on the descriptor."*
It does not. That decision puts the **interaction** (verb, roles, guard, effects) on a map-level
`location`, and `Map::locations` exists. `Socket::role` is already a descriptor-level *hook*, and
`descriptor.rs:95` documents `anchor` as exactly its lattice sibling. Hook on the descriptor,
interaction on the map.

## 4. Traps this work paid for — read before testing the GUI

**Driving the editor on macOS.** `scripts/vinput.py` is Linux/uinput and does **not** work here. It is committed as `scripts/macinput.py`. Note:

- **A chord needs a real modifier key-down.** Setting only `CGEventSetFlags` marks the event's
  modifier state but never produces a `ControlLeft`, and Bevy's `ButtonInput<KeyCode>` is built from
  actual key events — a flags-only chord arrives as the **bare key**. This wasted several runs.
- **There is a constant Y offset** between CG global-display coordinates and window coordinates on
  this display (measured: sending CG y=97 arrived as y=71, so **+26**). Bake it in.
- **Never estimate a widget's position from a screenshot.** Log its real rect —
  `Query<(&ComputedNode, &UiGlobalTransform)>` — and click the measured centre. UI nodes in Bevy 0.19
  carry `UiGlobalTransform`, **not** `GlobalTransform`; a query requiring the latter silently matches
  nothing. `ComputedNode`/`UiGlobalTransform` are in **physical** px; pointer positions are logical.

**⚠️ Run the editor against a COPY of `assets/`.** Automated `Enter` presses that miss their target
fall through to `Action::Accept` — "add to library" — and **six descriptors were accidentally
imported into `assets/emerge/library.ron`** during this work. They were removed via
`Library::parse → retain → validate → to_ron`; the file is back to 42 entries but was **re-serialised,
so hand formatting/comments in it are normalised**. Copy `assets/` to a scratch dir and pass that as
the project root.

**Screenshots**: `touch screenshot.request` in the binary's cwd (`crates/emerge-mapper/`, or the
project root you passed). Raise the window by unix id first —
`osascript -e 'tell application "System Events" to set frontmost of (first process whose unix id is $PID) to true'` —
or frames come back black, and **only the first capture after a raise is reliable**. A black frame is
~55 KB; a real one is 0.5–4 MB.

**Harness tests are serial.** `cargo test --features test-harness -- --test-threads=1`. Running them
in parallel produces meaningless failures and a stack overflow — they hold a `serial_guard()`.

**Bevy 0.19 specifics hit during this work**, beyond what `CLAUDE.md` lists:
- A missing `ResMut<T>` **panics the system** with "Resource does not exist" — `init_resource` every
  new resource in the same commit you add the system.
- A UI node sized only by its text is **7 logical px tall** when the text starts empty. State a
  `min_height` on anything clickable.
- `bevy_ui_widgets::Button` fires `Activate` on click; the observer pattern is
  `On<Activate>` + `Query<&Marker>` + `marker.get(activate.entity)`.

**BSD tooling.** `sed` on macOS does not support `\b`, and `\n` in a replacement inserts a literal
`n`. Several edits silently no-oped this way. Prefer a Python script written to a file over inline
heredocs (zsh quoting also bites).

---

## 5. Ground rules that shaped these decisions

- **One path.** No fallbacks, no stubs, no compatibility branches (`CLAUDE.md`). The fill's surface
  refusal and the "all eight staff rigs or none" behaviour both come from this.
- **Clutter is crowding, not element count.** `docs/ui.md` cites van den Berg et al. 2009
  (`10.1167/9.4.24`): spacing ≈ 0.5 × eccentricity. §1.2 (Vicente & Rasmussen) makes the test *"does
  this force interpretation?"*, and Yang et al. 2017 measured more information **improving**
  performance. So a crowded panel is fixed with grouping and spacing, **not** by deleting readouts.
- **Verify against the asset, not against yourself.** A manifest that agrees with itself proves
  nothing; `rigs_match_assets.rs` re-measures the GLB.
- **State tolerances honestly.** The drift guard is 20% and `clips.rs` pins the reference gaits at 3%,
  because `docs/artist_guide.md` says the back/strafe numbers are themselves rough — a tight bound
  there would assert their error, not the asset's truth.
