# 2026-07-25 — mold bleed, pause-flicker, wall picket fence

Write-up for three player region-captures (`region_2026-07-25_12-22-14-113`, `_12-25-53-009`,
`_12-27-00-608`), now fixed and deleted per this directory's `CLAUDE.md`. All three were reported on
an M5 MacBook Air (Metal backend) and absent on the player's previous NVIDIA card; none turned out to
be an authored Mac-specific code path (the repo has zero `target_os`/`Metal`/backend/AA config
anywhere) — each was a real logic bug whose worst case happened to be more visible under Metal, or,
for the third, wasn't actually GPU-dependent at all.

---

## 1. "The mold/mycelium shader goes beyond the wall"

**Cause.** `mycelia_floor.wgsl`/`mycelia_wall.wgsl` warp the UV used to sample the floor/non-floor
`control_tex` mask before testing coverage, using `reveal_warp_amp = 0.012` UV units against
`WORLD_EXTENT = 192` — a worst-case world-space displacement of `0.5 * 0.012 * 192 ≈ 1.15` units,
about **8x** `WALL_THICKNESS = 0.14`. The config comment's claim of "no cross-wall bleed" was false by
construction: the warp was easily large enough to sample clean across a single-cell-thick wall into
the next room's floor texel.

**Fix.** Both shaders now take a second, *unwarped* sample of `control_tex` at the fragment's own
texel and multiply its thresholded value into the warped coverage — so the warp can still wander the
reveal edge within a floor cell, but can never promote a true non-floor texel to "covered," regardless
of warp direction. `assets/config/config.ron`'s comment was corrected to say so.

**Files:** `assets/shaders/mycelia_floor.wgsl`, `assets/shaders/mycelia_wall.wgsl`,
`assets/config/config.ron`.

---

## 2. "z-fighting... flickering even when the game is paused"

**Cause.** `setup_camera_fx` (`src/light.rs`) attaches Bevy's `ContactShadows::default()` to the
camera. Its raymarch shader dithers the ray's start offset with `interleaved_gradient_noise` seeded on
the raw render-frame counter (`globals.frame_count`), on the upstream assumption that TAA will average
that dither out across frames. This project runs no TAA, and rendering itself never stops even while
the sim is paused (`WinitSettings::Continuous`, so the window keeps rendering at full rate
unfocused/occluded) — so the dither kept changing every render frame regardless of `Time<Virtual>`
being frozen. Every actual gameplay/mold-driven signal in this codebase *is* correctly pause-gated;
`ContactShadows`' per-fragment dither was the one mechanism that wasn't, and it reads exactly like the
soft, ever-shifting dark region the player captured.

**Fix.** `linear_steps` bumped 16 → 48 (length/thickness unchanged, so the shadow's reach and softness
are unaffected) — more steps over the same ray length shrinks the per-step (and thus jitter) distance,
which is what actually suppresses the dither's visible amplitude. If a future GPU still shows visible
dither at this step count, the correct next lever is temporal accumulation (`TemporalAntiAliasing`),
not more steps — that's a project-wide decision (MSAA vs. TAA tradeoff), not a local tune.

Also fixed, as an independently real instance of the same class of bug: `BloodPoolMaterial`
(`src/gore.rs`) decals are `AlphaMode::Blend` with no `depth_bias` override, and pools/splats are
continuously despawned/respawned through `PoolRing`. Two pools stamped at the same or near-identical
world position tied exactly on Bevy's `Transparent3d` sort key, so which one blended on top was
decided by ECS extraction order — not stable across frames, per this project's own determinism rule.
Fixed with a small, stable per-decal position jitter keyed on each spawn's existing seed value, at all
three `BloodPoolMaterial` spawn sites (floor pools, wall splatters, droplet splats).

**Files:** `src/light.rs`, `src/gore.rs`, `src/mycelia/material.rs` (doc-only hardening — the mold
floor overlay itself was checked and ruled out: it's a singleton with a proven-safe `depth_bias`).

---

## 3. "Something is seriously messed up with this renderer. Maybe this is a bevy issue?"

**Cause — not the renderer.** The screenshot showed a long corridor wall alternating cleanly between
lit wallpaper and flat dark (void-colored) gaps at a regular per-tile period — the signature of
per-cell independent fog reveal, not GPU rasterization (a genuine, different z-fighting bug in
wall/corner meshing was already fixed the day before, commit `db12284`, and stays fixed — this was a
separate, still-open issue).

Every wall segment is revealed as a unit with the one floor cell it walls. `update_los`
(`src/fog.rs`) gates that reveal on `Dungeon::line_of_sight`, which — on a diagonal Bresenham
step — required *both* orthogonal neighbours to be floor, a correct rule for blocking peeking through
a true diagonal wall slit. But in a 1-cell-wide corridor, that "orthogonal neighbour" is, by
construction, the corridor's own bounding wall the moment a unit's sightline into it isn't perfectly
axis-aligned — which is most of the time. Every such diagonal step failed the rule, so every other
corridor floor cell (and its wall segment) never left `Unseen`.

Confirmed with a synthetic-corridor unit test before touching the fix: an off-row viewpoint into a
1-wide corridor reproduced exactly this pattern (`TFFFFF...`) under the strict rule.

**Fix.** Added `Dungeon::line_of_sight_reveal`, a fog-reveal-only variant that blocks a diagonal step
only when *neither* orthogonal neighbour is floor (a true closed diagonal pinch), not merely when one
of them is the corridor's own wall. `Dungeon::line_of_sight` itself is untouched and keeps the strict
rule — it's also used by pathfinding smoothing and the laser LOS gate, where corner-cutting is a real
gameplay exploit. `fog::update_los` is the only caller switched to the lenient variant.

**Pinned by** `src/dungeon.rs::line_of_sight_reveal_sees_down_a_corridor_that_strict_los_partly_blocks`
— a hand-built 1-wide-corridor fixture where strict LOS blocks two near cells purely on the
diagonal-corner rule (the picket-fence signature) and the lenient variant sees both, while a genuinely
occluded deep cell (the sightline's pure-x steps actually pass through solid rock before its single
y-increment) correctly stays blocked under both — proving the fix doesn't paper over real occlusion.

**Files:** `src/dungeon.rs`, `src/fog.rs`.

---

**Verification:** full `cargo test` and `cargo test --features test-harness -- --test-threads=1`, both
green (the harness run's one failure, `every_wired_figurine_keeps_a_well_formed_pose_blend_through_a_
live_run`, is pre-existing on unmodified `main` — confirmed by reproducing it on a clean stash before
any of these fixes — and is an unrelated animation pose-blend issue, not touched by this pass).
