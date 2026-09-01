# `bevy_kimodo` — text-to-motion in the Animation tab

**Date:** 2026-09-01
**Scope:** a new workspace crate `crates/bevy_kimodo/`, plus a `Generate` sub-view in `crates/emerge-mapper/src/anim_tab.rs`
**Upstream:** [nv-tlabs/kimodo](https://github.com/nv-tlabs/kimodo) (Apache-2.0 code, NVIDIA model licences) and [localai-org/kimodo.cpp](https://github.com/localai-org/kimodo.cpp) (Apache-2.0 port)
**Bevy:** 0.19.0, verified against the pinned crate sources, not the web

---

## 0. What I verified, and what I did not

I cloned both repositories and read them. I read the Bevy 0.19.0 sources for `bevy_animation` and `bevy_gltf` from crates.io, and I read the actual node table out of `assets/characters/valkyrie.glb`. Every claim below with a file path or a signature behind it came from one of those.

`home-still` is **not connected in this session**, so the retargeting and loop-closure literature below is web-sourced and reasoned from what your own `2026-08-05-animation-editor.md` already cites (Kovar's Motion Graphs, GANimator's velocity-threshold contact labelling, Skeleton-Aware Networks' end-effector argument). Before Phase 5 is worth building, that corpus should get a proper search — it is exactly the body of work that phase depends on.

**The one thing nobody can verify from a repository is whether the output looks good on your character.** Phase 0 exists to answer that before any crate is written.

---

## 1. The good news, and the one hard problem

### The good news is your bone names

`valkyrie.glb` uses **Unreal mannequin naming** — `Root`, `pelvis`, `spine_01..03`, `clavicle_l`, `upperarm_l`, `lowerarm_l`, `hand_l`, `thigh_l`, `calf_l`, `foot_l`, `ball_l`, `neck_01`, `head`. Kimodo's SOMA-30 predicted skeleton maps onto that almost completely, and its SMPL-X 22 maps **exactly**, 22 of 22:

| SMPL-X 22 | valkyrie | SOMA-30 | valkyrie |
|---|---|---|---|
| `pelvis` | `pelvis` | `Hips` | `pelvis` |
| `left_hip` | `thigh_l` | `LeftLeg` | `thigh_l` |
| `left_knee` | `calf_l` | `LeftShin` | `calf_l` |
| `left_ankle` | `foot_l` | `LeftFoot` | `foot_l` |
| `left_foot` | `ball_l` | `LeftToeBase` | `ball_l` |
| `spine1/2/3` | `spine_01/02/03` | `Spine1`, `Spine2`, `Chest` | `spine_01/02/03` |
| `neck` | `neck_01` | `Neck1` | `neck_01` |
| `left_collar` | `clavicle_l` | `LeftShoulder` | `clavicle_l` |
| `left_shoulder` | `upperarm_l` | `LeftArm` | `upperarm_l` |
| `left_elbow` | `lowerarm_l` | `LeftForeArm` | `lowerarm_l` |
| `left_wrist` | `hand_l` | `LeftHand` | `hand_l` |
| `head` | `head` | `Head` | `head` |

Beware the SMPL naming trap: `left_hip` is the joint **at** the hip, so it drives the thigh; `left_foot` is the toe, not the ankle. Get that wrong and the whole leg is off by one bone.

SOMA-30 drops `Neck2`, `Jaw`, `LeftEye`, `RightEye` and the two per-hand end markers onto nothing. That is fine — an unmapped source joint contributes no curve, and an unmapped target bone keeps its rest pose.

### The hard problem is the rest pose, not the names

Kimodo's SOMA reference is a **T-pose** (`kimodo/assets/skeletons/somaskel77/somaskel77_standard_tpose.bvh`). A UE-mannequin rig rests in an **A-pose**. The standard correction is a per-joint constant:

```
D_j        = R_src_rest_global(j)⁻¹ · R_tgt_rest_global(j)
R_tgt_g(j,t) = R_src_g(j,t) · D_j
R_tgt_l(j,t) = R_tgt_g(parent(j),t)⁻¹ · R_tgt_g(j,t)
```

This is exact at rest and correct for small deviations, and it is what UE5's IK Retargeter and Blender's retarget add-ons do. Where it degrades is a large T↔A gap on the shoulder: the twist axis differs by ~45°, so elbow bends can swing out of plane. The industry answer is a **retarget pose** — a corresponding reference pose authored on both skeletons rather than the raw rest poses. Plan for that as a calibration step, not an afterthought; it is the difference between "recognisably the character" and "puppet".

Root translation is scaled by the hip-height ratio `tgt_leg_length / src_leg_length`, then multiplied through your `Rig::scale` (1.13 for the valkyrie) to land in the world units `cycle_distance` is declared in.

---

## 2. The four decisions, recorded

| Decision | Choice | Consequence |
|---|---|---|
| Crate name | **`bevy_kimodo`** | matches upstream spelling; searchable |
| Persistence | **export a file, artist merges in Blender** | **zero schema change** — `rigs.ron` and `SlotDef.clip` are untouched |
| CPU backend | **run kimodo.cpp's Go demo locally, same HTTP client** | one Rust transport; remote GPU and local CPU differ only by URL |
| Skeleton | **SOMA-30 first** | NVIDIA Open Model License, commercial use permitted |

The persistence choice is the one that saves the most work and it deserves saying out loud: `SlotDef.clip: usize` is *"the only identifier the runtime uses"*, an index into the rig's one GLB. Any in-editor persistence path forces a schema v4, a GLB **writer** in `emerge-core` (whose `glb.rs` is deliberately reader-only, hand-rolled, no glTF crate), and a migration of `rigs_match_assets.rs`. Exporting a `.bvh` beside the character instead means the artist re-exports the GLB and **your existing loop takes over unchanged**: `anim_watch` sees the mtime move, `emerge_core::clips` re-measures, the bench offers adopt with a provenance stamp. The generated clip arrives through the same door every hand-authored clip arrives through, and a human stays in the gate.

The skeleton choice is a licensing choice. kimodo.cpp's *default* model is `smplx-rp-v1`, which carries the **NVIDIA Internal Scientific Research and Development Model License** — no commercial use, no production, no redistribution of derivative models. SOMA and G1 are under the **NVIDIA Open Model License**, which permits commercial use. Set `"model": "soma-rp-v1.1"` explicitly on every request; the default is the wrong one for you.

---

## 3. What upstream actually gives us

### kimodo.cpp already has the HTTP API we want

`demo/main.go` is a real JSON service, not just a page:

```
POST /api/generate
  { "segments":[{"prompt":"...","frames":150}], "transition_frames":5,
    "steps":100, "seed":0, "model":"soma-rp-v1.1" }
  -> 202 { "id":"...", "status":"queued", ... }

GET  /api/animations                       -> [animation, ...]   (poll status)
GET  /api/models                           -> [{id,label,skeleton_key,license,commercial,available,parents,offsets}]
GET  /api/animations/<id>/rotations.f32    -> [T,J,4] little-endian f32, XYZW
GET  /api/animations/<id>/root.f32         -> [T,3]   little-endian f32
GET  /api/animations/<id>/animation.glb    -> node-only GLB, extras {"skeleton":..., "fps":30}
```

Constraints: 1–16 segments, 60–150 frames each, steps 1–1000. **30 fps** (`src/motion_rep.hpp:19`, and the GLB extras). Raw `.f32` streams are the right thing to consume — `animation.glb` is on Kimodo's skeleton, not yours, so it saves nothing once you are retargeting anyway.

`/api/models` is worth calling on connect and rendering verbatim: it reports `license`, `commercial` and `available` per model, so the editor can grey out what it must not use rather than trusting a string you typed.

### The Python side has no such API

`pyproject.toml` exposes four entry points — `kimodo_gen`, `kimodo_demo`, `kimodo_textencoder`, `kimodo_convert`. The demo is **Gradio**; `kimodo/model/text_encoder_api.py` talks to it with `gradio_client`. There is no REST surface for motion generation.

So the remote GPU backend needs a **sidecar you write** (~150 lines of FastAPI around `kimodo.model.Kimodo.__call__`). Write it to the kimodo.cpp contract above, and one Rust client speaks to both backends. That is Phase 6, and it is deliberately late: the CPU path proves the whole pipeline first, on your own machine, with no GPU box to stand up.

### The two backends are not at parity, and the trait must say so

kimodo.cpp's README lists what is **not implemented**: general constraint input, 77-joint SOMA expansion, skinned-mesh GLB export, quantised models. So the CPU path is **prompt-only** — no keyframes, no end-effector targets, no 2D paths. Do not model this as one interface pretending both sides are equal; expose a `Capabilities` struct and let the UI hide what the connected backend cannot do.

### Cost, both ways

GPU: ~17 GB VRAM, or **under 3 GB with `TEXT_ENCODER_DEVICE=cpu`** — the LLM2Vec Llama-3-8B text encoder is nearly all of it. CPU: the same encoder is the slow part; `KIMODO_TEXT_LAYER_CHUNK=1..32` tunes it. Both APIs also take a **precomputed 4096-float LLM2Vec embedding** and skip the text runtime entirely — which means an embedding cache keyed by prompt is the single highest-leverage optimisation on either path, and it belongs in the crate from the start.

---

## 4. Bevy 0.19 facts this depends on

All verified in the pinned sources.

**Building a clip in code works, and here are the exact calls:**

```rust
clip.add_curve_to_target(
    AnimationTargetId::from_names(path.iter()),          // bevy_animation-0.19.0/src/lib.rs:1316
    AnimatableCurve::new(
        animated_field!(Transform::rotation),            // animation_curves.rs:794
        UnevenSampleAutoCurve::new(keys)?,               // animation_curves.rs:757 — returns Result
    ),
);                                                        // lib.rs:284
```

`AnimationClip::set_duration` (`lib.rs:260`) pins the clip length; `add_curve_to_target` lengthens it automatically otherwise.

**The target path includes the animation root, and it is the scene root.** `collect_path` (`bevy_gltf-0.19.0/src/loader/gltf_ext/scene.rs:78-94`) seeds an empty path at the root and pushes each node name on the way down, so for the valkyrie the paths are:

```
Root    -> ["valkyrie_rig", "Root"]
foot_l  -> ["valkyrie_rig", "Root", "pelvis", "thigh_l", "calf_l", "foot_l"]
```

A runtime-built clip that uses `["Root", "pelvis", ...]` will hash to different `AnimationTargetId`s and animate **nothing, silently**. Derive the paths from the GLB — `emerge_core::glb` already reads the node hierarchy — rather than writing them down.

**Traps from `CLAUDE.md` that this feature will walk into:** a missing `Res<T>` panics rather than skipping, so the job-poll system takes `Option<Res<KimodoJobs>>` or the plugin `init_resource`s it; every `.run_if` in a chain is evaluated, so a `run_if(backend_connected)` guard must not hold a bare `Res`; and the bench stage parks the **main** camera at `stages::BENCH` precisely because a second `Camera3d` breaks every `Single<_, With<Camera3d>>` in the crate — the preview must not spawn one.

---

## 5. The crate

`crates/bevy_kimodo/`, **MIT OR Apache-2.0**, `publish = false`, mirrored to `Ladvien/bevy_kimodo`. It is a Bevy-ecosystem library, so it takes the permissive licence like `bevy_orca` and `bevy_stigmergy`, not the GPL the `emerge-*` family carries.

```
crates/bevy_kimodo/
  README.md          "Vibe Coded" warning, mirror notice, Bevy compat table, Examples
  CLAUDE.md          the non-negotiables (below)
  NOTICE             skeleton tables are copied from NVIDIA's Apache-2.0
                     kimodo/skeleton/definitions.py — attribution belongs here
  LICENSE-MIT / LICENSE-APACHE
  src/
    lib.rs           #![doc = include_str!("../README.md")], KimodoPlugin
    protocol.rs      serde types mirroring kimodo.cpp /api/* verbatim
    backend.rs       trait MotionBackend + Capabilities + HttpBackend (ureq 3)
    skeleton.rs      SOMA-30 / SMPL-X-22 / G1-34 names, parents, offsets — data only
    motion.rs        the IR: [T,J] local quats, [T,3] root, fps, foot contacts
    retarget.rs      BoneMap, retarget pose, D_j correction, hip-height scale
    loopify.rs       cycle detection + seam blend
    inplace.rs       root strip -> cycle_distance / phase_offset
    clip.rs          Motion -> bevy AnimationClip, paths taken from the target
    bvh.rs           Motion -> BVH text (no dependencies)
    plugin.rs        KimodoJobs, poll systems, KimodoSystems
  examples/
    generate.rs      terminal: POST a prompt, print frames/joints/timing
    retarget_bvh.rs  terminal: motion -> named target skeleton -> write .bvh
    preview.rs       window: generated clip on a gizmo stick figure
  tests/leaf.rs      dependency ratchet
```

**Non-negotiables for its `CLAUDE.md`:**

- **The crate never names a game type.** `retarget` takes a `TargetSkeleton { names, parents, rest_local }` the caller fills in; `emerge-mapper` builds one from the GLB. No `Rig`, no `SlotDef`, no `Playback` in here — the `bevy_stigmergy`-never-learns-what-a-wall-is rule.
- **Generation is an editor-time act and never runs in the sim.** This repo has determinism goldens; a network call inside a deterministic run is exactly what they exist to catch. `docs/llm_rule_authoring.md` already draws this line for the VLM — draw the same one here.
- **The caller owns the schedule.** Expose `KimodoSystems`; add nothing to `Update` on the caller's behalf.
- **No `unwrap`.** `UnevenSampleAutoCurve::new` returns a `Result`; a backend that answers 500 is an ordinary Tuesday.
- **The dep ratchet is `bevy`, `serde`, `serde_json`, `ureq`.** Widening it costs a deliberate edit.

---

## 6. Phases

### Phase 0 — Spike, before any crate exists

Stand up kimodo.cpp's Go demo locally. `POST /api/generate` with `"model":"soma-rp-v1.1"` and a walk prompt. Pull `rotations.f32`. Write a **throwaway** script — Python is fine, this code is going in the bin — that applies the `D_j` correction onto the valkyrie's skeleton and writes a `.bvh`. Import it into Blender next to `valkyrie_walk`.

**The gate: does it look like the character, or like a puppet wearing her skeleton?** Everything below assumes the answer is yes. If the arms are inside out, the retarget pose calibration moves from Phase 2 to Phase 0 and the estimate roughly doubles.

Also measure here: seconds per 150-frame generation on your CPU, cold and warm. If it is minutes rather than seconds, the UI is a queue-and-notify design, not a press-and-wait one, and that changes Phase 4.

### Phase 1 — Crate scaffold, protocol, HTTP backend

`protocol.rs` + `backend.rs` + `skeleton.rs`. `ureq 3` on a task-pool thread — the `vlm.rs` pattern (`crates/emerge-mapper/src/vlm.rs:1220`), never the UI thread. `GET /api/models` on connect, so the licence and availability flags come from the server rather than from a constant. Example 1 runs in a terminal and prints frames and joint count.

Tests: protocol round-trips against captured JSON; the backend against a stub server; skeleton tables are internally consistent (every parent index in range, exactly one root).

### Phase 2 — Motion IR, retarget, BVH writer

The heart of it. `BoneMap` as data (a table per source skeleton, target names supplied by the caller), the rest-pose correction, hip-height root scaling, and a BVH writer.

Tests worth having: **identity retarget is a no-op** (source skeleton as its own target reproduces the input to float tolerance); rest-pose round-trip; a golden BVH; a bone map that names a joint the target lacks fails **loudly**, in the same spirit as the anchor-detection fix in `2026-08-05-animation-editor.md` — silence looks like a pass.

### Phase 3 — Bevy clip builder and preview

`clip.rs` builds an `AnimationClip` with paths read from the target GLB, and the bench stage plays it through `emerge_anim::rigs::build` alongside the rig's real clips — the same "one blender, not a copy" argument `anim_stage.rs` already makes. Example 3 is the windowed one.

### Phase 4 — The `Generate` sub-view

`anim_tab::View` gains a third variant beside `One` and `All`. New file `crates/emerge-mapper/src/anim_gen.rs`, panel furniture from `chrome`, so the tab costs a variant and a file rather than another copy of the panels.

What is on it: prompt box (multi-segment, since the API takes up to 16), model picker fed by `/api/models` with the non-commercial ones marked, frames/steps/seed, backend URL and status, a job list, and — on a finished job — preview on the stage, then **Export BVH** beside the character GLB.

Then it hands off. `anim_watch` sees the artist's re-export, `emerge_core::clips` re-measures, the bench offers adopt. Nothing new is needed on that side.

### Phase 5 — Making a generated clip usable as a *gait*

This is the phase that is easy to underestimate. A diffusion sample is not a loop and your bench will correctly refuse it.

- **Loop closure.** `clips::loop_closure` will flag every raw generation. Find the best cycle inside the sample by autocorrelating over foot contacts and joint angles, trim to a whole number of cycles, blend the seam. Kovar's Motion Graphs distance metric is the reference and it is already in your corpus.
- **In-place conversion.** Your gait clips must have a bit-zero root translation. Measure the net XZ displacement per cycle **first** — that *is* `cycle_distance` — then zero the channel. The number you would otherwise have to measure comes out of the generation for free.
- **Phase offset.** Same: derive it from the trimmed cycle rather than measuring it afterwards.
- **Foot skate.** Kimodo post-processes for skate on *its* skeleton; retargeting onto different limb lengths reintroduces it. Kimodo also emits `foot_contacts [T,4]` (left heel, left toe, right heel, right toe) — ground truth for an IK lock, and for the velocity-threshold contact labelling the bench already wants.
- **Masks.** The valkyrie's `aim` and `fire` ride mask group 0. A generated full-body clip has no notion of that; producing a mask-correct upper-body action means generating one and discarding the legs, which mostly works and should be checked rather than assumed.

### Phase 6 — The remote GPU sidecar

`scripts/kimodo_serve.py`, FastAPI, mirroring the kimodo.cpp contract exactly so `HttpBackend` is unchanged. This is where constraints can finally be supported, since the Python model takes them and kimodo.cpp does not — which means `Capabilities` earns its keep the moment this lands.

### Phase 7 — Loose ends the house rules require

`docs/animation.md` gains the generation path. `BEVY_GAME_INFO.md` gains a note for the 3D artists — Kimodo output is UE-mannequin-mappable, which is an argument for keeping new characters on that naming. `bevy_kimodo` goes into `CRATES` in `scripts/mirror_crates.sh`. `BACKLOG.md` items move to `BACKLOG_ARCHIVE.md` as they land. And per `CLAUDE.md`, the QD/RL question needs an explicit answer even if the answer is "generation is an authoring tool, it does not enter the evolving loop" — written down, not assumed.

---

## 7. Risks, ranked

1. **Retarget quality.** Everything else is plumbing. Phase 0 is the whole mitigation.
2. **Loop closure and skate (Phase 5).** The gap between "a clip plays" and "a clip is a gait your blender can drive" is most of the real work, and it is easy to schedule as an afternoon.
3. **Licence leakage.** kimodo.cpp defaults to the one model you must not ship. Set the model explicitly, render `/api/models`' `commercial` flag in the UI, and read the NVIDIA Open Model License terms on *model output* before shipping a generated clip. That last one is a decision for a human, not for me.
4. **Two backends, one interface.** CPU is prompt-only. Model it as capabilities, not as parity.
5. **SOMA-30 has no fingers.** The valkyrie has full finger chains and a `rifle` in `hand_r`. Generated clips will leave the hands at rest, which for a rifle-carrying character reads as wrong even when the body is right. The 77-joint expansion that would fix it is unimplemented in kimodo.cpp.

---

## 8. Open questions, for you not me

- **Retarget pose calibration.** Where does the target rig's reference pose live — a new file, a `bevy_kimodo` sidecar, or derived automatically by aligning bone directions to canonical axes? Automatic is cheaper and sometimes wrong.
- **Provenance for generated clips.** Should `rigs.ron`'s `Provenance` learn to say "this slot came from a prompt"? It is arguably the same argument that made the adopt stamp exist — but it is a schema change, which the chosen persistence path otherwise avoids entirely.
- **Where the BVH lands.** Beside the GLB in `assets/characters/`, or in a `generated/` staging directory outside the asset tree so a half-finished experiment cannot be picked up by a loader?

---

## Sources

- [nv-tlabs/kimodo](https://github.com/nv-tlabs/kimodo) — README, `pyproject.toml`, `kimodo/skeleton/`, `kimodo/exports/`, `kimodo/assets/skeletons/somaskel77/somaskel77_standard_tpose.bvh`
- [localai-org/kimodo.cpp](https://github.com/localai-org/kimodo.cpp) — README, `include/kimodo/kimodo_capi.h`, `demo/main.go`, `demo/skeletons_extra.go`, `src/skeleton.hpp`, `src/motion_rep.hpp`
- `bevy_animation-0.19.0/src/lib.rs`, `src/animation_curves.rs`; `bevy_gltf-0.19.0/src/loader/mod.rs`, `src/loader/gltf_ext/scene.rs`
- This repo: `CLAUDE.md`, `crates/emerge-core/src/rigs.rs`, `crates/emerge-core/src/clips.rs`, `crates/emerge-anim/src/rigs.rs`, `crates/emerge-mapper/src/{anim_tab,anim_stage,anim_watch,vlm}.rs`, `assets/emerge/rigs.ron`, `assets/characters/valkyrie.glb`
- `docs/research/2026-08-05-animation-editor.md` — the failure model and the ranking this plan hangs Phase 5 off
- [Animation Retargeting: Transfer Mocap Between Skeletons](https://mocaponline.com/blogs/mocap-news/animation-retargeting) and [T-Pose Fix in UE5 and Unity](https://mocaponline.com/blogs/mocap-news/tpose-animation-retargeting-fix) — the T-vs-A reference-pose problem as practitioners describe it
