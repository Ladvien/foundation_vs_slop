# SCP-1048-C (rusted-metal copy) — game asset hand-off

**SCP-1048-C is not SCP-1048.** It is one of the *copies of itself* the Builder Bear assembles
from scavenged materials — this one from **rusted metal scraps** (the others: SCP-1048-A, human
ears; SCP-1048-B, a human infant). Per the source article the copies are **bear-shaped and the
same size** as the original, so this asset shares the original's silhouette, 8-bone rig and clip
set, and differs only in surface: corroded-metal texturing, riveted plates and bolt heads
scattered over the body, and a **crude scrap gun fused over its right paw** (the wiki records C
"exhibited extreme violence" during its escape — no specific weapon is canon; the gun is the game
reading of that violence, built in the same assembled-from-scrap language as the rest of it). The
sibling asset is documented at
[`../scp1048/README.md`](../scp1048/README.md); both bears can be loaded **simultaneously** —
every clip here is prefixed `scp1048c_*`, so animation names never collide with the original's
`scp1048_*` set.

> **Tonal note for the integrator:** 4 clips are inherited from the *benign* original and this
> asset **adds a 4-clip hostile set** (`aim_gun` / `fire_gun` / `pistol_whip` / `rage`) matching
> the article's "extreme violence". `draw_picture` was **dropped from this variant at sign-off**
> (tonally wrong on a violent copy, and the gun rode the scribbling arm). `dance` still ships as
> free legacy motion but reads wrong on C — wire `rest_idle` / `jump_in_place` / `sit_down` + the
> hostile four and leave `dance` unwired.

## 0. Artist-guide conformance (`docs/artist_guide.md` §3 / §14)

Checked against the shipped `.glb`, not assumed.

| Guide rule | Status |
|---|---|
| glTF 2.0 `.glb`, self-contained, embedded textures | ✅ 1.35 MB, 2 embedded images (PNG diffuse + JPEG normal) |
| Y-up, metres, no axis conversion | ✅ 0.330 m tall, Y-up |
| **Scene 0 is the asset** | ✅ 1 scene, default `scene 0` |
| JPEG textures permitted | ✅ rust normal is JPEG |
| **Normal-map tangents exported** (Bevy won't regenerate) | ✅ `TANGENT` on all 5 primitives (the rivet/barrel caps are triangle fans specifically so tangent generation succeeds) |
| Animations as separate glTF indices, 24 fps | ✅ 8 indices, 24 fps |
| No malformed stray geometry | ✅ clean bbox, no strays |
| Watertight (**if gibbed**) | ✅ **0 open edges** — 38 sealed shells, see note |
| In-place clips (no root motion) | ✅ **no clip translates the armature node** — see note |

**Measured for the §14 checklist** (glTF frame, metres):

| | |
|---|---|
| `footprint` (width, depth) | **(0.235, 0.133)** — width includes the down-and-out gun barrel |
| `height` (top of bbox) | **0.330** |
| `pivot` (XZ offset of bbox centre from origin) | **(0.029, 0.006)** — the X offset is the barrel; the body itself is centred, `(0,0)` is fine |

### Two notes on how those last two are satisfied

**Geometry is closed, in many shells.** The mesh is **38 sealed islands** (body + 2 eyes + 2 nose
bars + **30 scrap fragments** + the gun's receiver/barrel/magazine) with **zero boundary edges**. §9's hazard is *unclosed caps being
silently dropped*; every shell here is closed, so plane-slicing caps each one correctly and
`autogib` is safe if you ever want it — arguably more fitting here than on the plush original,
since a destroyed C would shed plates.

**Vertical motion lives on the root BONE, not the armature node.** `jump_in_place` and `sit_down`
need real vertical travel — the hop leaves the ground (**~90 mm** of lift), and the sit lowers the
body **~54 mm** or the folded-leg pose leaves the bear hanging in mid-air. Keying that on the
armature *object* would be baked root motion and would fight `Transform`. Instead it is keyed on
the root bone (`torso`), **inside** the skin: the exported glTF node for `scp1048c_rig` carries
**no translation channel at all**, so a clip can never move the entity's own `Transform`.
Verified in the shipped file.

> **Integrator note:** you own the `Transform` completely — nothing in these clips touches it.
> Every clip also keys the root bone explicitly (even at zero), so switching clips can't inherit a
> stale offset.

A **~33 cm bear-shaped effigy of rusted scrap metal**: corroded iron-oxide orange body with
near-black pitted zones (muzzle, inner ears, paw pads) and a dull worn-metal belly, **riveted
steel plates and bolt heads** over the surface, **tarnished steel ball-bearing eyes**, a
**welded-rod X nose**, and a **gunmetal scrap gun fused over the right paw** (boxy receiver
embedded into the paw, 55 mm barrel, magazine box beneath). The barrel runs along the **arm
bone's own axis** — down-and-out at rest like a lowered weapon, level at the horizon when
`aim_gun` raises the arm — so it aims where the arm aims and never at its own face. No bow tie —
a crude scrap copy isn't dressed. It is a **skinned rig** — the same 8-bone skeleton as the
original with 8 baked skeletal clips (4 benign + 4 hostile). Target engine: **Bevy 0.19**.

This model is produced by the same clean-room parameterised recipe as the original (one pipeline,
two `TeddyVariant` parameter sets). This doc is the contract + integration guidance; every number
below was read out of the actual exported `.glb`.

---

## 1. The contract (verified against the export)

| | |
|---|---|
| File | `assets/scp1048c/scp-1048-c.glb` — **~1.35 MB**, self-contained (all textures embedded) |
| Generator | Khronos glTF Blender I/O v5.1.20 (glTF 2.0 binary) |
| Scene | **1 scene, default `scene 0`** |
| Nodes | `scp1048c_rig` (armature root) → { `scp1048c_mesh` (skinned), `torso` (skeleton root) } |
| Mesh | **1 mesh · 5172 triangles · 5 primitives** (body / eyes / nose / fragments / arm gun) |
| Vertex attrs | `POSITION`, `NORMAL`, `TANGENT`, `TEXCOORD_0`, `JOINTS_0`, `WEIGHTS_0` — skinned + textured |
| Skeleton | **1 skin · 8 joints**, single root `torso`: `torso, head, ear_l, ear_r, upper_arm_l/r, upper_leg_l/r` |
| Skin weights | max 4 influences/vertex (glTF-normalized on export) |
| Textures | **2 embedded images** — the 1024² sRGB baked diffuse (base colour) + a tiled coarse-rust **normal** map; `extensionsUsed: [KHR_texture_transform]` (the rust normal's tiling, scale 4) |
| Units | **metres, Y-up** · base rests on **`y = 0`** · body centred on X (the gun barrel leads +X) |
| Animations | **8** — 4 benign + 4 hostile (see §4) |
| Materials | **5** (see §5) |

The mesh is **not a single watertight island** (eyes/nose/fragments are separate joined shells —
deliberately so: the fragments are supposed to read as separate scraps riveted on). Every shell is
individually sealed.

---

## 2. Size and scale

Native bounding box (Y-up, from the `.glb`):

| Axis | Extent | Range |
|---|---|---|
| **X (width)** | 0.235 m | −0.088 … +0.147 (the +X extreme is the lowered gun's muzzle) |
| **Y (height)** | 0.330 m | 0 … 0.330 (base planted at 0) |
| **Z (depth)** | 0.133 m | −0.060 … +0.073 |

Same canon size as the original (**the copies are the same size as SCP-1048**), authored unscaled.
Spawn with a render scale of **≈ 1.0** for canon size; scale as a spawn-time `Transform::scale`
if it needs to read larger — do not edit the asset. Base is at `y = 0`. The body itself matches
the original's envelope (plus a couple of millimetres of proud rivets); the extra width is the
rest-pose barrel pointing down-and-out on the right side.

**Facing:** the muzzle/eyes face **+Z** in the exported glb (Blender −Y front → glTF +Z). The
game's forward is local **−Z**, so **spawn with a 180° yaw** (`Quat::from_rotation_y(PI)`).

---

## 3. Loading (Bevy 0.19)

Mirror the skinned-creature pattern (crab / parasite): an **unscaled gameplay root** with a
**scaled model child** carrying the render scale + spawn yaw. Prelude name is **`WorldAssetRoot`**
(Bevy 0.19 renamed `Scene`→`WorldAsset`); the `GltfAssetLabel::Scene(0)` label is unchanged.

```rust
use bevy::prelude::*;
use std::f32::consts::PI;

const SCP1048C_GLB: &str = "scp1048c/scp-1048-c.glb";
const RENDER_SCALE: f32 = 1.0;   // canon ~0.33 m (same size as the original)

fn spawn_scp1048c(commands: &mut Commands, assets: &AssetServer, pos: Vec3) -> Entity {
    commands
        .spawn((
            Scp1048C,                                  // your marker component
            Transform::from_translation(pos),          // root is UNSCALED
            Visibility::Inherited,
        ))
        .with_child((
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(SCP1048C_GLB))),
            Transform::from_scale(Vec3::splat(RENDER_SCALE))
                .with_rotation(Quat::from_rotation_y(PI)),   // authored +Z, game forward -Z
        ))
        .id()
}
```

The glTF loader builds the skeleton + `AnimationPlayer`. Every clip is in-place — drive world
movement from your own `Transform`.

---

## 4. The eight baked clips

Skeletal (bone T/R/S on the 8 joints), **24 fps**, in-place. Resolve by name
(`Gltf::named_animations`). Names are prefixed **`scp1048c_`** — loading this bear alongside the
original never collides. Every clip was **individually reviewed and signed off in-viewport**
(2026-07-24).

**Benign four** (inherited from the original; `draw_picture` was dropped at sign-off):

| Name | glTF idx | Frames @24 fps | Loop | What it is |
|---|---|---|---|---|
| `scp1048c_rest_idle` | 0 | 72 (3.0 s) | **yes** | breathing/sway idle (default while unaware) |
| `scp1048c_dance` | 1 | 48 (2.0 s) | **yes** | inherited from the original — see the tonal note up top |
| `scp1048c_jump_in_place` | 2 | 40 (1.67 s) | one-shot | hops (root Z apex ≈ 0.09 m ×scale) |
| `scp1048c_sit_down` | 3 | 49 (2.04 s) | one-shot | stands → folds down onto its bottom; the plop rings the ears (settle jiggle baked in); **ends held in the seated pose** |

**Hostile four** (new on this variant — the article's "extreme violence"):

| Name | glTF idx | Frames @24 fps | Loop | What it is |
|---|---|---|---|---|
| `scp1048c_aim_gun` | 4 | 12 (0.5 s) | one-shot | raises the arm cannon from rest to a level aim (**+5.6° off horizon, measured**); hold the last frame while aiming |
| `scp1048c_fire_gun` | 5 | 10 (0.42 s) | one-shot | one shot: recoil kick (arm +14°, torso rocks, head snaps) and settle back to the aim |
| `scp1048c_pistol_whip` | 6 | 16 (0.67 s) | one-shot | melee with the weapon: the gun arm raises the scrap gun overhead, then clubs it down-across the body with a torso whip |
| `scp1048c_rage` | 7 | 36 (1.5 s) | **yes** | threat display: hunched forward loom, arms flailing (alternating up/down, both slamming forward at the bites), two forward lunge-snaps per loop, ears whipping — the hostile idle |

**Chain contracts (all measured on this export):**

- `aim_gun` → `fire_gun` → `fire_gun` → …: `aim_gun` **ends** in the shared aim pose and
  `fire_gun` **starts and ends** in it (measured seam 0.000 mm both ways), so aim once, then play
  `fire_gun` per shot with no cross-fade. Muzzle points forward (+Z in glTF, the facing) while
  aimed — never at its own face.
- `sit_down` ends held in the seated pose (hold the last frame); its root drops ~0.053 m through
  the fold to keep the feet on the floor. (The `draw_picture` clip this pose used to chain into
  was dropped from this variant.)
- `rage` loop endpoints are identical (0.000 mm), so it cycles with no hitch.

**Grounding, re-measured on this mesh with the fragments and gun attached:** worst floor
penetration across every frame of all **8** clips is **−7.3 mm** (`dance`; `pistol_whip`'s strike
reaches −7.2 mm — body/arm extremes; neither a fragment nor the gun ever leads the penetration;
the hostile clips measure −4.1 / −4.1 / −7.2 / −0.4 mm). The fragments stay attached through the
full `sit_down` fold (worst stand-off in the seated pose: 2.4 mm), and the gun rides the right
arm rigidly in every posed clip (it is weighted to the paw surface's own bone blend).

Build an `AnimationGraph`, add each clip by name, loop the idle, one-shot `jump_in_place` and
`sit_down`. The ear/head jiggle baked into `sit_down`'s landing is a damped spring model (see
`SCP_Characters/src/scp_characters/monsters/{softbody.py,bear_motion.py}`) — if you also drive a
runtime jiggle, don't stack the two on that clip.

---

## 5. Materials

Five PBR `StandardMaterial`s (values read from the `.glb`):

| Slot | Base colour | Metallic / Roughness | Covers |
|---|---|---|---|
| `scp1048c_body` | baked diffuse (base) + **rust normal** | 0.45 / 0.85 | corroded orange body, near-black pitted zones, worn-metal belly |
| `scp1048c_eye_glass` | `[0.18, 0.19, 0.21]` | **1.0** / 0.30 | tarnished steel ball-bearing eyes |
| `scp1048c_nose_thread` | `[0.16, 0.15, 0.14]` | **1.0** / 0.55 | welded-rod X nose |
| `scp1048c_fragment_mat` | `[0.13, 0.115, 0.10]` | **1.0** / 0.65 | the 30 riveted plates + bolt heads |
| `scp1048c_arm_gun_mat` | `[0.09, 0.09, 0.10]` | **1.0** / 0.45 | the scrap gun on the right paw (receiver / barrel / magazine) |

The body's colour variation (corrosion zones where the original's fur accents were) is **baked
into the diffuse texture** with smooth falloffs. A tiled **coarse rust normal map** (Poly Haven
"rust_coarse_01", CC0) gives the corroded surface; it is tiled with **`KHR_texture_transform`**
(scale 4) — **confirm Bevy 0.19 honours that extension**; if it doesn't, the rust normal shows at
1:1 (still rust, just larger features) or is ignored (falls back to a smooth surface — the base
colour is unaffected either way). The eyes, nose, every plate/rivet and the gun are **geometry**,
not painted. Leave the materials as authored (creature convention — not runtime-recoloured). The
game's OCIO view transform is `NONE`, so colours read roughly as authored.

---

## 6. Wiring it into the game

Code-spawned creature, **no `config.ron` furniture row**. Mirror a skinned creature module (crab —
`src/crab/`; parasite — `src/parasite.rs`): a `Scp1048C` marker + a spawn system emitting the §3
`WorldAssetRoot` child (scale + 180° yaw), an on-load system that builds the `AnimationGraph` by
name and loops `scp1048c_rest_idle`, and behaviour that swaps clips. A natural hostile loop:
`rest_idle` (unaware) → `rage` (target acquired, loop) → `aim_gun` (hold last frame) →
`fire_gun` per shot → `pistol_whip` in close. Leave `dance` unwired (tonal note).
Keep cosmetic-only systems out of the hashed sim set (the model is a **child** of the gameplay
entity, per the artist guide's §12 determinism contract).

Every shell is sealed (0 open edges), so `autogib` is safe if a destroyed C should shed plates.

---

## 7. Regenerating the asset (parameterised recipe)

Built by the same recipe as the original: **`SCP_Characters/examples/build_scp1048_teddy.py`**,
which is parameterised by a frozen `TeddyVariant` — the original is the `ORIGINAL` parameter set,
this asset is `RUSTED` (`key="scp1048c"`). In a live Blender session:

```python
build(RUSTED, export_dir=".../assets/scp1048c")   # or: blender ... -- scp1048c via __main__
```

What `RUSTED` changes against `ORIGINAL` (geometry pipeline is untouched — same metaballs, remesh,
rig, weighting, clips):

1. **Palette** — the procedural diffuse bake reads the variant's corroded palette (iron-oxide
   base, near-black accents, worn-metal belly) through the same smoothstep accent zones.
2. **Surface** — the tiled normal map is the bundled Poly Haven rust
   (`examples/data/rust_coarse_01_nor_gl_1k.jpg`, CC0), tiling 4, strength 3.5; body metallic 0.45.
3. **Accessories** — eyes become tarnished steel (metallic 1.0), the nose a welded rod;
   **no bow tie** (`build_bowtie=False`).
4. **Fragments** — `build_surface_fragments` scatters 30 plates/rivets (deterministic
   `random.Random(1048)`, area-weighted polygon sampling, keep-outs for the foot underside and
   the muzzle/eye region), orients each to its polygon normal, weights it to the local surface
   blend (`_rigid_skin_like_surface`), and joins all in one call.
5. **Arm gun** — `build_arm_gun` (`ArmGunSpec`) measures the right-paw centroid, embeds a boxy
   receiver into it, runs the barrel **along the arm bone's own axis** (triangle-fan caps — ngon
   caps kill glTF tangent export) and hangs a magazine box beneath, then weights the whole gun to
   the paw's bone blend and joins it.
6. **Clip selection** — `exclude_clips=("draw_picture",)` filters the benign set, and
   `include_hostile_clips=True` appends `bear_hostile_clips()`
   (`SCP_Characters/src/scp_characters/monsters/bear_motion.py`: aim/fire built on a shared
   `BEAR_AIM_POSE`, pistol_whip, rage) before `SkeletalMotionLibrary` bakes them.

**Verify a regenerated export:** `skins == 1` (8 joints), **8 animations** named
`scp1048c_{rest_idle,dance,jump_in_place,sit_down,aim_gun,fire_gun,pistol_whip,rage}`,
5 materials, 2 embedded texture images (baked diffuse + rust normal), `TANGENT` on all 5
primitives, base at `y = 0`, 0 boundary edges, tris ≈ 5170. Also confirm the `aim_gun`→`fire_gun`
chain still measures < 2 mm and that no frame of any clip drives the mesh below `y ≈ −0.008`.
