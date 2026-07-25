# SCP-1048-A ("the ear copy") — game asset hand-off

The first of SCP-1048's self-made copies: a bear **built out of human ears**, assembled from ~64
harvested ear shingles layered over a fleshy body. It has **no face** — no eyes, no stitched nose,
no bow tie; the head is a ball of overlapping ears. Canon-sized (**0.34 m**, same envelope as the
original) on the **same 8-bone rig**, so it shares the original's silhouette and skeleton.

> **Tonal note for the integrator:** this is a **hostile** copy. It inherits three clips from the
> benign original (`rest_idle`, `jump_in_place`, `sit_down`); `dance` and `draw_picture` were
> **dropped at sign-off** as tonally wrong for it. Its own attack is `scream`.

Shares the original's rig, in-place guarantee, root-bone motion contract and loading pattern —
those are documented once in [`../scp1048/README.md`](../scp1048/README.md) and not repeated here.
Every clip is prefixed `scp1048a_*`, so all four bears can be loaded **simultaneously** without
animation-name collisions.

> ⚖️ **This asset embeds CC-BY geometry. [`ATTRIBUTION.md`](ATTRIBUTION.md) ships beside it and must
> travel with any build that includes it.** See §8.

---

## 0. Artist-guide conformance (`docs/artist_guide.md` §3 / §14)

Read out of the shipped `.glb`, not assumed.

| Guide rule | Status |
|---|---|
| glTF 2.0 `.glb`, self-contained, embedded textures | ✅ 2.0 MB, 2 embedded images (PNG diffuse + JPEG normal) |
| Y-up, metres, no axis conversion | ✅ 0.340 m tall, Y-up |
| Scene 0 is the asset | ✅ 1 scene, default `scene 0` |
| **Normal-map tangents exported** (Bevy won't regenerate) | ✅ `TANGENT` on both primitives |
| Animations as separate glTF indices, 24 fps | ✅ 5 indices, 24 fps |
| No malformed stray geometry | ✅ clean bbox, no strays |
| In-place clips (no root motion) | ✅ no clip translates the armature node |
| Triangle budget | ⚠️ **11,680 tris — see the warning below** |

**Measured for the §14 checklist** (glTF frame, metres):

| | |
|---|---|
| `footprint` (width, depth) | **(0.198, 0.137)** |
| `height` | **0.340** |
| `pivot` (XZ offset of bbox centre from origin) | **(−0.004, 0.004)** — effectively centred; `(0,0)` is fine |

### ⚠️ Two things that differ from every other bear

**It is 2.6× the triangle budget.** 11,680 tris against 4,556 for the original and 5,172 for C.
The ear shingles are a joined 120-tri scan instance ×64. It is still small in absolute terms, but if
you ever spawn these in a crowd, this is the one to LOD or instance-limit first — budget as if you
had spawned 2–3 bears.

**It carries four UV sets** (`TEXCOORD_0` … `TEXCOORD_3`), where every other bear carries one. The
joined ear-scan template brought its own UV layers along. Only `TEXCOORD_0` is referenced by the
materials; the other three are dead weight in the vertex buffer and are the main reason the file is
2 MB. Harmless to Bevy — it binds UV0/UV1 — but do not assume UV1 means anything here.

---

## 1. The contract (verified against the export)

| | |
|---|---|
| File | `assets/scp1048a/scp-1048-a.glb` — **2.0 MB**, self-contained |
| Generator | Khronos glTF Blender I/O v5.1.20 (glTF 2.0 binary) |
| Scene | **1 scene, default `scene 0`** |
| Nodes | `scp1048a_rig` (armature root) → { `scp1048a_mesh` (skinned), `torso` (skeleton root) } |
| Mesh | **1 mesh · 11,680 triangles · 2 primitives** (body / ear shingles) |
| Vertex attrs | `POSITION`, `NORMAL`, `TANGENT`, `TEXCOORD_0..3`, `JOINTS_0`, `WEIGHTS_0` |
| Skeleton | **1 skin · 8 joints**, single root `torso` — identical layout to the original |
| Textures | **2 embedded images** — baked PNG diffuse + a tiled JPEG **normal** map (Poly Haven `leather_red_02`, CC0 — wrinkled flesh); `extensionsUsed: [KHR_texture_transform]` |
| Units | **metres, Y-up** · base rests on **`y = 0`** |
| Animations | **5** (see §4) |
| Materials | **2** (see §5) |

---

## 2. Size and scale

| Axis | Extent | Range |
|---|---|---|
| **X (width)** | 0.198 m | −0.103 … +0.095 |
| **Y (height)** | 0.340 m | 0 … 0.340 (base planted at 0) |
| **Z (depth)** | 0.137 m | −0.065 … +0.072 |

Authored at canon size and unscaled — spawn at `RENDER_SCALE = 1.0`. Slightly wider and 10 mm
taller than the original because the ear shingles stand proud of the body surface.

**Facing:** as with every bear in this family, the asset faces **+Z** and the game's forward is
local **−Z** — **spawn with a 180° yaw** (`Quat::from_rotation_y(PI)`).

---

## 3. Loading (Bevy 0.19)

Identical to the original — unscaled gameplay root, scaled model child carrying the yaw. Only the
path and marker change:

```rust
const SCP1048A_GLB: &str = "scp1048a/scp-1048-a.glb";
const RENDER_SCALE: f32 = 1.0;   // canon ~0.34 m
```

See [`../scp1048/README.md` §3](../scp1048/README.md) for the full spawn function.

---

## 4. The five baked clips

Skeletal (bone T/R/S on the 8 joints), **24 fps**, in-place. Resolve by name
(`Gltf::named_animations`) rather than by index if you can — but the indices below are stable and
were verified against this export.

**Inherited from the original** (`dance` and `draw_picture` deliberately absent):

| Name | glTF idx | Frames @24 fps | Loop | What it is |
|---|---|---|---|---|
| `scp1048a_rest_idle` | 0 | 72 (3.0 s) | **yes** | feet-planted side-to-side sway — the default |
| `scp1048a_jump_in_place` | 1 | 40 (1.67 s) | one-shot | hops (root apex ≈ 0.09 m × scale) |
| `scp1048a_sit_down` | 2 | 49 (2.04 s) | one-shot | stands → folds down onto its bottom; **ends held in the seated pose** |

**Its own hostile set:**

| Name | glTF idx | Frames @24 fps | Loop | What it is |
|---|---|---|---|---|
| `scp1048a_scream` | 3 | 32 (1.33 s) | one-shot | the attack: head thrown skyward (−35°), chest arched (−12°), arms flared rigid, held on a **sustained body shiver** (modal springs, 8-frame rise → 18-frame tremor hold), then release to neutral |
| `scp1048a_rage` | 4 | 36 (1.5 s) | **yes** | threat display: hunched forward loom, arms flailing in opposite phase, two forward lunge-snaps per loop, ears whipping — the hostile idle |

Natural sequence: `rest_idle` → (target acquired) → `rage` loop → `scream` on attack → back to
`rage`. `scream` starts and ends at neutral, so it cross-fades cleanly from either idle.

**The tremor is baked in.** `scream`'s shiver is the same damped-spring model as the ear settle,
velocity-kicked from rest so it eases out of the pose rather than snapping. If you also drive a
runtime jiggle, don't stack the two on this clip.

**Ground contact, measured through this export** (lowest mesh vertex over each clip):

| clip | lowest point |
|---|---|
| `sit_down` | −0.00 mm |
| `jump_in_place` | −0.01 mm |
| `scream` | −0.18 mm |
| `rage` | +0.63 mm |
| `rest_idle` | −3.12 mm |

`rest_idle`'s −3 mm is the sway's outer edge kissing the floor and is shared by every bear in the
family; it is not visible at gameplay camera distance.

---

## 5. Materials

Two PBR `StandardMaterial`s. **There is no eye or nose material** — the face is ears.

| Slot | Base colour | Roughness | Covers |
|---|---|---|---|
| `scp1048a_body` | baked diffuse + **flesh normal** | 0.70 | the fleshy under-body |
| `scp1048a_fragment_mat` | `[0.44, 0.22, 0.16]` | 0.60 | the ~64 ear shingles |

The normal map is tiled via **`KHR_texture_transform`**; if Bevy 0.19 does not honour the
extension the wrinkle detail shows at 1:1 (still skin, just coarser). Leave the materials as
authored — creature convention, not runtime-recoloured.

---

## 6. Wiring it into the game

Code-spawned creature, **no `config.ron` furniture row** — mirror the `scp999` module layout
(`src/scp999/`: `mod.rs` + behaviour + visuals, plugins registered in `lib.rs`). Nothing in `src/`
loads this asset yet; it is staged for a future `scp1048` module that would most naturally own all
four variants behind one marker + a variant enum, since they share a rig and a clip vocabulary.

Keep cosmetic-only systems out of the hashed sim set — the model is a **child** of the gameplay
entity, per the artist guide's §12 determinism contract.

---

## 7. Regenerating the asset

```bash
blender --background --python SCP_Characters/examples/build_scp1048_teddy.py -- scp1048a
```

The recipe is parameterised: one pipeline, four `TeddyVariant` records. Passing an unrecognised or
missing variant key now **fails loudly** rather than silently rebuilding the original over the top
of it.

**Verify a regenerated export:** `skins == 1` (8 joints), 5 animations named
`scp1048a_{rest_idle,jump_in_place,sit_down,scream,rage}`, 2 materials, 2 embedded images, base at
`y = 0`, and no frame of `sit_down` driving the mesh below `y = 0`. The build prints its achieved
shingle count (`placed 64/70 attempts`); that shortfall is expected — the count is a placement
*attempt* budget and 64 is the signed-off body.

---

## 8. ⚖️ Attribution (mandatory)

The ear shingles are derived from **"Human Ear Model" by ssavish274** (Sketchfab), licensed
**CC-BY 4.0**. The full notice — source, UID, licence, and the processing applied — is in
[`ATTRIBUTION.md`](ATTRIBUTION.md) beside the `.glb`.

**Any build or distribution that ships `scp-1048-a.glb` must carry that attribution.** The export
step writes the file automatically and fails if it is missing, because once the `.glb` leaves this
repo the provenance is unrecoverable from the geometry. If you bundle assets into a shipped build,
make sure this notice reaches your credits screen or licence file.
