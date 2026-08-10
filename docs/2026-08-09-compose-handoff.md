# Handoff — the Compose rebuild, 2026-08-09

**Branch `compose-carousel`, 12 commits, unpushed. `main` untouched.** Everything below was verified
against the source or a running editor on the day. Where a claim came from a review rather than from
me, it says so.

---

## 0. Read this first: the working tree is not clean, and not all of it is ours

| Path | Whose | What to do |
|---|---|---|
| `assets/emerge/site/library.ron` | **the author's live edits** | See §5. It currently makes `the_authored_edge_tokens_reach_the_editor` fail. Do **not** commit it without asking. |
| `docs/research/2026-08-09-composition-grammar-decisions.md` | untracked, predates this session | Leave it. Its §4 still needs the patch in §6. |

**Several agents share this repo.** One extracted `emerge-core/src/rng.rs` into `crates/det_rng` mid-session
and committed a red ratchet with it; another edited `bevy_speech_bubbles` and `bevy_stigmergy`.
**Always `git add` explicit paths.** A bare `git commit -a` will take someone else's half-finished work.

---

## 1. What the Compose tab is now

**A viewer. It writes nothing.** Authoring lives on the Map: arrange, `Shift+B` a box round it, `M`,
name it. `tests/compose_is_read_only.rs` is a source ratchet that fails if `compose.rs` ever names
`Compositions::FILE`, `save_atomic`, `to_ron()`, `fs::write` or `File::create` outside its test modules.

Four key rows, down from twelve: walk the focused list, which list the arrows walk, `O`/`P` previous
and next composition, `Enter` arm it for the Map.

The stage is a **carousel** — the focal composition at scale 1 pinned to the stage origin, up to two
neighbours either side at `1 → 0.55 → 0.30`, laid along the ground diagonal that reads horizontal at
the default yaw. The wings do not wrap: running out of miniatures is how the stage says which end of
the list you are at. Every visible group carries its id in world space.

**The seam inspector is not delivered** and cannot be, in this shape — a carousel never puts two
compositions side by side at the same scale. That is `FVS-R-13`.

---

## 2. Do this next, in this order

### 2.1 `FVS-R-12` — `bevy_debugger/input` cannot type

**Promoted to next, and it is the only thing gating sign-off on replace-on-capture.** It writes
`ButtonInput` but not the `KeyboardInput` message stream, so no agent can drive a text field. It
blocked three separate verifications in one session: naming on Compose, naming a capture on the Map,
and replace-on-capture. The crate is ours (`crates/bevy_debugger_mcp/`); a `kind: "Text"` emitting
`Key::Character` is the shape, and cursor injection landed the same day as the precedent.

### 2.2 The seven review findings still open

A max-effort review returned 15 confirmed findings. Three are fixed, two need no change (the rollback
deleted that code), **ten were skipped** — of which three have since been fixed, leaving these seven.
None bite the Site kit, which is why they are here rather than done.

| Where | What |
|---|---|
| `compose.rs` `footprint` | The `Anchored` branch returns the members' span but discards where it is *centred*, so a group whose members sit off their own origin renders outside the slot reserved for it. |
| `compose.rs` `height_of` | The `Anchored` branch treats every member as standing on the floor; `stack::resolve_y` seats a member with `on: Some(host)` on top of its host, so `tallest` is short by the stacked height and framing cuts the top off. |
| `compose.rs` `place_labels` | Writes `world_to_viewport`'s logical pixels straight into `Val::Px`, but the UI is scaled by `UiScale(1.2)` — every label sits 20% further from the top-left than the point it names, worst at the outermost miniatures. |
| `chrome.rs` NameBox | The inner panel carries neither `Pickable::IGNORE` nor `Hovered`, so the visible dialog reads as open world to every "is the pointer over UI" test — scrolling over it zooms the world behind it. |
| `chrome.rs` `paint_name_box` | Keyed on `Mode`, but `EditorState::grouping` is not mode-scoped: clicking the tab strip mid-name hides the box while the field keeps the keyboard, and every keystroke vanishes until `Esc`. |
| `compose.rs` `lay_out` | A stale `state.selected` is clamped inside `lay_out` but nowhere else, so the stage can stand up a different group from the one the panel marks selected, and `Enter` arms the unclamped one. |
| `headless.rs` | `pick_slot` is still never exercised by a test. `step_carousel` now is — see the pattern in §4. |

### 2.3 `FVS-R-5` — convert `site_67`'s architecture to stamps

Both design docs put this first for the grammar work, and the reason is structural: a grammar learned
from four prototypes with no co-occurrence data is degenerate whichever way it fails. Its traps are
already written down — five walls sit **centred on the tile seam** (`x = 0.0`, spanning `[-0.05, 0.05]`),
and **the floor rows must be replaced too**, because a tile carries its own floor and stamping onto
floored ground leaves two coplanar floors that z-fight.

---

## 3. Decisions made this session, so they are not relitigated

- **Nesting is called nesting.** `Descriptor::subgrid` is the per-mesh edge-token lattice, and
  `seating_divisions`/`face_bands` were split apart for exactly this reason. The word is retired for
  new things.
- **A stamp is an opaque instance** (`FVS-R-14`). Tools act on the instance, never through it: Delete
  removes the instance, move grabs the whole stamp, clone makes a second stamp. It is **not** a
  `Placement` add — giving stamped rows the loose-piece component makes every tool edit expanded rows,
  which is the failure the omission prevented. What is missing is a selectable identity, and the real
  work is `CloneSet` learning to hold a stamp as well as a piece. That is where nesting closes.
- **Capture writes to the loaded kit**, one path. A scratch kit plus a promote step is two.
- **Capturing over an existing name redefines it**, armed. It keeps the id so no stamp is stranded;
  `Stamped::of_fingerprint` already exists to notice the change and say so.
- **An empty composition is refused at load**, not at stamp time. Moving it downstream was tried and
  reverted the same day — see §6.

---

## 4. Things that will waste your time if you do not know them

**`keys::chord` renders the modifier now.** It used to return the bare field, so every message about
`Cmd+2` printed "2". If you write a chord into a string, use `chord` or `chord_text` — never
`binding(a).chord`.

**Driving a key in a headless test needs a system, not a `press` before `update()`.** Bevy clears
`ButtonInput` in `PreUpdate`, so a press written outside the frame is gone before `Phase::Act`. The
working pattern is in `the_carousel_stands_the_focal_group_up_with_its_neighbours`.

**A running editor breaks `bevy_debugger_mcp`'s tests.** `test_highlight_entities` asserts
`result.is_err()` against a hardcoded `localhost:15702`, so a live listener inverts it. `pkill -f
'debug/emerge-mapper'` before `cargo test --workspace`, or `--exclude bevy_debugger_mcp`.

**The VLM labeler needs an SSH tunnel.** `Connection refused` means
`ssh -L 9292:127.0.0.1:9292 -fN bmb` is not up. The default model is `qwen3-vl-30b` and the endpoint
serves it; the key is `EMERGE_VLM_KEY` in the gitignored `.env`.

**Run a second editor on `BEVY_BRP_PORT`.** Two instances silently fight over 15702 and you will drive
the wrong one — verify the port is free first, and confirm which process owns it.

**Never drive a destructive verb in an instance someone else is editing.** Not carefully — not at all.
I deleted nine pieces of the author's in-progress map demonstrating a verb, because I judged footprint
by *what I would place* (one piece) rather than by *what the verb enumerates* (every placement of the
armed descriptor). One `Cmd+Z` restored it pixel-identical, and that recovery is luckier than it looks:
`EditorState::record` is `push` with **no cap**, so nothing ages off. The kitbashing notes recommend
bounding the stack at 20–30, which would introduce exactly the aging-off failure this avoided.
Bounding it later needs a compensating guarantee.

**Mutation-test any ratchet you add.** `compose_is_read_only.rs`'s first draft `break`-ed at the first
`#[cfg(test)]`, which in `editor.rs` sits 900 lines above the write it was meant to find — it passed by
scanning nothing. Both ratchets added here were checked by injecting the violation and watching them
fail.

---

## 5. `library.ron` is dirty, and it is the author's

`site/wall` currently has `mount: OnSurface("worktop")` where the shipped file has `OnFloor`, a rescan
re-measured it to `0.10000067 × 2.3999999`, and one authored edge cell was lost (9 where the contract
wants 10). That is what makes `the_authored_edge_tokens_reach_the_editor` red, and it is also what
produces `placement wall@1 mounts on a worktop surface but records nothing under it` on the map.

**Ask before reverting.** `git checkout -- assets/emerge/site/library.ron` restores the contract, and
throws away whatever the author was doing.

---

## 6. One correction on the record, because it will look like churn

`86bd186` reverses a change from earlier the same session. The empty-composition refusal was moved out
of `validate_shape` into `expand`, on the sound argument that its own reason is about stamping. It was
wrong for two reasons found afterwards: `redraw_stamps` and `emerge-bevy` expand a whole map in **one**
call and the editor despawns every stamped row **before** it can fail, so one empty composition takes
every stamped row off the screen; and the load-time check names the empty composition while the
stamp-time one can only name the outer id of a nesting. Both halves are now pinned.

**Still owed:** the decisions doc's §4 falsification thresholds. One is inoperable as written —
"the 2-D histogram occupies < 5% of its populated bounding box" measures the populated region against a
box that region defines, so it can never fire. `10.48550_arXiv.2003.03377` (Alvarez, Dahlskog, Font &
Togelius, *Interactive Constrained MAP-Elites*) is the paper for it, and also carries `FVS-R-7`'s
missing authoring half already shipped: *"Brush painting with the lock button on preserves selected
tiles in all procedurally generated suggestions."* §4 says it must be committed **before any solve**.
