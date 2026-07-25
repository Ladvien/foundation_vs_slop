# 2026-07-25 — wall/floor mold seam and clustered sconce flicker

Write-up for two player region-captures (`region_2026-07-25_13-50-08-279`, `_13-52-28-448`), now
fixed and deleted per this directory's `CLAUDE.md`. Follow-up to the same day's earlier fixes (see
`2026-07-25-mold-bleed-pause-flicker-picket-fence.md`).

Three other captures from the same session (`region_2026-07-25_13-25-23-172` "mold seep under walls
into solid space", `_13-26-31-955` "walls are all red", `_13-38-32-992` "checkerboard flickering near
a nest") were investigated and given a defensive NaN-sanitizing guard in the mold shaders
(`no_nan()` in `mycelia_wall.wgsl`/`mycelia_floor.wgsl`) on the theory that a numerically unstable
simulation frame was reaching the shared wall material as a raw NaN pixel. None of the three recurred
in the player's next play session, but that was never explicitly confirmed fixed (the mechanism was a
best-effort hypothesis, not a bisected root cause the way the two below are) — those three captures
are deliberately **left in place**, not deleted, until confirmed.

---

## 1. "Mold is still on the side of the wall it shouldn't be. Fine on exterior walls, not on the floor
##    beneath it."

**Cause.** `mycelia_wall.wgsl` sampled the mold biomass field at a point `WALL_FOOT_OFFSET = 0.19`
world units (a full field texel) into the room from the wall's visible face, while the floor shader
sampled at its own bare fragment position — which, since the floor plane extends fully under the
wall's footprint, sits essentially *at* the wall's face. That ~0.19-unit gap between what the wall
paints and what the floor at that same visual spot shows was the mismatch.

Git history showed `WALL_FOOT_OFFSET` and the simulation's proper no-flux wall boundary (`bio_flux` in
`mycelia_sim.wgsl`) were introduced together in one commit (`762ffbf`). The offset's file-header
justification ("used to be drained dry by the leaking diffusion") described a problem that same
commit's no-flux fix already solved — stale reasoning carried forward. Its other, still-valid
justification (in the constant's own comment) was real but smaller: the field texel grid isn't
aligned to the dungeon cell grid, so sampling exactly at the wall's face risks landing in a
solid-classified texel. That only needs sub-texel clearance.

**Fix.** Shrunk `WALL_FOOT_OFFSET` from `0.19` to half a texel width (`0.09375`) — enough to clear the
texel-aliasing the constant exists for, without sampling far enough into the room to visibly disagree
with the floor at the seam. Updated both the constant's comment and the file header to drop the stale
"drained dry" framing.

**Files:** `assets/shaders/mycelia_wall.wgsl`.

---

## 2. "Tons of flickering near the crab nest... the wall sconces."

**Cause.** `attach_fixture_lights` (`src/light.rs`) gave every fixture (sconces, desk lamps) an
independent `failing: bool`, decided by hashing its own world position against
`flicker_fail_ratio` (0.12, "~1 in 8 tubes"). Nothing capped how many could be `failing`
simultaneously in one room. In a typical 6-sconce room the chance of two or more simultaneous
failures was already ~15%; on a level whose evolved `wall_lights_per_room` gene rolled toward the
high end of its `(0, 16)` range, a dense room's odds climbed well past 50%. Nests get no deliberate
extra fixture density — the reported room just caught this binomial tail.

**Fix.** Added `flicker_max_failing_per_room` to `LightingConfig` (default `1` in `config.ron`,
validated in `light.rs`), and made it an actual tunable rather than a hardcoded rule per the player's
explicit request. `attach_fixture_lights` now computes every fixture's roll first, groups fixtures by
room (`PlacedIn`/`RegionId` — the same association `furnish.rs` already tags them with), and — using a
stable hash tie-break, not iteration order — allows only the configured number of simultaneous
failures per room, forcing the rest to a steady hum regardless of their own individual roll.

**Files:** `src/light.rs`, `assets/config/config.ron`.

---

**Verification:** full `cargo test` (530 lib tests) and `cargo test --features test-harness` both
green. Running the full `tests/replay.rs` suite surfaced an unrelated but real consequence of the
*earlier* fog-of-war LOS fix (not these two fixes): the corrected reveal legitimately shifts crab
perception timing, moving the `GOLDEN_FIELD` stigmergy-field oracle (the actor golden did not move).
Bisected, confirmed stable across repeated runs, and re-pinned with a dated justification comment in
`tests/replay.rs`, matching that file's established convention for this exact situation.
