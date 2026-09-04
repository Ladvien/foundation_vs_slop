- Use home still and lookup research on whatever you are implementing BEFORE you implement it.  We want SOTA and best practices.  And it is filled with game development research and best practices.
- Do not use hardbreaks when writing markdown.  Markdown should be easy for humans to read when rendered.
- Consult Bevy documentation often. It can be found at codex_fs/offline_reference_docs/bevy-0.19-book/
- When given git commands, it should be for all edittable repos in this project.
- When creating new features, attempt to use Bevy's plugin pattern as much as possible.  Create separate workspace crates.  Create their own Github repo with idiomatic name.  This is to ensure reusable components are generated during our work. Ensure each respective crate has a warning label of "Vibe Coded" a the top of the README.md. Please refer to [bevy_plugins.md](docs/bevy_plugins.md).  Each separate create must include examples (1-3) demoing the crate.  This is allow inspection of crate behavior without inclusion in this game.  If you discover debugging needs, make recommendations on adding it to this plugin, ever evolving.
- Do not use unwrap() or anything that'd lead to a panic.  Code safe.  Handle errors.
- Leave academic paper references in comments, if a paper was used in writing the code.
- Rember compilation cost time; try to bunch changes and use `cargo check` to spot issues
- Always run the full test suite (including determinism and headless behavioral tests) after modifying gameplay/simulation code, and verify determinism before shipping.
- Do NOT assume design decisions on my behalf. When a design or scope choice is ambiguous (colors, coverage %, approach), stop and ask before implementing. Prefer focused/concrete changes over global post-process filters or over-engineered solutions.
- When investigating whether an issue is fixed, actually inspect the underlying data/code first before offering explanations; do not assume a file is broken or blame viewport/version.
- Ensure every feature added is correctly included in the RL/QD systems for evolving.
- Keep the file /mnt/codex_fs/game_assets/projects/scp_characters/BEVY_GAME_INFO.md up to date with game info, to ensure our 3D artists are able to make assets that fit the game well. (The old `game_assets/SCP_Characters/` path no longer exists — the project moved under `projects/`.)
- When items are complete, move them from `BACKLOG.md` to `BACKLOG_ARCHIVE.md`.  DO NOT DELETE `BACKLOG_ARCHIVE.md`.
- **Setting up a new machine?** `SETUP.md` — prerequisites, the gitignored `.cargo/config.toml` you must write by hand, both test layers, and the agent-debugger install.
- **Read the vendored Bevy 0.19 source before writing Bevy code** — not bevy.org, which tracks `main`. See "Bevy 0.19" below.
- Consult Bevy documentation often. It can be found at codex_fs/offline_reference_docs/bevy-0.19-book/
- Close the Bevy app as soon as you've finished your testing. Do not leave it running.

## Workspace & crates

Root package is the game (`foundation_vs_slop`, `src/`). Fifteen members live under `crates/`; thirteen mirror to `Ladvien/*` repos. **Seven are public** — `bevy_light_grid`, `bevy_speech_bubbles`, `bevy_orca`, `bevy_stigmergy`, `det_rng`, `map_elites` and `bevy_carnage`, each carrying a runnable example and, where the behaviour is only legible in motion, a gif. The rest stay private. **One publishes to crates.io: `bevy_carnage`.**

**The gore family is one crate, and that is a reversal recorded here because the arguments on both sides were real.** From 2026-09-02 to 2026-09-04 `bevy_carnage` composed seven leaf crates — `bloodstain` (blood as a material, engine-free), `bevy_wetmap`, `bevy_viscera`, and the four kernels `bevy_cross_section`, `bevy_flaymap`, `bevy_laceration`, `bevy_fracture_modes` — each a mirror and a crates.io release, each with a `tests/leaf.rs` forbidding a dependency back on the umbrella. The case for it was de-duplication and reuse; the case against, which won on 2026-09-04 in the review after 0.4.0 shipped, was that a game is meant to depend on `bevy_carnage` alone and a family reached through one door does not need seven version pins, seven READMEs and seven registry entries to keep aligned. **Since 0.5.0 each is a module of `bevy_carnage` under the name it was always re-exported by** — `bevy_carnage::bloodstain` (also `::blood`), `::wetmap`, `::viscera`, `::cross_section`, `::flaymap`, `::laceration`, `::fracture_modes` — so a consumer that followed the README changes nothing. What the boundaries bought is kept as convention: `crates/bevy_carnage/tests/leaf.rs::bloodstain_is_still_engine_free` holds the blood model to no `bevy`, no `glam`, no `std` and `[f32; 3]` in every signature, the conversion has one home in `src/v3.rs`, and each module's former `CLAUDE.md` is carried verbatim in `crates/bevy_carnage/CLAUDE.md`. The seven repositories are archived and their registry versions yanked; `scripts/mirror_crates.sh` no longer names them.

**Fifteen of `bevy_carnage`'s examples also build for `wasm32-unknown-unknown` and are published as a demo site** at `https://ladvien.github.io/foundation_vs_slop/`, from this tree, where the wasm toolchain and the hosting pages live. `scripts/build_web.sh` builds it, `scripts/demo_facts.sh` holds the roster against `web/play.html`'s allow-list, and the `site`/`pages` jobs in `.github/workflows/ci.yml` deploy it. **Wasm demos never enable `vfx`**: `bevy_hanabi`'s wasm support is WebGPU-compute-only and every new visual is CPU-side anyway, so GPU particles stay a native extra.

**`bevy_carnage` came back on 2026-09-01, and the arrow has now pointed three ways.** It was a member with a mirror until 2026-08-16; then [`Ladvien/bevy_carnage`](https://github.com/Ladvien/bevy_carnage) was the source of truth and this repo an ordinary consumer pinned by rev; now it is vendored at `crates/bevy_carnage` with `git subtree add`, history intact — all 32 `AG-*` commits are in this repo's log. **The pin is what forced the move, and the reason is the same one that vendored the agent debugger:** a pinned crate cannot be edited from the checkout that needs it changed, so every fix meant a sibling clone, a push and a rev bump. Phase 5 of that crate is a sustained body of work, and paying that toll per commit is not a plan.

**It does mirror back, and the note that said otherwise was wrong.** The old reason not to was that a `subtree split` carries only commits, so the crate's then-uncommitted audit harness could never reach the published repository. The reason that replaced it — that the two histories are *unrelated*, so a split push could not fast-forward — was simply incorrect, and the evidence is in the vendoring commit itself: `git subtree add` recorded `git-subtree-split: 1434a39` on `eacb160` **and made that commit its second parent**, so `Ladvien/bevy_carnage`'s `main` is an ancestor of this history and an unforced split push fast-forwards. So the crate is in `CRATES` in `scripts/mirror_crates.sh` like the other twelve, **the monorepo remains its source of truth**, and the flow is one-way: monorepo → mirror → crates.io. The registry release is cut from **this tree** with `cargo publish -p bevy_carnage`, never from the mirror, and the game keeps its path dependency rather than the published version — depending on the release would restore exactly the pin that made the crate unfixable while it needed sustained work. It is also no longer the one crate outside `cargo test --workspace`: its three frozen goldens — `fracture_output_is_bit_identical_across_runs`, `hash_f32_is_frozen`, `the_spatter_model_is_frozen` — are inside the gate for the first time.

**It was called `bevy_autogib` until its AG-025, and four families of name here deliberately did not follow it.** The rule that decided each one: **compiler-checked identifiers get renamed; serialized schema and golden-bearing names do not.** So `crate::carnage`, `CarnagePlugin`, `CarnageSystems` and `CarnageCache` moved, and these did not — the `autogib_*` keys in `GoreSettings` and `assets/config/config.ron` (the struct is `deny_unknown_fields`, so a renamed key refuses the authored config at startup), the `autogib_*` gene names in `squad_ai/world_genome.rs` (encode/decode order defines `WorldGenome::N` and every archived elite, so renaming moves every golden hash), `sim_harness::{autogib_ready, step_until_autogib_ready}` and `tests/autogib_determinism.rs` (named in `TESTING.md`'s invariants), and `GibSeq`/`GibKey`/`GibRing`. If you are here because a grep for the old name came back non-empty, that is why, and the full reasoning is beside the dep line in the root `Cargo.toml`.

Each crate is still reached by the path the game always used. **The facade is the API: changing a call site means editing `src/`, not `crates/`.**

| Crate | What it is | How the game reaches it |
|---|---|---|
| `det_rng` | The one seeded ChaCha8 stream + unbiased draws | `emerge_core::rng` re-exports it, so `crate::rng` is unchanged |
| `bevy_orca` | ORCA avoidance: 2-D linear program over discs | `crate::orca` (`src/lib.rs:119`) |
| `map_elites` | QD kernel: archive, three emitters, sep-CMA-ES, POET | `squad_ai::{qd, map_elites, cmaes, poet, interest, …}` (`src/squad_ai/mod.rs:43+`) |
| `bevy_devshot` | Sentinel-file screenshot capture | `crate::devshot` (`src/lib.rs:52`) |
| `bevy_stigmergy` | N influence channels + vectorial rally pheromone | `Stig(StigGrid<CHANNEL_COUNT>)` in `ai::field` (`src/ai/field.rs:134`) |
| `bevy_light_grid` | CPU illuminance grid creatures read | `LightField { core, dirty }` (`src/light.rs:341`) |
| `bevy_speech_bubbles` | World-space speech/thought balloons | `dialogue::{bubble, model}` re-exports (`src/dialogue/bubble.rs:15`) |
| `bevy_carnage` | Deterministic runtime gore: the slicer and watertight caps, the bake-once cache, bullet channels and their ejected plugs, **fracture that knows how it was loaded** (spiral/butterfly/transverse/comminuted, plus greenstick as an outcome), Grady's energy-driven fragment count, and the framework surface — `GoreTier`, the WCAG flash gate, the aim-exclusion cone, the decal budget. Wounds, impact-feel curves, decals and GPU blood sit behind its `vfx` feature. **0.4.0 (2026-09-04) added the pixel half and the preset, both behind `flesh` (default on):** `flesh::FleshMaterial`, an `ExtendedMaterial<StandardMaterial, _>` with per-tissue subsurface pre-integrated from Jensen's dipole, a wet clear coat, the blood film composited from `bloodstain::spectral`, and a dermis map — WebGL2-verified, two CPU-baked tables frozen by a golden — and `preset::{GorePlugin, Gore, GoreHit}`, the whole family behind one plugin: impact, slash, blunt (a `bloodstain::bruise` that ages) and burn (a `bloodstain::burn` that chars), spatter onto floors and cloth, drips, viscera, and the bone handoff that parts a body along its fracture modes. The flagship `carnage_web.rs` is now that preset and nothing else | `crate::carnage` facade + `squad::{FigurineSource, GunModel}` aliases (`src/carnage.rs`) |
| ↳ `bevy_carnage::bloodstain` | *module, was the `bloodstain` crate.* **Blood as a material**, engine-free (no `bevy`, no `glam`, no `std` — ratcheted): Carreau–Yasuda rheology with a Casson yield stress, Comiskey percolation spatter, stain morphology derived from the impact, the six forensic pattern classes, the drying timeline, pools, the bleed schedule, an inverse solver that recovers the wound from the stains, and — **0.3.0** — three injury kernels: `bruise` (Stam 2010's haemoglobin→bilirubin compartment model with a layered Kubelka–Munk colour), `burn` (Pennes bioheat and the Arrhenius damage integral, Gowrishankar 2004), `wick` (Lucas–Washburn imbibition for a shear-thinning fluid, Steinik 2024) | `crate::carnage` facade; `bevy_carnage::bloodstain` |
| ↳ `bevy_carnage::wetmap` | *module, was the `bevy_wetmap` crate.* Texture-space blood accumulation, **CPU-authoritative so it can be hashed** — two `Image`s per actor fed to a plain `StandardMaterial`, and a digest over the wet buffer | `crate::carnage` facade; `bevy_carnage::wetmap` |
| ↳ `bevy_carnage::viscera` | *module, was the `bevy_viscera` crate.* XPBD Cosserat-style strands with a tearing mesenteric membrane: guts that spill, coil, tether and tear | `crate::carnage` facade; `bevy_carnage::viscera` |
| ↳ `bevy_carnage::cross_section` | *module, was the `bevy_cross_section` crate.* Anatomical bands on cut faces: a sourced per-region thickness table, exact depth-below-skin on a convex cell, `UV_1` annotation, and a procedural strip at physical scale with a frozen digest | `crate::carnage` facade; `bevy_carnage::cross_section` |
| ↳ `bevy_carnage::flaymap` | *module, was the `bevy_flaymap` crate.* Texture-space flaying, CPU-authoritative: a depth buffer that peels skin→fat→muscle→cortex under hits and hands the bone off exactly once | `crate::carnage` facade; `bevy_carnage::flaymap` |
| ↳ `bevy_carnage::laceration` | *module, was the `bevy_laceration` crate.* A cut that gapes on a time curve scaled by skin tension and Langer-line orientation, onto a banded wound bed; kernel + plugin | `crate::carnage` facade; `bevy_carnage::laceration` |
| ↳ `bevy_carnage::fracture_modes` | *module, was the `bevy_fracture_modes` crate.* Sellán's fracture modes on a cell graph: fixed-schedule bake, `A_i` impact rows, partition by gluing tolerance | `crate::carnage` facade; `bevy_carnage::fracture_modes` |
| `emerge-core` | Engine-free world building: schemas, IR, solvers, WFC (re-exports `det_rng` as `rng`) | `crate::{geom, rng, wfc}` (`src/lib.rs:169`), `placement::{ir, …}` (`src/placement/mod.rs:26`) |
| `emerge-anim` | Pose blender | `crate::anim` (`src/lib.rs:23`) |
| `emerge-bevy` | Library + map → entities | `src/emerge_map.rs` |
| `emerge-mapper` | Standalone editor app — **not** a game dependency | `cargo run -p emerge-mapper` |
| `bevy_debugger_mcp` | Agent debugging: MCP server binary + the `bevy_debugger_bevy` plugin | `bevy_debugger_bevy::DebuggerPlugin` behind `--features debugger` (`src/lib.rs:374`) |

**Inside `squad_ai`, write `::map_elites::` with leading colons for the crate.** `squad_ai::map_elites` aliases `::map_elites::loops`, so the bare path resolves to the module, not the crate (`src/squad_ai/mod.rs:40`).

**The agent debugger is vendored whole, and that was a correction.** `crates/bevy_debugger_mcp/` holds both halves of [`Ladvien/bevy_debugger_mcp`](https://github.com/Ladvien/bevy_debugger_mcp) — the MCP server binary and the companion Bevy plugin the game links — brought in with `git subtree add`, history intact, and mirrored back out like every other crate.

It used to be a git dependency pinned to a rev, edited through a `[patch]` to a sibling clone. **That pin made the thing unfixable at the moment it needed fixing:** `bevy_debugger/input` answered `success: true` and moved nothing, because BRP handlers run in `Last` and Bevy clears `just_pressed` in the next `PreUpdate` — so every `just_pressed` action was unreachable by injected input. Repairing that meant editing another repo, cutting a rev and bumping. It is now an ordinary edit. `scripts/sync_debugger.sh` and the `[patch]` recipe are gone with the pin; keeping either alongside a vendored copy would be two paths to the same crate.

The game still reaches the plugin only through the optional `debugger` feature, so it is absent from every default and release build — `cargo tree -i bevy_debugger_bevy` finds no package unless the feature is on, and neither does `bevy_remote`.

**One caveat, measured rather than assumed:** that is true of the game's own build, not of `--workspace`. `cargo tree --workspace -i bevy_remote` *does* match, because Cargo unifies features across everything one build compiles and `bevy_debugger_bevy` needs `bevy/bevy_remote`. So the `--workspace` gate compiles a differently-featured `bevy` than a shipped build does. No extra system runs (adding `RemotePlugin` still requires the feature), but if a golden ever moves after touching the debugger's dependencies, this is the first thing to check.

The custom BRP methods and the `DebuggerPlugin`-owns-`RemotePlugin` trap are in `docs/bevy_debugger_mcp.md`; the crate's own non-negotiables are in `crates/bevy_debugger_mcp/CLAUDE.md`.

**Licensing is split on purpose.** The six `bevy_*`/`map_elites` libraries are **MIT OR Apache-2.0** with `publish = false` — a GPL crate in the Bevy ecosystem is unadoptable. The four `emerge-*` crates stay **GPL-3.0** with the game they were carved out of. `bevy_orca` also carries a `NOTICE`: it keeps RVO2's function names.

### Changes flow one way: monorepo → mirror

Mirrors are `git subtree split` of `crates/<name>/`, history intact. **Nothing is ever edited on the far side and nothing is ever pulled back.** Re-sync with `scripts/mirror_crates.sh` (idempotent — a no-op run reports `Everything up-to-date`).

A push that is **not** a fast-forward is the correct outcome, not a problem to route around: monorepo history was rewritten under the mirror, a human decision. Never `--force` past it; the script's header carries the deliberate resolution.

`crates/<name>/` *is* the mirror's root, so anything a standalone reader needs lives there: `README.md`, `CLAUDE.md`, any future `crates/<name>/.github/workflows/ci.yml` (GitHub ignores that path here; the split lifts it to where it runs).

**Each mirrored crate has its own `CLAUDE.md`** carrying that crate's non-negotiable — engine-free ratchets, "caller owns the schedule", no-transitions, the single spawner. Read it before editing under `crates/`; `scripts/mirror_crates.sh` refuses to mirror a crate that lacks one.

### Making a new crate — the checklist behind the rule up top

`docs/bevy_plugins.md` is the reference. What a crate must have at `crates/<name>/`, because that directory *is* its repo root:

- `README.md` opening with the **"Vibe Coded"** warning (see any existing crate for the wording), then the mirror notice, then an **Examples** section listing what to run.
- `examples/` with **1–3 runnable examples**. This is what lets someone judge the crate without building the game — prefer terminal output over a window when the idea can be shown that way, since a terminal example runs anywhere and costs no GPU.
- `CLAUDE.md`, `Cargo.toml` with `publish = false`, and `LICENSE-MIT` + `LICENSE-APACHE` (or `LICENSE` for the GPL `emerge-*` family).
- A `tests/leaf.rs`-style **dependency ratchet** naming the crate's allowed deps, so widening the boundary costs a deliberate edit rather than a passing build.
- The crate's name in `CRATES` in `scripts/mirror_crates.sh`, then `gh repo create` under an idiomatic name — `bevy_*` for a Bevy-ecosystem library, plain for an engine-free one.

`README.md`, `CLAUDE.md`, `Cargo.toml`, a license, `examples/*.rs`, and the "Vibe Coded" string are all **enforced by the mirror script**; it refuses to push a crate missing any of them.

Conventions from `docs/bevy_plugins.md` that `bevy_lint`'s `unconventional_naming` checks: **plugins end in `Plugin`, system sets end in `Systems`.** Also expected on new crates — `#![doc = include_str!("../README.md")]` in `lib.rs` (which makes the README the crate's front page, and it is already the mirror root), a Bevy compatibility table, and an explicit list of exposed `SystemSet`s and components. For a crate whose `CLAUDE.md` insists the caller owns the schedule, that list *is* the contract. Existing code does not meet the `Systems` suffix — all 10 `SystemSet` derives predate it (`gore::GibEconomy`, `ai`, `fog`, `health`, `light`, …) — so renaming those is its own change.

**A crate is for the reusable kernel, not the game content around it.** If the public API has to name a game type, extract the kernel and keep the game's vocabulary in the facade — the way `ai::field` owns the dungeon's world↔cell mapping and `bevy_stigmergy` never learns what a wall is.

### Facade newtypes, not traits

`Stig` and `LightField` wrap the crate type, keep the exact signatures, and delegate — so the extraction changed **zero call sites**. Reuse that shape; both alternatives are dead ends:

- `trait CellMap` can't work. Call sites pass `&dungeon` where `dungeon: Res<Dungeon>`, inference picks `M = Res<Dungeon>`, and the orphan rule forbids implementing a foreign trait for it — so all ~40 sites would need `&*dungeon`.
- `&dyn` puts a virtual call in the diffusion inner loop.

Game-shaped state stays in the shell: `LightField::dirty` is bake-gating for *this game's* fixtures, and a grid tracking it would guess at a schedule it doesn't own. Occlusion crosses the boundary as `los: impl Fn(IVec2, IVec2) -> bool` — monomorphised at the call site, returns a `bool`, so it can't perturb a float.

Editing a crate is editing the game — hence the `--workspace` warning under **Testing**.

Deeper rationale, already written: per-dependency comments in the root `Cargo.toml`, the header of `scripts/mirror_crates.sh`, `docs/2026-08-08-handoff.md`.

## Bevy 0.19 — read the vendored source, not the web

Pinned to **`bevy 0.19.0`**. bevy.org documents `main` and has been wrong for us more than once: every Bevy question answered *correctly* here came from the local copy, every one answered from memory or the web cost a build-and-run cycle. **Check before writing Bevy code, not after a failed build.**

All authoritative copies are local. `$BEVY` = `~/.cargo/registry/src/index.crates.io-*/bevy-0.19.0`:

| Source | Use it for |
|---|---|
| **`$BEVY/examples/` — 411 examples at exactly our version** | *"How do I use X?"* — grep here **first**. `3d/render_to_texture.rs`, `asset/asset_saving.rs`, `ui/`, `gizmos/`, `scene/`, `picking/` cover most of what this project reaches for. |
| **Crate source** — `~/.cargo/registry/src/*/bevy_{ecs,camera,ui,ui_widgets,gizmos,scene,light,image,picking}-0.19.0/src/` | Any API question in seconds, never stale: field vs component, required components, exact signatures. |
| **Rustdoc** — `cargo doc -p bevy --no-deps` → `target/doc/bevy/index.html` | Browsable index of the pinned API. `target/` is gitignored; rebuild after `cargo clean`. |
| **`$BEVY/_release-content/migration_guides.md`** | What moved since 0.18. |

### Traps already paid for

0.19 facts, verified in the source above. Each cost real time.

- **A missing `Res<T>` panics the system**, it does not skip. Take `Option<Res<T>>`, or have the plugin that registers the reader `init_resource` it.
- **Every condition in a `.run_if(..)` chain is evaluated — the chain does not short-circuit.** A bare `Res<T>` in a second `.run_if(..)` panics whenever that resource is absent, even behind an earlier condition that returned false. It shipped, crashing every launch that didn't use the feature. Use `Option<Res<T>>`. **The combinators are the other way round:** `and_then`/`or_else` short-circuit (`OrElseMarker::combine` is `a || b`, bevy_ecs 0.19.0 `schedule/condition.rs:1564`); `and_eager`/`or_eager` do not (`|`, line 1586). A condition holding a `Local` — including bevy's own `run_once` — must be joined with `or_eager`, or it silently stops updating on the frames the left-hand side already answered.
- **`Single<..>` silently skips its system** on a non-unique match, so **any second camera breaks every `Single<.., With<Camera3d>>`** — the audio listener, every billboard, `selection`, `drive_camera`. Filter positively on `crate::MainCamera`, never on `With<Camera3d>` alone.
- **A bundle containing two of the same component panics.** `button_visual()` already carries a `Node`; `.insert()` afterwards to override, do not pass a second one in the tuple.
- **`TransformGizmoPlugin` is unusable here.** Its overlay camera blanks an HDR main camera's output (measured: 13,343 → 183 distinct colours) *and* spawns the second `Camera3d` above.
- `Camera::clear_color` is a **field**; `RenderTarget` is a **separate component** (one of `Camera`'s `#[require]`s). `AmbientLight` is a **component** in 0.19 and applies per-camera.
- `add_plugins` tuples cap at **15** (`all_tuples!(impl_plugins_tuples, 0, 15, P, S)`, `bevy_app-0.19.0/src/plugin.rs:186`). Nest to go further.
- `Resource` is now a subtrait of `Component`; you cannot derive both.

`docs/ui.md` §5 carries the UI traps (grid placement panics, the 95-codepoint default font, `Pickable::IGNORE` on full-screen containers, layer ordering).

## Testing

**Read `TESTING.md` before writing or running tests** — what exists, how to run it, how to add to it. The one-liners:

- `cargo test --workspace` — deterministic-core layer (RNG/WFC/utility/ORCA/laser). Fast, GPU-free, the CI hard gate. **`--workspace` is load-bearing:** this workspace has a root package, so bare `cargo test` compiles no test target under `crates/` — that is how `crates/emerge-anim`'s 21 tests left the gate without anything going red.
- `cargo test --features test-harness -- --test-threads=1` — headless replay / liveness / SSIM. Boots the real game with no window; **needs a GPU**.

Non-negotiables: exact-hash only the **physics-off** core (`SimConfig::deterministic_core()`) — the Avian solver is not bit-reproducible, so physics-on runs use **liveness** oracles; hold `serial_guard()` in every harness test; new systems go on `FixedUpdate` if they touch pinned state (would appear in `snapshot_hash`), else `Update`. Strategy, oracle rules, and the full invariant list: `TESTING.md` §Strategy, §Invariants & determinism rules.

## Animation

**Read `docs/animation.md` before touching `src/anim/`, squad/crab/manca clip wiring, or a character GLB's clips** — it's the engineering guide; `docs/artist_guide.md` §4 holds the per-asset clip tables and the authoring contract.

The one idea: **no transitions.** Every clip stays resident on the `AnimationPlayer` and is never rewound — each frame the shared `anim::PoseBlender` only eases weights and advances one shared gait phase. So **never add `AnimationTransitions` to anything the blender drives** (its `PostUpdate` pass would stomp the weights).

Non-negotiables: the layer is **cosmetic** — `Update` only, never `FixedUpdate`; `PoseBlender` rides the **model child**, not the sim entity (issue #18); and it is the **deliberate exception** to the "wire every feature into RL/QD" rule above — invisible to `snapshot_hash` by construction, so a genome gene pointed at it would never move fitness. Cosmetic tuning goes in the `src/anim`/`src/squad` constants and `docs/artist_guide.md`, not the evolving systems. Gait clips must be authored **in-place** (zero root motion) with an honest per-cycle ground distance; `tests/valkyrie_asset.rs` pins the GLB contract.

## Determinism: ECS query order decides nothing

Query order is **not stable across `App` instances**. Anything it could decide — a shared RNG draw or counter, a `take(n)` budget, a clamped accumulate, a last-writer-wins write, a lethal pick — needs a stable **total** key: `sort_total!` (panics on a tie, naming the site), `util::sort_value_canonical` (ties interchangeable → sort the WHOLE value), or `// SORT-OK: <why>`. `tests/determinism_lint.rs` enforces it.

Four sites documented the exact trap they then fell into, so don't trust a comment claiming a total order. Both shapes: a key that is a **prefix of the value** (`(pos)` when the element is `(pos, payload)`), and a **tiebreak derived from the tied quantity** — `GibKey` hashed the position it existed to disambiguate.

A determinism probe on an idle box proves nothing: run it under load.

## Assets: what git carries, and what it does not

**A derived asset is build output.** `assets/ozea_kit/*.glb` is what `scripts/fbx_to_glb.py` makes from an `.fbx` on the library share — a resource compiler's output in exactly Gregory's sense (*Game Engine Architecture* 3e §7.2.1: source assets in native DCC formats pass through exporters and resource compilers on their way to the engine). Committing it is committing build artifacts, and git keeps every version of every binary forever: `assets/scp610/scp-610.glb` is in this repo's history **three times, at 27 MB, 5 MB and 4 MB**.

So git carries `assets/derived.json` — the recipe, and a **sha256 per output** — and `.gitignore` carries the bytes away. 418 Ozea meshes, 114 MB, described by 87 KB of manifest.

```sh
cargo fvs assets verify   # do the files on disk match the manifest?  (the default)
cargo fvs assets sync     # copy them from the cache on the share — no Blender, what a clone runs
cargo fvs assets stage    # put what is on disk into that cache, for everyone else
cargo fvs assets build    # regenerate from source with Blender, then verify and stage
```

The hash is not decoration. Blender's exporter is not promised to be byte-identical across versions, and an asset that quietly changed shape surfaces as a placement looking wrong three weeks later — Lamb & Zacchiroli's reproducible-builds argument (`10.1109/ms.2021.3073045`), made concrete. `verify` turns a drift into a named failure.

**The cache is on the library share**, content-addressed by that hash, at `$FVS_ASSET_LIBRARY/fvs_derived_cache` (default `/mnt/codex_fs/game_assets`). One machine with Blender pays the conversion; every other machine copies. Regenerate a manifest with `scripts/manifest_from_staging.py` — never by hand.

**Hand-authored binaries are the other class** — `assets/characters/`, `assets/scp610/`, anything out of TRELLIS. No script derives them, so a recipe cannot restore them. They go in **git-annex**, whose store is a plain directory on the same share:

```sh
git annex add assets/characters/new_rig.glb   # content to the annex, a pointer to git
git annex copy --to codex                     # push the bytes to the share
git annex get assets/characters               # pull them down on another machine
```

The remote is `codex` → `/mnt/codex_fs/fvs_annex`, `type=directory`, no encryption. No server, no daemon — it rides the NFS mount that is already there. `git annex info` lists it.

**Existing history is left alone, deliberately.** `git lfs migrate` and friends rewrite history, and this repo `subtree split`s eleven crates to mirrors, where a non-fast-forward push is a human decision that is never forced. So the annex takes **new** binaries; the 80 MB already in the pack stays where it is. Moving an existing asset in is an ordinary commit (`git rm --cached` then `git annex add`) — it stops *future* versions accumulating without touching the past.

## Additional game assets

Cataloged at `/mnt/codex_fs/game_assets/CATALOG.md` — use any of them.

## Screenshots and input — agents use BRP, and only BRP

**An agent looking at or driving this game uses `bevy_debugger_mcp` over BRP. Nothing else.** Run the game with `cargo run --features debugger`, then:

- `bevy_debugger/screenshot` — offscreen capture. Writes a PNG from an `Image` the mirror camera renders to, with optional `region` and `zoom`.
- `bevy_debugger/input` — keyboard, mouse, scroll **and cursor position** written into the game's own input state. Any `KeyCode` variant by name: `KeyW`, `F5`, `ShiftLeft`, `Numpad7`. `kind: "Cursor"` takes `x`/`y` in logical window pixels or `clear: true`, so a click-drag is expressible: aim, press, move, release.

  **A cursor injection only reaches a system that reads it.** The pointer lands in a `DebugCursor` resource, never in the window's own cursor — writing that makes Bevy's windowing backend move the *physical* mouse, which is the one thing this whole path exists to avoid. So a system calling `Window::cursor_position` directly is undrivable by an agent; it has to go through `bevy_debugger_bevy::cursor_position(&window, &debug_cursor)`. `emerge-mapper` does this once, in `view::Pointer`.

**This is the single path because the alternatives are not equivalent — they are worse in a specific way.** Capturing the window requires the window to be on screen, so it means raising the game: stealing focus, possibly switching Spaces, interrupting whoever is at the machine. Measured, same scene, one variable: **7,188 distinct colours focused, 1 unfocused**. Driving the OS keyboard is worse still — the keystrokes land in whatever window actually has focus, which may be someone's editor.

So these are **forbidden** for an agent, and a hook blocks them: macOS `screencapture`, `scripts/macinput.py`, `scripts/vinput.py`.

Details and the `DebuggerPlugin`-owns-`RemotePlugin` trap: `docs/bevy_debugger_mcp.md`. The debugger lives at `crates/bevy_debugger_mcp/`, so fixing it is an ordinary edit here — no pin to bump, and no sibling checkout to keep in sync.

**`emerge-mapper` speaks BRP too, behind the same feature.** `cargo run -p emerge-mapper --features debugger` adds `DebuggerPlugin` and the HTTP transport, so an agent drives the editor exactly the way it drives the game — and it reads the same offscreen image the window shows (`crates/emerge-mapper/src/surface.rs`), so the capture includes the panels. There is no separate mirror camera; `debug_capture.rs` was deleted with the rig it owned on 2026-08-18. It was carved out as a devshot-only caller while the debugger was a pinned git dependency; vendoring made that a choice rather than a constraint, and `bevy_debugger/input` is what the sentinel driver's verb list was always a hand-built stand-in for.

The port is **`BEVY_BRP_PORT`** — the variable `bevy_debugger_mcp`'s own config already reads, so one knob points both ends at the same socket. It defaults to 15702, which the game also uses: **running both with the debugger on at once needs the variable set**, or the second process fails to bind.

**`bevy_devshot` captures the whole frame including UI, and serves the game's player-facing Ctrl+P.** In the **game** it is still the only way to see a panel: Bevy renders a UI tree to one camera, so the game's mirror camera never receives the interface.

**In `emerge-mapper` that is no longer true, as of 2026-08-18.** The editor draws its world *and* its interface into a single offscreen image and shows that image in the window (`crates/emerge-mapper/src/surface.rs`), so `bevy_debugger/screenshot` returns the panels — with a region and a zoom — and needs nobody's screen. Reach for devshot there only for the player-facing path. If a shot of the *game's* interface is needed, that is still a devshot request; it is never a reason to raise the window yourself.

**Player region captures live in `debug_screenshots/`.** Ctrl+P drags a box and saves *just that region* to `debug_screenshots/region_<timestamp>.png` — a deliberate "look here" pointer. So if the player references something visual, **check `debug_screenshots/` newest-first and read `debug_screenshots/CLAUDE.md`.** From `src/region_capture.rs` (dev-only, stripped from release).
