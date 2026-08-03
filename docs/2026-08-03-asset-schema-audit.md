# Asset schema, tags and reuse boundary — an audit

**Why this exists.** `docs/2026-08-03-forge-plan.md` proposes a standalone world-building application.
Its design rests on findings that took three deep passes over the tree to establish and that are
expensive to re-derive. This is those findings, with locations, so the plan can be executed — or
argued with — without repeating the search.

Everything below was read from the tree at `85d8984`. Dead ends are recorded as such; a "no" here is a
result, not a gap.

---

## 1. The reuse boundary already exists

**~2,400 LOC imports no `bevy` and carries no game semantics.** It moves to a crate unedited:

| Module | Outward deps |
|---|---|
| `src/rng.rs` | `rand`, `rand_chacha` |
| `src/wfc.rs` | `crate::rng` only. Header states "no Bevy dependency" |
| `src/geom.rs` | `crate::rng::DetRng`. Header: "Engine-free (no Bevy)" |
| `src/placement/ir.rs` | `std::sync::Arc`, `serde` |
| `src/placement/solver.rs` | `rand_chacha`, `super::ir` |
| `src/placement/solvers/{wfc,constraint}.rs` | as above + `crate::wfc` |
| `src/placement/scatter.rs` | Header: "kept engine-free (no `bevy::`)" |
| `src/placement/manifest.rs` | `serde`, `super::ir::Role` |

`ir.rs:5`'s claim *"Nothing here imports `bevy::`"* is **true** — the only `bevy` string in the file is
that sentence. Its `crate::wfc` reference is a doc link, not a `use`.

**Single-symbol couplings** (each a one-line fix):

- `placement/solvers/metropolis.rs:30` → `crate::dungeon::WALL_THICKNESS`, used once as
  `const WALL_INSET`. Make it a parameter.
- `pathfind.rs:14` → `bevy::math::IVec2`, one line, no other coupling.
- `orca.rs:19` → `use bevy::prelude::*` but the only type used is `Vec2` (47 occurrences; no
  `Component`, `Resource`, `Query` or `Plugin`).

**The declared Bevy boundary** of the placement stack is `placement/furnish.rs` — `ir.rs:7` names it.
`placement/mod.rs` binds to `session::RunState` / `RunBuild`, so it is game-side.

**Mixed, needs splitting:** `util.rs` (generic grid/sort/hash helpers plus `nearest_planar`);
`dungeon/{config,layout,rooms,render}.rs` are bevy-free in their own text but inherit `mod.rs`'s
`use bevy::prelude::*` glob.

**Two independent RON surgeons solve the same problem** and should become one:

- `bake.rs:24-40` — `Leaf { path, span, text }`, `scan_ron_leaves`, `splice_block`, `find_block_value`,
  `scalar_eq` (compares by parsed `ron::Value`, so `0x5C09191` == `96506257` and the hex survives).
- `site_editor/source_map.rs` — line-oriented: `replace_field`, `trailing_comment`, `save_atomic`,
  `comment_split` (quote-aware).

**Process constraints any second app inherits:** one `App` per process (`sim_harness.rs:33`
`HARNESS_LOCK`); the logger is process-owned, not App-owned (`bin/train.rs:333-337`);
`ComputeTaskPool`/rayon globals must be set before `DefaultPlugins`; `config.ron` is read cwd-relative
via `std::fs`, so cwd is a contract.

**Precedent for two apps over one plugin graph:** `sim_harness.rs` keeps the *same* graph and omits the
device (`WindowPlugin { primary_window: None }`, `RenderPlugin { backends: None }`, `WinitPlugin`
disabled) — *"not a second code path"* (`sim_harness.rs:253`).

---

## 2. Tags do not reach gameplay

**This is the load-bearing finding.** The vocabulary is a *placement-solver* language, not a gameplay
language.

Of eight shipped affordance tokens, **one** has runtime consequence:

| Token | Consumer | Gameplay? |
|---|---|---|
| `emit` | `furnish.rs:491` → `light::LightEmitter` → `LightField` → crab/manca photophobia, SCP-150 steering, mold recoil, mushroom phototropism | **yes — the only one** |
| `screen` | → `light::ScreenEmitter` | cosmetic, "windowed-only, determinism-neutral" |
| `sit`, `back_to_wall` | `furnish.rs:882,922` — placement constraints | no |
| `sleep`, `store`, `hygiene` | **never read** outside `#[cfg(test)]` | no |
| `decor` | **never read anywhere in `src/`** | no |
| `door`, `pass` | `constraint.rs:92` — and unreachable: no shipped row has `Anchor{Opening}` | no |

**A placed prop keeps only `PlacedIn(RegionId)`** (`placement/mod.rs:90`) — a bare newtype. No key, no
role, no tag survives `furnish.rs:469-498`. Every downstream consumer treats furniture as anonymous
geometry: `squad_ai/perception.rs:125` folds it into an `examinable_pos` list alongside corpses;
`actions.rs:42` picks the nearest; `parasite.rs:757` uses positions for harborage; `mycelia` coats
anything that is not a light.

**The Site is the same or worse.** `site/activities.rs` is keyed entirely on `AreaId` and never touches
a prop. `knowledge/`, `research/`, `containment/` contain zero references to affordances, surfaces or
`SitePiece`. Exactly one Site prop has meaning and it is hardcoded by enum variant:

```rust
// src/site/visuals.rs:762
if p.piece == SitePiece::Slab { /* spawn StudySlab */ }
```

**Where semantics *do* reach gameplay: region tags, not prop tags.** `mycelia::damp_weight(&region
.props.tags)` drives mold susceptibility; `fruit::species_weight` drives mushroom room-affinity. Both
have closed vocabularies cross-validated against `config.ron:dungeon.room_types`.

**Consequence:** "wire tags into gameplay" is not a wiring job. It is a new system.

### Dead surface — drop it in any migration, do not inherit it

`category` (no reader, `#[allow(dead_code)]`), `Role::Custom` (never constructed), `Host::Floor` (zero
references), `Host::Opening` (implemented in the pure layer, **no spawn branch** in `furnish_region`),
and the four unread affordance tokens above.

### The one closed vocabulary

`furnish.rs:136-143` — `SURFACE_CLASSES = [("support", 1<<0), ("worktop", 1<<1)]`, called "THE single
source of truth". Both validators enforce a two-sided contract (a class nobody offers is an error).
`affordances`, `tags`, `group` are **unvalidated free text**.

⚠️ `docs/artist_guide.md:306` is stale — it still lists `support`/`worktop` as *affordances* and omits
`surfaces` and `y_offset`. `assets/config/furniture_kenney.ron:15,18` still authors
`affordances: ["support"]`, which is dead config post-split.

---

## 3. Layering is the missing primitive

**"Decals go on walls" cannot be expressed today, in either schema.**

`PropPlacement { piece, pos: (f32,f32), yaw, waive }` (`layout.rs:200`) has **no Y, no host, no
normal**. Everything vertical is derived:

1. **Floor decals are inferred from mesh height**, not declared:
   ```rust
   // layout.rs:844
   is_floor_marking = kit.rests_on(piece).is_none()
                   && height * y_scale <= FLOOR_MARKING_HEIGHT   // 0.15
   ```
   ⚠️ This has already produced a bug: a bare height threshold silently reclassified the 0.109 m mug,
   0.04 m data folder and 0.107 m books as floor markings, exempting them from the overlap rule. The
   `rests_on.is_none()` term is the 2026-08-02 patch.

2. **On-top-of** is `resting_on` (`layout.rs:784`): nearest prop in the same area rect offering the
   required surface class within `RESTS_ON_REACH = 2.5 m`, tie-broken by `(d, x, z)`. No host in reach
   is a **hard fault**, never a silent y = 0.

3. **Wall-mounted:** exactly one thing, and it is derived in code, never authored — `DoorPlaque`,
   spawned from `Doorway` records at `visuals.rs:685-705`. `WallPlacement` puts a piece on a *cell*
   with a yaw and carries no face or normal: *"it is furniture on a cell, not a face on an edge"*
   (`visuals.rs:707`).

The dungeon has the vocabulary the Site lacks — `ir.rs:214` `Host { Ceiling, Floor, Wall, Opening }`
and `Role::Anchor { host }` — but only `Wall` is implemented (`furnish.rs:550-593`, `sconce_row`), with
**one shipped item** (`wall_light`). `Ceiling` has a spawn branch and no user. `Opening` and `Floor`
have neither.

---

## 4. The two schemas, and why neither is a superset

| Concept | `ManifestItem` (`manifest.rs:20`) | `KitPiece` (`kit.rs:65`) |
|---|---|---|
| identity | `key: String`, open catalogue | struct field per `SitePiece`, **closed enum**, `deny_unknown_fields` |
| offers a surface | `surfaces: Vec<String>` | `surfaces: Vec<String>` — *identical field, vocabulary and bit semantics* |
| requires a surface | `role: Scatter { surface }` | `rests_on: Option<String>` — **same relation, two spellings** |
| XZ pivot | `pivot: (f32,f32)` | absent (every Ozea mesh re-origined) |
| uniform scale | absent (global `FURNITURE_SCALE`) | `scale: f32` |
| vertical stretch | absent | derived `y_scale = target_height / height` |
| facing | **absent** | `front: Option<f32>` |
| dispatch | `role: Role` | absent — placement is authored per record |
| service semantics | `affordances` | **absent entirely** |
| door | absent | `DoorPiece::opening: (f32,f32)` |

**The deepest divergence is identity.** `SiteKit`'s value is that a kit missing a structural piece
fails at **parse** time (`kit.rs:14-20`: "the failure mode of a missing structural piece is an invisible
wall"). An open string-keyed catalogue cannot do that. Any unification must replace the guarantee, not
drop it — the plan proposes a `required: [id, …]` list validated at load.

Bevy 0.19's `SceneComponent` is **not** a stronger replacement: `SceneComponentInfo` carries
`spawned_from_scene: bool` behind `#[cfg_attr(debug_assertions, component(on_add))]` — a debug-build
runtime check, peer to a load-time list.

---

## 5. Mesh import — the pipeline, and four unvalidated fields

**Everything is run by hand.** No build step, no `build.rs`, no CI asset job. Invocations are recorded
as copy-paste shell blocks in `assets/ozea/README.md` and `assets/site_dressing/README.md`.

| Script | Role |
|---|---|
| `mesh_origin.py` | The shared origin convention — **XZ-centred, base at origin**. `reorigin_to_base` clears parent with `CLEAR_KEEP_TRANSFORM` first (glTF import parents under a Y-up empty; without this the acid barrels export lying down), and keys a `seen` set on `id(obj.data)` because packs share mesh datablocks. `reorigin_group_to_base` uses one combined AABB — per-object would shear a two-part asset apart. |
| `fbx_to_glb.py` | Bulk converter. `transform_apply(scale=True)` is the **100× fix**: the FBX importer represents cm authoring as node `scale: 0.01` over 100× vertex data — valid glTF, correct render, but anything reading the AABB sees centimetres. **Writes `INVENTORY.md`** (W/H/D per mesh) — the only manifest generator in the project. |
| `blend_to_glb.py` | Per-object `.blend` splitter. Scale deliberately untouched: *"A convenience `--scale` would be the first step toward a kit nothing agrees on the size of."* |
| `ozea_wall_heights.py` | Re-*authors* heights instead of scaling: translates vertices above a profiled cut so only the featureless mid-section lengthens. |
| `glb_desaturate.py`, `glb_recompress_texture.py`, `import_retro_tvs.py` | Container-level material/texture surgery. `glb_recompress_texture.py:87` `fingerprint()` refuses to write if the node/mesh/skin/animation/accessor contract changed. |

**Measurement is offline or in tests. No Rust code measures a GLB at runtime.** The reader is
hand-rolled — `tests/common/mod.rs:25` `Glb::load` walks the chunks with `serde_json`, deliberately
avoiding a `gltf` crate: *"pulling a glTF crate in as a dev-dependency to read a header would be a
second, differently-behaved reader of an asset the engine already parses its own way."* The
measurement primitive is `tests/ozea_asset.rs:36` `accessor_bounds` (min/max over `POSITION` accessors,
in the file's own units).

### The four fields nothing validates

1. **`KitPiece::footprint` vs the mesh** — `tests/prop_footprint_contract.rs` covers `ManifestItem`
   only.
2. **`KitPiece::scale`** — the doc says *"must be derived from a measurement, not dialled by eye"*, and
   nothing checks it.
3. **`KitPiece::front`** — measured **once, by hand**. The method is written down (XZ centroid of the
   upper 45% of vertices against the whole mesh's) and **implemented nowhere**. Only two rows carry it.
4. **`DoorPiece::opening`** — measured by hand from POSITION accessors, only bounds-checked against
   `mesh.height`.

These are exactly what an importer should compute, which is why the plan calls the importer the part
the literature does *not* cover.

### What *is* validated

`ozea_asset.rs:179` globs `assets/ozea/*.glb` and asserts base-at-y=0 and XZ-centred within **5 mm**,
with `checked >= 18` as a floor so a broken glob fails. `:248` pins the wall family to 2.40 m *against
the asset*. `pieces.rs:269` and `manifest.rs:300` assert every referenced GLB exists on disk.

⚠️ `assets/site_dressing/` exists as a separate directory **because** `Mug.glb` measures `cx = −0.026`
and would fail the 5 mm glob.

---

## 6. Save/load conventions — three patterns, chosen by who owns the file

Mechanics are identical everywhere: `to_string_pretty` → tmp → `rename` (atomic). Policy differs:

| File | Policy |
|---|---|
| `persist.rs` (campaign) | **Refuse, never migrate.** `SAVE_VERSION` mismatch is an error. **No field carries `#[serde(default)]`** — several did, each justified by a comment describing an unreachable branch. Pinned by `no_saved_field_may_default_away_a_missing_one`. |
| `settings.rs` (user prefs) | The opposite, deliberately: **every field `#[serde(default)]`**, plus a `Damaged` latch that disables writes for the session so a malformed file is never overwritten with defaults. |
| `bake.rs` (config.ron) | **Splice, never re-serialize** — `config.ron` is ~563 comment lines; "the reasoning IS the file". |
| `site_editor/source_map.rs` (level data) | **Surgical line rewrite.** `site67.ron` is 1401 lines of which 217 are comments; the props list has *more prose than data*. A no-op save is byte-identical. |

**The `SaveGuard` lesson** (`persist.rs:560`): `load_campaign` refused a bad save loudly, then the next
`OnEnter(AppState::Site)` ran `save_campaign`, captured the empty world, and atomically renamed a
zero-specimen save over it. **Never write a file you refused to read.**

---

## 7. Prior art in-repo for a second app

- `src/bin/train.rs` (2,171 LOC) consumes the library as an external crate, never builds an `App`
  itself (calls `sim_harness::build_headless_app`), and owns the process-level logger.
- `src/bake.rs` is the precedent for **moving tool logic out of a binary into the library** so it is
  testable: *"a tool doing both [changing the sim and moving the ruler] in one step cannot be
  reviewed."*
- `src/site_editor/` (8 files) and `src/research_room/` are the dev-tool idiom: `#[cfg(debug_assertions)]`
  module, env var → marker resource declared **unconditionally** in `lib.rs`, every system
  `.run_if(resource_exists::<Marker>)` and on `Update`, never registered in `sim_harness`.

There is **no existing document** about crate splitting, workspaces or reuse. This is the first.
