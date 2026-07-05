# Dev Journal: 2026-07-05 - Autogib & Physics Gibs

**Session Duration:** ~one long session (multi-hour, several rebuilds incl. a ~6 min avian first-build)
**Walkthrough:** None

## What We Did

Three connected pieces of work on the gore/juice layer, plus two small quality-of-life tweaks.

1. **Autogib — procedural mesh fracture (`src/autogib.rs`, new).** Replaced the old procedural-cube
   gibs with real fragments cut from a character's own mesh. Approach chosen after reading the
   home-still corpus (Müller/Chentanez/Kim 2013 VACD; Sellán et al. 2022 "Breaking Good" fracture
   modes): both need machinery this project lacks (physics solver / offline tet-solve toolchain), and
   both name the realtime-appropriate standard as **pre-fracture once, swap in at runtime**. So we
   implemented the geometric plane-cutter prefracture family:
   - A pure, unit-testable triangle-soup slicer: Sutherland–Hodgman triangle clipping against a plane
     → welded boundary-loop assembly → fan-triangulated **watertight caps with planar cross-section
     UVs**. Recursive random-plane fracture into ~N fragments, count/size scaled off the mesh bbox.
   - Mesh-agnostic on purpose (the figurine is only a greybox; a real, possibly skinned character
     comes later): discovers body geometry by walking whatever scene the unit loads (skipping the
     gun), synthesizes missing normals/UVs, keys its cache off the source asset id (swap the GLB =
     zero code change), and reserves a documented `BakePose::Static` seam for a future skinned
     pose-bake (cites Wang et al. 2007).
   - Fragments show a **bloody meat cross-section**: two child meshes per chunk — outer skin (outfit
     tint) + cut faces (shared raw-meat material). The blaster flings off as one intact chunk.
   - 7 `#[cfg(test)]` slicer tests (cube slice, cap area, determinism, missing UV/normal, open-loop
     drop, degenerate plane).

2. **Physics gibs (avian3d).** User asked to "switch to using physics to have the gibs bounce about"
   and "keep it limited." Added `avian3d` (XPBD rigid-body engine; Müller et al. 2020). Scoped
   tightly: only gib chunks (fragments, gun, meat) are `RigidBody::Dynamic`; only the floor (one
   static half-space) and walls (per-wall cuboid colliders) are static bodies. Units/enemies/lasers
   stay entirely on their own custom systems. Removed the hand-rolled `update_gibs` integrator +
   shrink-out; chunks now tumble, bounce off each other and the room, settle, and sleep — capped at
   200 via a ring. Verified visually: fragments scatter and settle into a pile, guns come to rest,
   contained by walls.

3. **Scale + sound.** Bumped `FIGURINE_SCALE` 1.4 → 1.8 so people stand a bit taller than the 1.0
   walls / closer to enemy scale. Added a `GlobalVolume` master multiplier (`MASTER_VOLUME = 0.15`)
   to turn the whole game's audio down (user was watching a movie).

## Bugs & Challenges

### Autogib bake cached zero fragments (premature "done")

**Symptom:** Log showed `autogib: source has no usable body meshes; marking baked (no fragments)`,
and deaths produced no fragment gibs.

**Initial Hypothesis:** Asset path problem (we also *did* have one — see next).

**Investigation:** The bake walks a unit's `Children` to collect `Mesh3d` handles, and self-gated on
"all found handles present in `Assets<Mesh>`" (`all_loaded`). But before the GLTF scene instantiates
its descendants, the unit has **no** body `Mesh3d` children at all — so the collected soup was empty
*and* `all_loaded` stayed true (there were no handles to find missing). It marked the source baked
with an empty fragment set on the very first frame.

**Root Cause:** Conflated "no meshes found" (still streaming) with "genuinely no geometry."

**Solution:** Retry until the body soup is non-empty (mirroring how `recolor_units` retries until it
finds meshes); only mark baked on genuinely-degenerate geometry (zero extent).

**Lesson:** With async scene loading, "found nothing" ≠ "done." Gate on a positive signal (meshes
present), not the absence of a failure.

### Running the binary directly broke asset resolution

**Symptom:** `Path not found: target/debug/assets/...` for every asset; figurine never loaded.

**Root Cause:** Running `./target/debug/foundation_vs_slop` resolves the asset root relative to the
binary. The project expects the working dir = project root.

**Solution:** Always launch with `cargo run` for runtime verification.

### Physics colliders vs `Transform.scale`

**Symptom (anticipated):** The autogib fragments are baked in figurine-local units and the render
scale (`FIGURINE_SCALE`) was applied via the parent `Transform.scale`. avian colliders are defined in
their own local dims and don't necessarily track `Transform.scale`, risking a collider/visual
mismatch.

**Solution:** Put the render scale on the **child mesh** (`Transform::from_scale`) and keep the
parent rigid body at scale 1, with the collider sized in true world units (`half_extents * scale`).
No scale ambiguity for the solver. Also switched the fragment's local origin from vertex-centroid to
**bbox center** so a centered box collider lines up with the geometry.

### `update_gibs` shrink assumed a unit-cube mesh

**Symptom:** The old shrink-out did `tf.scale = splat(gib.half * 2.0 * frac)`, which only works when
the mesh is a unit primitive. Real fragments are world-sized, so this would have rescaled them wrong.

**Solution (interim):** Added a `base_scale` field so shrink lerps from the right size. **Then**, when
moving to physics, dropped the whole custom integrator + shrink — avian owns motion, chunks are
permanent+capped, so there's no shrink/pop at all.

**Lesson:** A field added to patch one design can become dead weight when the design shifts; delete it
rather than carry it.

### Devshot screenshots came back black (~57 KB)

**Symptom:** Some captures were exactly ~57 KB (the documented "occluded window → black drawable"
size).

**Solution:** Retry the `touch screenshot.request` capture in a loop until the PNG is > 150 KB (a
real frame). Works because `WinitSettings` renders continuously even unfocused; only a *fully*
occluded window goes black.

## Code Changes Summary

- `src/autogib.rs` (new): pure plane-slicer (`Soup`/`split_soup`/`clip_half`/`cap_side`/`fracture`),
  Bevy adapters (`append_mesh`/`soup_to_mesh`), `AutogibCache` + `bake_autogib` + `AutogibPlugin`,
  tests. Later: `Fragment`/`GunChunk` switched to bbox `center_local` + `half_extents` for colliders.
- `src/gore.rs`: added `GibSource` to `GoreEvent`; replaced cube `spawn_gibs` with `spawn_fragments`
  (two-mesh chunks + gun); then converted all chunks to avian bodies via a shared `spawn_gib_body`
  helper; removed `Gib`/`update_gibs`; unified caps under `GibRing`/`cap_gib_chunks`; reworked
  `GoreSettings` (dropped `gib_gravity`/`gib_lifetime`/`gib_count`/`gib_size`, added `gib_friction`,
  renamed `max_meat`→`max_gibs`).
- `src/dungeon.rs`: static half-space floor collider + per-wall cuboid colliders (`spawn_tile` now
  takes `Option<Vec3>` collider size).
- `src/main.rs`: `PhysicsPlugins::default()` + `Gravity(GIB_GRAVITY)`; registered `AutogibPlugin`.
- `src/squad.rs`: `pub GunModel`; death site fills `GoreEvent.gib`; `FIGURINE_SCALE` 1.4→1.8.
- `src/enemy.rs`, `src/laser.rs`: `gib: None` on their `GoreEvent`s.
- `src/audio.rs`: `GlobalVolume` master multiplier (`MASTER_VOLUME = 0.15`).
- `gore.ron`: synced tuning knobs (autogib_*, physics friction/restitution, `max_gibs`).
- `Cargo.toml`: added `avian3d`.

## Patterns Learned

- **Bake-once, swap-at-runtime prefracture**: the shipped-game standard for destruction — precompute
  fragments, cache keyed by source asset, instantiate on the event. Avoids runtime fracture cost.
- **Pure core + thin Bevy adapters**: keeping the slicer free of Bevy types made it unit-testable
  without an `App`. Only `append_mesh`/`soup_to_mesh` touch `Mesh`.
- **Scale on the child, physics on the parent**: sidesteps collider-vs-`Transform.scale` mismatches —
  the rigid body stays at scale 1 with world-unit colliders; the mesh child carries the render scale.
- **Retry-until-positive-signal for async assets**: gate on "meshes present," never on "no failure."
- **Scoped physics**: you don't have to make the whole game a physics sim — a handful of dynamic
  bodies + a few static colliders gives the effect while everything else keeps its bespoke systems.

## Open Questions

- Gib chunks currently pass *through* units/enemies (those aren't physics bodies) — intended for the
  "keep it limited" scope, but we may want actor colliders later so gibs bounce off the living.
- Fragment colliders are boxes (cheap). avian has `Collider::convex_hull_from_mesh` for snug tumbling
  if we want more realistic settling later.
- The skinned bake path (`BakePose::Skinned`) is reserved but unbuilt — needed when the real
  (skinned) character mesh replaces the greybox figurine.
- `avian3d` adds parry3d/nalgebra; first build is ~6 min. Fine, but worth remembering.

## Next Session

- Swap the greybox `figurine.glb` for the richer character mesh and confirm autogib "just works" (it
  should — the cache is asset-id-keyed). If that mesh is skinned (like `dimensional_crab.glb`),
  implement the `BakePose::Skinned` pose-bake seam.
- Optionally: actor colliders so gibs collide with the living; convex-hull fragment colliders; tune
  `gib_friction`/`chunk_restitution`/`GIB_GRAVITY` for feel.
