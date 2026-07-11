# Briefing: Converting the static mushroom asset packs into death-cap-style morph growers

**Date:** 2026-07-11
**Status:** Design briefing — no code written yet
**Author:** Claude (research + design pass)

---

## 1. Context & goal

We own a 16-species mushroom art pack (`/mnt/codex_fs/game_assets/mushrooms/`,
catalogued in `CATALOG.md §7`). Every species ships **Blender sources** (`.blend`)
plus baked `.glb`/`.fbx` exports in discrete size variants (Small / Medium / Large,
sometimes Xlarge, plus a pre-clustered Bunch).

Today the game grows exactly **one** mushroom — the death cap — and it grows
*beautifully*: a single mesh blended from sealed egg to adult across six glTF
**morph targets**, driven by one scalar `FruitBody.growth ∈ [0,1]`, held under a
perceptual motion-detection speed limit so it never visibly "pops."

**Goal:** give every species that same continuous morph growth, driven by the same
`FruitBody.growth` scalar and the same sim, so the whole pack becomes living
fruit bodies instead of static props. Because the `.blend` sources are editable,
this is achievable — but it is a **content-authoring project first, a code project
second.** This document briefs the change so we can scope it before committing.

---

## 2. How the death cap grows today (the target behaviour)

The mechanism, for reference — every species must end up matching this contract:

| Piece | Location | What it does |
|---|---|---|
| `DEATH_CAP_GLB` const + `load_death_cap` | `src/mycelia/fruit.rs:90`, `:281-283` | Loads one `.glb` into the `DeathCapScene` resource at startup |
| `FruitBody` component | `src/mycelia/fruit.rs:107-150` | Per-instance growth state: `growth`, `rise`, `scale`, `cluster`, `tint`, `bend`, `tilt`. **No species field yet.** |
| `grow_fruit_bodies` | `src/mycelia/fruit.rs:766-830` | The shared scalar-growth integrator (grow / stall-at-veil / reabsorb), gated by the perceptual `v_max` speed limit. **This is the one growth path and does not change.** |
| `drive_morph_weights` | `src/mycelia/fruit.rs:846-881` | Pushes `stage_weights(growth)` into the mesh's 6 `MorphWeights`. Hard-errors if the target count ≠ 6. |
| Perceptual constants + tests | `src/mycelia/perceptual.rs:86-194`, tests `:432-464` | `STAGE_*`, `RADIUS_PROFILE`, `CAP/VOLVA_RADIUS_M`, `BEND_*` — **all measured off the shipped death-cap `.glb`** — and the proof that the fastest vertex never exceeds the human motion threshold at any zoom. |
| `coat_fruit_bodies` / `tint_fruit_bodies` | `src/mycelia/fruit.rs:893-941`, `:945+` | Swaps the glTF material for the custom `MoldFruitMaterial` (part-mask vertex colours R=cap / G=flesh / B=volva, shader `assets/shaders/mycelia_fruit.wgsl`, which re-applies the morph and bends the stipe on the GPU). |

Two facts matter for the conversion:

1. **The death cap was procedurally generated.** Code comments reference
   `death_cap_procedural/`, `mushroom_gen.py`, `inspect_glb.py`
   (`perceptual.rs:86-101`) — **none of which are in the repo.** Only the baked
   `death_cap_growth.glb` ships. So there is **no existing art→morph pipeline to
   copy**; we are building one.
2. **The speed limit is measured from the actual shipped mesh, on purpose**
   (`perceptual.rs:98-101`: "the mesh the game loads is the only thing this limit
   may describe"). Every new growth `.glb` needs its own measured constants.

---

## 3. The core problem: the packs are independent sculpts, not a morph rig

glTF morph targets are **shape keys**: a single base mesh plus per-target vertex
*deltas*. They require every stage to share **identical vertex count *and* order**.

The authored size variants do **not**. Measured directly from the packs' `.glb`s:

| Species | Small | Medium | Large | Shared topology? |
|---|---|---|---|---|
| Fly Agaric | 576 | 864 | 1152 | ❌ independent sculpts |
| Destroying Angel | 480 | 512 | 544 | ❌ |
| Puffball (Fresh/Partial/Opened) | 96 | 152 | 184 | ❌ |
| Oyster | 352 | 352 | 352 | ✅ likely one base, edited |
| **Death cap (current, for contrast)** | — | **1379 verts, 3 prims, 6 morph targets** | — | single base + shape keys |

So we **cannot** simply import Small/Medium/Large and declare them keyframes. With
the lone possible exception of the Oyster (equal counts — worth a correspondence
check), each size is a separate mesh at a different resolution. Converting them to
a morph grower means, per species, **producing a single base mesh that carries
egg→adult shape keys on one shared topology.**

---

## 4. Recommended conversion pipeline (per species, in Blender)

The trick is to stop treating the three authored meshes as keyframes and instead
treat the **adult mesh as the one true base**, then generate the earlier stages as
deformations of that base so topology is preserved:

1. **Pick the base.** Take the Large/adult `.blend` mesh as the shape-key base
   (highest fidelity, final silhouette). Clean it: single manifold, sane origin at
   the stem root, +Y up, real-world scale (~13.9 cm like the death cap so
   `body_scale` stays meaningful).
2. **Author the stages as shape keys on that base** (6 targets to match the
   contract, or change `MORPH_TARGET_COUNT` — see §5). Two ways to make each key,
   fastest first:
   - **Surface-Deform / Shrinkwrap transfer:** bind a duplicate of the base to the
     authored *Small* / *Medium* meshes as targets and bake the deformed result
     into a shape key. This reuses the artist's silhouettes for free; the base just
     "shrinks onto" them. Best when the smaller variants are recognizable sub-shapes
     of the adult.
   - **Manual sculpt-back:** sculpt the base down to a primordium and a sealed
     "egg"/button for the earliest 2–3 keys (the packs rarely provide a true egg;
     the Puffball's Fresh→Opened is the exception and maps almost 1:1).
   - Order the keys along the same growth fractions the death cap uses
     (`STAGE_T = [0, .12, .28, .45, .62, .80, 1.0]`, `perceptual.rs:91`), including a
     **veil-rupture** analogue at ~0.45 for the veiled Amanitas (Fly Agaric, Death
     Cap, Destroying Angel). Species without a universal veil (Oyster, Turkey Tail,
     Morel, Chanterelle) just ramp monotonically.
3. **Assign the part mask.** The shader keys cap/flesh/volva off `COLOR_0` vertex
   colours (R/G/B), *not* textures. Either paint the same 3-channel mask
   (recommended — keeps every species on the one shader) **or** decide these species
   keep their authored PNG textures and we fork the material (see §6).
4. **Export `assets/mushrooms/<species>/<species>_growth.glb`** with shape keys
   exported as morph targets (glTF export → "Shape Keys" on).
5. **Re-measure the perceptual constants** from that baked `.glb` (rebuild the
   `inspect_glb.py` step as a small committed tool): per-segment max vertex
   displacement, stage heights, cap/volva radii, bend profile. These feed both
   `perceptual.rs` and must stay in sync with `mycelia_fruit.wgsl`.

The **Oyster** is the pilot: equal vertex counts suggest its variants are already
one base edited three ways, so its shape keys may transfer almost automatically —
prove the pipeline there before the harder sculpt-back species.

---

## 5. Supporting code changes (smaller than the art work)

Even once each `_growth.glb` exists, the engine still assumes one species. These
changes are the same regardless of growth mechanism, and mirror an existing
data-driven idiom in the codebase (`Vec<DampWeight>` at `mod.rs:354-379` +
`validate_damp_coverage` at `mod.rs:701-723`, wired in `config.rs:142`):

- **Species registry.** Add `species: SpeciesId` to `FruitBody`
  (`fruit.rs:107-150`). Replace the single `DeathCapScene` resource with a
  species-keyed scene table (`HashMap<SpeciesId, Handle<WorldAsset>>`), loaded from
  a new `Vec<SpeciesConfig>` in `MyceliaConfig`, filled from
  `assets/config/config.ron`. Each entry: `id`, `growth_glb` path, `body_scale`,
  `morph_target_count`, and the measured stage/radius constants (or a sidecar).
- **Per-species morph-target count.** `drive_morph_weights` currently hard-codes 6
  (`fruit.rs:846-881`). Read the count from `SpeciesConfig` so a species can carry
  4–6 keys. It already correctly writes only its own body's `MorphWeights`, so it
  generalizes cleanly and the **death cap path stays byte-for-byte unchanged.**
- **Species selection at pin time.** In `pin_fruit_bodies` (`fruit.rs:569-734`),
  after the room type is known, pick the species by a **seed-derived weighted
  choice** over per-species room affinity (a second small `Vec` mirroring
  `damp_weights`), then spawn with that species' scene handle instead of the
  hard-coded `DeathCapScene.0.clone()` at `fruit.rs:709-729`. Stay seed-only —
  `util::hash01_u32`, no entropy (determinism rule, `TESTING.md:62-68`).
- **Validation.** Add `validate_species_*` mirroring `validate_damp_coverage`
  (`mod.rs:701-723`): loud startup error if a species' affinity table names a
  room type the dungeon can't emit, or if a `growth_glb` is missing / its measured
  morph-target count disagrees with config. No silent defaults.
- **Toxicity / nutrition** (a separately-scoped follow-on): `FruitBody::amatoxin()`
  exists but is deliberately inert (`fruit.rs:167-169`, `grazing.rs:28-29`). Wiring
  per-species toxicity into `crabs_graze_fruit_bodies` requires deciding the
  crab-side effect and keeping it deterministic (grazing runs on `FixedUpdate`).
  **Out of scope for this briefing** — flagged so it isn't forgotten.

---

## 6. The one open design fork: materials

The death cap has **no textures** — it uses a 3-channel vertex-colour part mask and
the shared mold palette so it reads as "the same organism" as the floor mat
(`material.rs:152-159`). The authored packs are **textured** (a `.PNG` per species),
so a Fly Agaric is red-with-white-spots *in its own art*.

Two honest choices, pick one (this is not a fallback — it's one decision applied to
all species):

- **(A) Keep authored textures.** Species look correct out of the box; we fork the
  material so non-death-cap species render their own PBR texture (still needing a
  morph-aware + stipe-bend vertex shader variant). Cost: a second material/shader
  path, and species won't visually tie into the mold mat.
- **(B) Repaint the part mask, drop textures.** Every species stays on the one
  `MoldFruitMaterial` + `mycelia_fruit.wgsl`, unified look, one code path. Cost:
  hand-painting a cap/flesh/volva mask and per-species base colours; we lose the
  packs' texture detail.

Recommendation: **(B) for the veiled/amanita family** (they already suit the mold
aesthetic and share the veil-rupture beat), **(A) considered only if** an art review
says specific species must keep their texture identity. Decide before authoring —
it changes step §4.3.

---

## 7. Why this satisfies "one path per feature"

This is the elegant part, and the reason the `.blend` route beats the earlier
"scale + mesh-swap" idea: **every mushroom grows by exactly one mechanism** — the
shared `grow_fruit_bodies` scalar sim driving glTF morph weights. No staged-scale
branch, no swap-vs-morph split, no "death cap is special." The death cap becomes
simply *the first row of the species table*. Adding a species is data + an asset,
never a new code path.

---

## 8. Per-species stage inventory (authoring worklist)

What each pack gives us as raw material (from the `.glb` file listing):

| Species | Authored stages available | Notes for authoring |
|---|---|---|
| Puffball | Fresh, PartialOpened, Opened, Bunch | Closest to a real growth sequence — near 1:1 keyframes |
| Oyster | Small, Medium, Large (equal verts) | **Pilot** — topology likely already shared |
| Fly Agaric | Small, Medium, Large, Group Bunch | Veiled amanita → author veil-rupture key |
| Death Cap *(existing)* | already a 6-target grower | Reference / first table row |
| Destroying Angel | Small, Medium, Large, Cluster | Veiled amanita |
| Amethyst Deceiver | Small, Med, Large, Xlarge, Bunch | 4 size sculpts to draw from |
| Chanterelle | Small, Medium, Large, Cluster | Monotonic ramp (no veil) |
| Chicken of the Woods | Small/Mid/Large Full + LargeHalf, Bunch | Bracket fungus — grows on wood/walls |
| Turkey Tail | Small, Medium, LargeHalf, LargeFull, Bunch | Bracket — wall-mounted growth |
| Enoki | Small, Medium, Large, Bunch | Tall clustered stems |
| King Bolete | Small, Medium, Large, Cluster | Bulbous stipe |
| Morel | Small, Large, Bunch (no medium) | Only 2 size sculpts → more sculpt-back |
| Champignon | Small, Large, Bunch (no medium) | Only 2 size sculpts |
| Blue Pinkgill | Small, Medium, Large, Bunch | |
| Rosy Bonnet | Small, Medium, Large, Bunch | Small delicate |
| Ink Caps | Young, Ink1, Ink2, Old, Group | Deliquescing — Old = self-digesting cap, a natural late key |

The "Bunch"/"Cluster"/"Group" meshes are **pre-clustered** and should *not* be used
as growth stages — the sim already lays out caespitose flushes of individual bodies
(`cluster_sites`, `cluster_spacing`/`cluster_radius` in `config.ron`).

---

## 9. Risks & open questions

- **Authoring cost dominates.** 15 species × (retopo/transfer + 4–6 shape keys +
  mask/texture + re-measure) is real Blender labour. The Oyster pilot will tell us
  the per-species hours; Morel/Champignon (2 source sizes) and any species needing a
  sculpted egg are the expensive tail.
- **The measure tool must be rebuilt.** `inspect_glb.py` is referenced but absent;
  we need a committed, repeatable "measure a `_growth.glb` → emit `STAGE_*`
  constants" tool, or the speed-limit guarantee becomes a guess.
- **Shader/const duplication.** Stage constants are duplicated in
  `mycelia_fruit.wgsl` and must agree with `perceptual.rs` (`:146-150`). A per-species
  table means either per-species shader uniforms or a bounded shared layout.
- **Non-vertical growers.** Bracket fungi (Turkey Tail, Chicken of the Woods) grow
  *out of walls*, not up from the floor. Current pinning is floor-only
  (`pin_fruit_bodies` checks `dungeon.is_floor`). Wall-mounted growth is a separate
  feature; for v1 they'd fruit on the floor like the rest, or be deferred.
- **Wall clearance.** Each new silhouette must pass the `testbed.rs:229-352`
  wall-penetration audit, which is solved from `RADIUS_PROFILE` — so the measured
  radii must be honest.

---

## 10. Suggested phasing

1. **Pilot (Oyster).** Build the Blender→`_growth.glb`→measure tool and the species
   registry code against one species. Prove morph growth end-to-end in-game.
2. **Amanita family (Fly Agaric, Destroying Angel).** Reuse the veil-rupture beat;
   validates the veiled-growth path and material choice §6.
3. **Batch the remaining floor species** (Enoki, King Bolete, Champignon, Morel,
   Chanterelle, Blue Pinkgill, Rosy Bonnet, Amethyst Deceiver, Ink Caps, Puffball).
4. **Defer** bracket fungi (Turkey Tail, Chicken of the Woods) until wall-mounted
   growth exists, and **defer** toxicity/nutrition to its own change.

---

## 11. Verification (when we build it)

- `cargo test` — `perceptual.rs` speed-limit tests must pass for **every** species'
  measured constants (fastest vertex under the motion threshold at all zooms).
- `MYCELIA_FRUIT_TESTBED=1` — the `testbed.rs` wall-penetration audit, re-run per
  species (silhouette radius differs).
- In-game visual check via the self-screenshot path (`touch screenshot.request`;
  read `screenshot.png`, per `CLAUDE.md`): confirm each species erupts, morphs
  egg→adult smoothly with no pop, and reabsorbs.
- Determinism unaffected: fruit bodies stay cosmetic (Update-only, no `Health`,
  seed-derived), so `snapshot_hash` and the replay harness are untouched
  (`TESTING.md:273-277`).

---

*No code or assets were modified in producing this briefing.*
