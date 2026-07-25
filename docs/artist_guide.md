# Asset Builder's Guide — `foundation_vs_slop`

A guide for 3D CAD artists authoring or swapping assets for this game. Everything
here was read off the actual codebase — file paths, units, conventions, and the
hard rules a new model must obey to drop in without code changes.

> **Engine:** Bevy 0.19 (Rust). All assets load via Bevy's `AssetServer` from
> `assets/` (relative to the project root). Runtime format is **glTF 2.0 / `.glb`**
> only. No FBX, no USD, no proprietary formats at runtime.

---

## 1. World scale and units

The world is a tile grid. **1 tile = 1 metre = 1 Bevy world unit.**

| Constant | Value | Where |
|---|---|---|
| `TILE_SIZE` | 1.0 m | `src/dungeon.rs:19` |
| `WALL_HEIGHT` | 2.4 m (~8 ft ceiling) | `src/dungeon.rs:33` |
| `WALL_THICKNESS` | 0.14 m (slab flush to tile edge) | `src/dungeon.rs:26` |
| `DOORWAY_HEIGHT` | 2.0 m (lintel sits under header) | `src/dungeon.rs:36` |

A squad member renders ~1.82 m tall (the VALKYRIE rig at 1.61 m native ×
`FIGURINE_SCALE = 1.13`, `src/squad.rs:150`). Furniture ships at native 1:1
(`FURNITURE_SCALE = 1.0`, `src/placement/furnish.rs:40`). **Author all furniture,
props, and characters in real-world metres; do not pre-scale.** Doors should be
~2.05 m tall, shelves ~2.15 m, to clear under the 2.4 m ceiling.

**Coordinate system:** Y-up, right-handed (glTF default). The game's forward is
local −Z; models authored facing +Z need a 180° yaw at spawn (the VALKYRIE rig
does — see `src/squad.rs:526`).

---

## 2. Where assets live

```
assets/
├── characters/valkyrie.glb            # squad body (skinned, ~82k tris, 5 clips)
├── dimensional_crab/dimensional_crab.glb   # crab enemy (skinned, 3 clips)
├── scp150/scp-150.glb                 # SCP-150 parasite (skinned, 12 clips)
├── death_cap/death_cap_growth.glb     # reference mushroom (6 morph targets)
├── mushrooms/<species>/<Name>_Growth.glb   # 16 species, same contract
├── low_poly_flashlight/low_poly_flashlight.glb  # Researcher's held flashlight
├── low_poly_furniture/glb/…           # furniture kit (Beds, Lights, Tables…)
├── meat_chunks/meatpack.glb          # gib chunks
├── textures/                          # backrooms wallpaper + carpet + blood
├── shaders/                           # 17 .wgsl files (procedural materials)
└── config/config.ron                  # the master data file (see §7)
```

A parallel curated library lives at `/mnt/codex_fs/game_assets/` — see
`/mnt/codex_fs/game_assets/CATALOG.md`. Source-of-truth Blender files for the
characters are under `SCP_Characters/`, for the procedural mushrooms under
`mushrooms_procedural/_lib/`.

---

## 3. File-format and export rules

### Hard requirements (a model that violates any of these fails loudly at startup)

1. **glTF 2.0 binary (`.glb`)** — single self-contained file. Embedded textures OK
   and recommended (the VALKYRIE `.glb` embeds JPEG textures to stay 3.8 MB).
2. **Y-up, metres.** No axis conversion is applied at load.
3. **Scene 0 is the asset.** All spawn sites call
   `GltfAssetLabel::Scene(0).from_asset(path)`. Put the visible rig in scene 0.
4. **JPEG-enabled textures are fine** (Bevy is built with the `jpeg` feature
   specifically for VALKYRIE's embedded JPEGs — `Cargo.toml:28`).
5. **Watertight geometry if the model can be gibbed.** Squad units are
   plane-sliced at death (`src/autogib.rs`); Sutherland–Hodgman clipping caps
   each cut, but unclosed caps are silently dropped (never a panic), so an
   open mesh produces gibs with holes. Welded, closed meshes look right.
6. **No malformed stray geometry.** A 10-metre shelf FBX forced the kit swap to
   `Shelf D` (see `assets/config/config.ron:157`). Clean your bbox before export.

### Strongly preferred

- **Normal-map tangents exported** (Bevy does not regenerate them).
- **PBR base-colour factors as flat materials** (no textures) for pieces the
  game recolors at runtime — see §6. Textured pieces keep their textures.
- **Animations as separate glTF animation indices** (one clip per index), 24 fps,
  in-place (no root motion). The game drives `Transform` itself; clips only
  supply limb motion.

---

## 4. Character assets (skinned rigs)

### Squad — `characters/valkyrie.glb`

The integration contract is documented in
`/mnt/codex_fs/game_assets/SCP_Characters/gltf/valkyrie_bevy_integration.md`.
Key facts the code relies on:

| Property | Value | Code ref |
|---|---|---|
| Rig | MPFB2 `game_engine` skeleton, 60 bones, root bone `Root`, max 4 joints/vertex | `valkyrie_bevy_integration.md:122` |
| Native height | 1.61 m, feet at origin (×1.13 render scale → 1.82 m in game) | `valkyrie_bevy_integration.md:23` |
| **Facing** | the rig faces glTF **+Z**, so the character's own **right is −X** (`hand_r`, `foot_r`, `thigh_r` all sit at negative X). The figurine child carries a 180° Y rotation to point that at the unit's −Z | `src/squad.rs` (`spawn_unit`) |
| Clip count | 20 | `tests/valkyrie_asset.rs` |
| Rifle | rigid mesh parented to bone **`spine_03`**, node `Name` contains `"rifle"` | `src/squad.rs`, `src/autogib.rs:863` |
| Chest rig accent | node `Name` contains **`"chestrig"`** | `src/squad.rs` (`recolor_units`) |
| Hair | `AlphaMode::Blend`, double-sided, 2 embedded textures | `valkyrie_bevy_integration.md:141` |

**Why those `Name` substrings matter:** the game walks the spawned scene by `Name`
to (a) tag the rifle as `GunModel` so `autogib` bakes it as a separate intact
chunk (`tag_valkyrie_rifle`, `src/autogib.rs:850`), and (b) recolor only the
chest-rig mesh with the per-member outfit colour, leaving the body/gear/hair
authored materials untouched (`recolor_units`, `src/squad.rs`). If you
rebuild VALKYRIE or build a sibling character, **keep those node names** or the
death-fling and outfit-coloring systems silently no-op.

#### The locomotion blend space

Units do not play one clip at a time. Ten clips are always resident and are mixed
continuously by weight, over a **single shared gait phase** — see `src/anim/` and
`src/squad.rs` (the engineering guide is `docs/animation.md`). Consequences for
anyone re-authoring these clips:

| glTF index | Name | Slot | Duration | Phase offset | Cycle distance | Authored speed |
|---|---|---|---|---|---|---|
| 0 | `valkyrie_idle` | idle | 3.333 s | — (not a gait clip) | — | — |
| 1 | `valkyrie_idle_alert` | idle, aiming | 2.917 s | — | — | — |
| 5 | `valkyrie_walk` | forward, walk tier | 1.417 s | 0.000 (the reference) | 1.388 u | 0.98 u/s |
| 11 | `valkyrie_run` | forward, run tier | 0.750 s | −0.016 | 2.135 u | 2.85 u/s |
| 8 | `valkyrie_walk_back` | backward, walk tier | 1.458 s | −0.141 | 1.538 u | 1.05 u/s |
| 12 | `valkyrie_run_back` | backward, run tier | 0.583 s | −0.062 | 1.185 u | 2.03 u/s |
| 13 | `valkyrie_strafe_l` | **rightward** (see below) | 0.708 s | −0.031 | 1.937 u | 2.74 u/s |
| 14 | `valkyrie_strafe_r` | **leftward** (see below) | 0.583 s | +0.047 | 1.259 u | 2.16 u/s |
| 3 | `valkyrie_aim` | upper-body layer | 3.042 s | — | — | — |
| 4 | `valkyrie_fire` | upper-body layer, one-shot | 1.167 s | — | — | — |

Every other clip (`reload`, `crouch_walk`, `aim_walk`, `walk_start`/`walk_stop`
and their backward twins, `jump_fwd`/`jump_back`, `death`) is authored but **not
wired**: no mechanic drives it, and a wired clip nothing can trigger is a stub.

The rules these numbers impose:

1. **Clips must be in-place.** Every one has bit-exactly zero `Root` translation
   today, and it must stay that way — `unit_movement` already drives the
   character's transform, so baked root motion would move it twice. Enforced by
   `tests/valkyrie_asset.rs`.
2. **Cycle distance sets the cadence.** The game measures each clip's authored
   ground speed from the *planted foot's* travel relative to the static `Root`,
   then plays the blend at whatever rate keeps the feet planted at the unit's real
   speed. Re-author a clip with a different stride and the number in the table
   must be re-measured, or the feet will slide.
3. **One gait cycle per clip, and keep the phases aligned.** All six gait clips are
   reparameterised onto one normalised phase, so left-foot-down must happen at the
   same fraction of every clip. They currently agree to within 0.14 of a cycle
   (walk and run to within 0.016), and the table's phase offsets absorb the rest.
   A re-export shifts those offsets; they are re-measured by cross-correlating
   foot height, not guessed.
4. **The upper body must be separable.** `aim` and `fire` are masked so they drive
   only `spine_01` and above; the legs keep walking underneath. The masked-out
   bones are `Root`, `pelvis`, `thigh_l/r`, `calf_l/r`, `foot_l/r`, `ball_l/r`,
   `skirt_l/r`, `thigh_holster`, `ammo_pouch`. Renaming any of them shrinks the
   mask — `tests/valkyrie_asset.rs` fails if one goes missing.

> **The strafe clips are named backwards.** Measured from the planted foot,
> `valkyrie_strafe_l` (13) carries the body toward **−X**, which for a +Z-facing
> rig is the character's own **right**; `valkyrie_strafe_r` (14) goes left. The
> code wires them by measured direction and ignores the names. Please swap the
> names in the source asset when these are next touched.

#### Animations we still need

Ranked by how visible the gap is in play:

1. **`valkyrie_sprint`** — units move at up to 6.0 u/s but the run is authored for
   2.85 u/s, so at top speed the run clip is pinned at its 2× playback clamp and
   the feet start to slide. A clip authored around 5–6 u/s would give the blend
   space a real third tier.
2. **`valkyrie_turn_in_place_l` / `_r`** — a stationary unit turning to face a
   threat pivots on locked feet. (Needs the turn rate lowered on the code side to
   be worth playing; flagged, not yet changed.)
3. **`valkyrie_run_start` / `valkyrie_run_stop`** — `walk_start`/`walk_stop` exist
   with no run equivalent, and stopping from a sprint is the most visible pop.
4. **Additive aim poses** (`aim_up` / `aim_down` at max pitch, as deltas from the
   base aim pose) — the rifle cannot pitch at all right now.
5. **Walk-tempo strafes** — both strafes are authored at run tempo (2.2–2.7 u/s),
   so a unit sidestepping slowly runs them below their playback clamp.
6. **Consistency pass on the strafes** — `strafe_l` is 0.708 s / 1.937 u per cycle
   and `strafe_r` is 0.583 s / 1.259 u, so sidestepping left and right read
   noticeably differently. Diagonals (`strafe_fwd_l` etc.) would also soften the
   four-way angular blend, though four-way is workable.

### Crab — `dimensional_crab/dimensional_crab.glb`

| Property | Value | Code ref |
|---|---|---|
| Clips (glTF index) | 0 attack · 1 idle · 2 walk | `src/crab/setup.rs` |
| Native body length | ~3.06 m (Blender units) | `src/crab/mod.rs:65` |
| Render scale | 0.15 (→ ~0.46 m, ~1.5 ft tall) | `src/crab/mod.rs:66` |
| Playback rates | walk ×7, attack ×4 (clips authored very long) | `src/crab/mod.rs` |
| Clip switching | all three clips stay resident and cross-fade by weight — none is ever rewound, so a crab flickering between states no longer restarts its scuttle | `src/anim/`, `src/crab/movement.rs` |
| Facing | model's forward axis is the standard −Z after surface-nav seating; rotate at spawn if yours differs | `src/crab/mod.rs` |

Crabs crawl on **walls as floors**. The asset is seated on a surface (floor or
wall) by a surface-normal frame — any pose that looks right standing on the
floor will also look right pinned to a wall. Do not author a "wall mode" pose.

### SCP-150 parasite — `scp150/scp-150.glb`

| Property | Value | Code ref |
|---|---|---|
| Clips (12, glTF index) | **alphabetical in the export**: 0 Attack1 · 1 Attack2 · 2 BurrowOut · 3 Climb · 4 Forage1 · 5 Forage2 · 6 Idle_Alert · 7 Idle_Snug · 8 Leap · 9 Run · 10 Walk1 · 11 Walk2 — pinned by `tests/creature_clip_contract.rs`; a re-export that reorders them fails that gate | `src/parasite.rs` |
| Clip switching | wired clips cross-fade by weight and are never rewound; `BurrowOut` is the one **one-shot**, restarted from frame 0 on each eruption | `src/anim/`, `src/parasite.rs` |
| Native body length | ~3.6 m | `src/parasite.rs:54` |
| Render scale | 0.07 (→ ~0.25 m juvenile) | `src/parasite.rs:56` |
| Authored facing | **−X** (head/mouth at −X); spawn rotates −90° about +Y to align to engine −Z | `src/parasite.rs:69` |

The parasite embeds inside hosts and later erupts (`BurrowOut` clip, slowed to
×0.8 for drama). The burst leaves an open wound that needs its own small skinned
mesh; the code spawns the same `scp-150.glb` scene scaled down for the wound.

### SCP-1048 "Builder Bear" family — `scp1048/`, `scp1048a/`, `scp1048b/`, `scp1048c/`

Four bears from one parameterised recipe: the benign original plus the three hostile
copies it builds during play. They share an **8-bone rig** (root `torso`), a canon
**0.33 m** height, base at `y = 0`, and a clip vocabulary — but **not a clip order**.
Full per-asset contracts live in each `assets/scp1048*/README.md`; this is what the
code relies on.

| Property | Value | Code ref |
|---|---|---|
| Render scale | **1.0** (authored at canon size — the README sanctions 1.3–1.6 if it reads too small at RTS zoom; it is one constant) | `src/scp1048/mod.rs` (`RENDER_SCALE`) |
| Authored facing | **+Z** (muzzle/eyes/bow tie); the model child carries a 180° Y rotation to point that at the engine's −Z | `src/scp1048/mod.rs` (`spawn_scp1048_at`) |
| Root motion | **none** — no clip translates the armature node; vertical travel is keyed on the root *bone* inside the skin, so `Transform` is entirely the game's | all four READMEs §0 |
| Clip names | variant-prefixed (`scp1048_*`, `scp1048a_*`, …) so all four load **simultaneously** without animation-name collisions | `tests/creature_clip_contract.rs` |
| Clips — original (5) | 0 rest_idle · 1 dance · 2 jump_in_place · 3 draw_picture · 4 sit_down | `src/scp1048/anim.rs` |
| Clips — A, ears (5) | 0 rest_idle · 1 jump_in_place · 2 sit_down · 3 **scream** · 4 rage | `src/scp1048/anim.rs` |
| Clips — B, infant arm (6) | 0 rest_idle · 1 dance · 2 jump_in_place · 3 sit_down · 4 **tantrum** · 5 rage | `src/scp1048/anim.rs` |
| Clips — C, scrap + gun (8) | 0 rest_idle · 1 dance *(unwired)* · 2 jump_in_place · 3 sit_down · 4 aim_gun · 5 fire_gun · 6 pistol_whip · 7 rage | `src/scp1048/anim.rs` |
| Triangles | 4.6k / **11.7k (A)** / 4.7k / 5.2k | asset READMEs §1 |

Three things a re-author must not quietly "tidy":

1. **B's `tantrum` loops.** It is the only attack clip in the game that is not a
   one-shot — it is authored as a sustained fit and is driven as a *state* held for
   as long as the bear is attacking. Making it a one-shot would strobe it.
2. **C's `fire_gun` starts and ends in the aim pose** (measured 0.000 mm seam), so
   shots replay with no cross-fade and the bear simply holds its aim between them.
   `aim_gun` is played once and holds its last frame; do not re-author either to
   return to neutral.
3. **C's `dance` (index 1) is deliberately unwired** — it is legacy motion inherited
   from the benign original and reads wrong on a violent copy. Its index is still
   pinned by the contract test, because dropping it would shift all four hostile
   clips below it.

**SCP-1048-A is the first LOD candidate in the project**: 11.7k triangles (2.6× the
others) and **four UV sets**, three of which are dead weight in the vertex buffer and
are most of why the file is 2 MB. Budget an A as if you had spawned two or three
bears. It also **embeds CC-BY geometry** (`assets/scp1048a/ATTRIBUTION.md`) — that
notice is a licence condition and must travel with any shipped build; it is carried in
the repo-root `CREDITS.md`.

### Smiley boss — **no model file**

The Smiley is a **procedural Shadertoy face** on a camera-facing billboard quad
(`assets/shaders/smiley.wgsl`, ported from Martijn Steinrucken's *Smiley
Tutorial*). The "true form" unleashed on a lightning strike is a second
billboard quad driven by `assets/shaders/attack_sphere.wgsl`. The collider is a
material-less `Capsule3d` (radius 0.18, length 0.9 — deliberately smaller than
the 1.6 m visible face so most bolts miss). **Do not author a Smiley mesh** —
any change to its look is a shader edit, not a model swap. See `src/enemy.rs`.

---

## 5. Furniture and props — the data-driven placement grammar

Furniture is the cleanest example of the asset-swap architecture: **author one
RON manifest, change zero code.** The schema is in
`src/placement/manifest.rs:18` (`ManifestItem`); the shipped catalogue is the
`placement.furniture.items` list in `assets/config/config.ron` (starts at
line 89).

### Per-item fields

| Field | Type | Meaning |
|---|---|---|
| `key` | string | Opaque manifest key (referenced by rules; never interpreted) |
| `glb` | string | Path under `assets/` to the GLB |
| `category` | string | Opaque grouping token (currently unused by code) |
| `tags` | `[string]` | Opaque room-type tokens matched by the furnish pass (e.g. `["bedroom"]`) |
| `role` | enum | `Anchor { host: Ceiling\|Wall\|Opening\|Floor }` · `Tiled` · `Freestanding` · `Scatter { surface: "support"\|"worktop" }` |
| `footprint` | `(f32, f32)` | (width, depth) in metres = tiles. **Must match the GLB's actual bbox** — the solver reasons at this size. |
| `affordances` | `[string]` | Opaque tokens: `"sit"`, `"sleep"`, `"support"`, `"worktop"`, `"emit"`, `"screen"`, `"store"`, `"back_to_wall"`, `"hygiene"`, `"decor"`, `"pass"`, `"door"`. Matched, never interpreted. |
| `group` | `Option<string>` | Items sharing a group are drawn together by a soft `Near` band (e.g. toilet + sink both `group: "bath"`). |
| `height` | `f32` | Top of the GLB bbox in metres. **For `support`-affording pieces, this is the surface a Scatter prop rests on.** A wrong value floats or sinks the prop. |
| `pivot` | `(f32, f32)` | Local XZ offset of the bbox centre from the glTF origin. The spawn shifts the model by `−(yaw · pivot)` so the bbox centre — not the authored origin — lands on the placement point. **Defaults to `(0, 0)`**; an off-centre mesh (e.g. toilet, drawer, fridge) needs this or it pokes through walls. |

### How to add a new furniture piece

1. Export the GLB (rules in §3).
2. **Measure the bbox** in metres: width, depth, height, and the XZ offset of
   the bbox centre from the glTF origin.
3. Append a row to `placement.furniture.items` in `assets/config/config.ron`
   with the measured `footprint`, `height`, `pivot`, and the right `role` +
   `affordances`. Example (from the shipped file, `config.ron:138`):

   ```ron
   ( key: "bed_double", glb: "low_poly_furniture/glb/Beds/Bed Double.glb",
     category: "bed", tags: ["bedroom"], role: Freestanding,
     footprint: (1.72, 2.22), affordances: ["sleep", "support"],
     height: 0.714, pivot: (0.0, 0.278) ),
   ```

4. Run `cargo check`. There is no separate asset-import step — the next launch
   loads it. The placement solver (Metropolis MCMC for `Freestanding`, WFC for
   `Tiled`, direct attach for `Anchor`, inner-lattice scatter for `Scatter`)
   consumes it automatically.

### Role guide (which to pick)

| Role | Use for | How the solver places it |
|---|---|---|
| `Anchor(host: Ceiling)` | Ceiling fixtures | Deterministic, at room centre, at `WALL_HEIGHT` (2.4 m). *None currently in the kit — the "Ceiling Light" GLB is a table lamp; reclassified to `Scatter`* |
| `Anchor(host: Wall)` | Sconces, wall-hung props | At wall-adjacent floor cells, seated `WALL_LIGHT_HEIGHT = 1.8 m` up. Tags `CutawayMounted` so it hides when Q/E rotation makes its wall a near knee wall. |
| `Anchor(host: Opening)` | Doors | Spanning the doorway width; sits under the lintel. |
| `Tiled` | Small floor props (bins, clutter) | WFC-scattered, biased to wall-adjacent cells; ≤31 Tiled items total (the WFC `u32` prototype mask). |
| `Freestanding` | Beds, sofas, tables, fridges, plumbing | Metropolis MCMC: backs to walls, non-overlapping, sofa faces TV, group partners huddle. |
| `Scatter(surface: "support")` | Props resting on any `support`-affording surface | Inner 9×9 lattice of the support's top; seats on the surface's `height`. |
| `Scatter(surface: "worktop")` | Desk lamps (only on desks/tables, never beds/dressers) | Same as above, but gated to supports that also afford `"worktop"`. |

### Affordances the code already looks for

- `"emit"` → the piece gets a real `PointLight` + feeds the gameplay
  `LightField` (`src/light.rs`).
- `"screen"` → the piece is a TV; its cool-chromatic sub-mesh gets the animated
  CRT-static material (`assets/shaders/tv_static.wgsl`) and an eerie cool-cyan
  flickering spotlight instead of a generic fixture light. **A TV GLB must
  contain a sub-mesh whose `StandardMaterial::base_color` is cool and chromatic
  (green + blue > 3 × red + 0.05 in linear RGB) for the screen detector to
  find it** (`src/light.rs:1047`). The chassis must be neutral grey.
- `"support"` → a Scatter prop can rest on this piece's top.
- `"worktop"` → a desk lamp (Scatter surface `"worktop"`) can rest on this
  piece. A desk or table affords both `"support"` and `"worktop"`; a bed or
  dresser affords only `"support"`.
- `"back_to_wall"` → HARD `AgainstWall` constraint with angular wall-snap: the
  piece's back seats flush to the perimeter (fridge, toilet, sink, bath,
  chest of drawers).
- `"hygiene"` → bathroom fixture tag.
- `"door"` / `"pass"` → doorway anchor.

### The asset-swap proof

`assets/config/furniture_kenney.ron` is a 20-line file that swaps the **entire**
furniture kit to the Kenney prototype kit. The acceptance test loads it
directly. **A new kit is one RON file, zero code.** Mirror this pattern when
porting to a new furniture library.

---

## 6. Materials and recoloring

The game uses Bevy's `StandardMaterial` (PBR) for all loaded GLBs plus a set of
custom `Material` shaders for procedural surfaces. Know which is which:

### Standard PBR materials (authored in the GLB)

- **Body / gear / hair / rifle** on VALKYRIE: **left untouched**. The
  integration doc lists the palette as flat base-colour factors (matte, metallic
  0): black bodysuit/boots/eyewear, olive chest rig/backpack/ammo pouch,
  near-black belt+hooks/gloves/kneepads/respirator, skin-tone body, brown hair.
- **Chest rig** on VALKYRIE: **overwritten at runtime** with a flat outfit-
  coloured `StandardMaterial` (per-member colour from `crate::palette::OUTFITS`).
  If you rebuild the rig, keep the chest-rig mesh a flat olive PBR factor (no
  texture) so the recolor matches its authored style. The detector keys on the
  node `Name` containing `"chestrig"` (`src/squad.rs:675`).
- **Furniture**: shipped as-is. A sconce's emissive factor is overridden to
  `fixture_emissive = 2.2` to read as "light is on" (`src/light.rs`).
- **Floor tiles**: textured with `assets/textures/backrooms-carpet-diffuse.png`
  (CC0, from `amini-allight/backrooms-textures`).
- **Wall slabs**: textured with `assets/textures/backrooms-wall-diffuse.png`
  (same source). Walls are then **swapped at runtime** to the custom
  `MoldWallMaterial` (`assets/shaders/mycelia_wall.wgsl`) where the mold has
  colonised them — the original wallpaper texture and roughness are preserved
  as the `StandardMaterial` base of the `ExtendedMaterial`
  (`src/mycelia/material.rs:35`, `src/mycelia/mod.rs:1290`).

### Custom Material shaders (no GLB — procedural)

| Shader | What it renders | Source |
|---|---|---|
| `smiley.wgsl` | The Smiley boss face (camera-facing billboard) | `src/enemy.rs:339` |
| `attack_sphere.wgsl` | The Smiley's "true form" fractal sphere | `src/enemy.rs` |
| `tv_static.wgsl` | Animated CRT static on a TV's screen sub-mesh | `src/light.rs:691` |
| `blood_spray.wgsl` | Muzzle-flash blood mist billboard | `src/gore.rs:157` |
| `blood_pool.wgsl` | Flat floor blood decal (dries over `dry_time`) | `src/gore.rs:195` |
| `impact_fx.wgsl` | Laser-impact additive particle burst | `src/impact_fx.rs:44` |
| `nest.wgsl` | The crab nest portal dome on a wall | `src/nest.rs:57` |
| `mycelia_floor.wgsl` | The mold floor coating (lit PBR `ExtendedMaterial`) | `src/mycelia/material.rs:33` |
| `mycelia_wall.wgsl` | The mold wall coating (climbs up to `climb_height = 0.85 m`) | `src/mycelia/material.rs:35` |
| `mycelia_fruit.wgsl` | The mushroom fruit body (lit PBR, uses `COLOR_0` part mask) | `src/mycelia/material.rs:37` |
| `mycelia_sim.wgsl` | GPU compute: Physarum agents + Gray-Scott RD | `src/mycelia/` |
| `mycelia_blend.wgsl` | Frame-rate independent tick interpolation | `src/mycelia/` |
| `almond_water.wgsl` | The Almond Water puddle overlay (thin-film iridescence) | `src/almond_water/visual.rs` |
| `vhs.wgsl` | Full-screen VHS post-process (chroma, scanlines, noise, bloom) | `src/vhs.rs` |
| `health_bar.wgsl` | Floating health bar | `src/health.rs` |
| `noise.wgsl` | Shared hash21/vnoise/fbm chain | (utility) |

---

## 7. The master config — `assets/config/config.ron`

**Everything tunable about the game is data.** A missing or malformed file is a
loud startup panic (one path, no fallback). The file is 1508 lines and covers:
dungeon generation, the furniture manifest + Metropolis weights, gore, hair,
impact FX, AI tuning, behaviour tuning, sim (combat/swarm/boss/parasite), VHS,
mycelia (the GPU mold), dialogue, lighting, almond water, the CPU gameplay
mold, and audio.

**For an artist, the sections that matter are:**

- `dungeon` (line 20) — room types, sizes, liminality dial, corner-notching.
  Tag names here (`"bathroom"`, `"bedroom"`, `"office"`, `"kitchen"`,
  `"living"`, `"hall"`) are the **exact strings** furniture `tags` and mushroom
  `room_affinity` must match.
- `placement` (line 88) — the furniture manifest + Metropolis layout weights +
  density knobs (`tiled_per_room`, `freestanding_per_room`, `scatter_per_room`,
  `wall_lights_per_room`).
- `lighting` (line 1396) — fixture lumens, colour, range, flicker. TV screens
  get a cool-cyan cast (`SCREEN_COLOR`).
- `mycelia.species` (line 848) — the mushroom species table (see §8).

Edit and relaunch — there is no in-game panel.

---

## 8. Mushrooms — the procedural growth contract

Mushrooms are the most demanding asset in the game. Each species grows from a
sealed egg to a mature fruit body by blending **six glTF 2.0 morph targets**
driven by a single `growth: f32` in `[0, 1]`. The contract is fixed; every
species in `assets/config/config.ron:848` follows it.

### Required GLB structure

| Property | Value |
|---|---|
| File | `<Species>_Growth.glb` (high LOD) and `<Species>_Growth_low.glb` (faceted low LOD, same topology) |
| Node/mesh name | `<Species>` (high) / `<Species>_low` (low) |
| Basis (all morph weights 0) | the youngest stage (sealed egg / primordium) |
| Morph targets, in order | `grow_012`, `grow_028`, `grow_045`, `grow_062`, `grow_080`, `grow_100` |
| Sample points `STAGE_T` | `[0.0, 0.12, 0.28, 0.45, 0.62, 0.8, 1.0]` (index 0 is the basis) |
| Vertex attributes | `POSITION`, `NORMAL`, `TEXCOORD_0`, **`COLOR_0`** |
| Units | metres, Y-up |
| Animation clips | **none** — you drive the weights from `growth` |
| Morph deltas | **sparse accessors** (only vertices that move) |
| Triangles | 144–2400 (low) / up to ~2400 (high) |

### `COLOR_0` is a part mask, not artwork

- `R` = cap (pileus)
- `G` = flesh (stipe, gills, annulus)
- `B` = volva

Bevy's `StandardMaterial` would multiply base colour by vertex colour and tint
the cap pure red, so the `MoldFruitMaterial` shader overwrites `base_color`
outright and reads the mask itself (`src/mycelia/material.rs:175`). **There are
no textures on a mushroom asset.** The parts *are* the mask; the per-species
flat colours come from the RON (`cap_young`, `cap_old`, `stipe`, `volva`,
`substrate` in linear RGB).

### Per-species geometry the RON must carry

The `geom:` block in each species row is **measured offline** by
`mushrooms_procedural/_lib/inspect_glb.py` and pasted into the RON. A CI job
re-runs the sidecar and diffs against the RON, so a regenerated asset that
drifts **fails loudly**. Do not hand-edit these numbers. The fields:

```
stage_max_disp: [f32; 6]   // longest vertex chord per morph segment (speed limit basis)
stage_height_m: [f32; 7]   // apex height at each baked stage
egg_height_m: f32          // sealed spawn-state height
cap_radius_m: f32          // adult cap radius
volva_radius_m: f32         // adult base radius (sibling-spacing basis)
radius_profile: [f32; 16]  // widest radius in each of 16 equal height slices
bend_lo_m, bend_hi_m: f32  // stipe bending zone
```

### Adding a new species

1. Build the growth GLB (high + low) under
   `mushrooms_procedural/<Species>/Procedural Growth/` using the shared
   framework. The generator is pure Python (no Blender needed for the verify
   step; `python3 _lib/build.py "<Species>" asset high` runs the Blender build).
2. Copy the GLB into the game at `assets/mushrooms/<species_folder>/`.
3. Run `python3 _lib/inspect_glb.py <path>` to get the `geom:` block.
4. Append a row to `mycelia.species` in `assets/config/config.ron` with the
   measured `geom`, the species' `colors`, `light` behaviour
   (`Photophobic` / `Phototropic` / `Photophilic`), `toxicity`, `nutrition`,
   and `room_affinity` (tags must match the dungeon `room_types` list).
5. `cargo check` then launch. The species is now a first-class fruit body the
   mold can pin; crabs can graze it; the speed-limit invariant in
   `src/mycelia/perceptual.rs` is provable in a unit test because the geometry
   is data.

### The nine growth archetypes

| Archetype | Species |
|---|---|
| `veiled_egg` (egg → torn volva) | Death Cap, Fly Agaric, Destroying Angel |
| `gilled_ringed` | Champignon, Ink Caps |
| `gilled_plain` | Amethyst Deceiver, Blue Pinkgill, Rosy Bonnet |
| `bolete` | King Bolete |
| `funnel` | Chanterelle |
| `bracket` (stemless shelf, wall-mounted) | Turkey Tail, Chicken of the Woods, Oyster |
| `globe` | Puffball |
| `cluster` | Enoki |
| `morel` | Morel |

The `bracket` archetypes grow on **walls** (the `WALL_MOUNT_HEIGHT` path in
`src/mycelia/fruit.rs:736`); the rest grow on the floor.

---

## 9. Gore, gibs, and death

### Autogib (character fracture)

Squad units are **plane-sliced at death** from their own bind-pose mesh
(`src/autogib.rs`). The pipeline:

1. Pre-fracture the merged character mesh once (recursive random plane cuts,
   Sutherland–Hodgman clipping, watertight cap per cut, planar cross-section
   UV). Cached by source asset id.
2. At death, swap the fragments in. Each fragment is flung as a physics body
   (Avian).
3. The rifle subtree (anything under a node named `rifle`) is **pruned** from
   the body soup and baked as a separate intact chunk that flies off on death.
4. Caps get a "meat" material (`assets/meat_chunks/`); skin keeps the outfit
   tint.

**What this means for a character artist:**
- Author a **watertight, closed** body mesh (no open edges). Open caps are
  silently dropped — gibs will have holes.
- Keep the **rifle as a named node** (`Name` contains `"rifle"`) parented to
  `spine_03` inside the body scene, not a separate held-item GLB.
- The fracture reads **bind-pose** geometry, not the death pose. The visual gap
  is hidden by the speed of the gib burst — do not author a death pose.
- Sub-meshes with missing normals/UVs are synthesized, but author them right
  for best results.

### Meat chunks — `assets/meat_chunks/meatpack.glb`

Used for the generic "any death" meat chunks and the autogib cap material
textures. The FBX + 3 PNGs (raw meat, intestine, textured meat) live in the
same folder.

### Blood

Procedural — no GLB. `blood_spray.wgsl` is a muzzle-flash mist billboard;
`blood_pool.wgsl` is a flat floor decal that dries to matte maroon over
`dry_time = 22.0 s`. Pool size scales with the dead thing's mass
(`src/gore.rs::pool_scale`). Wall splats use the same decal projected onto
walls.

---

## 10. Lighting (diegetic fixtures)

- **Ambient:** warm fluorescent fill, `ambient_brightness = 200` lux, colour
  `(1.0, 0.98, 0.9)`.
- **Directional key:** `key_illuminance = 2000` lux (weak steep fill).
- **Fixtures:** each piece `affordances: ["emit"]` gets one real
  `PointLight` (Bevy clustered) at `fixture_intensity = 120,000` lumens,
  `fixture_range = 7.0 m`, cool-white colour `(0.92, 1.0, 0.94)` with a faint
  green cast (low-CRI halophosphate). Emissive mesh glow at
  `fixture_emissive = 2.2` (LDR).
- **Flicker:** ~1 in 8 tubes is a "failing" Backrooms fluorescent
  (`flicker_fail_ratio = 0.12`); the rest shimmer at
  `flicker_hum_depth = 0.06`.
- **TV screens:** `SCREEN_COLOR = (0.40, 0.78, 0.92)` cool cyan, 90,000
  lumens, 6.5 m range, wide cone (0.75 rad half-angle), faster irregular
  flicker (11 Hz).

A wall light's origin sits at `WALL_LIGHT_HEIGHT = 1.8 m` up the wall; the
sconce mesh should be authored to sit at that height when its back is flush to
the wall.

---

## 11. Camera and cutaway

Fixed **45° isometric** view (orthographic) looking from (+X, +Z). Q/E rotates
the map in 90° detents. Walls that face the camera (E/S edges — normal `−X` or
`−Z`) squash to `CAMERA_WALL_FRACTION = 0.25` of `WALL_HEIGHT` (a 0.6 m knee
wall you can always see over). Wall-mounted props tagged `CutawayMounted`
hide (scale → 0) when their host wall becomes a near knee wall, so they never
float in the cutaway gap.

**For an artist:** nothing to do at export — the cutaway is a runtime
transform. Just author wall sconces and wall-hung props to sit flush against a
wall at the right height; the game handles the rest.

The view's vertical FOV is `screen_fov_deg_v = 30°`, calibrated for a 27"
panel at ~60 cm. The mushroom growth speed limit
(`src/mycelia/perceptual.rs`) is computed against this so motion stays under
the 1.2 arcmin/s human detection threshold.

---

## 12. Determinism contract (why some things are code, not data)

The game has a bit-identical deterministic core
(`cargo test` — `SimConfig::deterministic_core()`). Anything that touches
pinned state (positions, health, RNG draws, lethal picks) is on `FixedUpdate`
and must be order-stable. **For an artist, the practical consequences:**

- **Muzzle offset is a `const`, not read from the rifle bone**
  (`MUZZLE_OFFSET`, `src/squad.rs:184`). Re-scaling the figurine must not move
  the muzzle's world position — the deterministic core would silently shift
  targeting and the golden hash would change.
- **The figurine scene is a child of the `Unit`, not the `Unit` itself**
  (`src/squad.rs:523`). The async GLB load churns the cosmetic child's archetype
  at a wall-clock-dependent tick, never the `Unit`'s. Keep the model on the
  child; do not merge it into the sim entity.
- **The `Leader` marker splits the hashed archetype**, so it is windowed-only.
  Do not tag anything in the model scene with gameplay markers.
- **Fruit bodies and gibs carry `Transform` but never `Health`** so they are
  excluded from the replay hash. A mushroom asset must not need a `Health`
  component to grow.

If you change something that could shift the deterministic core, run
`cargo test` and verify the golden hash is unchanged.

---

## 13. Testing your asset

1. **`cargo check`** — fast compile; catches RON schema errors at startup.
2. **`cargo test`** — the deterministic-core layer (RNG/WFC/utility/ORCA/laser).
   Fast, GPU-free. Catches any drift in the replay hash from a model swap.
3. **`cargo test --features test-harness -- --test-threads=1`** — headless
   replay / liveness / SSIM. Boots the real game with no window; **needs a
   GPU**.
4. **Visual check** — `touch screenshot.request` from the project root, wait
   ~1.5 s, then read `screenshot.png` (the game screenshots itself from the
   render pipeline via `src/devshot.rs`).
5. **Region capture** — Ctrl+P in-game, drag a box, release. Saves to
   `debug_screenshots/region_<timestamp>.png` for a deliberate "look here"
   pointer.

See `TESTING.md` for the full strategy, oracle rules, and the determinism
invariant list.

---

## 14. Quick checklist for a new asset

- [ ] Exported as `.glb`, Y-up, metres, scene 0 = the asset.
- [ ] Watertight (if it can be gibbed) — closed mesh, no stray geometry.
- [ ] Animations as separate glTF indices, in-place, 24 fps, looping cleanly.
- [ ] Node `Name`s preserved for code that walks by name (`rifle`, `chestrig`).
- [ ] Bbox measured: width, depth, height, XZ pivot offset.
- [ ] `assets/config/config.ron` row appended with measured `footprint`,
      `height`, `pivot`, and the right `role` + `affordances`.
- [ ] For a mushroom: 6 morph targets (`grow_012 … grow_100`), `COLOR_0`
      part mask (R cap / G flesh / B volva), `geom:` block from
      `inspect_glb.py`, high + low LODs.
- [ ] For a TV: cool-chromatic screen sub-mesh + neutral-grey chassis.
- [ ] `cargo check` + `cargo test` pass; golden hash unchanged.
- [ ] `touch screenshot.request` and read `screenshot.png` — looks right.

---

## 15. Key file references

| What | Where |
|---|---|
| Master config (RON) | `assets/config/config.ron` |
| Furniture manifest schema | `src/placement/manifest.rs:18` |
| Furniture spawn + scale | `src/placement/furnish.rs:40`, `:447` |
| Squad rig + animation | `src/squad.rs:165`, `:283` |
| VALKYRIE integration doc | `/mnt/codex_fs/game_assets/SCP_Characters/gltf/valkyrie_bevy_integration.md` |
| Autogib (death fracture) | `src/autogib.rs` |
| Smiley boss (no model) | `src/enemy.rs`, `assets/shaders/smiley.wgsl` |
| Crab enemy | `src/crab/mod.rs:95` |
| SCP-150 parasite | `src/parasite.rs:46` |
| Mushroom growth contract | `src/mycelia/species.rs`, `src/mycelia/fruit.rs` |
| Mushroom generator README | `/mnt/codex_fs/game_assets/mushrooms_procedural/PROCEDURAL_GROWTH.md` |
| Lighting (fixtures + TV) | `src/light.rs:633`, `:1028` |
| Gore (blood, gibs) | `src/gore.rs` |
| Determinism rules | `CLAUDE.md`, `TESTING.md` |
| Asset library catalog | `/mnt/codex_fs/game_assets/CATALOG.md` |