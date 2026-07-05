# Dev Journal: 2026-07-05 - Dimensional Crab Swarm

**Session Duration:** ~1 long session (multi-hour, several feedback rounds)
**Walkthrough:** None

## What We Did

Two chunks of work.

**Warm-up tweaks (smiley boss + audio):**
- Turned the smiley enemy into a single boss: `ENEMY_COUNT 6→1`, `START_HP 400→2400`, much slower (`MAX_SPEED 7→2.5`, `MIN_SPEED 0.8→0.4`, `ACCEL 1.5→0.4`), hits much harder (`CONTACT_DPS 12→72`, `CONTACT_RADIUS 0.9→1.2`). All in `src/enemy.rs`.
- Fixed footsteps that "sounded like 50 people": the four `carpet_*.ogg` files were the **same 8-second multi-step recording** fired every 0.12–0.5 s and overlapping into a crowd. Replaced with real ~0.5 s single footfalls (low-passed for a dull carpet tone), lowered `FOOT_VOL`, raised `MIN_STRIDE`.
- Swapped the click-to-move sound to a soft pop.

**The main event — `dimensional_crab` swarm enemy:**
A ~40-strong swarm of small animated crabs that navigate floors *and walls*, converge on the squad, and swarm/eat units like piranhas. New modules `src/surface_nav.rs` and `src/crab.rs`, plus integration edits across `dungeon.rs`, `laser.rs`, `audio.rs`, `health.rs`, `gore.rs`, `squad.rs`, `main.rs`.

Delivered across several feedback rounds:
1. Wall-capable navigation graph + shared flow field (surface pathfinding).
2. Skinned glTF rendering + animation (walk/idle/attack).
3. Piranha latch-and-eat: crabs climb onto units, spread over the body, bite.
4. Exponential stacked damage, bounding-box separation, back-slot preference, front-arc-only shooting, friendly fire.
5. A synthesized "squittering" swarm sound (density-throttled shared voice).
6. Per-enemy-scaled screenshake / blood-lens (crab deaths barely nudge the camera; boss death is a full kick).

## Bugs & Challenges

### "50 people walking" footsteps

**Symptom:** A 5-unit squad's footfalls sounded like a crowd.

**Investigation:** `ffprobe` on the footstep assets.

**Root Cause:** All four `carpet_*.ogg` were an identical **8.07-second recording of ~10 footsteps**, and the audio system fires one as a one-shot every 0.12–0.5 s — so dozens of 8-second multi-step clips overlapped.

**Solution:** Regenerated the four files from real single-footfall foley (~0.5 s each), low-passed for a dull tone; lowered volume and raised the cadence floor.

**Lesson:** When many-of-a-thing sounds wrong, check the *asset duration*, not just the trigger logic. A one-shot system assumes short clips.

### Walk animation "WAY too slow / legs not moving"

**Symptom:** Crabs moved but their legs looked static or crawled.

**Initial Hypothesis:** Animation wasn't wired up, or skinning wasn't applied.

**Investigation:** Added a temp diagnostic logging, per crab: wired-player count, state distribution, and the `AnimationPlayer`'s active-animation `seek_time` + weight over time. Result: **all 40 crabs wired, all in Walk, seek_time advancing at the set rate, weight 1.0** — the animation was 100% working. Then parsed the glb's animation accessors.

**Root Cause:** The walk clip is **10.5 seconds long per loop**. At `set_speed(2.5)` one leg cycle still took ~4 s.

**Solution:** `WALK_ANIM_SPEED = 7.0` (attack `4.0`). The clips are long, so they need to be played several × faster to read as a scuttle.

**Lesson:** Before assuming a system is broken, *measure it*. The diagnostic proved the pipeline worked and pointed straight at clip duration. Long authored clips need aggressive `set_speed`.

### Full wall pathfinding with no 3D nav graph

**Symptom:** Requirement was crabs navigate *across* vertical walls, but the world is a flat floor grid + derived wall slabs with no navmesh.

**Root Cause:** `flowfield.rs` is 2D-floor-only; walls aren't even stored (they're derived from floor↔non-floor edges).

**Solution:** `surface_nav.rs` — a `SurfaceGraph` whose nodes are floor cells **and** wall faces (one patch per walled edge), with edges for floor↔floor, floor↔wall-base (mount), wall↔wall along a run, and convex/concave corners. A multi-source Dijkstra `SurfaceField` (mirroring `EnemyField`) gives every patch a down-gradient neighbor toward the nearest unit; `Arc`-shared by all 40 crabs. Since every wall base links to its floor cell and the floor reaches the squad, every surface point has a descending path home (no local minima).

**Lesson:** You can lift a 2D flow field onto a 2.5D "surface manifold" of axis-aligned rectangles without a general navmesh — the tangent frames are trivial because every surface is axis-aligned. Wall-top crossings turned out to have **no valid geometry** here (single-sided perimeter walls, rock cells between rooms) — documented honestly rather than faked.

### Screenshots: window occlusion + VHS glitch

**Symptom:** `devshot` screenshots came back either black (~57 KB) or covered in chromatic-glitch smear.

**Investigation:** Black = the macOS window lost its Metal drawable when occluded behind other windows (only the *first* capture after window creation is reliable). The color smear = the VHS post-FX (`vhs.rs`) runs a **periodic 45 s glitch spike**; I kept capturing during it. Disabling the VHS/blood-lens plugins outright *panicked* (a resource `ensure_camera_settings` inserts went missing).

**Solution:** For calibration frames, wrote a temporary `vhs_fx.ron` config with all effect strengths = 0 (the shader is an exact passthrough at 0) instead of removing the plugin. Still limited to one real frame per run due to occlusion.

**Lesson:** This environment can't reliably screenshot the game. Verify behavior **numerically** (state/HP/height/latch counts via temp diagnostic systems) — it's more reliable than fighting the window, and often more precise. Neutralize post-FX via config, never by yanking a plugin other systems depend on.

### Piranha crabs never reaching the squad in tests

**Symptom:** Debug logs showed `attack=0` — no crabs latching — even after 8 s.

**Root Cause:** The nest-placement greedy scan picks the first scan-order-qualifying floor cells (top-left of the map), which are far from wherever the squad spawned. Lowering `CRAB_MIN_SPAWN_DIST` didn't help — the scan still favors early cells.

**Solution:** For verification, spawned a temp cluster directly at `dungeon.spawn`. Then `attack=18`, `max_y≈0.95` (climbing full body height), `climbing≈22` — piranha behavior confirmed.

**Lesson:** "Spawn near X" isn't the same as "spawn at X" when a greedy scan is involved. To test convergence behavior, place agents *at* the target, don't rely on min-distance relaxation.

### Exponential damage = instant death

**Symptom:** With `base 6 × count^1.6`, a full pile did ~700 DPS — units died in <0.2 s, defeating the "watch the swarm" goal.

**Solution:** Softened to `base 3 × count^1.5` (≈33 DPS at 5 crabs, ≈95 at 10, ≈270 at 20). Verified a swarm dropped a unit 90→5 HP over ~4 s.

**Lesson:** Super-linear curves blow up fast. Sanity-check the curve at representative N values (1/5/10/20) before committing the constants.

## Code Changes Summary

- `src/surface_nav.rs` (new): `SurfaceGraph` (floor+wall patches, 5 edge families) + `SurfaceField` (multi-source Dijkstra flow) + `CrabField`.
- `src/crab.rs` (new): `CrabPlugin` — spawn (4 nests, 2 wall-seeded), `crab_locomotion` (surface flow-field mode + piranha latch mode with body-relative back slots + Reynolds separation via spatial hash), animation graph plumbing, exponential contact damage, death gore, `CrabAttached` link.
- `src/dungeon.rs`: made `walled` pub, exposed `WALL_HEIGHT`.
- `src/enemy.rs`: single-boss stats; added shared `Hostile` marker; `hide_enemies_in_fog` → `With<Hostile>`; per-death `intensity` on gore.
- `src/laser.rs`: hit/target queries → `Hostile`; front-arc targeting gate; friendly-fire roll on shooting an attached crab; **`LASER_DAMAGE` currently 0.2 (1/50, TEMP)** per user request to observe the swarm.
- `src/audio.rs`: footstep/move-order swaps + tuning; `crab_squitter` density-throttled voice; `update_music` → `Hostile`.
- `src/health.rs`: `NoHealthBar` opt-out (crabs suppress their bars).
- `src/gore.rs`: `GoreEvent.intensity` + `death_intensity(hp,dps)` so screenshake/hitstop/blood-lens scale by the dead thing's mass; gib visuals unscaled.
- `src/main.rs`: registered `crab`/`surface_nav` modules + `CrabPlugin` (nested with `EnemyPlugin` to stay under the 15-plugin tuple cap).
- Assets: regenerated `carpet_*.ogg`, `move_order.ogg`; synthesized `enemy/squitter.ogg` (ffmpeg native `vorbis -strict -2`, since libvorbis is absent).

## Patterns Learned

- **Surface flow field**: generalize a 2D grid flow field to a graph of axis-aligned surface patches; one Dijkstra, `Arc`-shared by all agents — global nav is O(patches) per goal-change regardless of agent count.
- **Shared marker for cross-cutting concerns**: `Hostile` on both boss + crabs drives laser/fog/music uniformly, while type-specific AI stays on `Enemy`/`Crab`. DRY without collapsing behaviors.
- **Body-relative attachment slot**: store an agent's slot as an angle *relative to the host's forward* so it rides along as the host turns/moves (cling), and can be biased (toward the back) for gameplay.
- **Per-event feel intensity**: add an `intensity` field to a shared event (gore) so the same pipeline scales screenshake/juice by the source, instead of a flat spike per event.
- **Numeric verification over screenshots**: temp diagnostic systems logging counts/heights/HP are faster and more reliable than fighting an occluded window.
- **Density-throttled shared audio voice**: one voice whose cadence scales with N (footsteps, squitter) — never one voice per entity (that's the "50 people" bug).
- **Two disjoint `&mut` queries**: `Query<&mut Health, With<Hostile>>` + `Query<&mut Health, (With<Unit>, Without<Hostile>)>` coexist because the filters are archetype-disjoint.

## Open Questions

- **Guns are at 1/50 power** (`LASER_DAMAGE = 0.2`). Front-arc (#3) and friendly-fire (#4) are barely observable at that power because units rarely shoot crabs off. Restore to `10.0` (or a middle value) to actually see those mechanics.
- Wall-crawling is *capable* but crabs prefer the floor because units live on the floor (cheapest path). Only the wall-seeded nests exhibit lots of wall travel. A cost bias could make crabs prefer walls if we want it more visible.
- All tuning was verified numerically, not by eye (window occlusion). Scale/seat, animation speeds, back-spread arc, friendly-fire chance/damage, and the damage curve are best-guess constants — need a live playtest.
- Concave corners in the surface graph route via the floor connector rather than around the wall inside-corner; acceptable but not "true" wall wrap there.

## Next Session

- Restore `LASER_DAMAGE` (or set a balanced value) and playtest the front-arc + friendly-fire dynamics live.
- Eyes-on tuning pass on the crab once screenshots are viable: size/seat at 0.06 scale, walk-anim legibility (may need >7×), back-slot spread, squitter tone.
- Consider a wall-preference cost bias if more visible wall-crawling is wanted.
