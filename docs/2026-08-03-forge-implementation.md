# Forge — implementation handoff

**For whoever picks this up next.** The design lives in `2026-08-03-forge-plan.md`; the evidence under
it in `2026-08-03-asset-schema-audit.md`; the corrections that shaped it in
`2026-08-03-forge-plan-review.md`. This file is the working state: what is built, what is next, the
exact evidence for each decision so it need not be re-derived, and the traps already paid for.

Read `CLAUDE.md` § "Bevy 0.19" before writing any Bevy code.

---

## 1. Status

| Stage | State | Commit |
|---|---|---|
| 0a — carry-over docs, Bevy reference, CLAUDE.md | **done** | `701f13c` |
| 0b — workspace split, engine-free core | **done** | `4f912bf` |
| 1 — descriptor + map schemas | **done** | `99e911e` |
| 1 — GLB measurement | **done** | `a8ad36f` |
| 1 — RON surgery unification | **not started** | |
| 1 — vocabularies + converter + byte-pin | **not started** | |
| 2 — `forge-bevy`, dungeon furniture migrates | not started | |
| 3 — Site migrates, `required:` list | not started | |
| 4 — `forge-editor`, delete `src/site_editor/` | not started | |
| 5 — mesh importer UI | not started | |
| 6a / 6b — smart objects, single- then multi-actor | not started | |

**Baseline to hold:** workspace `cargo test` = 36 suites / 1135 tests green;
`cargo check --release --workspace --tests` compiles. `snapshot_hash` must not move through Stage 3.

### What exists in `crates/forge-core/`

| Module | Notes |
|---|---|
| `rng`, `wfc`, `geom` | moved unedited in 0b |
| `placement/{ir, solver, solvers/*, scatter, manifest}` | moved unedited except the two noted below |
| `placement/surfaces` | **new home** of `SURFACE_CLASSES` / `surface_bits` / `provided_surfaces` / `required_surface`, extracted from `placement::furnish` (which is the Bevy boundary and could not host something `manifest::validate_manifest` needs). Re-exported from `furnish` at the old path. |
| `descriptor` | the patch-shaped asset schema + `resolve()` |
| `map` | placements, `owned`, and **locations** carrying interactions |
| `glb` | hand-rolled reader, `measure()`, `origin_alignment()`, `derive_front()`, `front_detail()` |

Two couplings were broken in 0b and must not come back:

- `solvers::metropolis` took `dungeon::WALL_THICKNESS`; it is now `MetropolisSolver::new(weights,
  wall_inset)`. Callers pass `crate::dungeon::WALL_THICKNESS`.
- `manifest`'s `every_shipped_manifest_glb_exists_on_disk` called `crate::config`; it now lives in
  `tests/manifest_assets.rs` on the game side.

`crates/forge-core/tests/engine_free.rs` is the ratchet: `ALLOWED_DEPS` is closed to
`{serde, serde_json, ron, rand, rand_chacha}`, and no non-comment line may name an engine crate.
**Widening `ALLOWED_DEPS` in the same commit as the dependency is the intended workflow**, not a
workaround — that is the review step it exists to force.

---

## 2. Research evidence

Everything here was read from the local home-still corpus and verified verbatim. Paths are for
re-reading, not for trust — but nothing below needs re-deriving.

### 2.1 Interactions belong to a *location*, not a descriptor

**The single most expensive decision to get wrong**, because retrofitting it migrates every authored
map.

**Game AI Pro 3 ch.35 (FINAL FANTASY XV)** — `papers/ga/gameaipro3-ch35-ambient-interactions-improving-believability.pdf`:

> "smart locations abstract away from concrete objects… They are invisible objects that refer to
> multiple concrete objects. For example, a single smart location may refer to two chairs and a table.
> This allows it not only to inform agents about the existence and usability of individual objects, but
> also to capture relationships between them, such as furniture grouping… But smart locations do not
> just contain information; they essentially govern the usage of the objects they refer to."

Also: four emitter kinds (notification / script / spawn / player); role allocation is a randomized
greedy Monte-Carlo algorithm **explicitly allowed to fail**; typical instances are under four roles and
five actors; scripts are STRIPS-style rules over a tuple space.

**Game AI Pro 2 ch.11 (Smart Zones)** — `papers/ga/gameaipro2-ch11-smart-zones-create-ambience-life.pdf`:

> "Main roles are essential to execute the Living Scene… The scene won't start unless all the main roles
> are fulfilled with characters belonging to the Smart Zone."

> "If no NPC is able to take the role, the module starts a dynamic search operation to find an NPC able
> to take a main role around the Smart Zone. This search operation uses an expanded zone, which is
> automatically extended until the NPC is found or until the expanded zone wraps the world zone."

Supporting roles are favourable; extras are optional. This is why `map::RoleKind` has three variants and
why `validate` refuses a `Main` role with `min: 0`.

**Game AI Pro 4 ch.4** — `papers/ga/gameaipro4-ch04-knowledge-power-overview-ai-knowledge.pdf`:

> "a smart object is an abstraction that encapsulates some arbitrarily complex game logic behind a
> simple interface and ties this logic to a particular location or entity in the world. This is a way to
> embed knowledge in the world, instead of encoding it in an agent… The Sims is the poster child."

> "the types of links can often be reduced to a reasonable set of core capabilities, in which case a
> simple bit-mask can be used to represent the requirements for the link and the capabilities of the
> agent. Comparing these bitmasks is a very efficient way to filter out invalid links."

The bitmask point matters: `placement::surfaces::surface_bits` already does exactly this, so the
capability matcher in Stage 6 should reuse the pattern rather than invent one.

**Geishauser, Cheong & Nelson (FDG 2014)** — `papers/fd/fdg2014_fdg2014_demo_03.pdf`: Territories
compute slots in 2D space (a store table has one merchant slot and three customer slots), described as
"a step away from playing animations in rigidly fixed slots as seen for example in The Sims". Same axis
as sockets carrying a `role`.

### 2.2 Clearance is a schema field, not a solver detail

**Tutenel et al. 2010** — `10.1016/j.cag.2010.11.011` and the semantic-scene-description work: classes
contain *features*; **off-limits** features may overlap nothing, **clearance** features may overlap only
other clearance features. That distinction is why two chairs may share pull-out space but neither may
share it with a wall.

**Merrell et al. 2011** (already implemented as `placement::solvers::metropolis`) supplies numbers:
36″ bedside, 30″ in front of a seat, 24″ in front of shelving, 36″ around a dining table, 16–18″ coffee
table to seat.

`placement::ir::Predicate::Clearance(f32)` **already exists**. The descriptor had no way to state it,
which is why `descriptor::Clearance` was added at Stage 1 rather than later: adding it after
`check_prop_placements` is calibrated against Stage 3's gate is much more expensive.

### 2.3 Mixed-initiative authoring

**Liapis, Yannakakis & Togelius, *Sentient Sketchbook* (FDG 2013)** — real-time evaluation of the
designer's edit shown beside the canvas. Already the basis of the F7 editor's live rules panel.

**Alvarez et al. (FDG 2018)** — `10.1145/3235765.3235810`. The **lock brush**: the designer locks tiles,
the room subdivides into mutable/immutable zones, and every generated suggestion preserves the locked
ones; the genotype becomes a tree over zones rather than one gene per tile. This is `map::Placed::owned`.

**Smelik, Tutenel, de Kraker & Bidarra (2010)** — `10.1145/1814256.1814258`. **Cite as aspiration, not
prior art.** §5.3 names Locking, Scoping and Grouping, but as *"possible facilities, which are inspired
from image processing software"*, in a paper whose own assessment is that integrating procedural
generation with manual editing is *"so far as good as unaddressed."* The correction is recorded in
`2026-08-03-forge-plan-review.md` §4 and applied in `2026-08-03-kitbash-editor.md`.

**Karth & Smith (FDG 2017)** — WFC *is* finite-domain constraint solving. This is why an owned tile is a
**unary constraint** rather than a special case: `wfc::collapse_grid` already takes a per-cell `initial`
domain bitmask and `wfc.rs` already has `initial_domains_restrict_output` asserting a pinned cell holds.

### 2.4 Measured facts about this project's assets

From `a8ad36f`, validated against the shipped kit by `tests/mesh_measurement.rs`:

| mesh | upper-45% XZ asymmetry | kit records |
|---|---|---|
| `chair` | 166 mm | `Some(90.0)` |
| `command_chair` | 83 mm | `Some(90.0)` |
| `stool` | **12 mm** | `None` |
| `bench` | 1.9 mm | `None` |
| `mess_table` | 2.0 mm | `None` |

`FRONT_MIN_OFFSET = 0.05` sits in the gap. **`site::kit`'s prose is slightly wrong** — it says a stool
and a bench "measure symmetric to within a centimetre", and the stool is 12 mm; a threshold read off
that sentence rejects it. Its asymmetry is a modelling detail, not a backrest.

All 45 kit meshes reproduce their hand-measured height and footprint to 2 cm, 0 skipped for node
transforms.

---

## 3. Remaining work

### 3.1 Finish Stage 1

**(a) Unify the two RON surgeons.** `bake.rs` has `Leaf { path, span, text }`, `scan_ron_leaves`,
`splice_block`, `find_block_value`, `scalar_eq` (compares by parsed `ron::Value`, so `0x5C09191` and
`96506257` are equal and the hex spelling survives). `site_editor/source_map.rs` has the line-oriented
`replace_field`, `trailing_comment`, `comment_split` (quote-aware), `save_atomic`. Move the generic half
into `forge-core`; leave `SourceMap` as a typed wrapper over it.

Keep both mechanisms — they solve different problems. `bake`'s span splicer works on nested config
blocks; `source_map`'s line rewriter works because every record in `site67.ron` is exactly one line
(measured: 1070 of them). Pin the existing contracts: a no-op save is **byte-identical**, a move changes
exactly one line, undo restores bytes including the trailing comment.

**(b) Vocabularies.** `descriptor`'s `kind` / `effects` / `look` need validated token sets, in the shape
`placement::surfaces` already uses (closed table, two-sided validation, error names the offending item
*and* token). Note the precedent that region room-types are cross-validated against
`config.ron:dungeon.room_types` by `mycelia::validate_damp_coverage`.

**(c) Converter + byte-pin.** Convert `config.ron:placement.furniture` (41 items) and `kit_ozea.ron`
(45 pieces) to descriptors. The gate is that converted descriptors reproduce today's semantics exactly
— a diff, not a judgement. **Drop the dead surface rather than carry it**: `category` (no reader),
`Role::Custom` (never constructed), `Host::Floor` (unused), `Host::Opening` (no spawn path), and the
four unread affordance tokens (`sleep`, `store`, `decor`, `hygiene`).

Mapping notes: `Role::Scatter { surface }` and `KitPiece::rests_on` are **the same relation in two
spellings** and both become `Mount::OnSurface { class }`. `Role::Anchor { host: Wall }` becomes
`Mount::OnWall { height }` — the sconce row hardcodes `WALL_LIGHT_HEIGHT = 1.8` in `furnish.rs`.
`is_floor_marking`'s height heuristic becomes `Mount::Overlay { on: Floor }`, stated rather than
inferred.

### 3.2 Stage 2 — `forge-bevy`

Loads descriptors + map, spawns, and attaches what does **not** survive spawn today: interned `Tags`,
`SmartObject`, and `MountedOn` / `PlacedIn` **as Bevy relationships** rather than bare components. The
repo already runs three relationship pairs (`HeldAt`/`SiteSpecimens`, `MemberOf`/`SquadRoster`,
`Holding`/`HeldBy`), so this is idiomatic, and the reverse index ("what is on this worktop?") comes free
instead of being hand-maintained.

Two documented relationship gotchas: Bevy expresses an empty target by **removing** the component (so
always `Option<&T>`), and target order is attach order, **never** a total order — anything that picks
must sort by a stable key first.

Migrate the **dungeon furniture first** (41 items, already string-keyed, no closed enum to replace).
Gate: `snapshot_hash` unmoved.

### 3.3 Stage 3 — the Site migrates

The hard part is identity. `SiteKit` is a struct with one field per `SitePiece` and
`deny_unknown_fields`, so a kit missing a structural piece fails at **parse** time; the failure it
guards is an invisible wall. Replace with a **`required: [id, …]`** list validated at load.

Bevy's `SceneComponent` is *not* a stronger substitute: `SceneComponentInfo` carries
`spawned_from_scene: bool` behind `#[cfg_attr(debug_assertions, component(on_add))]` — a debug-build
runtime check, peer to a load-time list.

### 3.4 Stages 4–6

Per the plan. Two things to carry forward:

- **Stage 6 is split** because its original gate ("one interaction drives an agent") was satisfiable in
  an afternoon and proved nothing. 6a is single-actor; **6b's gate is "four agents fill a four-seat
  table with no deadlock and no double-booking."** Both game-AI chapters report the query API is the
  easy half and role allocation the hard one.
- **`src/site_editor/` is deleted at Stage 4**, not maintained alongside. Its parts port: `source_map`,
  `edit` (undo/redo + live faults), `thumbs` (the photo-booth baker), `pick` (data-space footprint
  hit-test), `overlay`, `ghost`, `panel`.

---

## 4. Traps already paid for

Bevy-specific ones are in `CLAUDE.md` § "Bevy 0.19". These are project-specific and cost real time:

| | |
|---|---|
| **`TransformGizmoPlugin` is unusable here** | Its overlay camera blanks an HDR main camera: the frame went 13,343 → 183 distinct colours, median luminance 57 → 0. Forcing `ClearColorConfig::None` did not help — it composites over an HDR/bloom camera and blanks it regardless. Its second `Camera3d` also killed nine `Single<.., With<Camera3d>>` systems. Removed; `crate::MainCamera` is the positive filter that prevents recurrence. |
| **A leaked full-screen UI node hides the world *and* eats every click** | `TitleRoot` is 100%×100% with an opaque background and no `Pickable::IGNORE`. Leaving `Title` on its first frame raced `spawn_title`, so `OnExit` despawned nothing. Wait for the entity, not for a frame count. |
| **The editor's placement was never broken** | A self-test drove it without a mouse: records 86 → 87, body spawned at the right world position, `mesh=true drawn=true`. It had been working into a blanked framebuffer the whole time. When something "does not work", prove which half. |
| **Three bugs were invisible to a green suite** | A blanked framebuffer, a leaked node, a duplicate-component panic. All found by rendering a frame and measuring it. **Render and look, every stage.** |
| **`pkill -f <name>` can match its own launcher** | It killed the shell that had just started the game, producing exit 143 and an empty log. Use `pgrep -f '[f]oo'` and kill by PID. |
| **Linux truncates process names to 15 chars** | `pkill -x foundation_vs_slop` silently matches nothing, so old instances survive and answer the screenshot sentinel. Two instances tiled side by side produced a completely misleading capture. |

### Verification recipes

```bash
cargo test --workspace                       # hard gate: 36 suites / 1135 tests
cargo check --release --workspace --tests    # the CI job a debug-only import once broke
cargo test --features test-harness -- --test-threads=1   # replay/liveness; needs a GPU, ~62 min
```

Visual, on this host:

```bash
FVS_SITE_EDITOR=1 ./target/debug/foundation_vs_slop &   # walks to Site-67, opens the F7 panel
sleep 85 && touch screenshot.request && sleep 6         # → screenshot.png
```

Measure the frame rather than eyeballing it — a distinct-colour count and median luminance caught the
blanked framebuffer that a glance called "dark". A healthy Site-67 frame at 3440×1440 is ≈11,000
distinct colours with median luminance ≈47; a blanked one is ≈180 with median 0.

**Keystroke and mouse injection are not available on this host** (no `ydotool`/`wtype`/`xdotool`, and
the user is not in the `input` group). To exercise an input path, add a temporary self-test system that
calls the same function the input handler would, and delete it afterwards — that is how the placement
path was proven while the screen was blank.

---

## 5. Open decisions

1. **Crate naming.** `forge-*` is a placeholder and nothing depends on it.
2. **Map comments.** The surgical writer exists because `site67.ron` is 15% comments and the props list
   carries more prose than data. An editor-owned map may not need it; a hand-authored one does. Decide
   per file by who owns it, and say so *in* the file. `save_using_saver` (0.19) covers the
   machine-owned half.
3. **`stretch_y`.** Game policy (a 2.0 m wall mesh made to reach 2.4 m), not an art fact. Under the
   patch model it is more honestly a project-level layer over the descriptor base than a descriptor
   field. Currently on `Align` because that is where the Site kit kept it.
4. **BSN.** The descriptor is patch-shaped so a `.bsn` port stays mechanical. Revisit the moment a
   first-party `.bsn` asset loader lands — it is listed first under Bevy's "What's Next".
5. **Phase 4 of the old plan is still open**: dressing the research wing and the containment cell
   fronts (`BACKLOG.md:833-839`). Note `src/site/visuals.rs:579` `enclose_containment_cell` exists and
   is **never called** — the enclosure may be half-written already.
