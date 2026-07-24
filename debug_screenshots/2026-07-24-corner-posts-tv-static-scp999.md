# 2026-07-24 — corner posts, TV static/glow, SCP-999 spawn

Write-up for three player region-captures (`region_2026-07-24_13-00-12-363`, `_13-01-11-508`,
`_13-01-30-867`), now fixed and deleted per this directory's `CLAUDE.md`.

---

## 1. "Half of the corner pieces poke through the wall"

**Cause.** Two defects in `dungeon::spawn_tiles`, both about the same shared `WALL_THICKNESS²` column.

*The one the player saw:* corner **posts** were centred on the tile vertex
(`post_pos = ((cx+0.5)·T, H/2, (cz+0.5)·T)`) while every wall slab is inset flush — a wall's outer face
sits exactly on the tile boundary. So a post spanned `[V−0.07, V+0.07]` on both axes: half of it stuck
through the wall face into the void, and it filled only a *quarter* of the notch it existed to close.
Worked case — floor `(0,0) (1,0) (0,1)`, rock `(1,1)`: the walls leave a gap centred at **(0.43, 0.43)**
but the post spawned at **(0.50, 0.50)**.

*The one underneath it:* the greedy `CORNERS` pass consumed adjacent walled **pairs** as L-shaped corner
arms and left any third/fourth edge at full tile length. A cell walled N+E+W double-occupied
`x ∈ [−0.50,−0.36] × z ∈ [−0.50,−0.36]`; a fully-walled cell double-occupied two such columns — coincident
faces, so z-fighting at every dead-end cap and notched corner.

**Fix.** One uniform rule replaces the corner templates and the greedy state (`corner_arms` and `CORNERS`
are gone):

- an **E/W** slab always runs the full tile length along Z;
- an **N/S** slab is **trimmed** by `WALL_THICKNESS` at each end whose perpendicular E/W edge is walled.

The asymmetry is what makes it watertight: at a corner the E/W slab owns the shared column and the N/S
slab yields it. `edge_wall()` is the single place that rule lives (it also returns the trimmed-end count,
which indexes the pre-built mesh set). `corner_post()` then decides per *quadrant* of a vertex and insets
the post into the floor side, choosing the side from **which cell owns the adjacent slab** — not from
which cells are floor, since those disagree at a diagonal pinch.

**Pinned by** (`src/dungeon.rs`, GPU-free):
`walls_of_a_cell_never_overlap_and_leave_no_gap` (all 16 wall subsets: slabs pairwise disjoint, and both
corner columns of every walled edge covered), `corner_posts_are_inset_flush_and_never_overlap_a_wall`
(all 16 floor/rock arrangements around a vertex), and `concave_corner_post_sits_at_the_notch_not_the_vertex`.

All three were confirmed **non-vacuous** by deliberately breaking the code three ways: never-trim
reproduced the overlap, always-trim reproduced a bare corner, and a vertex-centred post reproduced the
reported bug verbatim (`post [1.43,1.57]` against a home cell ending at `1.5`).

---

## 2. "What happened to the TV glow and static?"

**Cause — a system race, not a lighting bug.** `mycelia::coat_furniture` walks every descendant of every
`PlacedIn` root and does `remove::<MeshMaterial3d<StandardMaterial>>().insert((MeshMaterial3d(coated),
MoldCoated))`. `light::glow_screens` looks for that *same* component on the TV's CRT face to swap in the
unlit `TvStaticMaterial`. Both are plain `Update` systems with no ordering between them. Whenever mold
won, the face was marked `MoldCoated`, `glow_screens` could never see it again (and re-scanned that TV
every frame for the rest of the run), and the screen stayed a flat lit teal panel. The unlit material
supplies **both** the static and the self-glow, which is why one race lost both.

Ruled out first: the classifier and the spot direction were fine. `TV A.glb`'s screen material
`baseColorFactor [0.0415, 0.2303, 0.2344]` passes `glow_screens`' chromatic test (`0.465 > 0.175`), and the
screen quad's normal is `+Z`, so the `PI` yaw-flip in `attach_screen_lights` beams the spot out the front.

**Fix.** `coat_furniture`'s roots query gains `Without<crate::light::LightEmitter>` — every emitting prop
(tube, sconce, lamp, TV) carries it via `affords("emit")`. Deliberately a *static* filter rather than a
marker plus `.before()`: `Commands` are deferred, so a marker written by the lighting would not be visible
to mold in the same frame anyway. Cost: mold no longer climbs lamps or a TV chassis.

**Verified in the running game.** A temporary probe reported
`screen_emitters=2 · spotlight=true · static_applied=true · meshes: standard=2 tv_static=1` — the CRT face
carries `TvStaticMaterial`, the grey chassis and bezel correctly do not. Measured on the rendered frame,
the screen's mean horizontal gradient went **1.50 → 50.27** (it had been the *flattest* surface in the
player's capture, flatter than the wallpaper); brightness 155 → 196, sd 15.5 → 68. That is real self-lit
snow. The probe was removed afterwards.

---

## 3. "Randomise SCP-999's start; it shouldn't start next to the squad"

**Cause.** `scp999::spawn_scp999` deliberately fanned blobs around `dungeon.spawn` with `dz` starting at 1
— one tile behind the squad. The doc comment called it intentional ("starts *with* the squad").

**Fix.** The same deterministic greedy far-from-spawn scan the enemies and crab nests already use: skip any
floor cell closer than `sim.scp999.spawn_min_dist` to the squad spawn, keep accepted cells `SPAWN_SEP`
apart, `warn!` loudly and place nothing if the level offers no such cell. `spawn_min_dist` is a **new
evolvable dial** (shipped 18.0), wired through `Scp999Tuning` → `config.ron` → `validate_tuning` → the
world genome (`N` 106 → 107, one new `BOUNDS` row, encode + decode). No saved world elites are checked in,
so nothing needed regenerating.

With `count = 1` and no RNG the cell is fixed for a given level layout — deterministic and far from the
squad, with `spawn_min_dist` as the lever that moves it. The dead `nearest_floor` helper was deleted.

**Pinned by** `tests/scp999.rs::the_comfort_blob_starts_out_in_the_level_not_beside_the_squad`.

**Goldens did not move.** Both `tests/replay.rs` baked hashes (`GOLDEN`, `GOLDEN_FIELD`) pass unchanged —
in that no-threat seed the squad's FEAR never rises, so the tickle-calm is a no-op either way, and wall
geometry is not folded into `snapshot_hash` (it folds `(Transform, Health)`; wall tiles have no `Health`).
`GOLDEN_DUNGEON` hashes the grid, which `spawn_tiles` does not touch.
