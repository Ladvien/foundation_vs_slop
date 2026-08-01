# Asset enrichment plan — widening behaviour, not just decorating

*2026-08-01. Survey of `/mnt/codex_fs/game_assets/` against what the game actually consumes, ranked by
**behaviour unlocked per unit of work** rather than by mesh count.*

---

## 0. The gap, in one table

| | library | shipped | ratio |
|---|---:|---:|---:|
| Game-ready 3D models | 4,931 | ~120 | 2% |
| Vendor asset packs | 50 | 7 | 14% |
| Ozea sci-fi library | 1,639 files / 37 sub-packs | **2 manifest items** | ~0% |
| SCP character packs | 22 | 4 (999, 1048×4, 150, 610) | 18% |
| Music files | 5,076 | 0 | **0%** |
| SFX files | 854 | ~60 | 7% |
| Retargetable mocap clips | 22 | 0 | **0%** |
| PBR texture sets | 173 files / 39 unextracted zips | ~2 diffuse | ~1% |

The furniture manifest is **41 items** drawn from 7 kits, with an affordance vocabulary of seven
strings: `store` (12), `emit` (10), `sit` (6), `decor` (5), `screen` (4), `hygiene` (3), `sleep` (1).

**The interesting number is not 2%.** It is that several *behaviours the engine already supports* have
no assets pointed at them, so the code path exists and never executes. Those come first.

---

## Tier 0 — behaviour the grammar already supports, assets already in-repo, only DATA missing

**This is the highest-ratio work on the page: no new files, no new code, no licence questions.**

### 0.1 Doorways are unfurnished (`Anchor(host: Opening)`)

The placement grammar supports four anchor hosts — `Ceiling | Wall | Opening | Floor`. The manifest
authors **two** (`Ceiling` ×1, `Wall` ×1). `Opening` and `Floor` have **zero entries**.

Meanwhile `assets/kenney_prototype-kit/Models/GLB format/` — already in the repo, already licensed —
ships `door-rotate`, `door-sliding-double`, `door-sliding-double-round`, `door-sliding-double-wide`,
`door-garage` and more, among 145 GLBs.

So every doorway in every level is an empty hole, and the code that would furnish it runs against an
empty candidate set. **Adding manifest rows turns doors on.** And doors are not decoration — they are
the geometry the quarantine cordon, line-of-sight, and the ambient `ATTENTION` field all key off, so
this widens *behaviour* immediately: a closed door is a LOS break, which is the containment verb for
the watch feed.

### 0.2 `Anchor(host: Floor)` — freestanding fixtures

Same shape. The Kenney kit has columns, crates and indicators; `sci-fi-capsule` (7 meshes) and
`sci-fi-lab-machine` (6 meshes) are the Site-appropriate versions.

**Estimated cost for Tier 0: an afternoon of manifest authoring, zero engine change.**

---

## Tier 1 — widen the AFFORDANCE vocabulary (data + small code)

Affordances are what make a prop *behavioural* rather than visual: the placement grammar and the AI
both read them. Seven strings is thin, and one of them (`sleep`) has a single item.

Each new affordance below is a behaviour the squad or the swarm can have opinions about:

| New affordance | What it would mean | Assets that already exist |
|---|---|---|
| `cover` | Breaks line of sight — so it breaks `ATTENTION`, which is now a containment verb | `survival-psx` (38 props), `simple-apocalypse-2-free`, `cardboard-box-set`, crates in the Kenney kit |
| `barricade` | Blocks a doorway; the swarm must path around or break it | `low-road-signs` (17), `warning-signs-set` (29), furniture packs |
| `power` | Powers a room's lights; cutting it darkens the zone (photophobic crabs already steer on light) | `sci-fi-lab-machine`, `black-honey-robotic-arm` (64 meshes), Ozea |
| `readable` | A document the knowledge system can file — ties props to operative beliefs | `low-poly-papers-set`, `old-blueprints` (15), `antique-book-set` (51) |
| `keyed` | Gates a room behind a clearance level | `scp-keycards` + `scp-cb-keycards` (5 levels each) |
| `dispense` | An interactable that yields a consumable | `scp-294` (the coffee machine — canon, and funny) |

**`readable` and `keyed` are the two that reach furthest**, because they connect the placement grammar
to systems that already exist and are under-fed: operative knowledge (beliefs are currently only
acquired from creatures) and the Site's progression.

---

## Tier 2 — visual identity per room type (data-heavy, no engine change)

FVS-Q-8 made biome resolution **per zone**, so "this room is a lab / a dormitory / a storeroom" is now
a well-formed statement. The manifest is the only thing stopping each from looking different.

| Room character | Kit | Files |
|---|---|---|
| Foundation lab / clean sci-fi | `models/scifi/ozea_ultimate_library/` | **1,639** across 37 sub-packs — the single largest untapped resource |
| Living quarters | `tinylivingpack` (47), `low-poly-furnitures-full-bundle` (33), `furniture-set-free` (52) | 132 |
| Abandoned / overrun | `simple-apocalypse-2-free`, `survival-psx` (38) | 41 |
| Storage / logistics | `cardboard-box-set`, `candy_boxes_pack` (101), `acid-barrel-pack` (7) | 109 |
| Signage everywhere | `warning-signs-set` (29), `low-road-signs` (17), `scp-logo` | 47 |

**Ozea is the headline.** 1,639 files, two manifest entries. It is the kit whose look the Site is
supposed to have, and it is 99.9% unused.

⚠️ Ozea's conversion is tracked (FVS-N-10) and was correctly de-scoped as non-blocking — the Site
shipped greyboxed. That judgement stands; this is the *upgrade* it deferred, and it is now the
cheapest way to make the game look like its own concept art.

---

## Tier 3 — the SCP roster (needs code per creature; rank by primitive reuse)

**22 character packs are unshipped.** They are not equal: the right order is by how much existing
machinery each reuses.

| Candidate | Reuses | Cost |
|---|---|---|
| **SCP-079** (AI on a computer) | The watch-feed pattern almost exactly — static, screen, generates. And it is *literally* an AI antagonist, which is the SCP-9191 theme | **S** — nearest neighbour to shipped code |
| **SCP-294** (vending machine) | `dispense` affordance; no AI at all | **S** — a prop with a verb |
| **SCP-173** (statue) | Needs the per-entity directional watch primitive (FVS-C-6) | M–L, already scoped |
| **SCP-096** (shy one) | Same watch primitive, inverted | M–L |
| **SCP-939** (quadruped, ×2 packs) | Crab locomotion is quadruped; nest/pack behaviour exists | M |
| **SCP-049 / 106 / 682** | New locomotion and new rules | L–XL |

**SCP-079 is the recommendation**: it is the closest thing in the library to code that already works,
and it advances the endgame theme rather than padding the roster. SCP-173/096 stay deferred exactly as
the backlog argues (leading with 173 is "an amateur tell", and they need new engineering first).

---

## Tier 4 — surfaces and light (unblocks Push 11)

Push 11's goal — "surfaces respond to light (normal + ORM maps against an irradiance environment)" —
is **asset-blocked, and the assets exist**:

- `textures/pbr/` — extracted **`Blood_2K`, `Guts_2K`, `guts-bl`**, `Fabric019_4K`, each with 7–11 maps
  (albedo, normal DX+GL, roughness, height, AO, subsurface). These are horror-themed and disjoint from
  the hand-curated Godot set — exactly what the gore and mycelia layers want.
- 39 **unextracted zips** with 1k/2k/4k variants for `concrete_0031`, `decals_0008`, `ground_0014`,
  `ground_0046`, `imperfection_0003`. ⚠️ 4K variants are 236–273 MB each — take the 1k/2k.
- `textures/terrain/` — 20 PBR terrain maps.

**Highest-value single swap:** the dungeon currently runs on two diffuse textures. Giving concrete and
the mould field real normal + roughness is the change that makes the whole game read as lit.

---

## Tier 5 — audio (feeds the acoustic program)

`docs/2026-08-01-acoustic-program.md` stages 2–4 are all asset-hungry, and the library is enormous and
**entirely unused**: 5,076 music files across 21 packs, 854 SFX.

- **Stage 2 (audible slop)** wants the `400 Sounds Pack`'s **Retro (25)** and **Machines (7)**
  categories — the watch feed should sound like a machine mass-producing something.
- **Stage 3 (ambience as containment readout)** wants the packs' `Ambiences/` folders.
- **Stage 4 (sim-driven music)** wants `Loops/` — every pack ships loops specifically for layering,
  which is the dynamic-layering technique the procedural-music paper describes.

⚠️ Many packs mirror the same content in `wav/`, `ogg/`, `mp3/`. Take `ogg` only — the repo already
standardises on it, and the file counts above triple-count.

---

## Recommended order

1. **Tier 0 — doors and floor anchors.** Free (data only, assets in-repo), and it turns on a LOS/cover
   layer that the containment verbs already read. Do this first regardless of anything else.
2. **Tier 4 — concrete + mould PBR.** One kit swap, largest visual delta per hour, unblocks Push 11.
3. **Tier 2 — Ozea into the lab room type.** The Site finally looks like a Foundation site.
4. **Tier 1 — `readable` + `keyed` affordances.** Connects props to the knowledge and Site systems,
   which is behaviour rather than dressing.
5. **Tier 5 — audio for acoustic stage 2**, alongside that build.
6. **Tier 3 — SCP-079** as the next anomaly.

## What this plan deliberately does not propose

- **No mesh-count-driven work.** FVS-N-23 already established the frame is CPU-bound at 238 fps, so
  "more props" is not a performance problem — but it is also not automatically a *game* improvement.
  Every tier above is justified by a behaviour, not by a count.
- **No mocap yet.** The 22 retargetable clips are tempting, but `docs/animation.md` makes the animation
  layer the deliberate exception to the wire-everything-into-RL/QD rule, and gait clips must be
  authored **in-place with zero root motion** against a pinned GLB contract. Retargeted mocap will
  fight that contract. Worth doing, worth scoping separately.
