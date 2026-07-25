# SCP-1048 ("Builder Bear") — game asset hand-off

## The family

Four bears ship from one parameterised recipe, sharing an **8-bone rig**, a clip vocabulary and a
canon ~0.33 m size. Clip names are prefixed per variant (`scp1048_*`, `scp1048a_*`, …), so all four
can be loaded **simultaneously** without animation-name collisions. This document is the base
contract — the variant docs cover only what differs.

| Asset | What it is | Clips | Tris | Doc |
|---|---|---|---|---|
| `scp1048` | the benign original | 5 | ~4.6k | this file |
| `scp1048a` | **hostile** — a bear built of human ears, no face | 5 | **11.7k** ⚠️ | [`../scp1048a/README.md`](../scp1048a/README.md) |
| `scp1048b` | **hostile** — an infant arm through a torn seam | 6 | 4.7k | [`../scp1048b/README.md`](../scp1048b/README.md) |
| `scp1048c` | **hostile** — rusted scrap copy with an arm gun | 8 | 5.2k | [`../scp1048c/README.md`](../scp1048c/README.md) |

Only the original ships `draw_picture`; the three copies drop it as tonally wrong. `rest_idle`,
`jump_in_place` and `sit_down` are common to all four. **A embeds CC-BY geometry** and carries a
mandatory `ATTRIBUTION.md`.

Nothing in `src/` loads any of them yet — they are staged for a future `scp1048` module, which
would most naturally own all four behind one marker plus a variant enum.

---

## 0. Artist-guide conformance (`docs/artist_guide.md` §3 / §14)

Checked against the shipped `.glb`, not assumed.

| Guide rule | Status |
|---|---|
| glTF 2.0 `.glb`, self-contained, embedded textures | ✅ 690 KB, 2 embedded images (PNG diffuse + JPEG normal) |
| Y-up, metres, no axis conversion | ✅ 0.330 m tall, Y-up |
| **Scene 0 is the asset** | ✅ 1 scene, default `scene 0` |
| JPEG textures permitted | ✅ fabric normal is JPEG |
| **Normal-map tangents exported** (Bevy won't regenerate) | ✅ `TANGENT` on all 4 primitives — *this was missing and was fixed for this export* |
| Animations as separate glTF indices, 24 fps | ✅ 5 indices, 24 fps |
| No malformed stray geometry | ✅ clean bbox, no strays |
| Watertight (**if gibbed**) | ✅ **0 open edges, 0 non-manifold edges** — see note |
| In-place clips (no root motion) | ✅ **no clip translates the armature node** — see note |

**Measured for the §14 checklist** (glTF frame, metres):

| | |
|---|---|
| `footprint` (width, depth) | **(0.176, 0.130)** |
| `height` (top of bbox) | **0.330** |
| `pivot` (XZ offset of bbox centre from origin) | **(0.000, 0.008)** — essentially centred; `(0,0)` is fine |

### Two notes on how those last two are satisfied

**Geometry is closed, in several shells.** The mesh is 6 sealed islands (body + 2 eyes + 2 nose bars +
bow tie) with **zero boundary edges and zero non-manifold edges**. §9's hazard is *unclosed caps being
silently dropped*; every shell here is closed, so plane-slicing caps each one correctly and `autogib`
is safe if you ever want it. (SCP-1048 is benign and normally isn't gibbed.)

**Vertical motion lives on the root BONE, not the armature node.** `jump_in_place` and `sit_down` need
real vertical travel — the hop must leave the ground (**94 mm** of lift), and the sit must lower the
body **~54 mm** or the folded-leg pose leaves the teddy hanging in mid-air. Keying that on the
armature *object* would be baked root motion and would fight `Transform`. Instead it is keyed on the
root bone (`torso`), **inside** the skin: the exported glTF node for `scp1048_rig` carries **no
translation channel at all**, so a clip can never move the entity's own `Transform`. Verified in the
shipped file.

> **Integrator note:** you own the `Transform` completely — nothing in these clips touches it. Every
> clip also keys the root bone explicitly (even at zero), so switching clips can't inherit a stale
> offset; without that, going from the grounded `sit_down`/`draw_picture` into `rest_idle` sank the
> teddy ~54 mm through the floor.

A small, canon **~33 cm plush teddy bear**: smooth tan fur, a **protruding muzzle** with a
**cross-stitch (X) thread nose**, **glossy black glass-ball eyes**, dark inner ears / paw pads, and
a **black bow tie**. It is a **skinned rig** — an 8-bone skeleton with 5 baked skeletal clips.
Target engine: **Bevy 0.19**.

This model is a **clean-room reproduction**: it was rebuilt as original geometry after studying a
reference plush's construction (metaball-blended body → remeshed, part-based, texture-coloured — the
same *technique* the reference used, none of its mesh data). This doc is the contract + integration
guidance; every number below was read out of the actual exported `.glb`.

---

## 1. The contract (verified against the export)

| | |
|---|---|
| File | `assets/scp1048/scp-1048.glb` — **~1.0 MB**, self-contained (all textures embedded) |
| Generator | Khronos glTF Blender I/O v5.1.20 (glTF 2.0 binary) |
| Scene | **1 scene, default `scene 0`** |
| Nodes | `scp1048_rig` (armature root) → { `scp1048_mesh` (skinned), `torso` (skeleton root) } |
| Mesh | **1 mesh · ~4556 triangles · 4 primitives** (body / bow-tie / eyes / nose) |
| Vertex attrs | `POSITION`, `NORMAL`, `TEXCOORD_0` (UV), `JOINTS_0`, `WEIGHTS_0` — skinned + textured |
| Skeleton | **1 skin · 8 joints**, single root `torso`: `torso, head, ear_l, ear_r, upper_arm_l/r, upper_leg_l/r` |
| Skin weights | max 4 influences/vertex (glTF-normalized on export) |
| Textures | **2 embedded images** — the 1024² sRGB baked diffuse (base colour) + a tiled curly-teddy fabric **normal** map; `extensionsUsed: [KHR_texture_transform]` (the fabric normal's tiling) |
| Units | **metres, Y-up** · base rests on **`y = 0`** · centred on X |
| Animations | **5** (see §4) |
| Materials | **4** (see §6) |

The mesh is **not a single watertight island** (the eyes/nose/bow-tie are separate joined parts,
like the reference plush). That is intentional; this benign entity is not gibbed.

---

## 2. Size and scale

Native bounding box (Y-up, from the `.glb`):

| Axis | Extent | Range |
|---|---|---|
| **X (width)** | 0.176 m | −0.088 … +0.088 |
| **Y (height)** | 0.330 m | 0 … 0.330 (base planted at 0) |
| **Z (depth)** | 0.130 m | −0.058 … +0.073 |

Authored at its **true canon height (~0.33 m)** and unscaled. Spawn with a render scale of **≈ 1.0**
for canon size; scale up (e.g. `1.3`–`1.6`) as a spawn-time `Transform::scale` if it needs to read
larger — do not edit the asset, so a rescale stays one number. Base is at `y = 0`.

**Facing:** the muzzle/eyes/bow tie face **+Z** in the exported glb (Blender −Y front → glTF +Z). The
game's forward is local **−Z**, so **spawn with a 180° yaw** (`Quat::from_rotation_y(PI)`).

---

## 3. Loading (Bevy 0.19)

Mirror the skinned-creature pattern (crab / parasite): an **unscaled gameplay root** with a
**scaled model child** carrying the render scale + spawn yaw. Prelude name is **`WorldAssetRoot`**
(Bevy 0.19 renamed `Scene`→`WorldAsset`); the `GltfAssetLabel::Scene(0)` label is unchanged.

```rust
use bevy::prelude::*;
use std::f32::consts::PI;

const SCP1048_GLB: &str = "scp1048/scp-1048.glb";
const RENDER_SCALE: f32 = 1.0;   // canon ~0.33 m

fn spawn_scp1048(commands: &mut Commands, assets: &AssetServer, pos: Vec3) -> Entity {
    commands
        .spawn((
            Scp1048,                                   // your marker component
            Transform::from_translation(pos),          // root is UNSCALED
            Visibility::Inherited,
        ))
        .with_child((
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(SCP1048_GLB))),
            Transform::from_scale(Vec3::splat(RENDER_SCALE))
                .with_rotation(Quat::from_rotation_y(PI)),   // authored +Z, game forward -Z
        ))
        .id()
}
```

The glTF loader builds the skeleton + `AnimationPlayer`. Every clip is in-place — drive world
movement from your own `Transform`.

---

## 4. The five baked clips

Skeletal (bone T/R/S on the 8 joints), **24 fps**, in-place. Resolve by name
(`Gltf::named_animations`).

| Name | glTF idx | Frames @24 fps | Loop | What it is |
|---|---|---|---|---|
| `scp1048_rest_idle` | 0 | 72 (3.0 s) | **yes** | breathing/sway idle (default) |
| `scp1048_dance` | 1 | 48 (2.0 s) | **yes** | canon "dances while observed" wiggle |
| `scp1048_jump_in_place` | 2 | 40 (1.67 s) | one-shot | hops (root Z apex ≈ 0.09 m ×scale) |
| `scp1048_draw_picture` | 3 | 60 (2.5 s) | **yes** | **seated** arm-scribble gesture (drawing) |
| `scp1048_sit_down` | 4 | 49 (2.04 s) | one-shot | stands → folds down onto its bottom; the plop rings the ears (settle jiggle baked in as the landing follow-through) |

**`sit_down` → `draw_picture` chain seamlessly.** Both are built on one shared seated pose
(`BEAR_SEATED_BASE`: legs folded forward onto the floor, torso upright, head tilted at the work), and
`sit_down`'s **last frame is bit-for-bit `draw_picture`'s first frame** (measured max vertex delta
0.01 mm). So the natural sequence is `rest_idle` → `sit_down` → `draw_picture`, with no cross-fade
needed at the sit→draw boundary. Both clips drive the **root down** (~0.053 m) to ground the fold —
a seated teddy whose root stayed put would hang above the floor.

Build an `AnimationGraph`, add each clip by name, loop the idle, one-shot `jump_in_place` and
`sit_down`. The ear/head jiggle is a damped spring model (ear-flop 3 Hz / head-bob 2.2 Hz, see
`SCP_Characters/src/scp_characters/monsters/{softbody.py,bear_motion.py}`); it is **velocity-kicked
from rest**, so a clip carrying it eases out of the neutral pose instead of snapping to a peak on
frame 1. It is baked into `sit_down`'s landing — if you also drive a runtime jiggle, don't stack the
two on that clip.

---

## 5. Materials

Four PBR `StandardMaterial`s (values read from the `.glb`):

| Slot | Base colour | Roughness | Covers |
|---|---|---|---|
| `scp1048_body` | baked diffuse (base) + **fabric normal** | 0.9 | fur/muzzle/ears/pads/belly colour + curly plush surface |
| `scp1048_eye_glass` | `[0.015, 0.015, 0.02]` | **0.05** | glossy black glass-ball eyes (+ `KHR_materials_specular`) |
| `scp1048_nose_thread` | `[0.04, 0.025, 0.018]` | 0.75 | cross-stitch (X) thread nose |
| `scp1048_bowtie_mat` | `[0.02, 0.02, 0.025]` | 0.45 | black satin bow tie |

The body's colour variation (fur / muzzle / inner-ear / paw-pad / belly) is **baked into the diffuse
texture** with smooth falloffs — no flat per-face patches. A tiled **curly-teddy fabric normal map**
(Poly Haven "Curly Teddy Natural", CC0) gives the plush surface; it is tiled with
**`KHR_texture_transform`** (scale ≈ 10) — **confirm Bevy 0.19 honours that extension**; if it
doesn't, the fabric normal shows at 1:1 (still fabric, just coarser) or is ignored (falls back to a
smooth surface — the base colour is unaffected either way). The eyes and nose are **geometry** (a
protruding bead / a thread X), not painted. Leave the materials as authored (creature convention —
not runtime-recoloured). The game's OCIO view transform is `NONE`, so colours read roughly as authored.

---

## 6. Wiring it into the game

Code-spawned creature, **no `config.ron` furniture row**. Mirror a skinned creature module (crab —
`src/crab/`; parasite — `src/parasite.rs`): a `Scp1048` marker + a spawn system emitting the §3
`WorldAssetRoot` child (scale + 180° yaw), an on-load system that builds the `AnimationGraph` by name
and loops `scp1048_rest_idle`, and behaviour that swaps to `dance`/`draw_picture`/`jump_in_place`.
Keep cosmetic-only systems out of the hashed sim set (the model is a **child** of the gameplay
entity, per the artist guide's §12 determinism contract).

The mesh is safe to skip `autogib` (it is not a single watertight island, and this entity is benign).

---

## 7. Regenerating the asset (clean-room recipe)

The teddy is built procedurally in the `SCP_Characters` repo (needs Blender; no MPFB2). The complete
reproducible recipe is **`SCP_Characters/examples/build_scp1048_teddy.py`** (`build(export_dir=...)`
runs the whole pipeline in a live Blender session). The clean-room construction, verified in-session:

1. **Body** — a metaball figure (ellipsoid body + head + muzzle + neck; chained ball limbs + ear
   bumps; `resolution 0.028`, `threshold 0.65`) → convert to mesh → **voxel remesh** (`0.021`) →
   **decimate** to ~4000 tris → scale to 0.33 m, ground base at `y=0`, centre X. This yields the
   smooth continuous body (no lumpy loft).
2. **Rig** — 8 edit-bones (`torso, head, ear_l/r, upper_arm_l/r, upper_leg_l/r`) positioned to the
   body, then `ARMATURE_AUTO` bone-heat weighting.
3. **Texture** — UV unwrap, then a **procedural diffuse bake** (per-face 3D-position colour function:
   warm-brown fur, dark reddish-brown muzzle around the snout tip, dark inner ears + paw pads, a
   subtle lighter belly; smooth `smoothstep` falloffs; edge-padded to hide UV seams; 1024² sRGB).
   *(Headless-safe variant: use a box/spherical UV projection instead of the interactive
   `smart_project`.)* Then layer a tiled **fabric normal** map for the plush surface (Poly Haven
   "Curly Teddy Natural", CC0, bundled at `examples/data/curly_teddy_nor_gl.jpg`).
4. **Face + accessory geometry** — two glossy-black **glass-ball eyes** (UV spheres, half-embedded on
   the face above the muzzle, weighted to `head`); a **cross-stitch nose** (two thin dark bars in an
   X on the snout tip, weighted to `head`); a **pinched-box bow tie** at the throat (weighted to
   `torso`). Each with its own material; joined into the body mesh (4 material slots total).
5. **Animate + export** — apply the 5 clips (`SkeletalMotionLibrary`), then
   `export_monster_glb([rig, mesh], ".../scp1048.glb")` (GLB embeds the packed texture; `export_yup`,
   fps 24), and rename to `scp-1048.glb`.

**Verify a regenerated export:** `skins == 1` (8 joints), 5 animations named
`scp1048_{rest_idle,dance,jump_in_place,draw_picture,sit_down}`, 4 materials, 2 embedded texture
images (baked diffuse + fabric normal), base at `y = 0`, tris ≲ 5000. Also confirm `sit_down`'s last
frame still matches `draw_picture`'s first (they share `BEAR_SEATED_BASE`) and that no frame of
either drives the mesh below `y = 0`.
