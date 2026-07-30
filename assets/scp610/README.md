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

**Status: asset delivered, not yet wired into the game.** No `src/scp610/` module, no
`Faction`/`BrainId` entry, no row in `tests/creature_clip_contract.rs` exist yet — see §8.

---

## 1. The contract (verified against the export)

| | |
|---|---|
| File | `assets/scp610/scp-610.glb` — **4.9 MiB**, self-contained (was 27.4 MiB — see the note below) |
| Generator | Khronos glTF Blender I/O (glTF 2.0 binary) |
| Nodes | `scp610_rig` (Armature, 13 bones) → `scp610_mesh` (skinned mesh child) |
| Mesh | **3,921 verts · 7,912 triangles** (ceiling `INFECTED_MAX_TRIS_N = 9000`) · 2 material slots |
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

| Finding | Detail |
|---|---|
| **Only ONE texture set survives** | The exported `scp610_mesh_body` material carries the **blood/flesh** color+roughness+normal images only. The **guts/scar** set is absent — Blender's glTF exporter flattens the shader graph's Mix node down to whichever branch feeds the Principled BSDF's Base Color for static export, and with `scar_blend` itself excluded (below), there's no data path left for the second texture to ride along. **In-game today, the shipped material reads as a single, unblended flesh texture.** |
| **`scar_blend` does not export, by design** | It's a plain `FLOAT`/`POINT` generic mesh attribute, not a `COLOR_0` vertex color — Blender's glTF exporter only walks `mesh.color_attributes`, so this is dropped from the `.glb` unless `export_attributes=True` is passed (it isn't). Deliberate: a `COLOR_0` export would hit Bevy's `StandardMaterial` default of multiplying base colour by vertex colour (see §7 for the actual fix). |
| **Vertex-color risk — checked, confirmed benign** | The imported Quaternius source body carries 2 baked-in `BYTE_COLOR` corner attributes (`Color`, `Color.001`). Sampled directly: values are **(0.991–1.0, 0.991–1.0, 0.991–1.0, ~1.0)** — i.e. white/neutral. Even though Bevy's default multiply-by-vertex-color behavior would apply, multiplying by ~1.0 is visually a no-op. **Not a live bug in this build**, but don't assume the same holds after any future re-export — re-check if the source body asset ever changes. |
| **The embedded normal map is 25.4 MB (PNG)** | `others_0014_normal_opengl_2k.png` — the blood pack's normal map ships as lossless PNG straight from source and dominates the file's ~27 MB size. If load time/footprint matters, re-export with this downscaled (e.g. 1024²) or converted to a compressed format; this is a size/quality tradeoff, not a correctness bug. |
| Eye material | Small hard-edged slot, solid color, unaffected by any of the above. |

---

## 7. Procedural growth via textures — recommendation

This is the part worth building deliberately rather than shipping the single-texture fallback
above forever. Three tiers, escalating in effort, **all grounded in techniques this game already
ships** for mycelia (`src/mycelia/`, `BEVY_GAME_INFO.md` §6 and §8) — nothing here is a new
architecture pattern for this codebase, just SCP-610 adopting what mushrooms and mold already do.

### Tier 0 — free, no new assets (do this first)

Already covered in §4: vary the `mutation` `MorphWeights` per-instance or over time. Zero new
textures, zero new shaders, immediate visual variety.

### Tier 1 — restore the flesh/scar blend, the mycelia-mushroom way

This directly fixes the §6 finding (single unblended texture). The mushroom growth contract
(`BEVY_GAME_INFO.md` §8) already solved the identical problem — "a per-vertex value needs to pick
between materials without Bevy's default vertex-color multiply corrupting it" — via **`COLOR_0` as
a part mask, not artwork**: `MoldFruitMaterial` (`src/mycelia/material.rs:175`) reads `COLOR_0`'s
channels as a lookup key and **overwrites** `base_color` outright instead of multiplying by it.

Do the same for SCP-610:

1. **Blender-side (small change, `monsters/infected.py`/`core/utils.py`):** in addition to (or
   instead of) the current generic `scar_blend` attribute, bake the same value into a real
   `COLOR_0` vertex-color attribute (e.g. `R = scar_blend`, `G = B = 0`) so it survives export as
   `mesh.color_attributes`, matching what the mushroom pipeline already does.
2. **Bevy-side (new file, e.g. `src/scp610/material.rs`):** an
   `ExtendedMaterial<StandardMaterial, Scp610FleshExt>`, one WGSL shader
   (`assets/shaders/scp610_flesh.wgsl`, sibling to `mycelia_fruit.wgsl`) that:
   - samples the flesh texture set (already embedded — §6) and the scar/guts texture set (**not**
     currently shipped in the `.glb`; source files live at
     `/mnt/codex_fs/game_assets/pbr/Guts_2K-JPG/others_0003_{color,roughness,normal_opengl}_2k.{jpg,png}`
     — load them as loose assets under a new `assets/scp610/textures/` the same way floor/wall
     textures are loose files loaded separately from their GLB in §6),
   - reads `COLOR_0.r` as the blend factor (`mix(flesh, scar, color_0.r)`),
   - **overwrites** `base_color`/`normal`/`roughness` rather than multiplying, exactly like
     `MoldFruitMaterial` does for its part mask.
3. Swap the material at spawn time (same "swap at runtime" pattern already used for walls the mold
   has colonised, `BEVY_GAME_INFO.md` §6).

This is a bounded, well-precedented piece of work — the hard design questions (how do you get a
per-vertex blend into Bevy without the multiply bug) are already answered by code sitting in this
same repository.

### Tier 2 — animated, spreading growth (ambitious, optional)

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

## 8. Wiring it into the game (not done — the unavoidable next step regardless of §7)

SCP-610 is a **code-spawned creature**, no `config.ron` row needed. Mirror `src/scp1048/`
(`mod.rs`/`anim.rs`/`brain.rs`/`behavior.rs`/`effects.rs`/`replicate.rs`) — the closest structural
template: another skinned, rigged, multi-clip creature. Concretely, still needed:

- A `Faction`/`BrainId` entry (`src/ai/faction.rs`, `src/ai/brain.rs`) — one variant can cover the
  whole species the way `Faction::Bear` covers all 4 SCP-1048 variants.
- A `src/scp610/` module in the same shape as `src/scp1048/`.
- A row in `tests/creature_clip_contract.rs`: `SCP610_GLB`, `SCP610_WIRED` (clip index → name, per
  §5's table), `SCP610_CLIP_COUNT = 5`.
- A resolved spawn yaw (§2) and render scale sanity check (authored at real scale, but verify
  against an actual in-scene spawn before shipping).
- A decision on `scp610_death` (§5) — gib via `autogib` like everything else, or actually use the
  baked clip.

None of this is blocked by the §7 material work — it's orthogonal, asset-loading/animation-contract
plumbing that has to happen regardless of which growth tier is implemented.

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
