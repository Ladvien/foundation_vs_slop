# Almond Water — a biological-heal resource that seeps from concrete

**Status:** Idea / design capture. Not a committed plan, not started.
**Lore reference:** `docs/lore/2026-07-13-backrooms-almond-water.md` (Backrooms Object 1; canon posture, IP hazards, the drift story).
**Scope of this note:** Phase 1 only — the resource field, the heal, and the puddle visual. Extractors and the "belief/inversion" mechanic are deliberately deferred (see §7).

## TL;DR

- **What:** *Almond Water* (Backrooms Object 1, post-drift healing reading) — a substance that **bubbles up
  from concrete floor and walls**, pools, and **heals biological entities** that stand in it. It is a
  **real, finite, depletable quantity** in the world (so a later phase's SCP extractors have something to
  tap), tuned **high-volume/abundant** so pools rarely run dry under normal play.
- **Model it as its own `AlmondWater` resource**, following the `LightField` "environmental field, not a
  `Stig` channel" precedent (`src/light.rs:180-191`), but with `Stig`'s accumulate→evaporate→diffuse
  dynamics (`src/ai/field.rs`) since — unlike light — it is dynamic and consumable. Per-cell scalar over
  the shared 192×192 dungeon grid.
- **A `Biological` ECS marker component** on all flesh creatures (units, crabs, mancae, Smiley boss;
  **not** `Nest` — stone). A `FixedUpdate` heal system queries `With<Biological>`, mirrors `medic_heal`
  (`src/squad_ai/actions.rs:118-148`), and **drinks down** the local cell as it heals.
- **The emergent behavior we're buying** (the reason this is interesting): stigmergic **foraging over a
  regenerating resource**. Wounded creatures cluster on rich seeps, drain them, and a depletion front
  moves — no explicit coordination code. Grounded in the stigmergy + reaction-diffusion literature (§4).
- **Visual:** a **slightly iridescent** puddle — three composited layers: (1) procedural **bubble-up
  blooms** welling from the concrete, gated by the sim field; (2) a **physically-based thin-film
  interference** tint (oil-slick iridescence); (3) an almond base. Cosmetic, windowed-only, so it touches
  no determinism goldens.

---

## 1. Why its own resource (architecture)

`src/light.rs:180-191` already argues *why* an environmental field is its own resource and **not** a
`Stig` channel: light is static/environmental, `Stig` channels are creature-emitted pheromones; folding
one into the other is a hidden second path. Almond Water is likewise environmental (sourced from geometry,
not agents) and **named for future extractors**, so it earns its own resource — but unlike light it is
**dynamic and consumable**, so it borrows `Stig`'s update kernel. One clean path: one `AlmondWater`
resource with `LightField`'s query interface (`sample`/`gradient`) and `Stig`'s
`evaporate_diffuse`-style tick.

Grid substrate is the shared one every field uses: **192×192 dungeon cells**, `TILE_SIZE = 1.0`,
row-major via `crate::util::row_major`/`in_grid`, floor-masked. World↔cell through
`Dungeon::world_to_cell` / `cell_center` / `is_floor` (`src/dungeon.rs:1212-1435`).

This is textbook ECS practice: a new feature = *identify the components that store the data, attach them to
the right entities, implement systems that process them*, with minimal impact on existing code
(Landyshev 2024; Tasnim & Zhao 2026). Composition over inheritance.

## 2. The field (`AlmondWater` resource)

Mirror `LightField` (`src/light.rs:192-248`):

- `sources: Vec<f32>` — per-cell **seep rate**, baked **once at Startup** from geometry (pure, out of
  `snapshot_hash`, like `habitat::build`). A floor cell is a **strong seep** if any 4-neighbour is
  non-floor (a wall borders that edge — "bubbles up from the walls of concrete"); every floor cell gets a
  **weak baseline seep** ("bubbles up from the floor"). Pure geometry → deterministic, no RNG.
- `level: Vec<f32>` — current water **volume** per cell (the depletable quantity).
- `scratch: Vec<f32>` — diffusion double-buffer (mirrors `Stig::scratch`).
- `capacity`, `peak`.
- Methods: `sample`/`gradient` (copy the central-difference read from `LightField`), `drink(cell, amount)
  -> f32` (subtract up to `amount`, clamp ≥0, return actually removed), and
  `#[cfg(feature="test-harness")] fold_fingerprint` (copy `Stig::fold_fingerprint`).

**Update system `accumulate_evaporate_diffuse`** (FixedUpdate), floor-cells only, in order (copy
`Stig::evaporate_diffuse` discipline, `src/ai/field.rs:265`):
1. accumulate: `level[c] = (level[c] + sources[c]·dt).min(capacity)`
2. evaporate: `level[c] *= (1 − evaporate·dt).clamp(0,1)`
3. diffuse: double-buffered 4-neighbour blend, floor-masked.

Steady state per cell ≈ seep vs evaporate+drink → abundant but self-limiting.

## 3. Healing (`Biological` marker + consuming heal)

- **`Biological` marker** — `#[derive(Component)] pub struct Biological;` next to `Health` in
  `src/health.rs` ("living flesh Almond Water can heal — a positive tag, so nests/stone with `Health` are
  excluded by construction"). Inserted **at spawn** alongside `Health::new` at the four flesh sites
  (`src/squad.rs` Unit, `src/crab.rs` Crab, `src/parasite.rs` Manca, `src/enemy.rs` Smiley) — at spawn,
  not mid-sim, to avoid a runtime **archetype migration** (Tasnim & Zhao 2026). **Not** on `Nest`
  (`src/nest.rs`).
- **`almond_water_heal`** (FixedUpdate, after the field update): `Query<(&Transform, &mut Health),
  With<Biological>>`; skip if `current >= max`. Per entity: `cell = world_to_cell(pos)`;
  `want = min(heal_rate·dt, max-current)`; `got = drink(cell, want / heal_per_unit_water)`;
  `current += got · heal_per_unit_water` (clamped to `max`). Drinking **drains** the cell — the
  consumable coupling.

**Determinism:**
- Writes `Health` on FixedUpdate → enters `snapshot_hash` → the deterministic-core golden
  (`0x6716f1718a9774d1`, `TESTING.md:102`) and, once the field folds into `field_hash`
  (`src/sim_harness.rs:395+`), the field golden (`0x5d60_2962_2213_5600`) both change. Deliberate,
  human-reviewed re-pin in `tests/replay.rs`.
- **Drink contention:** collect candidates, sort by `(cell, current.to_bits(), pos bits)` before applying
  so several drinkers on one cell drain in a value-stable, order-independent order (float add/sub is
  non-associative — same discipline as `snapshot_hash`'s sorted rows and `Stig`'s sorted deposits).
- **Order vs `medic_heal`:** both systems `&mut Health`, so Bevy serializes them regardless (Redmond et
  al. 2025); pin the order explicitly (`almond_water_heal.after(medic_heal)`) so two heal sources on one
  unit in one tick compose deterministically.

Register `AlmondWaterPlugin` (Startup bake + the two FixedUpdate systems, behind an `AlmondWaterWritten`
set) in **both** the game (`src/lib.rs`, after `DungeonPlugin`) and the headless harness
(`src/sim_harness.rs`, where `LightFieldPlugin` is added) so it's exercised deterministically.

## 4. The emergent behavior (why this is worth building)

This is the payoff, and it comes for free from the stigmergy + reaction-diffusion structure:

- **Stigmergy** (Parunak 2005; Heylighen 2015; SOTA: Salman, Garzón-Ramos & Birattari 2024): agents
  **sense and modify a shared environmental medium**; local interactions + self-organization → coherent
  global behavior, and *because interaction is local, it scales without overwhelming any agent*. Almond
  Water is that medium — a depletable, self-regenerating field. Wounded creatures descend toward richer
  water (the same `sample`/`gradient` taxis toolkit crabs already use on `LightField`), drain a seep, and
  the depletion front pushes them onward. Competition over the resource is emergent, not scripted — and
  because it heals *everything* biological, it becomes a contested territory between the squad and the
  swarm.
- **Reaction-diffusion** (Painter & Maini 1997; *Diffusion-Limited Growth of Microbial Colonies* 2018):
  the seep→diffuse→evaporate update *is* a reaction-diffusion process on a surface, so the field naturally
  **pools and patterns** rather than spreading uniformly — the desired "bubbles up and pools on concrete"
  look, and a substrate that reads as alive.

## 5. Visual — a slightly iridescent puddle (three layers)

Cosmetic `AlmondWaterVisualPlugin`, **windowed-only, NOT in harness** (mycelia determinism firewall,
`src/mycelia/mod.rs:954`), on `Update`, gated on `RenderApp`. Uses the render clock — never the sim clock —
so it cannot perturb goldens. Reuse the mycelia GPU control-texture pattern: upload `level` as a 192²
texture each frame via `ExtractResourcePlugin`, sampled by a floor-plane material shader (new
`assets/shaders/almond_water.wgsl`). If it grows, split into `src/almond_water/{mod,visual}.rs`.

**Layer A — bubble-up blooms (the "it seeps up" motion).** Adapt the layered-bloom technique (Dave Hoskins
`hash13` value-hash → per-bloom random pos/shape/color; a `flower()`-style blob that grows via `p *= lT·5`
and fades via `smoothstep(sin(lT·π))`; ~20 layers summed at staggered time offsets for a continuous
field). Adaptations: run it in **floor/cell UV**, not screen space, so blooms anchor to the concrete;
**gate count/intensity by the sampled `level`** and bias spawn toward strong-seep/wall-adjacent cells so
blooms appear where water actually bubbles up (zero level ⇒ no blooms); keep amplitude low and color within
an almond/pale range. (`hash13` = Hoskins, "Hash without Sine".)

**Layer B — thin-film iridescence (oil-slick / Newton's rings), physically based.** The color is thin-film
interference: the water film is ~a wavelength thick, reflections off its top and bottom surface interfere,
and the in/out-of-phase angle differs per wavelength, splitting white light into color.
- **Optical path difference** `OPD = 2·n·d·cos(θ₂)`, `θ₂` the *refraction* angle inside the film via
  Snell: `cos(θ₂) = sqrt(1 − (n_air/n)²·(1 − cosθ₁²))`, `cosθ₁ = dot(N, V)`. Per-channel reflectance
  `0.5 + 0.5·cos(2π·OPD/λ)` at `λ = (700, 550, 400) nm` (R,G,B). Precompute `2π·n·d` into a uniform; only
  `cos(θ₂)` + three `cos` per fragment.
- **Boundary physics for water-on-concrete:** `n_air(1.0) < n_water(1.33) < n_concrete(~1.55)`, so a 180°
  flip occurs at **both** interfaces and cancels → constructive condition `2·n·d·cosθ₂ = mλ` (the
  anti-reflection-coating case, not the soap-bubble half-integer case).
- **Palette:** real oil/water films read as **golds, teals, blues, magentas — not a clean spectral
  rainbow** (the reflection is a wavelength mixture); bias the mapped tint toward that muted palette so it
  stays tasteful and almond-appropriate. Keep `iridescence_strength` low; more water = more sheen.

**Layer C — almond base tint**, composited under A/B:
`color = mix(almond_tint, almond_tint · iridescent_tint, iridescence_strength · levelN)`.

## 6. Config + tests

- **Config** (`AlmondWaterConfig` slice, follow `LightingConfig` at `src/light.rs:31-165` + validator +
  `GameConfig` field + RON section): `strong_seep`, `weak_seep`, `capacity`, `evaporate`, `diffuse`,
  `heal_rate`, `heal_per_unit_water`, and visual params `almond_tint`, `min_visible_level`,
  `film_thickness_nm`, `film_ior` (≈1.33), `iridescence_strength`. All validated finite/positive/in-range,
  one loud `Err` each — no fallback.
- **Tests** (GPU-free deterministic core): same seed → identical `field_hash` + `snapshot_hash` across two
  runs; `bake_sources` deterministic and wall-adjacent cells get the strong rate; heal caps at `max` and
  never exceeds available water; `drink` drains exactly and clamps at 0; every `Biological` also has
  `Health`. Plus a headless liveness check (`--features test-harness`): a wounded biological on wet
  concrete regains HP.
- **Visual check** via devshot: `touch screenshot.request` → read `screenshot.png`; confirm puddles pool
  along walls/concrete and show a subtle iridescent sheen that shifts with view angle (orbit the camera
  between two shots). Confirm the visual plugin is absent from the harness.

## 7. Deferred (future phases — data model chosen so none need a rework)

Extractors (tap `level`); the **belief/inversion** mechanic (item does what the population believes — lore
§6, "the only version with teeth"); color variants (grey/green/red/blue); counterfeits ("Almonb Water");
boil-before-drinking; the pre-drift cyanide-mimic. The per-cell scalar `level` + the config slice leave
room for all of them.

Note the lore's own warning (`docs/lore/…-almond-water.md` §0): the straight-heal reading is "Exhibit A for
the slop side," and the inversion is the version "with teeth." This idea ships the heal version by request,
with the inversion left as a clean, deliberate future hook.

## 8. References

- Landyshev, A. (2024). *The Role of ECS Architecture in Video Games Development.* DOI 10.52058/2695-1592-2024-9(40)-176-184.
- Tasnim, A. & Zhao, T. (2026). *The Essence of Entity Component System.* DOI 10.1145/3748522.3779910.
- Redmond, P., Castello, J., Calderón Trilla, J. M. & Kuper, L. (2025). *Exploring the Theory and Practice of Concurrency in the ECS Pattern.* DOI 10.1145/3763050.
- Parunak, H. Van Dyke (2005). *A Survey of Environments and Mechanisms for Human-Human Stigmergy.*
- Heylighen, F. (2015). *Stigmergy as a Universal Coordination Mechanism I & II.* DOI 10.1016/j.cogsys.2015.12.002 / .007.
- Salman, M., Garzón-Ramos, D. & Birattari, M. (2024). *Automatic design of stigmergy-based behaviours for robot swarms.* DOI 10.1038/s44172-024-00175-7.
- Painter, K. J. & Maini, P. K. (1997). *Spatial pattern formation in chemical and biological systems.* DOI 10.1039/a702602a.
- *Diffusion-Limited Growth of Microbial Colonies* (2018). DOI 10.1038/s41598-018-23649-z.
- Belcour, L. & Barla, P. (2017). *A Practical Extension to Microfacet Theory for the Modeling of Varying Iridescence.* SIGGRAPH — SOTA physically-based thin-film model our closed-form approximates.
- Thin-film interference: standard optics (Hecht, *Optics*; the oil-film-on-water / anti-reflection-coating cases). Reed, N. (2013) — the cheap `sin(2π·t/(dot(L,H)·λ))·0.5+0.5` angle-cheat we started from. Hoskins, D. — "Hash without Sine" (`hash13`).
