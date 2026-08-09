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
  and must be computed from the members' **resolved** tops. `descriptor::placed_height` is
  `extent.height × stretch_y` and **not** `× scale` — its own doc says `extent.height` "is already the
  post-scale value", and it replaced a `drawn_height` that multiplied by scale a second time. A member
  the library cannot measure is a refusal, not a guess.
  Resolved, not summed: a member may rest on another, so the height is the tallest `resolve_y` answer
  plus that piece's own height. That runs against a provisional zero-height envelope, which is sound
  only because ceiling-mounted members are refused first — they are the one mount that reads
  `bounds.1`, the very number being derived.
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
- **The commit door, through a fixture** (`a_captured_group_is_written_and_reads_back`): validates,
  writes atomically, adopts only on success; a duplicate id refuses and leaves both the file and the
  in-memory project untouched.

**~~An agent cannot drive this one end to end~~ — fixed while building step 3; see §3.8.** The gap
below was real when step 1 shipped and is not any more: `bevy_debugger/input` now carries a cursor
position, so the box drag is drivable. The paragraph stands as written because the reasoning in it —
treating a missing capability in a crate we own as a fixed constraint — is the thing worth not
repeating.


`bevy_debugger/input` takes `kind` / `action` / `key` / `button` and scroll `x` / `y` — **no cursor
position**. The gesture that fills a `CloneSet` is a box *drag*, so no injected input can produce one.
The keyboard half (`M` opens the field, `Enter` commits) is drivable; the mouse half is not. Either a
human checks the drag, or `bevy_debugger_bevy` grows cursor positioning — it is a vendored crate now,
so that is an ordinary edit.

---

## Step 2 — interface tokens: cell → 2-D face component

**The goal as first written was "one token per face". The data says that is wrong, and the measurement
is below.** What survives is the *dimension* reduction; what does not survive is collapsing a face to a
single word.

**Why it is safe to do early, and why I first said otherwise.** I argued this was a migration that a
later corner formulation would redo. That is wrong on Merrell's model: *Continuous Model Synthesis* and
`10.1109/tvcg.2010.112` keep edge **and** vertex assignments in one propagation, with vertex state
*defined as a set of adjacent edge assignments*. A corner constraint is therefore **additive over the
same tokens**, not a rewrite of them. That part stands.

### What the shipped kits actually present (measured 2026-08-09, both libraries)

**192 faces carry a subgrid. Four of them present two tokens at once**, and they are the four that
should: `site/wall_doorway`, `site/wall_doorway_wide`, `site/wall_window` and `site_greybox`'s
`wall_doorway_wide` read `wall` at the jambs and **nothing through the opening**. Their middle columns
are *absent from the file* rather than authored `edge: None`, which reads the same downstream —
`Subgrid::at` returns `None` either way.

So a face is not one token. Collapsing it would either fault every doorway or pick a winner, and
`Interface`'s own doc forbids picking: *"Derived and reported, never resolved by picking one."*

**Every one of the 192 is uniform in y.** The variation is entirely lateral. That is the dimension
reduction the corpus argued for, landing one level further down than I wrote it: a face is not a 2-D
cell grid, it is a **1-D lateral run**, and the vertical axis of the shipped kits carries no
information at all.

**But that uniformity is a property of the descriptors, not of the format.** `interface` samples `wy`
across the envelope and skips a member whose `y` span does not contain it, so a group mixing
`site/wall_low` with `site/wall` presents `wall` low and nothing high. Vertical variation is
*producible the moment step 1's capture is used*, so it may not be designed out.

### What is actually downstream

| Reader | Effect of changing it |
|---|---|
| `compose.rs:453` → `summarise_face` | one display line — **the only non-test consumer of `Interface::faces`** |
| `grammar.rs:722` → `adjacency::face` | the WFC prototype **signature**; collapsing it is what would move a golden |

`adjacency::face` is therefore **out of scope after all** — the per-cell vector is exactly what
distinguishes a doorway from a wall in the learned grammar. And the shipped `compositions.ron` holds
one `Anchored` group, which has no interface, so nothing shipped exercises `Interface::faces` at all.

### Decided: bands

`Interface::faces` is `[Vec<Band>; 4]`, a `Band` being a rectangle of the face that presents one
token. The decomposition is **strips first, then runs within a strip** — rows that read identically
across merge, then each strip splits where its token changes. That order is what makes it *canonical*:
a greedy rectangle cover of the same cells has several valid answers, and one face must have one
description.

**Positions are metres, not the fractions the option was written with.** `adjacency::seam` already
settled that comparing faces is a question about *where two pieces physically touch* rather than about
whether they are the same shape — it was changed away from whole-face equality for exactly that
reason — and normalised coordinates would reintroduce the defect for envelopes of different sizes.
Metres also read better in the panel: `wall across -1.50 to -0.50 m` tells an author the door is 1.2 m
wide, where `wall 0–25%` needs the tile size to decode. Say the word and the display can quote
percentages instead; the stored form should stay physical either way.

`summarise_face` became `face_rows` and no longer summarises. It quotes only the axis that varies — a
plain wall is one word, a doorway carries a span and no height, a group mixing a low piece with a tall
one carries the height and no span.

**One thing the fixtures taught, worth keeping:** a subgrid is indexed at
`descriptor::divisions(d, per_tile)`, so authoring at one density and reading at another is not a
coarser view of the same piece — it is a piece most of whose cells are absent. The first
division-independence test failed for that reason and was wrong, not the code. `tiled_divided` exists
so a test comparing two densities authors both.

**Acceptance — met.** `emerge-core` 384 unit tests green, `emerge-mapper` 119 + 26 headless green,
`adjacency::faults` untouched, and `adjacency::face` deliberately not touched at all.

---

## Step 3 — lattice seating on Compose

**Written to be executed from a cold start**, like step 1. Everything below was read out of the source
or the shipped RON on 2026-08-09.

### 3.0 The finding that shapes it: the lattice already exists

I expected to design a seating lattice. There is one, and it is the grid the editor has always used:

| | Quantum | Where it is already applied |
|---|---|---|
| Horizontal | `grid::SNAP` = **0.5 m** | `editor::snap(v) = (v / SNAP).round() * SNAP`, on every `Placed::at` |
| Vertical | `SNAP / policy.divisions` = **0.25 m** default | `editor::lift_step`, on every `Placed::lift` |
| Origin | envelope centred on zero | `interface` builds its scratch map at the origin, `Map::floor_rect` centres there |

`Member::at` is already written in that frame — step 2's doorway fixture put its jambs at `±1.0` in a
3 m envelope, which is on the lattice, and I had not noticed I was choosing lattice positions.

**So step 3 makes the lattice visible and navigable; it does not define one.** Anything that invented a
second quantum here would be a second grid for the same act, and the note under `GridSpacing` already
records what that cost the first time — the drawn grid said 1.0 m while the snap was 0.5 m.

### 3.1 What the corpus says about the surface

Re-checked against home-still on 2026-08-09; distill is up (8,249 embedded documents), so the standing
note that it was down is stale.

**Merrell et al., *Interactive Furniture Layout Using Interior Design Guidelines*** (in the library as
`furnitureLayout2`) is the nearest prior art and names its own lineage: *"Our interface is inspired by
Igarashi and Hughes' [2001] work on suggestive interfaces."* The mechanism worth taking is not the MCMC
sampler — it is the interaction: *"The user can constrain the suggestions by fixing some of the items
in place… This approach allows the user to **progressively pin down** the desired layout."* Seating is
incremental and per-member, never an all-or-nothing solve.

**Infinigen Indoors** (`10.48550/arxiv.2406.11824`) states the relation this project's `Mount::OnSurface`
is a weaker form of. `StableAgainst` *"checks that the child's surface is parallel to the parent's, **the
child is not overhanging**, and the child's surface is exactly at the specified margin"*; `SupportedBy`
adds *"the centroid of the child object is contained within the convex hull of the intersection between
the child and the parent"* — *"to ensure zero torque by gravity"*, the coffee cup teetering on the
table's edge. **`stack::resolve_y` checks neither.** A member can be seated half off its host and
nothing says so. That is real evidence for step 5's occupancy test and is written down here rather than
built now, because the non-goals below still hold.

**Bukowski & Séquin, *Object associations*, SIGGRAPH 1995** (`10.1145/199404.199427`) is the classic on
objects that know what they attach to. **Not read** — CrossRef has it, there is no OA PDF, and
`paper_download` finds nothing. Cited here from Merrell's reference list, not from the text.

Already applied and unchanged: Tutenel's *"snapping to the nearest valid location"*, and pcgbook ch.11's
three constraints, of which the third governs this step — *"only when specifically requested"*, so every
verb below is a keypress and nothing seats itself.

### 3.2 Scope: the Map captures, Compose seats

Step 1 gave the Map a capture verb, so **creation is answered**. Step 3 gives Compose the verbs to
refine what capture produced: walk the members, move one on the lattice, raise it, turn it, remove it.

**Adding a member from the library is deliberately not in this step.** It would be a second library
browser beside the Tiles tab, and step 4 — authoring real site tiles — is the step that says whether
seating-only is enough. This is a stated scope decision, not a stub: every verb that ships is complete.

### 3.3 What the author sees

**The group, staged in 3D, through `composition::expand` — never a second interpretation.**
`redraw_stamps` already does exactly this for the Map: build a scratch map, `expand` the stamps against
it, `stack::resolve_y` over a map carrying both, then `spawn_piece` → `emerge_bevy::spawn_descriptor`.
Compose builds a scratch map whose bounds are the envelope and whose single `Stamped` is this group at
the origin, and runs the same three calls. So what Compose shows *is* what a stamp produces, which is
the crate's "borrowed, not copied" rule and the reason `spawn_piece` exists at all.

Drawn over it with gizmos: the envelope box, the `SNAP` lattice on its floor, and a highlight on the
selected member. An `Anchored` group has no envelope; it stages, and the box and lattice are absent
rather than invented.

### 3.4 Files

| File | Change |
|---|---|
| `crates/emerge-mapper/src/compose.rs` | `ComposeState::member` cursor; the staging system, the gizmo pass, and the five verbs; `ComposeUndo`. |
| `crates/emerge-mapper/src/compose.rs` | The pure core: `seat`, `staged_rows`, `apply_seat`. Unit-testable with no `App`. |
| `crates/emerge-mapper/src/keys.rs` | Five new rows — see the census budget below. |

### 3.5 The verbs, and the census budget

Compose shows **3 rows** today (`up`/`down` collapse to one, `Enter`, `R`), so there are nine spare
under the twelve-row ceiling `no_context_carries_more_than_a_learnable_vocabulary` enforces. Five are
taken:

| Chord | `does` | Note |
|---|---|---|
| `left` / `right` | walk this group's members | Symmetric with `up`/`down` walking the groups. Needs no new letter. |
| `T` `F` `G` `H` | seat the member / raise | **The same cluster the Tiles lattice cursor uses.** Contexts are separate, so this is one gesture meaning one thing on two surfaces, not a collision. |
| `[` / `]` | seat the member / raise | Same `does` as the row above, so `rows()` collapses them — the Tiles pairing again. |
| turn chord + `Shift` | turn a quarter / Shift: 15° | Two chords, one row, the `Generate`/`GenerateDeclared` idiom. **Reuse the Map's own turn chord** rather than inventing one. |
| `REMOVE_KEY` | remove this member | The constant the Tiles tab already binds. |
| `Z` / `Shift+Z` | undo / redo | Map, Tiles and Anim each have this pair; an editing surface without it would be the odd one out. |

**Quarter turns on the bare key, 15° on Shift, and that ordering is the argument.** A group is a tile:
`adjacency::quarter_turns` refuses a yaw that is not a multiple of 90° and names the piece, so a
tokened member turned 45° makes the whole group's interface underivable. It only bites a member whose
subgrid carries edge tokens — `interface` skips the rest — so 15° stays reachable for a chair drawn up
to a table, on the modifier, where it cannot be hit by accident.

### 3.6 The write model — immediate, because that is the one `compositions.ron` already has

The Map edits in memory, sets `project.dirty`, and saves explicitly. **`compositions.ron` does not work
that way and must not start:** `record_selected` and step 1's `keep_as_group` both `save_atomic` on the
keypress. A staging buffer for this file would be a second write model for one file.

So each seat writes through the same door step 1 built: mutate a clone, `Composition::validate_shape`
**and** `composition::validate` against the whole set, `to_ron`, `save_atomic`, adopt only on success.
A refusal leaves the file and the in-memory set exactly as they were and goes to `status.problem`.

That is safe here for a reason specific to this file: `compositions.ron` **carries no `//` comments on
purpose**, recorded in its own `note`, precisely because `to_ron` reserializes. There is nothing for a
rewrite to lose.

**`of_fingerprint` is not touched by a seat.** It records what a member's *body* was built against;
moving one changes no body. Writing it here would make every seat look like a re-record.

**Undo holds whole `Compositions` values**, most recent last, bounded. The Map's own note argues the
shape — *"Every variant's inverse is another variant of this same enum"* — and for a file this small
the simplest thing with that property is the value itself. The Tiles tab already keeps its own stack
(`ImportState::undo`), so a third is the established pattern rather than a new one.

### 3.7 Acceptance

- `cargo test -p emerge-mapper` green; `cargo test --workspace` green; **no golden may move**.
- **Unit, synthetic** (`crates/emerge-mapper/CLAUDE.md`: no shipped assets):
  - `seat` snaps to `SNAP` horizontally and `SNAP / divisions` vertically, and **refuses to leave the
    envelope** rather than clamping — a member outside the bounds it is being seated in is a refusal
    the author has to see;
  - an `Anchored` group has no envelope, so seating it is refused by name rather than bounded by a
    made-up box;
  - a quarter turn lands on a multiple of 90 from any start, so `quarter_turns` cannot refuse a member
    that was only ever turned by this verb;
  - `of_fingerprint` survives a seat unchanged.
- **The commit door, through a fixture:** a seat writes and reads back; a seat that would produce an
  invalid composition writes nothing and leaves the in-memory set untouched; undo restores the previous
  value and redo re-applies it.
- **Headless:** the new systems are registered and the app survives frames in `Mode::Compose` — the
  thing no unit test can see, and the reason `tests/headless.rs` exists.

**An agent can drive this one end to end**, unlike step 1: every verb is a keypress, so
`bevy_debugger/input` reaches all of them. The missing cursor position only blocked the box drag.

### 3.8 What changed while building it

**The cursor gap was closed rather than worked around.** `bevy_debugger/input` now takes
`kind: "Cursor"`, so step 1's box drag is drivable too. It writes a `DebugCursor` resource and **not**
`Window::set_cursor_position`: Bevy's windowing backend diffs the window's cursor against a cache each
frame and asks the platform to move the *physical* pointer (`bevy_winit-0.19.0/src/system.rs:433`),
and the cache is `pub(crate)`, so a plugin cannot suppress it. `emerge-mapper` consumes it through
`view::Pointer`, filled once a frame in `Phase::Sense` — `window` had no other use in `editor.rs`, so
seventeen window params became one resource.

The example written for it (`cursor_drag_lands`) immediately found a real ordering bug: the injection
queue was ordered *per key*, so a release overtook two still-pending moves and a drag committed at the
wrong corner while every individual rule behaved as written. Order is now total.

**Compose stopped sharing the Map's camera, reversing a decision that was on the record.** The old
argument — *"the tab is a list and a detail pane… a camera that jumped to a stage and back would make
that one gesture look like two places"* — rested on a premise the seating verbs remove. A surface that
edits geometry it cannot show is worse than a camera jump, and the Tiles tab has jumped and restored
for as long as it has existed.

**The verbs' key row landed on `Y`/`U`, not `,`/`.`, and a test decided it.** `rows()` joins a
collapsed row's chords with `", "`, so a chord that *is* a comma prints `, , .` and cannot be read
back; `collapsing_rows_loses_nothing` failed naming the vanished chord.

**Delivered:** `left`/`right` walk the members, `T F G H` `[` `]` seat, `Y`/`U` turn, `Delete` drops,
`Z`/`Shift+Z` undo — 8 rows of Compose's 12. The group stands on its own stage through
`composition::expand`, with the envelope and the `SNAP` lattice drawn over it. `emerge-mapper` 127
unit + 27 headless green.

---

## Step 3 — original sketch (superseded by §3.0–3.7 above)

The authoring surface: show the selected group's envelope subdivided, seat members into it, write
through the same door. Reuses the Tiles tab's subgrid editor idiom (cell cursor, layer picker,
verbs). **No schema change**: seating writes `Member::at` / `lift`, which already exist.

## Step 4 — four real site tiles: **done, and the verdict is below**

Authored in `assets/emerge/site/compositions.ron`; pinned by `tests/site_tiles.rs` (9 tests).

### The claim held

`site/tile_floor`, `site/tile_wall_n`, `site/tile_corner_nw`, `site/tile_doorway_n` — each exactly
`1.0 × 2.4 × 1.0`, each deriving a clean interface that presents what it is made of. **Not one wall
piece in the kit is cell-sized** (`site/wall` 0.1 × 1.0, `wall_corner` 0.22 × 0.22, `wall_doorway`
0.46 × 2.06) and `grammar::learn` refuses every one of them by name. A group of floor-plus-wall is
exactly the cell. That is the design's central claim, demonstrated on shipped assets rather than
argued.

A 3 × 3 room of them expands, stacks, and passes `adjacency::faults` with **zero** seam
disagreements — the half a per-tile test cannot reach, since four correct tiles can still contradict
each other where they meet.

### It said the ergonomics were wrong, twice, and both are fixed

1. **A 0.5 m lattice cannot seat a 0.1 m wall.** Flush is at `−0.45`, off the lattice by construction.
   Step 3 had implemented CGA's *absolute* split value only; `Shift`+seat now flushes to a face,
   which is the *relative* one. Without this the tiles were unauthorable by the tool that exists to
   author them.
2. **The shipped map straddles seams.** `site_67` centres walls on the tile boundary, half in each
   neighbour. Read as a composition such a wall sits on no face at all and the group presents
   nothing. Tiles inset instead — a real divergence from existing content, recorded as a todo rather
   than silently reconciled.

### And it argued for tags over variants, three times without being asked

The open question was whether the cross product gets authored or composed. Every time step 4 reached
for an authored variant, **the assets refused**:

- **The doorway could not be placed, only built.** `site/wall_doorway` is 2.06 m — neither a 1 m cell
  nor a 2 m one. `wall_header` lifted 2.0 m gives a lintel with the opening beneath, which is a better
  tile *and* the first shipped thing to produce a vertically banded face — the case step 2 kept
  representable on the argument that y-uniformity was a property of the descriptors and not of the
  format. This is the group that proves it.
- **The corner is two seatings of one piece**, not a fifth mesh.
- **One wall tile serves all four orientations** — stamped at each quarter, pinned by
  `one_wall_tile_covers_four_orientations`.
- **The sconce could not be authored at all**: the Site kit has no wall-mounted piece. `wall_light`
  lives in the main pack.

Four for four. Sturgeon's tags, Karth & Smith's edge-constrained multi-tile modules and CGA's
query-time `Shape.occ` all said the cross product need not be enumerated; the kit has now said it
cannot be.

### Looked at, not only measured

Every claim above was numeric until it was checked in a running editor
(`cargo run -p emerge-mapper --features debugger -- . site_67 --kit site`, driven over BRP). That
distinction is not pedantry — this project's own notes record *"three Site editor bugs that were
invisible to a green test suite and visible only in a measured frame"*, and a clean `adjacency::faults`
is a statement about **tokens agreeing**, not about geometry meeting.

What the frames showed: the envelope and its 0.5 m lattice draw correctly; the corner tile's two walls
meet **closed**; the doorway's lintel sits at the top of the envelope with the opening beneath, reading
as a doorway rather than a floating beam; the floor's edges sit on the envelope, the parts that
overhang being the mesh's own connector nubs. And the one that mattered — **three tiles stamped side by
side make one continuous wall**, with clean butt joints, no gap and no doubled thickness.

The stamps were placed by injected cursor plus a click, in a real window, with the machine's own mouse
untouched — the capability added earlier the same day, used in anger rather than only in its tests.

**One finding no test would have produced.** A tile carries its own floor, so stamping one onto ground
that already has floor leaves **two coplanar floors** — visible as a changed surface in the frame, and
something that would z-fight in motion. That is why converting `site_67`'s hand-placed architecture
must *replace* its floor rows as well as its wall rows, not add stamps beside them.

### The one decision step 5 is still gated on

**Is a lit wall a tag on the wall tile, or a fifth authored variant?** Tag → step 5 shrinks and the
cross product is never authored. Variant → the kit needs a sconce first, and step 5 stays as scoped.
Everything above points at *tag*, but it is a schema decision and belongs to the author.

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
