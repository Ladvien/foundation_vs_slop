# Meshing crate architecture — Bevy, wgpu, naga, and the CAD constraint

**Date:** 2026-08-11
**Question:** how to structure a project implementing these meshing algorithms on Bevy + Naga + naga_oil,
with the meshing crate as light as possible, Bevy only for examples, and reuse in CAD.
**Method:** verified against crates.io API, published `.crate` tarballs, real `Cargo.toml` files, the
`bevyengine/bevy-website` migration guide source, and the wgpu mesh-shading spec. Version numbers are
measured, not remembered. Anything unverified is marked.

---

## 0. Three corrections to the premise, up front

**1. Drop naga_oil.** Last release **v0.18, 2025-06-26** — 14 months stale. It is a Bevy-owned crate
solving a Bevy problem. Worse, Bevy 0.19's own release notes say: *"We've supported WESL for more than
a year"* and they plan to *"port our existing internal shaders to use WESL, and endorse it as the
shader language of choice for Bevy."* naga_oil is the outgoing path. Depending on it would put a
Bevy-shaped, deprecating dependency into a crate whose whole point is being engine-agnostic.

What you actually want from it — `#import` and `#ifdef` in WGSL — has two better answers, in §5.

**2. Naga, yes — but not as a runtime dependency.** You already get naga transitively through wgpu.
The genuinely valuable direct use is **offline validation in CI**: parse and validate every shader
permutation in a unit test, on a machine with no GPU. That's a real win and it's cheap. Details in §5.

**3. Mesh shaders are reachable from inside Bevy — wgpu's maturity is the blocker, not Bevy.** wgpu
**v28 shipped `Features::EXPERIMENTAL_MESH_SHADER`**; Vulkan and DX12 HAL backends are merged; WGSL
frontend parsing is done (`enable wgpu_mesh_shader;`, `@task`/`@mesh` attributes). Bevy 0.19 pins wgpu
**29.0.3**, so the feature is *in your dependency graph today*, and Bevy gives you both hatches you
need: `WgpuSettings.features` to request it at device creation and
`RenderDevice::wgpu_device(&self) -> &wgpu::Device` to use it. See §6.1 for the exact mechanism.

What's actually missing is wgpu-side: the spec says *"expected to undergo breaking changes"*, there's
an open redesign issue (#9170), it does not work in browsers, and Bevy's own docs warn that
`WgpuSettings.features` *"may cause renderer initialization to fail"* on adapters that don't support
what you ask for. So the 23.4× from the speed analysis is a **year-two item behind a cargo feature and
a runtime capability check**, not a v1 target. Design so it's reachable; don't build on it now.

---

## 1. Is a new crate even justified?

Yes. The gap is specific and verifiable.

| Crate | Latest | Last release | Verdict |
|---|---|---|---|
| `fast-surface-nets` | 0.2.1 | 2025-01-03 | Closest thing. **Disqualified for CAD**: surface nets only (no sharp features/QEF), `SignedDistance: Into<f32>` forecloses f64, pins **glam 0.29** |
| `isosurface` (swiftcoder) | 0.1.0-alpha.0 | 2021-01-29 | **Right architecture, dead crate.** Zero deps, own `Vec3`, `Extractor` sink trait, MC + EMC + DC. Its 2 dependents still pin the 2018 `0.0.4` |
| `block-mesh` | 0.2.0 | 2022-04-17 | Frozen 4y but still the Bevy blocky standard (7 live dependents). Blocky quads only |
| `building-blocks` | 0.7.1 | 2021-09-23 | **Dead — repo archived 2023-11-13** |
| `tessellation` | 0.11.0 | 2026-03-29 | Healthy Manifold DC — **nalgebra-locked** |
| `fidget-mesh` | 0.5.0 | 2026-08-03 | Very active (Keeter) — nalgebra-locked, and coupled to fidget's tape evaluator |
| `transvoxel` | 2.0.0 | 2025-11-11 | Alive, self-describes as experimental, **0 dependents** |
| `fornjot` | 0.49.0 | 2024-03-21 | **Dead — archived 2026-06-19.** README: *"This project has been shut down. Its goals were never reached."* |
| `truck` | 0.4–0.6 | all 2024-09-20 | Dormant 23 months. Uses **cgmath** — a third math library |

**The unoccupied niche:** a `no_std`-capable, math-library-agnostic crate doing surface nets **and**
dual contouring with QEF/sharp-feature preservation, supporting f32 **and** f64, pinning no math
library in its public API. Nothing on crates.io does all four.

Adjacent bonus: **B-rep → mesh tessellation exists in exactly two dormant places** (truck-meshalgo,
opencascade-rs FFI). If the crate is genuinely CAD-reusable, that's a second unoccupied niche it opens
onto.

---

## 2. The single biggest trap: glam version skew

This decides the public API, so it comes before the crate layout.

| | requires glam |
|---|---|
| **Bevy 0.19.0** (2026-06-19) | **^0.32.0** |
| Bevy 0.18 / 0.17 | ^0.30 |
| `parry3d` 0.30.2 → `glamx` 0.3 | ^0.33 |
| `fast-surface-nets` 0.2.1 | ^0.29 |
| `block-mesh` 0.2.0 → `ilattice` ^0.1 | ^0.19 |

A Bevy 0.19 project using `fast-surface-nets` compiles **two incompatible copies of glam**. Add
`block-mesh` and it's **three**. Bevy is still pinned to `hexasphere = "18.0"` purely because
hexasphere 19 wants glam 0.33 and Bevy wants 0.32.

**Consequence: put no math type in the public signature.** Use `[f32; 3]` / `[u32; 3]`. This is not a
style preference — it is what every crate that successfully serves both worlds already does:

- `fast-surface-nets` uses glam internally, emits `Vec<[f32;3]>`. This is exactly why `csgrs` (an
  nalgebra CAD crate) *can* depend on it.
- `ndshape` — `fn linearize(&self, p: [Coord; N]) -> Coord`. Pure arrays. Which is why a crate frozen
  since 2022-02-13 is still used by everything.
- **Even `parry3d`, the most math-opinionated crate in the ecosystem, uses `Vec<[u32;3]>` for indices.**

**Use glam internally.** Not because it's better in the abstract, but because **Dimforge — the authors
of nalgebra — migrated parry (0.26.0, 2026-01-09) and rapier (0.32) off nalgebra onto glam/glamx**,
citing graphics-community adoption and, decisively, that *"rust-gpu has first-class support of glam.
Unfortunately nalgebra internals are too complex for it to be compiled with rust-gpu."* They report
performance was unchanged: *"nothing changed, at all."* glam has 46.4M recent downloads to nalgebra's
15.0M, is `no_std` with **zero required dependencies**, and keeps a rust-gpu port viable.

Pin the internal glam to **exactly what Bevy ships (0.32)**, so the `bevy_` crate's `From` impls are
zero-cost rather than a conversion.

---

## 3. The crate layout

Three crates, two workspaces.

```
meshcore/                    # workspace A
  Cargo.toml                 #   glam 0.32 only. no_std + alloc. Arrays in the API.
  src/
  examples/                  #   Bevy examples, gated by required-features
  meshcore-gpu/              #   + wgpu 29.0.3. Takes &wgpu::Device / &Queue / &mut CommandEncoder.
                             #     Never mentions Bevy.
bevy_meshcore/               # workspace B — separate, so Bevy's feature unification
  Cargo.toml                 #   doesn't contaminate meshcore's local builds
```

**Two workspaces, not one.** Verified: with a shared workspace, building `-p meshcore` alone gives
glam only `libm`; building the whole workspace gives glam `std`, `serde`, `bytemuck`, `encase`,
`rand` via feature unification. Downstream consumers are unaffected, but your own `cargo test` and CI
stop testing what you ship.

**Why the wrapper is worth it despite the extra crate** — the core gets ~10× the reach:

| core | 90-day downloads | Bevy wrapper | |
|---|---|---|---|
| egui | 4,681,151 | bevy_egui | 374,351 |
| parry3d | 594,122 | bevy_rapier3d | 55,916 |
| hexasphere | 1,759,946 | *(none — Bevy consumes it directly)* | |

And it decouples release cadence in the direction that matters. `bevy_egui` 0.40.0 shipped
**2026-06-19 — the same day as Bevy 0.19**; `avian3d` 0.7.0 the next day. The wrapper rides Bevy's
train. But `bevy_egui` 0.41.1 is still on `egui = "0.35"` while egui shipped 0.36.0 — **the core moved
on and nothing broke.** That asymmetry is the entire argument for the split.

### `meshcore/Cargo.toml`

```toml
[package]
name = "meshcore"
edition = "2024"

[dependencies]
glam = { version = "0.32", default-features = false, features = ["libm"] }
# nothing else. no wgpu, no bevy, no rayon in default.

[features]
default = ["std"]
std   = ["glam/std"]
f64   = []                      # widen the solver scalar
rayon = ["dep:rayon", "std"]
mint  = ["glam/mint"]           # From/Into only — nalgebra bridge without depending on nalgebra
bevy_reflect = ["dep:bevy_reflect"]

[dependencies.bevy_reflect]     # LEAF crate, not the `bevy` umbrella
version = "0.19"
default-features = false
features = ["glam"]
optional = true

[dev-dependencies]              # NOT propagated to consumers — verified
bevy = { version = "0.19", default-features = false, features = [
  "bevy_pbr", "bevy_render", "bevy_winit", "bevy_window", "x11",
] }
naga = { version = "29.0.3", features = ["wgsl-in"] }   # CI shader validation

[[example]]
name = "carve"
required-features = ["std"]
```

**Verified empirically:** a consumer crate depending on `meshcore` by path resolves **3 packages**
(consumer, glam, meshcore) — **0 bevy, 0 wgpu** — while `meshcore`'s own lockfile has 137 packages
including 20 bevy crates. `cargo build --examples` with the feature off finishes in 0.64s and never
compiles Bevy. Cargo book: *"These dependencies are **not** propagated to other packages which depend
on this package."*

### Examples: dev-dependencies, not a separate examples crate

Both patterns keep Bevy out of consumers' graphs. The dev-dependency pattern (`hexx` 0.25.0) wins on
one axis that matters a lot:

| | dev-deps + `required-features` | separate examples crate |
|---|---|---|
| Bevy in consumer graph | No | No |
| CI notices when examples break | **Yes** | No |
| Examples stay current | Yes | **No — rots silently** |

The evidence is brutal: **`block-mesh`'s examples are pinned to bevy 0.13. `fast-surface-nets`' are
pinned to bevy 0.7.** Both used the separate-crate pattern and nothing in CI ever complained.

One gotcha, verified by unpacking hexx's published `.crate`: Cargo **silently strips every
`[[example]]` section from the published manifest** when `exclude` covers `examples/`, while keeping
the `[dev-dependencies]` declarations. Harmless for consumers; it does mean docs.rs can't scrape your
examples.

---

## 4. The public API shape

Two layers, both of which already exist separately in the ecosystem and **neither crate offers both.**

**Layer 1 — caller-provided reusable buffer.** The primary API. This is what every performance-serious
crate does, because chunked meshing re-meshes thousands of chunks per edit and allocation dominates:

```rust
pub fn surface_nets<S: Sdf>(
    sdf:    &S,
    shape:  &impl Shape3,
    min:    [u32; 3],
    max:    [u32; 3],
    out:    &mut MeshBuffer,        // has reset() that resizes, never reallocates
);
```

`fast-surface-nets` does exactly this. `binary-greedy-meshing` goes furthest — a persistent `Mesher`
owning *all* scratch buffers with `clear()` documented as *"reset the buffers without reallocating."*

**Layer 2 — trait sink.** For consumers who want zero intermediate copy:

```rust
pub trait MeshSink {
    fn vertex(&mut self, p: [f32; 3], n: [f32; 3]) -> u32;
    fn triangle(&mut self, a: u32, b: u32, c: u32);
}
```

`transvoxel` has this and states the motivation outright: *"use your own MeshBuilder. If you're using
bevy, we have an implementation in our examples code."* `meshopt` has the cleverest variant —
`VertexDataAdapter { data: &[u8], vertex_stride, position_offset }`, which operates on any vertex
layout without knowing the type.

Ship `MeshBuffer: MeshSink` so the simple case stays one line, and `bevy_meshcore` implements
`MeshSink` writing straight into `Mesh` attribute arrays.

**Avoid** the third pattern — returning a freshly-allocated owned `Mesh` with math-library types in
it. That's what `fidget-mesh`, `tessellation`, `mcubes` and `csgrs` do, and it's why none of them are
reusable across the game/CAD boundary.

### f64 for CAD

`fast-surface-nets`' `SignedDistance: Into<f32>` bound is what disqualifies it for CAD, and it's worth
not repeating. Two options:

- **A `Real` trait bound**, f32 default — like `transvoxel`'s `Coordinate: Float`. Simpler, one crate,
  some monomorphization bloat.
- **rapier's pattern** — one source tree, two `[lib]` targets with
  `required-features = ["f32"]` / `["f64"]`. Proven for genuinely mutually-exclusive precision. The
  Cargo book endorses it: *"There are rare cases where features may be mutually incompatible... This
  should be avoided if at all possible."*

Start with the trait bound. Note that the QEF is where precision actually bites — `AᵀA` squares the
condition number, which is the whole reason the QR/Givens formulation exists. **The closed-form
rotation-equivariant vertex rule from the adjacent-math audit sidesteps this entirely for the
three-plane case** and is worth being the default path:

```
c  = (p₁+p₂+p₃)/3          dᵢ = nᵢ·(pᵢ−c)
x  = c + [ d₁(n₂×n₃) + d₂(n₃×n₁) + d₃(n₁×n₂) ] / [ n₁·(n₂×n₃) ]
```

Fall back to the regularized normal-equation form only for >3 planes.

---

## 5. Shaders: what to do instead of naga_oil

### The composition problem

You want `#import` and `#ifdef` across WGSL files. Three options, ranked:

**(a) `include_str!` + a 40-line preprocessor.** Honestly the right v1 answer. `const` string
concatenation at compile time, `#[cfg]`-driven variant selection in Rust rather than in the shader.
Zero dependencies, zero version coupling, works identically standalone and inside Bevy. Most WGSL
"composition" needs are `include` and a boolean, and you will spend less time on this than on reading
naga_oil's docs.

**(b) `wesl-rs` (WESL 0.2, updated 2026-05).** The real answer if the shader tree gets big. Framework-
agnostic by design — the README describes itself as *"like what naga_oil does for Bevy"* but not tied
to it. Implemented today: imports, inline import paths, conditional compilation (`@if`/`@elif`/`@else`),
Cargo shader packages. Experimental: const-expression lowering, validation. Planned: automatic
bindings, namespaces, **generic functions**. Works at build time or runtime. It is also where Bevy has
said it is going, so this is the option that converges rather than diverges.

**Candid caveat:** WESL 0.2 is not production-hardened, and the features you'd most want for a meshing
kernel (generics over scalar type, binding structs) are in the *planned* column. Adopt it when the
shader count justifies it, not on day one.

**(c) naga_oil.** Only if you need bit-compatibility with Bevy's own shader imports. You almost
certainly don't, for a compute kernel.

### The good use of naga: CI validation

This is the part worth building immediately.

```toml
[dev-dependencies]
naga = { version = "29.0.3", features = ["wgsl-in"] }
```

```rust
#[test]
fn all_shader_variants_validate() {
    for src in SHADER_VARIANTS {
        let module = naga::front::wgsl::parse_str(src).expect("parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        ).validate(&module).expect("validate");
    }
}
```

No GPU, no device, runs in seconds on any CI box, and catches the entire class of "shader compiles on
my Vulkan driver, explodes on DX12." For a crate with combinatorial shader variants (algorithm ×
precision × sharp-features × normals) this is the highest-value 30 lines in the project.

---

## 6. The GPU crate and the wgpu pin

**Bevy 0.19 pins `wgpu 29.0.3`, `wgpu-types 29.0.3`, `naga 29.0.3`, `encase 0.12`, `glam 0.32.0`.**
(Source: `crates/bevy_render/Cargo.toml` at `v0.19.0`.)

**You must match `wgpu` exactly.** Cargo will *silently* resolve two majors side by side — no
resolution error. Verified: a crate with `bevy = "0.19"` + `wgpu = "30"` locks 317 packages containing
both wgpu 29.0.4 and 30.0.0. The failure appears later, at the type level:

```
error[E0308]: mismatched types
  expected `wgpu_types::TextureFormat`, found a different `wgpu_types::TextureFormat`
note: there are multiple different versions of crate `wgpu_types` in the dependency graph
```

Plus two independent wgpu instances at runtime. **This drift is live right now:** `egui-wgpu` uses
wgpu 30.0 while Bevy 0.19 uses 29.0.3, which is precisely why `bevy_egui` does not depend on
`egui-wgpu` and reimplements the renderer instead.

### How `bevy_hanabi` handles it — copy this

```toml
# Same versions as Bevy 0.19 (bevy_render)
wgpu = { version = "29.0.3", default-features = false, features = [
  "wgsl", "dx12", "metal", "vulkan", "naga-ir", "fragile-send-sync-non-atomic-wasm",
] }
```

It reaches raw wgpu through the escape hatch: `RenderDevice::wgpu_device(&self) -> &wgpu::Device`.
Note that Bevy re-exports many wgpu types from `bevy_render::render_resource` (`CommandEncoder`,
`ComputePass`, `TextureFormat`, `ShaderModule`, `BufferUsages`…) but **does not `pub use wgpu`**, and
**`Device`, `Queue`, `Adapter`, `Instance` are not re-exported.** So you declare wgpu yourself, at
Bevy's exact version.

**Design rule for `meshcore-gpu`:** its public API takes `&wgpu::Device`, `&wgpu::Queue`,
`&mut wgpu::CommandEncoder` — never a Bevy type. Bevy side calls `render_device.wgpu_device()` and
hands them in. CAD side passes its own. Then publish a hard compatibility matrix and expect one
release per wgpu major:

| meshcore-gpu | wgpu | bevy |
|---|---|---|
| 0.1.x | 29.0.3 | 0.19 |

If you'd rather not carry the full wgpu dependency, note that `bevy_egui` and `bevy_mesh` depend on
**`wgpu-types` only** — plain data (formats, usages, limits), no device or driver, far cheaper to
compile and far less to keep in sync. Good enough if the crate only needs to describe buffer layouts.

### 6.1 Using a wgpu feature Bevy doesn't expose (e.g. mesh shaders)

Bevy is not a wall here. Two hatches, both verified in the 0.19 docs.

**Request the feature at device creation.** Features cannot be added after the device exists, so this
has to happen when the app is built:

```rust
app.add_plugins(DefaultPlugins.set(RenderPlugin {
    render_creation: WgpuSettings {
        features: WgpuFeatures::EXPERIMENTAL_MESH_SHADER,
        ..default()
    }.into(),
    ..default()
}));
```

`WgpuSettings.features` is documented as *"The features to ensure are enabled regardless of what the
adapter/backend supports. Setting these explicitly may cause renderer initialization to fail."* That
warning is the reason to gate this behind a cargo feature and check
`RenderDevice::features() -> Features` at startup, falling back to the compute path when absent.

**Then use the raw device.** `RenderDevice::wgpu_device(&self) -> &wgpu::Device` returns wgpu 29.0.3's
`Device` directly. From there you build your own pipeline and run it in your own `RenderGraph`-schedule
system using `RenderContext::command_encoder()`.

**What you give up:** your mesh-shader draws bypass Bevy's mesh-drawing path entirely — no material
system, no batching, no automatic shadow-pass participation for those primitives. That is the cost of
custom rendering in general, not of mesh shaders specifically, and it's the same cost `bevy_hanabi`
pays. For a chunk mesher that owns its own material anyway, it's close to free.

---

## 7. Bevy 0.19's render rewrite makes the wrapper much smaller

`bevy_render::render_graph` **no longer exists in 0.19.** From the 0.18→0.19 migration guide:

> ### Render Graph as Systems
> The `RenderGraph` API has been removed. Render passes are now systems that run in `Core3d` or
> `Core2d` schedules. […] The `ViewNode` trait is replaced by a regular system using the `ViewQuery`
> parameter. `RenderContext` is now a system parameter instead of being passed as `&mut`. […] **The
> `RenderGraph` schedule […] remains as the top-level schedule for non-camera rendering.**

**A meshing compute pass is non-camera work**, so it targets the `RenderGraph` schedule. The whole
integration is now roughly this, from Bevy 0.19's own compute example:

```rust
render_app
    .add_systems(RenderStartup, init_mesher_pipeline)
    .add_systems(Render, prepare_bind_group.in_set(RenderSystems::PrepareBindGroups))
    .add_systems(RenderGraph, mesh_dirty_chunks.before(camera_driver));

fn mesh_dirty_chunks(
    mut ctx: RenderContext,
    bind_groups: Res<MesherBindGroups>,
    pipeline_cache: Res<PipelineCache>,
) {
    let mut pass = ctx.command_encoder()
        .begin_compute_pass(&ComputePassDescriptor::default());
    // ...
    pass.dispatch_workgroups(nx, ny, nz);
}
```

Versus 0.18, which needed `#[derive(RenderLabel)]`, `impl render_graph::Node`, a stateful
`fn update(&mut self, world: &mut World)`, `add_node`, and `add_node_edge`.

**The one porting cost:** a render node's `&mut self` state must become a `Resource` driven by an
ordinary `Render`-schedule system. That's the real work, and it's less code than before.

`bevy_meshcore` should depend on **leaf crates** — `bevy_app`, `bevy_ecs`, `bevy_asset`, `bevy_mesh` —
with `bevy_render` **optional**, gated behind a `gpu` feature. Bevy 0.17–0.19 carved `bevy_camera`,
`bevy_light`, `bevy_shader`, `bevy_mesh` and `bevy_material` out of `bevy_render` specifically so this
is possible. A consumer who only wants CPU meshing into a `Mesh` asset then never compiles the
renderer.

---

## 8. Build order

Staged so each step is falsifiable before the next depends on it.

**Stage 0 — skeleton (days).**
`meshcore` with `MeshBuffer`, `MeshSink`, `Shape3`/`Sdf` traits, and naive surface nets. glam 0.32
internal, arrays out. One Bevy example under `required-features`. The naga validation test, even with
one trivial shader. **Exit criterion:** a consumer crate's `cargo tree` shows 3 packages.

**Stage 1 — the CAD-differentiating algorithm (weeks).**
Dual contouring with the closed-form equivariant vertex rule as default and the regularized normal
equations as fallback. **Clamp the minimizer to (1−ε) inside its cell** and instrument
self-intersections per 1,000 triangles — this is item 2 on the opportunities list and it settles
whether runtime convex decomposition is available. `f64` via the `Real` bound here, because this is
where precision bites.

**Stage 2 — `meshcore-gpu` (weeks).**
wgpu 29.0.3, compute-shader path, `&wgpu::Device` API. Standalone harness first, no Bevy at all — if
it can't run headless against raw wgpu, the abstraction leaked. Then `bevy_meshcore` adds ~100 lines
of `RenderGraph`-schedule system.

**Stage 3 — the measurement nobody has published (days, high value).**
MC vs Surface Nets vs Dual Contouring, same grid, same hardware, same codebase. **This comparison does
not exist in the literature for 2020+ hardware**, and Surface Nets has no credible published timings
at all despite being what engines ship. You'd have the only implementation where it's apples-to-apples.

**Stage 4 — mesh shaders, when wgpu's API settles.**
`EXPERIMENTAL_MESH_SHADER` behind a cargo feature, reached via `render_device.wgpu_device()`. Gate it
off by default and keep the compute path as the fallback forever — no browser support, and the spec
says breaking changes are expected.

---

## 9. What I'd flag as the risky assumptions

- **Two workspaces adds friction** — no `cargo test --workspace` across the boundary, and you'll
  hand-sync versions. It buys correct local feature resolution. If that trade annoys you, one
  workspace plus a CI job that builds `-p meshcore` in isolation catches the same class of bug.
- **The `Real` trait bound will produce monomorphization bloat** in the GPU-adjacent code paths.
  Measure before assuming rapier's two-`[lib]` split is unnecessary.
- **Pinning glam to Bevy's version means CAD consumers on parry3d get a duplicate glam** (0.32 vs
  0.33 via glamx). Arrays in the public API make this survivable rather than fatal — that's the whole
  point — but it's a real cost of choosing Bevy's pin.
- **WESL 0.2 not being production-hardened** is the main reason §5 recommends the dumb preprocessor
  first. If WESL's `generic functions` land, revisit immediately — generics over scalar type is
  exactly the thing WGSL's absence of them makes painful for a meshing kernel.

---

## Sources

Verified against: crates.io API and published `.crate` tarballs (all version/date/dependency facts) ·
`bevyengine/bevy-website` `content/learn/migration-guides/0.18-to-0.19.md` · `bevy_render`
`Cargo.toml` at `v0.19.0` · wgpu `docs/api-specs/mesh_shading.md` @ v30 · wgpu mesh-shader tracking
issue #7197.

- [Bevy 0.19 release notes](https://bevy.org/news/bevy-0-19/)
- [Bevy 0.18→0.19 migration guide](https://bevy.org/learn/migration-guides/0-18-to-0-19/)
- [wgpu mesh shading spec (v30)](https://github.com/gfx-rs/wgpu/blob/v30/docs/api-specs/mesh_shading.md)
- [wgpu mesh shaders tracking issue #7197](https://github.com/gfx-rs/wgpu/issues/7197)
- [naga_oil](https://github.com/bevyengine/naga_oil)
- [wesl-rs](https://github.com/wgsl-tooling-wg/wesl-rs)
- [Dimforge — the year 2025 in review](https://dimforge.com/blog/2026/01/09/the-year-2025-in-dimforge/) (parry/rapier nalgebra → glam)
- [hexx](https://github.com/ManevilleF/hexx) · [block-mesh-rs](https://github.com/bonsairobo/block-mesh-rs) · [fast-surface-nets-rs](https://github.com/bonsairobo/fast-surface-nets-rs)
- [bevy_hanabi](https://github.com/djeedai/bevy_hanabi) · [bevy_egui](https://github.com/vladbat00/bevy_egui) · [parry](https://github.com/dimforge/parry)
- [fornjot (archived)](https://github.com/hannobraun/fornjot) · [building-blocks (archived)](https://github.com/bonsairobo/building-blocks) · [swiftcoder/isosurface](https://github.com/swiftcoder/isosurface)
- [Cargo Book — features](https://doc.rust-lang.org/cargo/reference/features.html) · [Cargo Book — dev-dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#development-dependencies)
