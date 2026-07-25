# SCP-1048-B ("the infant-arm copy") — game asset hand-off

The second of SCP-1048's copies: outwardly the most bear-like of the family — pale, smooth,
glass-ball eyes and a stitched nose intact — **except for a human infant's arm protruding from a
torn seam in its torso**. Canon-sized (**0.33 m**) on the **same 8-bone rig** as the original.

> **Tonal note for the integrator:** this copy is **hostile**, but it still ships `dance` — the
> uncanny read is deliberate: it behaves like the benign original right up until it doesn't.
> `draw_picture` was dropped at sign-off. Its own attack is `tantrum`.

Shares the original's rig, in-place guarantee, root-bone motion contract and loading pattern —
documented once in [`../scp1048/README.md`](../scp1048/README.md) and not repeated here. Every clip
is prefixed `scp1048b_*`, so all four bears can be loaded **simultaneously** without animation-name
collisions.

---

## 0. Artist-guide conformance (`docs/artist_guide.md` §3 / §14)

Read out of the shipped `.glb`, not assumed.

| Guide rule | Status |
|---|---|
| glTF 2.0 `.glb`, self-contained, embedded textures | ✅ 1.1 MB, 2 embedded images (PNG diffuse + JPEG normal) |
| Y-up, metres, no axis conversion | ✅ 0.330 m tall, Y-up |
| Scene 0 is the asset | ✅ 1 scene, default `scene 0` |
| **Normal-map tangents exported** (Bevy won't regenerate) | ✅ `TANGENT` on all 4 primitives |
| Animations as separate glTF indices, 24 fps | ✅ 6 indices, 24 fps |
| No malformed stray geometry | ✅ clean bbox, no strays |
| In-place clips (no root motion) | ✅ no clip translates the armature node |
| Triangle budget | ✅ 4,700 tris — in line with the original |

**Measured for the §14 checklist** (glTF frame, metres):

| | |
|---|---|
| `footprint` (width, depth) | **(0.237, 0.130)** |
| `height` | **0.330** |
| `pivot` (XZ offset of bbox centre from origin) | **(−0.030, 0.008)** — see the note below |

### The pivot is offset, and that is the arm

The bbox spans **−0.149 … +0.088** on X: the infant arm protrudes ~60 mm further to one side than
the body does to the other. So the geometric centre sits **30 mm off the origin**, even though the
*bear* is centred.

**Keep the pivot at `(0,0)` anyway.** The origin is under the bear's own mass, which is what you
want for locomotion, footstep placement and turn-in-place. If you drive selection boxes or spatial
queries off the raw bbox centre, this asset will read as if it stands 30 mm to one side — use the
body, not the bbox, for anything the player aims at.

---

## 1. The contract (verified against the export)

| | |
|---|---|
| File | `assets/scp1048b/scp-1048-b.glb` — **1.1 MB**, self-contained |
| Generator | Khronos glTF Blender I/O v5.1.20 (glTF 2.0 binary) |
| Scene | **1 scene, default `scene 0`** |
| Nodes | `scp1048b_rig` (armature root) → { `scp1048b_mesh` (skinned), `torso` (skeleton root) } |
| Mesh | **1 mesh · 4,700 triangles · 4 primitives** (body / eyes / nose / infant arm) |
| Vertex attrs | `POSITION`, `NORMAL`, `TANGENT`, `TEXCOORD_0`, `JOINTS_0`, `WEIGHTS_0` |
| Skeleton | **1 skin · 8 joints**, single root `torso` — identical layout to the original |
| Textures | **2 embedded images** — baked PNG diffuse + a tiled JPEG **normal** map (Poly Haven `leather_white`, CC0 — fine pale skin); `extensionsUsed: [KHR_texture_transform]` |
| Units | **metres, Y-up** · base rests on **`y = 0`** |
| Animations | **6** (see §4) |
| Materials | **4** (see §5) |

---

## 2. Size and scale

| Axis | Extent | Range |
|---|---|---|
| **X (width)** | 0.237 m | −0.149 … +0.088 (the arm accounts for the −X reach) |
| **Y (height)** | 0.330 m | 0 … 0.330 (base planted at 0) |
| **Z (depth)** | 0.130 m | −0.058 … +0.073 |

Authored at canon size and unscaled — spawn at `RENDER_SCALE = 1.0`.

**Facing:** faces **+Z**, game forward is local **−Z** — **spawn with a 180° yaw**
(`Quat::from_rotation_y(PI)`).

---

## 3. Loading (Bevy 0.19)

Identical to the original — unscaled gameplay root, scaled model child carrying the yaw:

```rust
const SCP1048B_GLB: &str = "scp1048b/scp-1048-b.glb";
const RENDER_SCALE: f32 = 1.0;   // canon ~0.33 m
```

See [`../scp1048/README.md` §3](../scp1048/README.md) for the full spawn function.

---

## 4. The six baked clips

Skeletal (bone T/R/S on the 8 joints), **24 fps**, in-place.

**Inherited from the original** (`draw_picture` deliberately absent):

| Name | glTF idx | Frames @24 fps | Loop | What it is |
|---|---|---|---|---|
| `scp1048b_rest_idle` | 0 | 72 (3.0 s) | **yes** | feet-planted side-to-side sway — the default |
| `scp1048b_dance` | 1 | 48 (2.0 s) | **yes** | the canon "dances while observed" wiggle — see the tonal note up top |
| `scp1048b_jump_in_place` | 2 | 40 (1.67 s) | one-shot | hops (root apex ≈ 0.09 m × scale) |
| `scp1048b_sit_down` | 3 | 49 (2.04 s) | one-shot | stands → folds down onto its bottom; **ends held in the seated pose** |

**Its own hostile set:**

| Name | glTF idx | Frames @24 fps | Loop | What it is |
|---|---|---|---|---|
| `scp1048b_tantrum` | 4 | 30 (1.25 s) | **yes** | the attack: an infant's fit — chin-tucked 10° forward lean, arms flailing in opposite phase, head shaking side to side, whole body bouncing 8 mm at the flail peaks (**two flail cycles per loop**) |
| `scp1048b_rage` | 5 | 36 (1.5 s) | **yes** | threat display: hunched forward loom, arms flailing, two forward lunge-snaps per loop, ears whipping — the hostile idle |

Both hostile clips **loop**, so this copy has no one-shot attack: drive it as a state, not an
event. Natural sequence: `rest_idle`/`dance` → (target acquired) → `rage` loop → `tantrum` loop
while attacking.

`tantrum`'s bounce rides the **root bone**, not the armature node, so it still cannot move your
`Transform` — the hop is inside the skin like `jump_in_place`'s.

**Ground contact, measured through this export** (lowest mesh vertex over each clip):

| clip | lowest point |
|---|---|
| `sit_down` | −0.00 mm |
| `jump_in_place` | −0.01 mm |
| `tantrum` | +0.85 mm |
| `rage` | +0.63 mm |
| `rest_idle` | −3.12 mm |
| `dance` | −6.59 mm |

`dance`'s −6.6 mm (a foot corner clipping at the wiggle's extreme) and `rest_idle`'s −3.1 mm are
shared with the original and are not visible at gameplay camera distance.

---

## 5. Materials

Four PBR `StandardMaterial`s:

| Slot | Base colour | Roughness | Covers |
|---|---|---|---|
| `scp1048b_body` | baked diffuse + **pale-skin normal** | 0.60 | body, with the torn seam baked into the diffuse around the arm's exit point |
| `scp1048b_eye_glass` | `[0.015, 0.015, 0.02]` | **0.05** | glossy black glass-ball eyes |
| `scp1048b_nose_thread` | `[0.04, 0.025, 0.018]` | 0.75 | cross-stitch (X) thread nose |
| `scp1048b_infant_arm_mat` | `[0.46, 0.31, 0.26]` | 0.55 | the protruding infant arm |

The dark **tear** around the arm is painted into the baked diffuse at the same measured anchor the
geometry exits from, so texture and geometry cannot drift apart. The normal map is tiled via
**`KHR_texture_transform`**. Leave the materials as authored.

---

## 6. Wiring it into the game

Code-spawned creature, **no `config.ron` furniture row** — mirror the `scp999` module layout
(`src/scp999/`, plugins registered in `lib.rs`). Nothing in `src/` loads this asset yet; it is
staged for a future `scp1048` module that would most naturally own all four variants behind one
marker + a variant enum, since they share a rig and a clip vocabulary.

Keep cosmetic-only systems out of the hashed sim set — the model is a **child** of the gameplay
entity, per the artist guide's §12 determinism contract.

---

## 7. Regenerating the asset

```bash
blender --background --python SCP_Characters/examples/build_scp1048_teddy.py -- scp1048b
```

**Verify a regenerated export:** `skins == 1` (8 joints), 6 animations named
`scp1048b_{rest_idle,dance,jump_in_place,sit_down,tantrum,rage}`, 4 materials, 2 embedded images,
base at `y = 0`, and no frame of `sit_down` driving the mesh below `y = 0`.
