# Plan: make Compose an authoring tab

**Written to be executed from a cold start.** Every path, key and acceptance test below was checked
against the source on 2026-08-09. Design rationale is in `2026-08-09-unified-composition.md`; the
corpus check that settled it is `research/2026-08-09-grid-composition-corpus-check.md`.

---

## 0. Why this order

The Compose tab **reads** groups and cannot author one — which is why `compositions.ron` is
hand-written and has one row. Steps 1–3 need **no schema change**: `Envelope::Bounded`, `Member`,
`interface` and `Stamped` already exist and `interface` already resolves members against the
envelope-as-map exactly as the game will.

That matters because the one thing the corpus could **not** settle is whether the furniture kit and
the WFC solvers survive the inversion (a floor tile *provides* floor rather than standing on it). That
risk lives entirely in step 5. Steps 1–4 buy the evidence to decide it.

## 0.1 What the vetting added

Three constraints from `pcgbook_chapter11` (mixed-initiative), which `compose.rs` already cites:

1. **"the human has final say over what is produced by the tool."** Capture never guesses. If the
   envelope cannot be inferred, it refuses and says why — it does not pick.
2. **"All content that a human can produce … must be possible for the computer to generate on its
   own."** So capture produces **`Bounded`**, not `Anchored`: an anchored group presents no interface,
   and a group the solver cannot place is content the human can make and the computer cannot.
3. **"Should the computer be allowed to make new suggestions whenever it wants, or only when
   specifically requested?"** Capture is explicitly invoked by a key. Nothing is captured implicitly.

---

## Step 1 — build-from-selection

**Goal.** A box on the Map becomes a `Composition` in `compositions.ron`.

**The interaction, and why it needs no new tool.** `Shift+B` (`Action::CloneMode`) already drags a box
and puts a **set in hand**: `CloneDrag.held: Option<CloneSet>`, holding `ClonePiece { descriptor,
offset, yaw, tip, lift, note, owned, owned_because, on }` where `on: CloneHost` is already
`Layer` / `InSet(usize)` / `Outside`. That is exactly a composition's member list. This adds a second
verb on a state that already exists rather than a third box-drag.

### Files

| File | Change |
|---|---|
| `crates/emerge-mapper/src/keys.rs` | `Action::GroupFromSet`; bind `KeyCode::KeyM`, `Context::Map`. Declare it **immediately after `CloneMode`** and give it the **same `does` string**, so `rows()` collapses it into the clone row — Map is at its 12-row ceiling and `no_context_carries_more_than_a_learnable_vocabulary` enforces it. Suggested shared `does`: `"move / clone a set / keep as a group"`. Add to the action list in `every_action_has_exactly_one_binding`. |
| `crates/emerge-mapper/src/editor.rs` | `EditorState::grouping: Option<String>` (typed id, `pinning`'s shape). `Field::Group` row. `group_keys` in `keys::Phase::Text` (model on `pin_reason_keys`). The handler + the pure conversion below. |
| `crates/emerge-mapper/src/editor.rs` | `pub fn composition_from_set(...) -> Result<Composition, String>` — **pure**, so it is unit-testable with no `App`. |

### The conversion, precisely

`composition_from_set(set: &CloneSet, id: &str, library: &Library) -> Result<Composition, String>`

- **id** — `naming::to_snake_case`, forced not checked (the map-name rule). Empty after forcing is a
  refusal.
- **envelope** — `Bounded { size }`. Width/depth from `set.half` doubled. **Height is not in `CloneSet`**
  and must be computed from the members' drawn tops (`descriptor::placed_height`, which folds `scale`
  and `stretch_y`). A member the library cannot measure is a refusal, not a guess.
- **member ids** — `short_id(descriptor)` plus an index, deduplicated. Must be stable and unique.
- **`at`** — **relative to the tile centre, not the drag anchor.** `interface` builds its scratch map at
  `origin (0,0,0)` with `bounds = size`, and `Map::floor_rect` centres on zero. So subtract
  `set.centre_off` from each `ClonePiece::offset`.
- **`on`** — `CloneHost::InSet(i)` → `Some(member_id[i])`; `Layer` → `None`; **`Outside` → refuse.** The
  host stayed outside the box, so the group cannot carry it, and a member whose host is missing is a
  group that will not resolve. Name the piece in the refusal.
- **members sorted by id** — `Composition::validate_shape` refuses otherwise and names the order. One
  group, one encoding.
- **`of_fingerprint: None`** — unrecorded, not stale. The author presses `R` on Compose to record.

### The write

Same door `record_selected` uses: push onto `project.compositions.compositions`, then
`Composition::validate_shape()` **and** `composition::validate(&comps, &library)` **before** touching
disk; then `to_ron()` → `ron_surgery::save_atomic(emerge_dir.join(Compositions::FILE), &text)`.
A refusal at any point leaves both the file and the in-memory set exactly as they were, and goes to
`status.problem` so it sticks.

On success: `status.note`, and set `compose.armed = Some(id)` so the next Map click stamps what was
just made.

### Acceptance

- `cargo test -p emerge-mapper` green; `cargo test --workspace` green.
- **Unit tests, synthetic** (per `crates/emerge-mapper/CLAUDE.md`: no shipped assets) on
  `composition_from_set` with a hand-built `CloneSet` + `Library`:
  - members come out sorted by id;
  - `InSet` repoints to the member's id, `Layer` gives `None`, `Outside` **refuses and names the piece**;
  - `at` is relative to the centre — a set whose pieces straddle the centre has both signs;
  - `Bounded` size covers every piece's footprint and the tallest member's drawn height;
  - an unmeasured member refuses rather than defaulting a height.
- **Live check over BRP** (`--features debugger`, `BEVY_BRP_PORT` set): place two pieces, `Shift+B`
  round them, `M`, type an id, `Enter`; then `4` and confirm the Compose panel lists the group with a
  derived interface and no faults.

---

## Step 2 — interface tokens: cell → 2-D face component

**Goal.** `Interface::faces` carries **one token per face**, not one per boundary cell.

**Why it is safe to do early, and why I first said otherwise.** I argued this was a migration that a
later corner formulation would redo. That is wrong on Merrell's model: *Continuous Model Synthesis* and
`10.1109/tvcg.2010.112` keep edge **and** vertex assignments in one propagation, with vertex state
*defined as a set of adjacent edge assignments*. A corner constraint is therefore **additive over the
same tokens**, not a rewrite of them. Cell → face is the only migration of the data.

**Scope.** `emerge_core::composition::{Interface, interface}`, `emerge_core::adjacency`,
`compose.rs::summarise_face`. A face becomes one token derived from its cells, with the existing fault
when they disagree — the fault machinery already exists and does not change.

**Acceptance.** `summarise_face` stops needing to summarise. `adjacency::faults` still catches a
genuine disagreement between abutting pieces. Workspace green. **Expect no golden to move** — if one
does, stop and re-measure rather than re-pinning.

---

## Step 3 — lattice seating on Compose

The authoring surface: show the selected group's envelope subdivided, seat members into it, write
through the same door. Reuses the Tiles tab's subgrid editor idiom (cell cursor, layer picker,
verbs). **No schema change**: seating writes `Member::at` / `lift`, which already exist.

## Step 4 — author three or four real site tiles

floor+wall, floor+wall+sconce, a corner, a doorway. Stamp them. **This is the step that produces the
evidence for step 5**, and the one most likely to say the ergonomics are wrong. If authoring a tile per
combination is tedious even with nesting, the answer shifts toward Sturgeon's tags — a "lit wall" is a
wall plus a light tag, not a fifth authored variant — and step 3's UI changes shape.

## Step 5 — the schema

Only after step 4. Split values typed absolute/relative (CGA); the occupancy test **with
`Shape.occ("noparent")` scoping written in the same commit**; then retire the positional `Mount`
variants. Step 5 is where a golden is expected to move.

---

## Non-goals — do not touch while executing steps 1–3

- **`Mount`**, `stack::datum`, `stack::same_layer`. This is where the unproven inversion bites.
- **The occupancy test inside a composition.** `composition::interface` calls no overlap rule today
  (verified: no `blocking`, no `plans_overlap`, no `same_layer`). The CGA self-occlusion trap is
  therefore *prospective*. When the test is added, `noparent` scoping goes in with it.
- **The corner constraint.** Blocked on reading Lagae & Dutré for **layered vs replacement** — Merrell
  layers, and "square tiles with colored corners" reads like substitution. The PDF is staged at
  `/mnt/home-still/papers/LD/LD06AWTCECC.pdf`; `scribe_convert(stem="LD06AWTCECC")` then
  `distill_index` from wherever the server's papers root is.
- **Free placement.** `Placed::at` stays `(f32, f32)`. Structure is gridded; dressing is not — the
  corpus is unanimous.
