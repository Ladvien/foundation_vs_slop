# SCP-610 "The Flesh that Hates" — game asset hand-off

Stage-2 "animate infected" per canon: a humanoid base with 2 original human limbs (rigged,
run/attack-capable), 3 asymmetric mutant limbs (Geometry-Nodes-refined, jiggle-only), 5 unrigged
growth nodules, and an asymmetric head-lobe bulge. All motion is either a baked skeletal clip in
the `.glb` or driven `MorphWeights`/bone rotations you write yourself. Target engine: **Bevy
0.19**.

This doc is the contract + integration guidance for wiring SCP-610 into `foundation_vs_slop`. It
was written against the **actual exported file** (numbers below were read out of the `.glb` and
its JSON chunk, not assumed). Companions: `/mnt/codex_fs/game_assets/SCP_Characters/BEVY_GAME_INFO.md`
§4 (SCP-610 subsection) and the SCP-999/SCP-1048 hand-offs in this same `assets/` tree — SCP-610 is
a hybrid of both (skinned rig **and** a morph target, where SCP-999 has only the morph and
SCP-1048 only the rig).

**Status: wired and shipping.** `src/scp610/` exists (FVS-C-1), `tests/creature_clip_contract.rs`
pins all five clips, and as of FVS-K-1 the flesh↔scar blend §6 reported missing is **shipping** —
see §6 and §7.

> ### ⚠️ The builder that produces this file was non-deterministic until 2026-07-30
>
> Rebuilding SCP-610 used to give a **different mesh every time**, and roughly one run in three
> died on `AssertionError: stub topology mismatch: basis has 3919 verts, grown target has 3906`.
> Runs that *passed* still disagreed: eight headless builds produced five distinct `.glb` files and
> two different triangle counts (7882 / 7912) from identical inputs. The shipped asset was whichever
> mesh a lucky run happened to emit.
>
> Two causes, both the same bug class — **an unordered iteration deciding geometry**:
> * `monsters/infected.py::_find_patch` accumulated the attach patch into a `set` of `BMFace`
>   proxies and returned `list(patch)[:N]`. BMesh proxies hash on their C pointer, so the patch's
>   order *and membership* varied per process and differed between the collapsed and grown bmeshes.
>   That order is handed to `extrude_discrete_faces`, so it sets each new face's `f.index` — which
>   is exactly the ID `infected_tentacle_nodes`' Random Value node keys its bump selection on.
> * `_extrude_stub` passed a set-ordered vertex list to `bmesh.ops.pointmerge`, so *which* vertex
>   survived the tip weld — and therefore which index held it — varied. That one is invisible in the
>   base mesh and shows up only in the `mutation` morph target, whose deltas are per-index.
>
> Both are fixed upstream (BFS-ordered list + `sorted(..., key=index)`). **Verified: six consecutive
> headless builds are byte-identical**, at 7,912 tris — the same topology this contract already
> pinned. If you rebuild and the numbers move, suspect a new unordered iteration before anything
> else.

---

## 1. The contract (verified against the export)

| | |
|---|---|
| File | `assets/scp610/scp-610.glb` — **5.0 MiB**, self-contained (was 27.4 MiB — see the note below) |
| Generator | Khronos glTF Blender I/O (glTF 2.0 binary) |
| Nodes | `scp610_rig` (Armature, 13 bones) → `scp610_mesh` (skinned mesh child) |
| Mesh | **3,921 verts · 7,912 triangles** (ceiling `INFECTED_MAX_TRIS_N = 9000`) · 2 material slots |
| Vertex colour | **`COLOR_0` = the scar blend mask**, one attribute, both primitives. R carries the flesh↔scar factor, G/B spare, A = 1. **It is data, not artwork — see §6.** |
| Skeleton | **13 bones**, single root `torso` — `torso, head, upper/lower_leg_l/r, upper/lower_arm_l/r, mutant_limb_0/1/2`. 2-bone chains on the original legs/arms only; mutant limbs are 1-bone jiggle stubs; growth nodules are **unrigged** (ride the nearest trunk bone's own skin weight) |
| Shape keys → morph targets | **`Basis`** (index 0, weight 1 — the still-human read) + **`mutation`** (index 1, weight 0 at rest) |
| Units | metres, Y-up · base planted at **`y = 0`** |
| Watertight | **yes** — 1 closed manifold island, 0 non-manifold edges (verified after a triangulation/bowtie fix during this build — see §6 note) |
| Animations | **5**, all in-place (see §5) |

**Recompressed 2026-07-30 (28.7 MB → 5.2 MB), geometry untouched.** 88% of the original file was a
single 2048² **16-bit** PNG normal map (25.4 MB; the colour and roughness maps together are 1.1 MB) —
35% of the game's entire `assets/` tree for a creature that spawns nowhere. At this game's fixed iso
zoom SCP-610 is ~1.9 m tall and never exceeds a few hundred pixels, so 2K/16-bit was about an order of
magnitude past what the screen can resolve; it is now 1024² 8-bit.

Done with `scripts/glb_recompress_texture.py`, which rewrites the GLB container in place rather than
round-tripping through Blender — **specifically to protect the contract in the table above.** An
exporter is free to reorder animations, rename morph targets or retriangulate, any of which would
silently break this asset for the sake of a texture change. Verified unchanged across the swap:
15 nodes, 1 mesh, 1 skin, 13 joints, 171 accessors, 21,706 verts / 7,912 tris, the five clips **in
order**, and the `mutation` morph target. The script asserts all of that on its own output and refuses
to write if any of it moved.

The `mutation` morph weight is the whole story: **0.0 = "still looks human"** (canon Stage 1 —
the collapsed limb stubs are a few millimetres, anatomically invisible), **1.0 = full "animate
infected"** (canon Stage 2 — limbs extend to their tapered, twisted, bump-refined full length and
the head-lobe bulges out). This is not cosmetic wobble like a blob's squash/stretch — it is the
disease progressing. See §4.

---

## 2. Size and scale

Two different bounding boxes matter, because the mesh's own extent changes with the morph weight
(reading `object.dimensions` in Blender does **not** pick this up — it reports the rest/Basis
box regardless of shape-key value; the numbers below for the grown state were read directly off
the `mutation` shape key's own vertex data):

| State | X (width) | Y (depth) | Z (height) |
|---|---|---|---|
| **Rest** (`mutation = 0.0`, T-pose, arms spread) | 1.80 m | 0.28 m | 1.82 m |
| **Grown** (`mutation = 1.0`) | 1.80 m | **0.86 m** | **1.90 m** |

The Y and Z growth is the mutant limbs extending sideways/backward and the tallest one spiking
above the crown (canon: "the head may become misshapen and elongate," "additional branches of
flesh will grow" — the build deliberately lets one limb read above the head silhouette).

Authored at **real human scale already** (unlike the blob or crab, which ship oversized and get a
spawn-time render scale) — a `Transform::from_scale(Vec3::ONE)` should read correctly. The rig's
own **T-pose rest** is the bind pose; standard skinned-mesh spawn conventions apply (see SCP-1048's
`src/scp1048/` for the closest structural template — another skinned, rigged creature).

**Facing:** authored front = **−Y** in Blender space (same ring-loft convention `bear.py`/blob use).
The resulting engine-space spawn yaw has **not been measured** — no game-side spawn code exists yet
to test against (see §8).

---

## 3. Loading (Bevy 0.19)

```rust
use bevy::prelude::*;

fn spawn_scp610(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((
        SceneRoot(assets.load(GltfAssetLabel::Scene(0).from_asset("scp610/scp-610.glb"))),
        Transform::IDENTITY,   // authored at real scale; verify spawn yaw before shipping (see §2)
        Scp610Mutation::default(), // drives the `mutation` MorphWeight — see §4
    ));
}
```

The mesh node carries both a skin (`JOINTS_0`/`WEIGHTS_0`, 13 bones, ≤4 influences per vertex —
glTF's cap, already normalized at export) **and** `MorphWeights` (2 targets). Bevy applies both:
the `AnimationPlayer` drives bone TRS + (optionally) morph weight channels from a clip; your own
system can additionally push the `mutation` weight directly, same as SCP-999's `BlobJiggle`
pattern layers a runtime effect over a baked clip.

---

## 4. The `mutation` morph — this is the disease, not a wobble

```rust
use bevy::prelude::*;

/// Drives the `mutation` MorphWeights target (index 1; index 0 is Basis and stays 0).
/// Unlike SCP-999's jiggle springs, this is typically a slow, one-directional progression
/// (or a fixed per-instance value), not a reactive oscillator.
#[derive(Component)]
pub struct Scp610Mutation {
    pub target: f32,      // where it's heading — 0.0 (passing) .. 1.0 (fully turned)
    pub current: f32,
    pub rate_per_sec: f32, // how fast it creeps toward target; 0.0 = instantaneous/static
}

impl Default for Scp610Mutation {
    fn default() -> Self {
        Self { target: 1.0, current: 1.0, rate_per_sec: 0.0 } // spawn already fully mutated
    }
}

const T_MUTATION: usize = 1; // MorphWeights index — Basis (0) is never driven directly

fn drive_scp610_mutation(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Scp610Mutation)>,
    children: Query<&Children>,
    has_mw: Query<(), With<MorphWeights>>,
    mut weights: Query<&mut MorphWeights>,
) {
    let dt = time.delta_secs();
    for (root, mut m) in &mut q {
        if m.rate_per_sec > 0.0 {
            let step = m.rate_per_sec * dt;
            m.current += (m.target - m.current).clamp(-step, step);
        } else {
            m.current = m.target;
        }
        let Some(mw_entity) = has_mw.get(root).is_ok().then_some(root)
            .or_else(|| children.iter_descendants(root).find(|e| has_mw.get(*e).is_ok()))
        else { continue };
        let Ok(mut mw) = weights.get_mut(mw_entity) else { continue };
        if let Some(w) = mw.weights_mut().get_mut(T_MUTATION) {
            *w = m.current.clamp(0.0, 1.0);
        }
    }
}
```

Three uses this unlocks, all free (no new geometry, no new textures):

1. **Population variety** — spawn several SCP-610 instances at different fixed `target` values
   (0.3, 0.6, 1.0) so not every infected reads identically "done."
2. **A turning sequence** — a still-passing NPC crosses a `target` threshold and creeps from 0→1
   over some seconds, visually dramatizing the canon "scar tissue starts to move of its own accord
   and grow at a rapid rate" beat, in-engine, with zero new assets.
3. **Gameplay tell** — read `current` to gate when the creature "counts as" mutated for AI/combat
   purposes (e.g. don't let it lunge-attack until `current` is past some threshold).

---

## 5. The five baked clips

| Name | glTF idx | What it is |
|---|---|---|
| `scp610_idle` | 0 | agitated tremor — canon: infected seek contact even before pursuing |
| `scp610_chase_run` | 1 | in-place gait cycle — **verified zero root-bone translation every frame** |
| `scp610_writhe_rage` | 2 | close-range aggro loop, mutant limbs flail |
| `scp610_lunge_attack` | 3 | one-shot grab — canon infection vector is touch |
| `scp610_death` | 4 | controlled collapse — **not gibbed**. Every other creature in this game dies via `autogib` fracture instead of a skeletal clip; this clip is unwired to anything and needs an explicit decision before it does anything at runtime |

All 5 are in-place (24 fps, no root motion) — drive world movement from your own `Transform` and
let the clip supply only the limb motion, same convention as SCP-999's clips.

```rust
fn build_scp610_graph(
    gltf: &Gltf,
    graphs: &mut Assets<AnimationGraph>,
) -> (Handle<AnimationGraph>, std::collections::HashMap<&'static str, AnimationNodeIndex>) {
    let mut graph = AnimationGraph::new();
    let mut idx = std::collections::HashMap::new();
    for name in ["scp610_idle", "scp610_chase_run", "scp610_writhe_rage",
                 "scp610_lunge_attack", "scp610_death"] {
        let clip = gltf.named_animations[name].clone();
        idx.insert(name, graph.add_clip(clip, 1.0, graph.root));
    }
    (graphs.add(graph), idx)
}
```

**Not yet built:** a stationary Stage-3 "rooted" form (canon: infected eventually root themselves
and spread growth across surrounding objects) — deferred, a separate environmental-coating design
effort rather than a creature variant. Mycelia's mold-coating system (§8 of `BEVY_GAME_INFO.md`) is
the natural template for that later effort, not for this animate stage.

---

## 6. Materials — what actually survives the export (verified, not assumed)

The Blender-side material is designed as **one combined body slot**, cross-fading two real PBR
texture sets (blood/guts photography) by a per-vertex `scar_blend` float attribute, so the
mutant-limb/nodule boundary reads as a soft gradient rather than a hard polygon-slot cut. Verified
directly against the exported `.glb`'s JSON chunk:

> **⚠️ Rewritten 2026-07-30 (FVS-K-1). The three findings below used to say the blend could not
> ship. It ships now** — the fix is the custom Bevy material §7 Tier 1 always named as the answer.
> The history is kept because the *reasoning* still holds for every other species: a per-vertex
> blend has no glTF material-level representation, so it can only travel as a data attribute that a
> custom shader reads.

| Finding | Detail |
|---|---|
| **Both texture sets now reach the game — by different routes** | The `.glb` still embeds only the **blood/flesh** color+roughness+normal set: Blender's exporter flattens the shader graph's Mix node to whichever branch feeds Base Color, and that has not changed. The **guts/scar** set instead ships as **loose files** in `assets/scp610/textures/` (`scar_color.jpg`, `scar_roughness.jpg`, `scar_normal.png` — 1024², 1.7 MB total, downscaled from `/mnt/codex_fs/game_assets/textures/pbr/Guts_2K-JPG/`), loaded separately and sampled by `assets/shaders/scp610_flesh.wgsl`. Same split the dungeon floor/wall textures already use. |
| **`scar_blend` now exports — as `COLOR_0`, and it is a MASK** | The builder bakes the value **twice**: `scar_blend` stays a `FLOAT`/`POINT` attribute for the Blender preview material, and a second `FLOAT_COLOR`/`POINT` attribute named `blend_mask` is exported **by name** as `COLOR_0` (`export_vertex_color="NAME"`). Measured on the shipped file: body primitive R spans 0.0→1.0 with **2,968 of 21,624 verts mid-gradient** — a real feather, not a hard cut. |
| **⚠️ `COLOR_0` is no longer neutral, and that is a live constraint** | It used to be. The old file shipped **three** colour sets — a forced all-1.0 `COLOR_0` plus both stray Quaternius attributes as `COLOR_1`/`COLOR_2` (0.9911–1.0) — because `export_all_vertex_colors` defaults `True`. Bevy's `StandardMaterial` multiplies base colour by `COLOR_0`, and multiplying by ~1.0 was a no-op, so nobody noticed. **That reasoning has now expired.** The strays are deleted in the builder and `COLOR_0` carries the mask, so *any* material reading it must **overwrite** rather than multiply. Measured consequence: on the **eye** primitive the mask is **0.0 at all 82 verts**, so a plain `StandardMaterial` there renders the eye **black**. Both slots are therefore replaced at spawn — see §7. |
| **The embedded normal map is downscaled** | `others_0014_normal_opengl_2k.png` shipped as a 2048² 16-bit PNG (25.4 MB, 88% of the file). Now 1024² 8-bit (1.8 MB) via `scripts/glb_recompress_texture.py`, which rewrites the container rather than round-tripping Blender, precisely so the contract in §1 cannot move for a texture change. |
| Eye material | Small hard-edged slot, solid color — **no longer unaffected**, see the `COLOR_0` row above. |

---

## 7. Procedural growth via textures — Tier 0 and Tier 1 SHIPPED, Tier 2 open

> **Status 2026-07-30 (FVS-K-1): Tier 0 and Tier 1 are both done.** Tier 0 landed with FVS-C-1
> (`src/scp610/mod.rs::drive_mutation` ramps the morph over `MUTATION_SECS`). Tier 1 landed here:
> the builder bakes `COLOR_0`, the loose scar set lives in `assets/scp610/textures/`, and
> `src/scp610/material.rs` + `assets/shaders/scp610_flesh.wgsl` do the blend. The recipe below is
> kept as the record of the design, with the two things that changed in the doing marked inline.
> **Tier 2 remains open** and is close to `BACKLOG.md`'s FVS-Q-7 — read that before starting it, so
> the two don't build the same thing twice.

Three tiers, escalating in effort, **all grounded in techniques this game already
ships** for mycelia (`src/mycelia/`, `BEVY_GAME_INFO.md` §6 and §8) — nothing here is a new
architecture pattern for this codebase, just SCP-610 adopting what mushrooms and mold already do.

### Tier 0 — free, no new assets (do this first) ✅ SHIPPED

Already covered in §4: vary the `mutation` `MorphWeights` per-instance or over time. Zero new
textures, zero new shaders, immediate visual variety.

### Tier 1 — restore the flesh/scar blend, the mycelia-mushroom way ✅ SHIPPED

This directly fixes the §6 finding (single unblended texture). The mushroom growth contract
(`BEVY_GAME_INFO.md` §8) already solved the identical problem — "a per-vertex value needs to pick
between materials without Bevy's default vertex-color multiply corrupting it" — via **`COLOR_0` as
a part mask, not artwork**: `MoldFruitMaterial` (`src/mycelia/material.rs:175`) reads `COLOR_0`'s
channels as a lookup key and **overwrites** `base_color` outright instead of multiplying by it.

Do the same for SCP-610:

1. **Blender-side** (`monsters/infected.py::_bake_scar_blend_weights`, `monsters/export.py`): bake
   the value into a real colour attribute **in addition to** the generic `scar_blend`, and export it.
   *Two changes from this recipe, both forced by what was measured:*
   - the attribute is **`FLOAT_COLOR`, not `BYTE_COLOR`** — 8 bits over a 4-ring feather is ~4
     visible steps in a gradient whose entire job is to have none;
   - export is **`export_vertex_color="NAME"` + `export_all_vertex_colors=False`**, not "just add
     the attribute". Left at its defaults the exporter *also* appends every stray attribute
     (`COLOR_1`, `COLOR_2`) and, when a mesh has colour attributes but no material uses them, emits
     a **forced all-1.0 `COLOR_0`** — which is what the old file shipped. Naming the attribute is
     what makes `COLOR_0` mean exactly one thing.
2. **Bevy-side** (`src/scp610/material.rs`): an `ExtendedMaterial<StandardMaterial, Scp610FleshExt>`
   plus `assets/shaders/scp610_flesh.wgsl` (sibling to `mycelia_fruit.wgsl`) that:
   - samples the flesh set (embedded — §6) and the scar set (loose, `assets/scp610/textures/`,
     downscaled from `/mnt/codex_fs/game_assets/textures/pbr/Guts_2K-JPG/` — note the source path
     moved under `textures/`, this doc used to give the old one),
   - reads `COLOR_0.r` as the blend factor (`mix(flesh, scar, color_0.r)`),
   - **overwrites** `base_color`/`normal`/`roughness` rather than multiplying, exactly like
     `MoldFruitMaterial` does for its part mask.
3. Swap the material at spawn time (same "swap at runtime" pattern already used for walls the mold
   has colonised, `BEVY_GAME_INFO.md` §6). **Both slots, not just the body** — §6's `COLOR_0` row
   explains why leaving the eye on a stock `StandardMaterial` renders it black.

The hard design question (how do you get a per-vertex blend into Bevy without the multiply bug) was
already answered by code sitting in this same repository; the work was in the export contract, not
the shader.

### Tier 2 — animated, spreading growth (ambitious, optional) — OPEN

For a genuinely "living, actively growing" read rather than a static blend: mycelia's floor/wall
coating (`mycelia_floor.wgsl`/`mycelia_wall.wgsl`) drives its look from a **GPU-compute-simulated
field texture** (Physarum agents + Gray-Scott reaction-diffusion, `src/mycelia/mod.rs`). The same
technique, at a much smaller/cheaper per-character scale, could modulate the Tier-1 blend factor
over time — e.g. a small reaction-diffusion texture (or even just a scrolling/evolving noise
field) sampled in `scp610_flesh.wgsl` and multiplied into the `COLOR_0.r` blend factor, so the
scarred regions visibly pulse or slowly creep rather than sitting at one fixed ratio. This reuses
the *existing* mycelia simulation infrastructure's shape (a compute pass writing a texture a
material samples), scoped down to one character instead of a whole dungeon floor — not a new
subsystem, a smaller instance of one that already runs.

**Recommended order:** ship Tier 0 immediately (already possible with the current `.glb`), do
Tier 1 as the first real follow-up (fixes a confirmed, documented gap), treat Tier 2 as a "if this
creature needs to feel more alive" polish pass, not a blocker.

---

## 8. Wiring it into the game — DONE

SCP-610 is a **code-spawned creature**, no `config.ron` row needed (its *containment rule* is
authored there, like every other anomaly's). What this section listed as outstanding:

| Was needed | Status |
|---|---|
| A `Faction`/`BrainId` entry | **Not needed, deliberately.** 610 does not move, perceive or attack, so it carries `Faction::Anomaly` (an existing perception partition with an empty drive list) and **no** `BrainId`/`Mode`/`Fact` entry. Those are append-only enums whose discriminants index saved beliefs, mode distributions and archived RL policies — a creature that perceives nothing must not grow them. See `src/scp610/mod.rs`'s module doc. |
| A `src/scp610/` module | ✅ FVS-C-1. Gameplay/visuals plugin split; the shared `spawn_scp610_at` builder mirrors `scp999::spawn_scp999_at`. |
| A row in `tests/creature_clip_contract.rs` | ✅ `scp610_clip_indices_still_name_the_clips_the_asset_promises` — all five clips, by index. This is the test that catches a re-export reordering them, so **run it first after any rebuild**. |
| Resolved spawn yaw + render scale | ✅ Authored at real human scale, so `RENDER_SCALE = 1.0` and no Y offset. |
| A decision on `scp610_death` (§5) | ✅ FVS-K-1: **the baked clip, not `autogib`.** 610 is the one creature in the game that does not fracture — the clip exists precisely so it collapses instead. |

---

## 9. Secondary motion — a documented gap on the asset side, relevant here

The asset-side `Scp610`/`Infected` Python class docstring claims the mutant limbs "jiggle as
secondary motion, the same modal spring-mass model the bear's ears use"
(`monsters/scp610.py`) — this is **not actually wired up** on the asset side:
`INFECTED_MUTANT_LIMB_JIGGLE_HZ/_DAMPING/_IMPULSE_DEG` and `INFECTED_NODULE_JIGGLE_*`
(`monsters/constants.py`) are defined but never consumed anywhere in `infected_motion.py`. If you
want reactive limb jiggle in-engine (rather than only the hand-authored `writhe_rage` clip), the
cleanest fix is a Bevy-side spring system in the same shape as SCP-999's `BlobJiggle`
(this repo's own `assets/scp999/README.md` §5) or the bear's bone-rotation jiggle
(`src/scp1048/`), driving the `mutant_limb_0/1/2` bones' local rotation directly rather than
morph weights, using those same constant values as the starting `hz`/`damping`/`impulse` — found
while researching this hand-off, flagged here rather than silently left for someone to rediscover.
