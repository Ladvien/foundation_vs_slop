# emerge-mapper — a standalone world-building application

**Status:** design, approved 2026-08-03. Nothing below is built.
**Companions:** `2026-08-03-asset-schema-audit.md` (the findings this rests on),
`2026-08-03-emerge-mapper-plan-review.md` (the review that corrected it),
`2026-08-03-kitbash-editor.md` (owned tiles + WFC, folds into Stage 1/4).

**The name.** The application is **emerge-mapper**, named 2026-08-03. Three crates carry it:
`emerge-core` (engine-free schema, validation, solvers, RON surgery, mesh measurement), `emerge-bevy`
(the runtime plugin the game consumes), and `emerge-mapper` (the standalone binary). The earlier
`forge-` prefix was a placeholder and is gone.

## Context

The F7 Site editor shipped, but it is a dressing tool welded to one game's hub. What is wanted is a
**separate application** that authors levels and assets and emits what the game consumes, reusable
across games: map save/load, a **layering** system (decals on walls, distinct from physical stacking),
a **mesh import + alignment** tool, and **tags** that drive gameplay.

Three findings from the audit decide the shape:

1. **~2,400 LOC is already engine-free** and moves unedited. The reuse boundary exists; it has no crate
   around it.
2. **Gameplay reads essentially no prop tags.** Only `"emit"` has runtime effect; a placed prop keeps
   only `PlacedIn(RegionId)`. **Smart objects are a new system, not a wiring-up.**
3. **Layering is the missing primitive.** `PropPlacement` has no Y, host or normal; "floor decal" is
   *inferred from mesh height* (a threshold that already misclassified a mug); the only wall-mounted
   object is `DoorPlaque`, hardcoded in Rust. "Decals on walls" is unsayable today.

## Decisions

| | |
|---|---|
| Schema | A third schema; both existing ones migrate |
| Tags | **Typed axes**, not one flat list |
| Tags → gameplay | Full smart-object behaviours |
| Editors | The standalone app replaces the F7 tool |
| Interactions | **On a map-level location, not on the descriptor** |
| BSN | **Track it; shape the descriptor as a patch. Do not adopt `bsn!` for authored content yet** |

---

## The BSN position

Bevy 0.19 shipped BSN. Verified in the vendored source: `bsn!` / `bsn_list!` in
`bevy_scene_macros-0.19.0`; `bevy_scene` split with `bevy_world_serialization-0.19.0` alongside. The
overlap is real and must be stated rather than discovered in 0.21:

- **Patch semantics** — a BSN expression is a patch, fields layering over defaults. That is exactly the
  descriptor → map-instance-override model. Reinventing it silently is the mistake.
- **Instance caching** (`:` syntax) resolves a scene once and layers per-instance data on top — built
  for "200 copies of one prop".
- **`SceneComponent`** is weaker than it sounds: `SceneComponentInfo` carries `spawned_from_scene: bool`
  behind `#[cfg_attr(debug_assertions, component(on_add))]` — a debug-build runtime check, peer to the
  `required:` list below, not a replacement for `deny_unknown_fields`.

**Position.** 0.19 ships no first-party `.bsn` *asset loader* and invites bespoke formats meanwhile,
and our content is authored by an editor writing files rather than by Rust code — so `bsn!` is the
wrong shape today. But the descriptor is defined as **a patch over defaults** (every field optional,
absence means inherit), so the eventual port is mechanical rather than a fourth migration. Revisit the
moment a `.bsn` loader lands; it is first under "What's Next".

---

## Research grounding

- **Smart objects** — Game AI Pro 4 ch.4: knowledge embedded in the world, not the agent; matching by
  **capability bitmask**, which `surface_bits`/`rests_on_bits` already implement here.
- **Smart Zones** — Game AI Pro 2 ch.11 (de Sevin, Chopinaud & Mars). Roles stratified: *main* must all
  be filled or the scene does not start, *supporting* favourable, *extras* optional; if no NPC can take
  a main role the search radius expands until one is found.
- **Smart Locations** — Game AI Pro 3 ch.35 (FINAL FANTASY XV), verbatim: *"a single smart location may
  refer to two chairs and a table… capture relationships between them, such as furniture grouping… they
  essentially govern the usage of the objects they refer to."* Role allocation is randomized greedy
  Monte-Carlo, explicitly allowed to fail.
- **Tutenel et al.** — classes contain *features*; **off-limits** features may overlap nothing,
  **clearance** features may overlap only other clearance features. **Merrell et al. 2011** (already
  implemented as `solvers/metropolis.rs`) supplies numbers: 36″ bedside, 30″ in front of a seat, 24″ in
  front of shelving, 36″ around a dining table.
- **Smelik, Tutenel, de Kraker & Bidarra (2010)** — **cite as aspiration, not prior art.** Locking,
  Scoping and Grouping are named in §5.3, but as *"possible facilities, which are inspired from image
  processing software"*, in a paper whose own words are that integrating procedural generation with
  manual editing is *"so far as good as unaddressed."*

---

## Architecture

```
crates/
  emerge-core/     engine-free. schema, validation, solvers, RON surgery, GLB measurement.
                  deps: serde, ron, rand, rand_chacha. NO bevy.
  emerge-bevy/     runtime plugin: load → spawn → tags, relationships, smart-object query.
  emerge-mapper/   the standalone application (bin).
src/              the game; depends on emerge-core + emerge-bevy.
```

`sim_harness.rs` is the precedent for two apps over one plugin graph — same graph, device omitted,
*"not a second code path."*

Moves unedited into `emerge-core`: `rng`, `wfc`, `geom`, `placement/{ir, solver, solvers/{wfc,
constraint}, scatter, manifest}`. Only real break: `solvers/metropolis.rs` reads
`dungeon::WALL_THICKNESS` for one constant → parameter.

**Unify the two RON surgeons** (`bake::scan_ron_leaves` and `site_editor::source_map`) into one module;
`source_map` becomes a typed wrapper. **Promote GLB measurement** from `tests/common/mod.rs::Glb` +
`tests/ozea_asset.rs::accessor_bounds` into the library, still hand-rolled — the reason for having no
`gltf` dep survives the move.

### The descriptor — a patch, and *not* where interactions live

```ron
(
  id: "ozea/mess_table",
  mesh: "ozea/mess_table.glb",
  align:  ( scale: 1.0, stretch_y: None, y_offset: 0.0, pivot: (0.0,0.0), front: Some(90.0) ),
  extent: ( footprint: (1.6, 0.8), height: 0.75 ),

  // THE LAYERING AXIS — replaces Role, rests_on, and the is_floor_marking height heuristic.
  mount: OnSurface(( class: "worktop" )),
  //  OnFloor | OnWall(( height: 1.8 )) | OnCeiling | InOpening(( clear: (0.9,2.0) ))
  //  | Tiled | Overlay(( on: Wall ))        ← decals. Currently inexpressible.

  // Tutenel's feature types. `Predicate::Clearance(f32)` exists in the IR; the DESCRIPTOR has no way
  // to state it, so nothing forbids a chair flush to a wall with its seat socket inside the wall.
  clearance: [ ( dir: Front, dist: 0.76 ) ],

  offers: ( surfaces: ["support","worktop"],
            sockets: [ ( id: "seat_n", role: "diner", at: (0.0,0.75,-0.5), yaw: 0.0 ) ] ),

  kind:    ["furniture","table"],     // typed axes, each with its own validated vocabulary
  effects: ["uses-electricity"],
  look:    ["brown","metal"],
)
```

Every field optional; absence inherits — the patch shape BSN would want.

**Identity.** `SiteKit`'s parse-time completeness proof is replaced by a **`required: [id, …]`** list in
the game's descriptor set, validated at load. Same guarantee, no enum, works for any game.

**Drop the dead surface rather than inherit it:** `category`, `Role::Custom`, `Host::Floor`,
`Host::Opening`'s unimplemented path, and the four unread affordance tokens.

**Interactions are NOT on the descriptor.** A table plus four chairs is *one* affordance, not five.
FFXV puts this on an invisible **location** referring to multiple concrete objects and governing their
use; Smart Zones add the role strata:

```ron
// map-level
locations: [
  ( id: "galley_table_1",
    props: ["mess_table@3", "stool@7", "stool@8", "stool@9"],
    interactions: [
      ( verb: "eat",
        roles: [ ( name: "diner",  kind: Main,       min: 1, max: 4, socket_role: "diner" ),
                 ( name: "server", kind: Supporting, min: 0, max: 1 ) ],
        guard: None,
        effects: [ Restore(( drive: "stamina", rate: 0.2 )) ] ),
    ] ),
]
```

Sockets carry a `role` so allocation has a hook. A single-prop interaction is the degenerate case —
nothing is lost, and the expensive retrofit is avoided. `ir::Guard(String)` (declared, unused) is the
precondition slot.

### `emerge-bevy`

Attaches what does not survive spawn today: interned `Tags`, `SmartObject`, and **`MountedOn` /
`PlacedIn` as Bevy relationships** rather than bare components. The repo already runs three relationship
pairs (`HeldAt`/`SiteSpecimens`, `MemberOf`/`SquadRoster`, `Holding`/`HeldBy`), so this is idiomatic,
and the reverse index ("what is on this worktop?") is engine-maintained instead of hand-rolled inside
an index resource.

Mind the documented gotchas: Bevy expresses an empty relationship target by *removing* the component
(always `Option<&T>`), and target order is attach order, never a total order.

### `emerge-mapper`

`source_map`, `edit`, `thumbs`, `pick`, `overlay`, `ghost`, `panel` port across rather than being
rewritten. Adopt from 0.19: **`InfiniteGridPlugin`** (importer ground plane), **Feathers number input**
(nudging `align`), **`SettingsPlugin`** (editor layout prefs), **`save_using_saver`** (writing baked
thumbnails).

> **Do NOT adopt `TransformGizmoPlugin`.** Measured here 2026-08-03: its overlay camera blanks an HDR
> main camera's output (frame went 13,343 → 183 distinct colours, median luminance 57 → 0) and its
> second `Camera3d` silently killed nine `Single<.., With<Camera3d>>` systems. Removed on `main`.
> Ground-drag plus `[`/`]` covers the need; revisit if upstream fixes the clear-config bug.

**The importer closes the four unvalidated fields** the audit lists. It measures the GLB and proposes
footprint/height from the AABB, `pivot` from the bbox centre, `y_offset` from the base, and **`front` by
the documented method** (XZ centroid of the upper 45% of vertices vs the whole mesh) — written down in
`kit.rs` and implemented nowhere. It flags the 100× centimetre case and reports origin alignment in
CATALOG.md's existing vocabulary (`centered` / `base-at-origin (Z)` / `corner-at-origin` / `offset`).
A socket with no reachable clearance is a fault it can flag.

---

## Stages

| | Deliverable | Gate |
|---|---|---|
| **0a** | Carry-over docs; local Bevy reference; CLAUDE.md section. No code. | a fresh session can execute 0b from the repo alone |
| **0b** | Workspace split; move the engine-free 2,400 LOC. No new features. | goldens unchanged, 31/31 |
| **1** | Descriptor (patch-shaped, with `clearance`) + map + **locations**; validation; unified RON surgery; converter from both schemas | converted descriptors reproduce today's semantics exactly |
| **2** | `emerge-bevy` loads/spawns; **dungeon furniture migrates first** (41 items, already string-keyed) | `snapshot_hash` unmoved |
| **3** | Site migrates; `required:` replaces the enum guarantee | `check_prop_placements` reports the same faults |
| **4** | `emerge-mapper` reaches F7 parity; delete `src/site_editor/` | the 19 writer tests port and pass |
| **5** | Mesh importer: measure, derive `front`, preview, emit | re-derives the shipped kit's 45 measurements |
| **6a** | Smart objects, single-actor: index, query API, a `UseSmartObject` behaviour in `ai::utility` | one interaction drives an agent |
| **6b** | **Multi-actor: role allocation + orchestration** | **four agents fill a four-seat table with no deadlock and no double-booking** |

The engine bump is already done — the repo is on `bevy 0.19.0`. The resources-as-components lookup
indirection is the first thing to suspect if a perf number moves. Stages 5 and 6 may swap with 4.
**6b is the real cost**; both game-AI chapters report the query API is the easy half and
allocation/orchestration the hard one.

## Verification

- **Goldens are the spine.** Stages 0–3 must not move `snapshot_hash`; run it under load, not idle.
- **Byte-pin the migration** — a diff, not a judgement.
- **The writer contract ports intact**: no-op save byte-identical; a move changes exactly one line;
  undo restores bytes including comments.
- **Importer against ground truth**: re-derive `kit_ozea.ron`'s heights/footprints and
  `chair`/`command_chair`'s `front: Some(90.0)` from the GLBs.
- **Render and look, every stage.** Three F7 bugs were invisible to 31 green suites and found only in a
  rendered frame — a blanked framebuffer, a leaked full-screen node, a duplicate-component panic.

Bevy traps are in `CLAUDE.md` § "Bevy 0.19 — read the vendored source, not the web".

## Decided, 2026-08-03

These were the plan's open questions. All four are now answered; they are recorded here rather than
deleted, because the reasoning is what a future reader will want.

1. **Naming** — `emerge-mapper`, with `emerge-core` and `emerge-bevy` under it. See the header.

2. **Map comments: promote them to fields.** The surgical writer exists because `site67.ron` is 15%
   comments, but for a *new* format that is solving the wrong problem — the answer is to stop having
   comments the serializer can lose. Every addressable thing in a map carries an optional `note:`, so
   an editor-owned map round-trips through an ordinary serializer with its prose intact.

   The schema already had one of these before the question was asked: `Placed::owned_because` is a
   reason stored as *data* precisely so nothing can strip it. `note:` is that generalised.

   The split is per file class, and it is one path each: **hand-authored files** (`site67.ron`,
   `config.ron`) keep text surgery, because their prose is a 48-line ASCII floor plan and paragraphs
   introducing blocks of records — none of it attached to a record, so none of it has a field to live
   in. **Emerge maps** are serialized normally and never text-spliced.

3. **`stretch_y` is a project patch layer.** The descriptor states the mesh's real height; a
   project-level patch layers the game's policy on top:

   ```ron
   // ozea/wall.descriptor.ron — the art
   extent: ( height: 2.0 )
   // project/fvs.patch.ron — this game's 2.4 m walls
   "ozea/wall": ( align: ( stretch_y: Some(2.4) ) )
   ```

   This is the patch model doing the job it exists for, and it is what keeps one game's wall height out
   of a library meant for several.

4. **An author owns *cells*, not pieces.** The kitbash brush paints the grid cells a piece covers, and
   an owned cell becomes a unary constraint. The solver needs no new concept for this:
   `wfc::collapse_grid` already takes a per-cell `initial` domain bitmask, and
   `initial_domains_restrict_output` already asserts that a pinned cell survives collapse. It is also
   exactly Alvarez et al.'s lock brush, where locked tiles subdivide the room into mutable and
   immutable zones. The cost — moving an owned piece means repainting — is accepted.
