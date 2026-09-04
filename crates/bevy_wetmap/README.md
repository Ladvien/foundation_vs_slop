# bevy_wetmap

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

Texture-space blood accumulation for Bevy: paint a world-space hit into a mesh's own UVs, then let it run downhill, creep into its neighbours, soak into the substrate and dry in place — **on the CPU, on an integer tick, so you can hash it.**

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. Issues and PRs belong upstream.

![A pale sphere shot six times; each stain runs under gravity, spreads, and dries to brown in place](https://raw.githubusercontent.com/Ladvien/bevy_wetmap/main/docs/wetmap.gif)

## The family

This crate is one of eight that make up one gore stack. **`bevy_carnage` is the umbrella**: depend on it alone and every kernel below is re-exported under its own name, so a game needs one dependency line and can never end up with two versions of a leaf. Depend on a kernel directly only when you want it without the rest — each one stands alone, and none depends on `bevy_carnage` back.

| crate | what it is | reach it as |
|---|---|---|
| [`bevy_carnage`](https://github.com/Ladvien/bevy_carnage) · [crates.io](https://crates.io/crates/bevy_carnage) | the umbrella — plane cuts with watertight caps, bores, energy-driven fracture, wounds, decals, impact feel; **re-exports every crate below** | `bevy_carnage` |
| [`bloodstain`](https://github.com/Ladvien/bloodstain) · [crates.io](https://crates.io/crates/bloodstain) | blood as a material: Carreau–Yasuda rheology, Comiskey spatter, stain morphology, drying, spectral colour by thickness and oxygenation — engine-free, `no_std` | `bevy_carnage::blood` |
| **`bevy_wetmap` — this crate** | texture-space blood that runs, spreads and dries — CPU-authoritative, so a canvas can be hashed | `bevy_carnage::wetmap` |
| [`bevy_viscera`](https://github.com/Ladvien/bevy_viscera) · [crates.io](https://crates.io/crates/bevy_viscera) | XPBD strands with a tearing mesentery: guts that spill, coil, tether and tear | `bevy_carnage::viscera` |
| [`bevy_cross_section`](https://github.com/Ladvien/bevy_cross_section) · [crates.io](https://crates.io/crates/bevy_cross_section) | anatomical bands on a cut face from a sourced per-region thickness table, via `UV_1` | `bevy_carnage::cross_section` |
| [`bevy_flaymap`](https://github.com/Ladvien/bevy_flaymap) · [crates.io](https://crates.io/crates/bevy_flaymap) | texture-space flaying: skin, fat, muscle, cortex peel under hits, with a once-per-canvas bone handoff | `bevy_carnage::flaymap` |
| [`bevy_laceration`](https://github.com/Ladvien/bevy_laceration) · [crates.io](https://crates.io/crates/bevy_laceration) | a cut that gapes on a time curve, scaled by skin tension and Langer-line orientation | `bevy_carnage::laceration` |
| [`bevy_fracture_modes`](https://github.com/Ladvien/bevy_fracture_modes) · [crates.io](https://crates.io/crates/bevy_fracture_modes) | Sellán's fracture modes on a cell graph: a fixed-schedule bake, impact projection, gluing partition | `bevy_carnage::fracture_modes` |

Every crate is deterministic where it can be — fixed schedules, no clocks, frozen digests over its CPU state — and every one carries the same *Vibe Coded* warning as this file. The four added on 2026-09-04 (`bevy_cross_section`, `bevy_flaymap`, `bevy_laceration`, `bevy_fracture_modes`) are kernels `bevy_carnage` composes; `bloodstain` is the one with no engine in it at all. Fourteen of the family's examples run in a browser at [ladvien.github.io/foundation_vs_slop](https://ladvien.github.io/foundation_vs_slop/).

## Why CPU-authoritative is the whole point

Texture-space wound and blood accumulation exists in shipped AAA — *The Last of Us Part II* keeps blood and wound render targets per character — and everywhere it exists it is a **GPU render target**. That is why nobody can hash one. A render target is written by a fragment shader and read back, if at all, through an asynchronous copy that arrives some frames later; its contents are a function of driver rounding, of raster order, and of when the readback landed. It is a picture, not a state.

This one keeps the state on the CPU in integers and treats the GPU as **output only** — the rule `bevy_carnage` states at `src/vfx.rs:6-19` and this crate inherits. Two consequences, and they are the product:

- **`digest()` is a fold over the blood, not over a screenshot.** FNV-1a over `(amount, age)` in row-major order. Two runs of the same scripted hits give the same `u64`; one hit moved by a single texel gives a different one. So a wetmap can be a golden, which as far as we can tell has not been true of one before.
- **Nothing is read back, ever.** `Assets<Image>` is written by exactly one function (`WetCanvas::flush`) and read by nobody. A future idea of the form "sample the wetmap in a shader and feed it to gameplay" has to be refused — the CPU buffer is already there and is already the authority.

There is **no custom shader and no shipped asset.** The crate owns two `Image`s per actor and hands them to a plain `StandardMaterial`. The dry surface underneath is composited *into* the canvas on the CPU, so there is nothing left to blend in WGSL.

## The model

State per texel is `amount: u8` (normalised coverage) and `age: u16` (ticks since the youngest blood there landed) — **one `Vec<(u8, u16)>` in row-major order**, which *is* the canonical order the tick pass walks. No sort is needed and none may be added.

`tick` runs four passes, in exactly this order:

1. **Drip.** A wet texel holding more than `drip_rate` sheds the **excess** one texel along `gravity_uv` and keeps the rest, so a run leaves a trail instead of translating wholesale like a sprite. The step is quantised to the dominant axis: a fractional step would need interpolation, which is a second movement model. What leaves a texel arrives in exactly one other, or is lost at the border — nothing else, because the step is a translation and at most one texel can step into another. It is **conserved to the byte wherever the destination is also wet**: a wet destination sheds down to the threshold, so its residue is at most `threshold` and the parcel is at most `255 − threshold`, which sums to exactly 255 and needs no clamp. The one lossy case is blood running onto a dry crust that is *already saturated*, and there the loss is the model rather than a rounding error — `amount` is normalised coverage, so a texel at 255 is fully covered and more blood on it is not representable because it would not be visible.
2. **Spread.** `spread_rate` diffuses into the 4-neighbourhood, written as an **antisymmetric flux** on each edge's coverage difference, `round(spread_rate/4 · (aᵢ − aⱼ))`. `f32::round` is odd, so the flux one way is exactly minus the flux the other: conserved to the byte again.
3. **Time.** `age` increments and the substrate takes its cut. Age stops at `dry_ticks`, which makes a fully dried canvas a **fixed point** — nothing moves, no byte changes, and no upload is requested.
4. **Shade.** `bloodstain::dry::appearance` writes the sRGB and roughness bytes: oxyhaemoglobin → methaemoglobin → hemichrome for colour, and `wet_roughness` → `dry_roughness` for gloss.

Both moving passes read a snapshot taken at their own top and write disjoint slots, so **traversal order cannot change the answer.** That is the property that makes the digest worth taking.

**Dry paint does not move.** Wetness gates every change to `amount` in passes 1–3, which is what makes a run stop where it stopped rather than creeping forever, and what stops a dried crust from soaking away.

`lib.rs` is `#![doc = include_str!("../README.md")]`, so this is a doctest and the compiler checks it:

```rust
use bevy::prelude::*;
use bevy_wetmap::{StainShape, WetCanvas, WetSettings};

// Once, when the actor spawns. `WetmapPlugin` gives you `WetSettings` and the upload budget.
fn wire_up(
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> (WetCanvas, Handle<StandardMaterial>) {
    let canvas = WetCanvas::new(images, 128, [0.78, 0.66, 0.60], 0.55);
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(canvas.albedo()),
        metallic_roughness_texture: Some(canvas.roughness()),
        // REQUIRED: Bevy multiplies these scalars by the texture, so the shipped values
        // would scale the map away. See `bevy_pbr-0.19.0/src/pbr_material.rs:157-163`.
        perceptual_roughness: 1.0,
        metallic: 1.0,
        ..default()
    });
    (canvas, material)
}

// Per hit, from your own gunfire code; then once per fixed tick, from your own schedule.
// This crate has no clock, so the tick number comes from you.
fn on_hit(
    canvas: &mut WetCanvas,
    mesh: &Mesh,
    actor: &GlobalTransform,
    muzzle: Vec3,
    direction: Vec3,
    shape: &StainShape,
    settings: &WetSettings,
    tick: u32,
) {
    canvas.paint_world(mesh, actor, muzzle, direction, shape, tick);
    canvas.tick(tick, Vec2::new(0.0, 1.0), settings);
}
```

## Compatibility

| `bevy` | `bevy_wetmap` |
|---|---|
| 0.19 | 0.2 |

## What it exposes

| Kind | Name | What it is |
|---|---|---|
| Plugin | `WetmapPlugin` | Adds `WetSettings` and the upload budget on `Update`. Registers nothing else. |
| `SystemSet` | `WetmapSystems` | The one system above, so you can order against it. |
| Component | `WetCanvas` | One actor's canvas. Holds the buffer and the two image handles. |
| Resource | `WetSettings` | Six dials: `drip_rate`, `spread_rate`, `dry_ticks`, `absorbency`, `max_canvas_updates_per_tick`, `humidity`. |
| Constant | `UV_SPAN_M` | How much world one UV unit is taken to be — see below. |
| Re-export | `StainShape`, `bloodstain` | The silhouette `paint_*` takes, and the blood model it comes from. |

`WetCanvas`: `new`, `albedo`, `roughness`, `paint_uv`, `paint_world`, `tick`, `flush`, `digest`, `wetted_area`, plus the read-only `size`, `amount_at`, `age_at`, `is_dirty`, `dirty_since`.

## Two things the caller owns, and one convention

**The schedule.** `tick` takes the tick *number*. Nothing here reads a clock, virtual or real, because a wetmap that read one could not be a golden. Call it from your own `FixedUpdate` before `Update`, and the plugin uploads what changed.

**Which way is down.** `gravity_uv` is in UV space. Whether `+v` runs down an actor's chest or across it is a property of the atlas, and only you know it.

**One UV unit is one metre** (`UV_SPAN_M`). A texture coordinate carries no scale, so the bridge from `StainShape::major` in metres to texels has to be stated somewhere; it is stated once, as a constant, rather than as a per-canvas dial every call site would have to agree on.

## The upload budget, and why the default size is 128

A 128×128 canvas at `Rgba8UnormSrgb` is `128 · 128 · 4 = 65 536` bytes — **64 KB per upload** — and this crate owns **two** images per actor (albedo and metallic-roughness), so one canvas is 128 KB of `Assets<Image>` writes each time it is flushed. At the shipped `max_canvas_updates_per_tick = 4` that is **512 KB per frame**. At 256 it is 2 MB/frame and at 512 it is 8 MB/frame, which is a bandwidth budget rather than a texture.

So **128 is the default and 256 is the practical ceiling.** Blood reads fine at 128 because a stain is a blob, not text.

`flush` uploads **only when dirty**, and the plugin picks the oldest-dirty canvases first by a stable key, so a canvas that has been waiting cannot be starved by one that keeps being repainted.

## One interpretation worth knowing about

**`absorbency` is a fraction of the wet *lifetime*, not a fraction per tick**, and the arithmetic is why. Read per tick, the shipped `0.15` leaves `0.85³⁰ ≈ 0.008` of the blood after half a second: every stain would vanish long before `dry_ticks = 1800` could dry it, and a wetmap would never show a dried stain at all. Read as a lifetime fraction it is exactly what its name says — a soaking substrate keeps 15 % of what lands on it, spread evenly across the drying. It is applied as an integer schedule (`floor(age · round(255 · absorbency) / dry_ticks)`, differenced), so the per-tick deltas sum to the cumulative exactly and there is no float residue to drift. Faint spatter soaks away to nothing; a pool loses 15 % and stays.

## References

- Möller & Trumbore, *Fast, minimum storage ray-triangle intersection*, J. Graphics Tools 2(1), 1997 — `doi:10.1080/10867651.1997.10487468`. The UV lookup, reimplemented here rather than borrowed from the gore crate that composes this one.
- Fowler, Noll & Vo, FNV hash (1991) — the digest.
- Oum, Lieberman & Aylward, *A feel for disgust: tactile cues to pathogen presence* — `doi:10.1080/02699931.2010.496997`. Why roughness is a first-class channel rather than a darker albedo.
- Laan et al., *Morphology of drying blood pools* — `doi:10.1016/j.forsciint.2016.08.005`, and Smith, Nicloux & Brutin — `doi:10.1038/s41598-020-65465-4`. Reached through `bloodstain::dry`, which owns the timeline.

## Examples

```sh
cargo run -p bevy_wetmap --example canvas_digest   # terminal: two identical runs, two equal digests, and one that differs
cargo run -p bevy_wetmap --example wetmap_paint    # click-paint a sphere and watch it run and dry. Needs a GPU.
```

`canvas_digest` is the crate's headline claim, run as a program: it paints a scripted sequence into a scratch `Assets<Image>` twice, prints both digests, then moves a single hit by **one texel** and prints the third. No window and no GPU, so it runs anywhere — including over ssh, which is how a claim about reproducibility ought to be checkable.

`wetmap_paint` is the same model with a renderer attached, and it is also **demo 8** on the project's demo site. Click to shoot the subject, `Space` for a burst, `G` to turn gravity on the texture, `D` to hide the digest. Watch blood run down the geometry, thin as the substrate takes its cut, and set in place. The digest is on screen because the interesting thing about this crate is that a picture of blood has a number under it.

That key legend is duplicated verbatim into the site page's `#notes-wetmap_paint` block, and **the page is the spec** — if the two ever disagree, the page is right and the example is wrong.

## License

MIT OR Apache-2.0, at your option.
