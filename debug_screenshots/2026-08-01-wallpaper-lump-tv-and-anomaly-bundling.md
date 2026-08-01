# 2026-08-01 — wallpaper, the green dot, the TV in the wall, and the cornered anomalies

Write-up for five player region-captures from one session (`region_2026-08-01_21-20-45-563`,
`_21-21-03-938`, `_21-23-36-462`, `_21-24-09-476`, `_21-25-30-346`). Fixed and deleted per this
directory's `CLAUDE.md`.

Two captures from the same session are **not** covered here and were deliberately left in place:

- `_21-19-22-273` (*"This is what the site looks like."*) — the Site layer, owned by other work in flight.
- `_21-21-51-945` (SCP-610) — **half fixed**; see §5. The T-pose the note describes is still real, so the
  pointer stays until the asset lands.

---

## 1. "I still have backrooms carpet with 'concrete' walls (looks more like marbel)."

**Cause — `mycelia::coat_walls` repainted every wall in the map with one arbitrary biome's material.**

```rust
// Every wall shares one `StandardMaterial` handle, so read the base off whichever we see first ...
let Some((_, first)) = walls.iter().next() else { return; };
```

That comment was true before biomes shipped and false since. `dungeon::render::spawn_tiles` builds
**two** wall materials — `wall_mats[0]` the Backrooms wallpaper, `wall_mats[1]` the concrete — and keys
each slab on `dungeon.biome(cell)`, so a room and its own walls agree by construction
(`layout::resolve_biomes`, FVS-Q-8). `coat_walls` then read the base off `walls.iter().next()` — an
arbitrary **ECS query result** — and stamped that single coated material onto every wall entity. Floors
were untouched, so the level came out with correct per-zone carpet under one wrong wall texture
everywhere. That is `CLAUDE.md`'s "ECS query order decides nothing" rule, in the render layer: query
order was choosing the entire level's wall treatment.

**The yellow wallpaper was never missing.** `assets/textures/backrooms-wall-diffuse.png` is an
olive/yellow damask — vertical stripes, chevron motifs — exactly the Yellow Wallpaper the player named.
It was being painted over.

**Fix.** Key the coated material by its **base handle**, sharing the `CoatedFurniture` cache that the
sibling `coat_furniture` already used correctly. One `MoldWallMaterial` per distinct base, not one per
map.

**A second bug fell out of the same function.** The old `Local<bool> done` latch outlived the run. Tiles
are `run_scoped()` and `spawn_tiles` moved to `OnEnter(RunState::Active)` (FVS-N-13), so every
expedition after the first spawned fresh walls the latched system never looked at — **no mould on any
wall from run 2 onward**. Replaced with a per-entity `MoldCoated` marker, so the query empties on its
own and refills when the next run's tiles spawn. `reset_coated_cache` clears the per-run wall material
keys so the cache cannot accumulate two dead entries per expedition.

**Verified in the running game.** Captured frame: olive damask walls over olive carpet at camera cell
(80, 112), which `debug_screenshots/fps_trace.csv` confirms is `Backrooms` biome — wall matching its own
zone again.

## 2. "…(looks more like marbel)" — the concrete itself

The player was right, and measurement backs it. The shipped Ground 0046 measured mean srgb
0.599/0.595/0.579 with an albedo standard deviation of **0.019** — a pale, near-featureless wash, which
is what reads as polished stone rather than as a Foundation facility.

Replaced with **ambientCG Concrete 028** (CC0), board-formed architectural concrete: vertical plank
seams, panel joints, pour staining. Measured on the shipped file: 0.395/0.395/0.394, sd **0.030** — 33%
darker and 58% more surface variation. It also ships *authored* AO/roughness/normal maps, so
`concrete-orm.png` now packs real AO + roughness instead of relief inferred from luminance by
`scripts/derive_surface_maps.py`.

The diffuse is grey-world channel-balanced (chroma 0.031 → 0.001) to strip a warm cast — a channel
balance rather than a saturation crush, because crushing saturation would flatten the pour staining that
makes it read as concrete. The desaturation constraint is deliberate and documented: per
`docs/lore/2026-07-12-scp-color-language.md` §6 the concrete zone is the **desaturated counterweight** to
the Backrooms yellow, so the less chroma it carries the better. At 0.001 the new set is *more* neutral
than the one it replaces.

> Also corrected in `assets/textures/CREDITS.md`: the numbers that file quoted for Ground 0046
> (0.518/0.518/0.517, chroma ~0.001) did not match the shipped 1024² JPEG. They were evidently taken
> from the 2K source before the downscale and JPEG pass.

⚠️ **Not yet seen in-engine in a Concrete zone.** The squad's start is Backrooms on the shipped seed and
camera movement needs input injection, which is blocked in this environment. The asset is verified on
disk (viewed, measured, format-matched); its appearance under game lighting wants a look next run.

## 3. "When the crabs pick up food it turns into a green dot"

**Not a carry visual — the player had correlated two coincident events.**

Gibs keep their meatpack mesh through the entire haul. `crab::foraging::carry_gibs` flips the chunk
`Dynamic → Kinematic` and drives its `Transform`; nothing is spawned, hidden or re-materialed, and the
chunk floats *ahead of* the crew rather than riding on a crab's back.

The green sphere was `parasite::InfestationLump` — the SCP-150 **gestation tell**, an olive emissive
`Sphere` parented to any infested host. The player's own capture proves it: the "carrying" crab reports
**+720 triangles / +1 primitive** over its siblings, and 720 is exactly Bevy's default ico-sphere. The
crab was pregnant, not hauling.

**Fix (player's call: "make it a lump actually on the mesh. Red and swollen").**

The old lump sat at one shared `LUMP_LOCAL = (0.0, 0.45, 0.0)` for every host, documented as "a generic
spot that reads on both the unit figurine and a crab". Measuring the GLBs says it read on neither:

| host | measured body | old lump at world y | verdict |
|---|---|---|---|
| squad unit | mesh spans y −0.284 … 1.846; root carries `FIGURINE_SCALE` 1.13 | **0.51** | mid-thigh, not the chest |
| crab | carapace tops out at **0.408** | 0.45 | floating clear of the shell |

So "where a swelling sits on me, and how big it is" is now a per-host fact: `Parasitizable` carries
`lump_seat` + `lump_scale`, set from each creature's own measured constants
(`squad::UNIT_LUMP_SEAT`, `crab::CRAB_LUMP_SEAT`). Both are value fields present from spawn, so the
hashed host archetype never churns — the `Infestation` determinism invariant.

Colour went olive → an inflamed blood-under-skin red, rougher-to-wetter, squashed into a swelling rather
than a ball. Deliberately **not** `palette::GOC_RED`: that is Type Red — *regenerator* — in the GOC
taxonomy, it is what hostile bolt fire is, and `palette.rs`'s own test asserts laser fire **is** that
colour. A parasite is not a regenerator. The asymmetric twitch is kept; it is the part that was working.

Grounded in Kätsyri, Mäkäräinen, Förger & Takala 2015 (`10.3389/fpsyg.2015.00390`), whose
strongest-supported uncanny-valley hypothesis (4 of 4 studies) is **perceptual mismatch between the
realism levels of individual features** — a smooth flat-shaded primitive on a photo-textured organic
body reads as UI, which is precisely how the player read it. Same paper FVS-K-1 used for SCP-610's eye.

⚠️ The new seats are derived from measured GLB bounds but have not been eyeballed in-game; an infestation
needs gameplay time and the intro conversation holds the sim paused.

## 4. "This TV is poking through the wall and doesn't fit the aesthetic."

**The CRT is not furniture.** `broadcast::spawn_screens` — the watch-feed anomaly (FVS-C-7) — loaded
`retro_tvs/retro_tv_large.glb` directly and bypassed the placement grammar entirely. Its only filters
were `is_floor` and a minimum distance from spawn:

* **It clipped the wall every time, by geometry not by luck.** `TILE_SIZE` 1.0 and `WALL_THICKNESS` 0.14
  leave **0.36 m** of clear floor from a walled cell's centre; the set is 0.881 m wide, half-extent
  **0.44 m**. Every screen in a walled cell poked ~0.08 m through.
* **It faced world +Z regardless of the room** — an identity quat on a prop whose entire mechanic is
  being looked at, so the screen could stare into the wall behind it.

**Fix (player's call): wall-mount it.** The screen now seats off `Dungeon::wall_faces_near` the way
`placement::furnish` seats its wall sconces — lifted to `SCREEN_MOUNT_HEIGHT` 1.40 (set top at 2.06 under
a 2.4 m wall), pushed `0.281 + 0.02` along the inward normal so the chassis back sits flush, and yawed
`atan2(normal.x, normal.z)` so the glass faces the room. Local +Z **is** the screen direction for this
kit — `light::attach_screen_lights` documents it, adding a PI flip precisely because Bevy's spot axis is
−Z while the glass faces +Z.

Camera-facing walls are skipped for the same reason `furnish::wall_runs` skips them: those are cut to
knee height for the isometric view, so anything mounted at head height would hang in the cutaway gap.

`spawn_screen_at` now takes a whole `Transform` rather than a bare `Vec3` — a screen's **yaw is
gameplay**, since it is contained by looking away from it, and the old signature could only ever produce
the identity rotation.

> Noted while in the file: the doc comment claimed `spawn_screen_at` was `pub` "so the Research Room dev
> palette can place one by hand". Nothing in `src/research_room/` references this module. Left `pub` and
> the comment corrected, so the palette wires to this rather than growing a second spawn path.

## 5. "610, 1048, and 1048-A, and Smiley are all bundled in the corner. We should have rules about what spawns where."

**Cause — five copies of one greedy raster scan, and no cross-species spacing anywhere in the codebase.**

`enemy::spawn_enemies`, `scp999::spawn_scp999`, `scp1048::seed_bears`, `crab::setup::spawn_crabs` and
(per-room, but with the same tail) `scp610::spawn_scp610_blooms` each scanned `for y { for x { .. } }`
from cell (0,0) and took the first cells past a radius from the squad. Two consequences:

1. **`Dungeon::spawn` is the site nearest the level *centre***, so the first row-major cell past a radius
   is always at low x / low y. A minimum distance consumed in scan order is a corner-seeking rule in
   disguise. (`broadcast.rs` documents the same trap costing that anomaly its entire feature, FVS-N-30.)
2. **Each scan tracked separation only from its own kind.** Five species independently picked the same
   corner and stacked there. SCP-1048-A/B/C compounded it: the copies are built inside the parent's own
   cell (`replicate.rs`, 0.35 m jitter), so cornering the bear corners its whole brood.

SCP-610 had received a partial fix already — stride the region list, one bloom per evenly-spaced room —
which distributed blooms relative to *each other* and left the real problem standing: bloom `i = 0` took
`eligible[0]`, the lowest-index room, still the corner. Its regression test could not catch this, because
it asserted the *span* of the three blooms on a synthetic diagonal room list — a property that stays true
while all three sit in one corner of a real level next to four other anomalies.

**Fix — Stage B of `slop/research/2026-07-24-world-population-grammar.md`.** New
`src/placement/anomalies.rs`: **one pass places the whole roster** against a single shared list of
already-placed sites, so separation is cross-species by construction. Each species' spawner now reads its
solved cells instead of running a scan.

Selection is **Mitchell's best-candidate** (SIGGRAPH 1991) — draw 64 seeded candidates and keep whichever
maximises the distance to the nearest already-placed anomaly — the cheap approximation to Poisson-disk
sampling (Bridson 2007). It spreads by construction: there is no scan order left for a corner to win. An
exhaustive "farthest cell" would be deterministic too, and degenerate — it drives everything to the map's
extremities, the same bug with the opposite sign.

This is the object-level half of what Smelik, Tutenel, de Kraker & Bidarra call **consistency
maintenance** (`10.1016/j.cag.2010.11.011`): structure from *"constraints (e.g. minimum distance between
certain objects) between semantic objects"*, resolved centrally rather than by whichever generator ran
last.

**What it gives up:** "one bloom per room". That was the right unit for the fiction — area denial means
nothing in a corridor you can back out of. What replaces it is `sim.anomaly_separation` (18 tiles
shipped, comfortably past a large room's diagonal), which buys the same thing across the whole roster
rather than within one species.

**One-path discipline:** a level that cannot hold its population under the separation rule warns loudly
and places fewer. It never relaxes the spacing to fit the count — pinned by
`an_unsatisfiable_separation_is_reported_rather_than_relaxed`.

**Determinism:** own RNG sub-stream (`PLACEMENT_SEED ^ splitmix64(ANOMALY_STREAM)`), so it cannot shift a
single furniture draw; eligible cells `sort_total!`-keyed on the cell coordinate; species visited in a
fixed declared order, scarcest-and-most-constrained first.

**Verified:** five new tests in `src/placement/anomalies_tests.rs` (cross-species spacing, per-species
spawn minimum, anti-corner spread, seed determinism, unsatisfiable-reporting). On a real 192² level at
the shipped separation of 18.0, the full roster places with **no shortfall warnings**.

## 6. SCP-610 — half fixed, capture retained

*"SCP-610 just keeps falling over and over again. It is also stuck in the T-pose, with no animation, just
a rotation down when it dies."* Both halves are real, and only one is code.

**Fixed: the loop.** The death clip was registered `Slot::free`, which `anim::wire` turns into
`active.repeat()`, so a 1.292 s collapse looped forever. It is now `Slot::one_shot`, triggered once on
the health-crossing edge via `PoseBlender::target_weight` (the `parasite::drive_manca_animation` idiom).
The old comment claimed `PoseBlender` had no "play once" — it does; `Playback::OneShot` has existed
alongside `Free`/`Gait` all along and `parasite` and `scp1048` both use it. No `AnimationTransitions`
anywhere near the blender, which `docs/animation.md` forbids.

**NOT fixed: the T-pose. It is in the asset, and no wiring change reaches it.** Decoding
`assets/scp610/scp-610.glb`'s rotation channels:

| clip | arm/leg bones | verdict |
|---|---|---|
| `scp610_idle` | 8 bones × **2 keyframes, zero spread** | pinned to bind = T-pose; only torso/head/mutant limbs move (3–8° tremor) |
| `scp610_death` | same | torso pitches ~75°, legs bend, **arms stay spread** |
| `scp610_writhe_rage` | same | only head + mutant limbs (spread 0.34–0.42) |
| `scp610_chase_run` | `upper_arm_*` 17 keys, spread 0.366 | real motion — but the bloom does not travel |
| `scp610_lunge_attack` | `upper_arm_*` 15 keys, spread 0.652 | real motion — but the bloom does not attack |

So "stuck in a T-pose with just a rotation down" is a literally accurate description of what was
authored, and the two clips that *do* move the arms animate behaviours a stationary, non-attacking bloom
does not have. Loading them would buy a dead graph node and an unchanged T-pose. This is recorded in the
`CLIP_DEATH` doc comment so nobody hunts for a wiring fix again.

**Remaining work:** re-author the arm/mutant-limb channels in the `scp_characters` generator and
re-export. That builder was non-deterministic until 2026-07-30 (five distinct GLBs in eight builds), so
re-verify by re-running, and honour the `COLOR_0` export-by-name contract in
`assets/scp610/README.md` §6.

---

## Determinism / goldens

- **Cosmetic, no hash movement:** §1 (`coat_walls` is windowed-only behind the mycelia firewall), §2
  (textures), §3 (`drive_infestation_tell` is `Update`, windowed-only, never registered headless), §6's
  animation change (the animation layer is `Update`-only and invisible to `snapshot_hash`).
- **Moves goldens, expected:** §4 (screen transforms carry `LaserTarget`) and §5 (every anomaly's spawn
  `Transform` changed). **The harness replay goldens need a deliberate re-pin.**
- **`world_genome::N` grew 156 → 157** for `sim.anomaly_separation`, which invalidates the world elite
  archive exactly as `audio_genome`'s 15 → 16 did for FVS-K-1.
- The new gene lives in the **world** genome on purpose: that archive bins on `world_descriptor`'s
  measured outcomes (total deaths × total lives), which anomaly spacing genuinely moves — unlike the
  level archive's structural axes, which is the N-21 degeneracy trap. Still a prediction; confirm with
  `train probe` before trusting it.
