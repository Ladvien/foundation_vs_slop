# SCP-999 (the blob) — game asset hand-off

The friendly, amorphous **orange gelatinous "Tickle Monster."** A morph-target gel mound with **no
skeleton** — all motion is either a baked morph-weight clip in the `.glb` or a runtime soft-body
jiggle you drive yourself, both writing the same `MorphWeights`. Target engine: **Bevy 0.19**.

This doc is the contract + integration guidance for wiring SCP-999 into `foundation_vs_slop`. It was
written against the **actual exported file** (numbers below were read out of the `.glb`, not assumed).
Companion: the squad handoff in `/mnt/codex_fs/game_assets/SCP_Characters/gltf/valkyrie_bevy_integration.md`.

---

## 1. The contract (verified against the export)

| | |
|---|---|
| File | `assets/scp999/scp-999.glb` — **33,944 bytes (~34 KB)**, self-contained |
| Generator | Khronos glTF Blender I/O v5.1.20 (glTF 2.0 binary) |
| Scene | **1 scene, default `scene 0`** — the asset is in scene 0 |
| Nodes | `scp999` (Empty, placement root) → `scp999_gel` (mesh child) |
| Mesh | **146 verts · 288 triangles · one material** (`scp999_gel_mat`) |
| Skeleton | **none** — `skins == 0`, no `JOINTS_0`/`WEIGHTS_0`. Not a skinned rig. |
| Textures | **none** — procedural material, `images == 0` (nothing to embed) |
| Units | **metres, Y-up** · base rests on **`y = 0`** · centred on X and Z |
| Vertex attrs | `POSITION`, `NORMAL` (+ 5 morph targets, each with `POSITION`+`NORMAL`) |
| Morph targets, **in order** | `squash` (0), `stretch` (1), `wobble_x` (2), `wobble_y` (3), `pulse` (4) |
| Animations | **5** (see §4) |
| Watertight | **yes** — 1 closed manifold island, 0 non-manifold edges, 0 open boundary edges |

The morph order is verified against `mesh.extras.targetNames` at export. If you resolve targets by
name, assert this order at load rather than trusting the index.

---

## 2. Size and scale

Native bounding box (Y-up, straight out of the `.glb`):

| Axis | Extent | Range |
|---|---|---|
| **X (width)** | 2.50 m | −1.25 … +1.25 |
| **Y (height)** | 1.52 m | 0 … 1.52 (base planted at 0) |
| **Z (depth)** | 1.60 m | −0.80 … +0.80 |

SCP-999 is authored **large and unscaled** — the same convention the crab and parasite use (the game
applies a render scale at spawn: crab `0.15`, parasite `0.07`). Canon SCP-999 at rest is a dome
**~2 m wide, ~1 m tall**, so:

- For an **adult-sized (~1 m tall) mound**, spawn with a render scale of **≈ 0.65** (1.0 / 1.52).
- Author the scale as a spawn-time `Transform::scale`, **not** by editing the asset — keep the `.glb`
  at native metres so a future rescale is one number.

**Facing:** the blob is radially symmetric in plan and has no "front," so the game's forward = local −Z
convention is moot — no spawn yaw is needed. (Contrast VALKYRIE/parasite, which do need a spawn
rotation.)

---

## 3. Loading (Bevy 0.19)

```rust
use bevy::prelude::*;

fn spawn_scp999(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((
        SceneRoot(assets.load(GltfAssetLabel::Scene(0).from_asset("scp999/scp-999.glb"))),
        Transform::from_scale(Vec3::splat(0.65)),   // ~1 m tall; tune to taste
        BlobJiggle::default(),                        // optional reactive soft body — see §5
    ));
}
```

The glTF loader puts a `MorphWeights` on the mesh-node entity (`scp999_gel`) and gives the primitive a
`MeshMorphWeights::Reference(parent)`. **Mutate the node's `MorphWeights` only.** It appears one or
two frames *after* the scene spawns, so any system that writes it must tolerate its absence early.

---

## 4. The five baked clips

Play any clip with a standard `AnimationPlayer` + `AnimationGraph`; the loader wires the
`AnimationTarget`s for you. **Resolve clips by name** (`Gltf::named_animations`) — the glTF index order
is stable but non-obvious (`ooze_forward` is index 0 because it also carries a node-TRS channel).

| Name | glTF idx | Frames @24fps | Loop | Channels | What it is |
|---|---|---|---|---|---|
| `scp999_idle_wobble` | 1 | 48 | **yes** | weights | gentle breathing sway (default idle) |
| `scp999_tickle_pulse` | 2 | 24 | **yes** | weights | the giggle — a double squash-and-swell bounce |
| `scp999_ooze_forward` | 0 | 48 | **yes** | **weights + translation** | in-place oozing surge (morph **and** node TRS in one clip; net-zero translation — the engine moves the entity) |
| `scp999_emote_happy` | 3 | 36 | no | weights | one-shot happy stretch-up / squash-down bounce |
| `scp999_settle` | 4 | 60 | no | weights | **baked soft-body jiggle** — an impulse rings the modes and decays to rest |

`scp999_ooze_forward` is the proof of the morph+TRS merge: one animation, two channel kinds on the gel
node. Because every clip is **in-place** (24 fps, no root motion), drive world movement from your own
`Transform` and let the clip supply only the wobble.

### AnimationGraph sketch (load by name)

```rust
// After the Gltf asset has loaded (e.g. in an on-load system):
fn build_blob_graph(
    gltf: &Gltf,
    graphs: &mut Assets<AnimationGraph>,
) -> (Handle<AnimationGraph>, HashMap<&'static str, AnimationNodeIndex>) {
    let mut graph = AnimationGraph::new();
    let mut idx = HashMap::new();
    for name in ["scp999_idle_wobble", "scp999_tickle_pulse",
                 "scp999_ooze_forward", "scp999_emote_happy", "scp999_settle"] {
        let clip = gltf.named_animations[name].clone();
        idx.insert(name, graph.add_clip(clip, 1.0, graph.root));
    }
    (graphs.add(graph), idx)
}
// Insert AnimationGraphHandle + AnimationPlayer on the entity that owns the AnimationPlayer
// the loader created, then player.play(idx["scp999_idle_wobble"]).repeat();
```

Set the looping clips (`idle_wobble`, `tickle_pulse`, `ooze_forward`) to repeat; let the one-shots
(`emote_happy`, `settle`) play once.

---

## 5. Runtime soft-body jiggle (optional, reactive)

Soft-body physics cannot live *inside* a `.glb` (glTF ships only TRS/skin/morph-weight timelines), so
"soft body" is delivered two ways that share **one physics model** — a damped spring-mass-damper per
morph mode (modal dynamics à la Pentland & Williams 1989, "Good Vibrations"; source of truth:
`SCP_Characters/src/scp_characters/monsters/softbody.py`):

- **Baked** — `scp999_settle` is that model rung by a fixed impulse and baked to a weight timeline.
  Deterministic, free, plays like any clip. Use for scripted moments or when you don't run the physics.
- **Runtime** — the same springs, struck by the entity's **actual acceleration** each frame, so the
  blob wobbles reactively when it moves, stops, or is hit. Engine code below.

They **layer**: run a primary clip (`idle_wobble`, `ooze_forward`) and add the runtime jiggle on top as
reactive secondary motion. Don't run `settle` **and** the runtime jiggle together — both are impulse
responses and would double up.

```rust
use bevy::prelude::*;

// Morph target order in scp-999.glb — verified against mesh.extras.targetNames.
const T_SQUASH: usize = 0;
const T_STRETCH: usize = 1;
const T_WOBBLE_X: usize = 2;
const T_WOBBLE_Y: usize = 3;
const T_PULSE: usize = 4;
const N_TARGETS: usize = 5;

/// One damped harmonic oscillator: x'' + 2 zeta omega x' + omega^2 x = 0, struck by impulses.
/// Same model + substepping as monsters/softbody.py::integrate_damped.
#[derive(Clone, Copy)]
struct Spring { x: f32, v: f32, omega: f32, zeta: f32 }
impl Spring {
    fn new(hz: f32, zeta: f32) -> Self {
        Self { x: 0.0, v: 0.0, omega: std::f32::consts::TAU * hz, zeta }
    }
    fn kick(&mut self, impulse: f32) { self.v += impulse; }
    fn step(&mut self, dt: f32, substeps: u32) {
        let h = dt / substeps as f32;
        for _ in 0..substeps {
            let a = -(self.omega * self.omega) * self.x - 2.0 * self.zeta * self.omega * self.v;
            self.v += a * h;
            self.x += self.v * h;
        }
    }
}

#[derive(Component)]
pub struct BlobJiggle {
    vertical: Spring, // squash <-> stretch (signed)
    wobble_x: Spring,
    wobble_y: Spring, // Bevy is Y-up, so the ground plane is XZ
    pulse: Spring,
    prev_pos: Option<Vec3>,
    prev_vel: Vec3,
    accel_gain: f32,  // impulse per m/s^2 of acceleration
    substeps: u32,
}
impl Default for BlobJiggle {
    fn default() -> Self {
        Self {
            vertical: Spring::new(2.4, 0.22), // BLOB_JIGGLE_VERTICAL_HZ / _DAMPING
            wobble_x: Spring::new(1.7, 0.28), // BLOB_JIGGLE_WOBBLE_*
            wobble_y: Spring::new(1.7, 0.28),
            pulse:    Spring::new(3.1, 0.35), // BLOB_JIGGLE_PULSE_*
            prev_pos: None,
            prev_vel: Vec3::ZERO,
            accel_gain: 0.015,
            substeps: 8,                      // BLOB_JIGGLE_SUBSTEPS_N
        }
    }
}

/// Descendant carrying MorphWeights (the glTF mesh node under the scene root).
fn morph_entity(root: Entity, children: &Query<&Children>, has_mw: &Query<(), With<MorphWeights>>) -> Option<Entity> {
    if has_mw.get(root).is_ok() { return Some(root); }
    for child in children.iter_descendants(root) {
        if has_mw.get(child).is_ok() { return Some(child); }
    }
    None
}

/// Excite the springs from the blob root's acceleration, integrate, and ADD the jiggle on top of
/// whatever the AnimationPlayer already wrote. Order this AFTER `animate_targets` so it layers on the
/// primary clip. Tolerates MorphWeights being absent for the first frame(s).
pub fn drive_blob_jiggle(
    time: Res<Time>,
    mut blobs: Query<(Entity, &GlobalTransform, &mut BlobJiggle)>,
    children: Query<&Children>,
    has_mw: Query<(), With<MorphWeights>>,
    mut weights: Query<&mut MorphWeights>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 { return; }
    for (root, xf, mut j) in &mut blobs {
        let pos = xf.translation();
        let vel = match j.prev_pos { Some(p) => (pos - p) / dt, None => Vec3::ZERO };
        let accel = (vel - j.prev_vel) / dt;
        j.prev_pos = Some(pos);
        j.prev_vel = vel;

        let g = j.accel_gain;
        j.vertical.kick(-accel.y * g);          // falling/landing compresses; rising stretches
        j.wobble_x.kick(accel.x * g);
        j.wobble_y.kick(accel.z * g);
        j.pulse.kick(accel.length() * g * 0.5); // any jolt breathes the surface

        j.vertical.step(dt, j.substeps);
        j.wobble_x.step(dt, j.substeps);
        j.wobble_y.step(dt, j.substeps);
        j.pulse.step(dt, j.substeps);

        let Some(mw_entity) = morph_entity(root, &children, &has_mw) else { continue };
        let Ok(mut mw) = weights.get_mut(mw_entity) else { continue };
        let w = mw.weights_mut();
        if w.len() < N_TARGETS { continue; }
        let vx = j.vertical.x;
        if vx >= 0.0 { w[T_STRETCH] += vx; } else { w[T_SQUASH] += -vx; }
        w[T_WOBBLE_X] += j.wobble_x.x;
        w[T_WOBBLE_Y] += j.wobble_y.x;
        w[T_PULSE]    += j.pulse.x.max(0.0);
    }
}

// app.add_systems(PostUpdate, drive_blob_jiggle.after(bevy::animation::animate_targets));
```

Tuning: `accel_gain` sets bounciness; each spring's `hz`/`zeta` set frequency and settle speed. Keep
them in sync with `monsters/constants.py` (`BLOB_JIGGLE_*`) if you want the runtime feel to match the
baked `settle` clip exactly. `wobble_y` (morph target 3) is authored but no baked clip drives it — it
exists precisely for this reactive lateral wobble on the Z axis.

---

## 6. The translucent gel material

`scp999_gel_mat` is a translucent-orange PBR material (values read from the `.glb`):

| Property | Value |
|---|---|
| `baseColorFactor` | `[0.72, 0.28, 0.05, 1.0]` (linear — warm orange) |
| `roughnessFactor` | 0.15 |
| `metallicFactor` | 0.0 |
| `KHR_materials_transmission` | `transmissionFactor: 0.85` |
| `KHR_materials_ior` | `ior: 1.33` |

The export **does** carry the see-through gel via `KHR_materials_transmission` + `KHR_materials_ior`
(both in `extensionsUsed`). Volumetric subsurface (`KHR_materials_volume`) was **not** exported, so
there is no thickness-based scatter. **Bevy's support for `KHR_materials_transmission` is
version-dependent** — confirm in your Bevy 0.19 build. The **guaranteed floor** if transmission isn't
honored is an opaque warm-orange, low-roughness `StandardMaterial`, which still reads as SCP-999. Leave
the material as authored (creature convention — like the crab, it is not runtime-recolored).

> Note: the game's only OCIO view transform is `NONE` (no AgX/Filmic), so highlights on the glossy gel
> clip flat. That's an engine color-management setting, not an asset problem.

---

## 7. Death / gibbing

The mesh is **watertight** (single closed manifold, no open edges), so it is safe for `autogib`
plane-slicing if you ever let SCP-999 be gibbed — caps will close cleanly. It has no skeleton and no
`rifle`/`chestrig` named sub-nodes, so none of the squad-specific autogib special-casing applies. In
practice SCP-999 is a friendly entity and probably never gibs; the watertightness is a free guarantee,
not a requirement.

---

## 8. Wiring it into the game

SCP-999 is a **code-spawned creature**, not furniture, so it needs **no `config.ron` row** (the
furniture manifest is for the placement solver). Mirror an existing creature module rather than
inventing a new pattern:

- **Crab** — `src/crab/` (`mod.rs`, `setup.rs`): a skinned enemy with an `AnimationPlayer`, a render
  scale, and a clip-index map. The closest structural template for "load a creature `.glb`, build an
  AnimationGraph, play a state-driven clip."
- **Parasite** — `src/parasite.rs`: another `.glb` creature with a 12-clip set and a spawn rotation.

Recommended shape for a `src/scp999.rs` (design left to you):

1. A `Scp999` marker component + a spawn system that emits the `SceneRoot` from §3 with a render-scale
   `Transform` and (optionally) `BlobJiggle`.
2. An on-load system that reads the `Gltf` asset, builds the `AnimationGraph` by name (§4), and starts
   `scp999_idle_wobble` looping.
3. Behaviour: swap to `tickle_pulse`/`emote_happy` on interaction, `ooze_forward` while moving (drive
   the `Transform` yourself; the clip is in-place). Add `drive_blob_jiggle` in `PostUpdate` after
   `animate_targets` for reactive wobble, **or** trigger the baked `settle` clip on a landing — not both.

If you want SCP-999's numbers to be data-driven (render scale, move speed, jiggle gain), add a small
`scp999` block to `config.ron` and read it the way the crab/parasite tuning is read — but that is a
gameplay choice, not an asset requirement.

---

## 9. Regenerating the asset

SCP-999 is procedural — regenerate it from the `SCP_Characters` pipeline (needs Blender; **no MPFB2**
required for the blob):

```python
import sys, os
ROOT = "/mnt/codex_fs/game_assets/SCP_Characters"
sys.path.insert(0, os.path.join(ROOT, "src")); sys.path.insert(0, ROOT)
import scp_characters as scp
scp.monsters.Scp999().build().animate().export("/home/ladvien/foundation_vs_slop/assets/scp999")
# writes scp999.glb — rename to scp-999.glb to match the scp-150.glb convention.
```

The build is **test-pinned** in the source repo (`tests/golden_monsters.json`,
`tests/blob_contract.py`, `tests/test_scp999.py`, `tests/test_monster_outcomes.py`): 288 tris, 6 shape
keys (Basis + 5), 5 animations, watertight, within the canon envelope. **Always verify a regenerated
export**: 5 animations, `skins == 0`, 5 morph targets named `squash/stretch/wobble_x/wobble_y/pulse`,
and the `scp999_settle` weight curve decaying to zero. The full technique writeup (baked-vs-runtime
soft body, the modal physics) lives at
`/mnt/codex_fs/game_assets/SCP_Characters/docs/scp999_bevy_integration.md`.
