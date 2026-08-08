# Lighting for a Fluorescent Backrooms Game in Bevy 0.19: A Literature Review and System Design

## TL;DR
- **Build the light on a baked irradiance-volume probe grid, not runtime GI.** For a largely static maze lit by uniform overhead fluorescents, a baked diffuse irradiance field (Greger's Irradiance Volume, encoded per DDGI/Ramamoorthi–Hanrahan) gives near-offline quality at raster cost on mid-range GPUs — and, critically, the same probe grid is the queryable light field your gameplay code needs.
- **Model fluorescent tubes as emissive geometry + a small number of real Bevy lights, because Bevy 0.19 has no built-in area/tube light.** Approximate each fixture visually with an emissive mesh, bake its indirect contribution into the probe grid, and drive direct shading/soft shadows from a modest set of clustered `SpotLight`/`PointLight` sources plus GTAO and the new 0.19 contact shadows; reserve the LTC area-light math (Heitz 2016) for a future custom-shader tier.
- **Maintain one CPU-side light grid as the gameplay source of truth, kept in sync with the baked probes.** Sample irradiance on the CPU for stealth/visibility and entity-behavior logic; do not read back the GPU. Hardware ray tracing (Bevy's experimental Solari) is an optional top tier, never the baseline.

---

## PART 1 — LITERATURE REVIEW: Real-Time Lighting Under a Millisecond Budget

### The governing problem
All of real-time lighting is an argument about how to cheat Kajiya's **rendering equation** (Kajiya, "The Rendering Equation," SIGGRAPH 1986), which states that outgoing radiance at a point is the emitted radiance plus the integral over the hemisphere of incoming radiance times the BRDF times a cosine term. The integral is recursive — incoming radiance is itself the outgoing radiance of other surfaces — so exact evaluation means simulating all light bounces. Offline path tracers converge this integral with thousands of samples per pixel; a game has roughly **16.6 ms** (60 fps) or **33 ms** (30 fps) for *everything*, of which lighting gets a fraction. Every technique below is a different bargain: precompute the integral, restrict its frequency content, cache and reuse partial results, or sample it stochastically and denoise. The reflectance factor in the integral is itself modeled by the **Cook–Torrance microfacet BRDF** (Cook & Torrance, "A Reflectance Model for Computer Graphics," 1982), the foundation of modern physically based shading.

### (a) Precomputed and baked lighting — the cheapest photons are the ones you computed yesterday
**Intuition.** If the geometry and lights don't move, the indirect-light integral has a fixed answer at every point; compute it once offline and look it up at runtime.

**Lightmaps** store outgoing diffuse radiance in a texture atlas unwrapped over static geometry — a per-texel cached solution to the rendering equation. Runtime cost is a single texture fetch; the trade-off is that they only work for static geometry and static lights, cost significant memory and bake time, and don't light dynamic objects.

**Precomputed Radiance Transfer** (Sloan, Kautz & Snyder, "Precomputed Radiance Transfer for Real-Time Rendering in Dynamic, Low-Frequency Lighting Environments," SIGGRAPH 2002) generalizes this: it precomputes a *transfer function* per surface point that maps low-frequency incident lighting (soft shadows, interreflections, caustics from the object onto itself) into transferred radiance, representing both lighting and transfer in low-order spherical harmonics. This lets lighting rotate/change at runtime while the object stays rigid — richer than a lightmap, but restricted to low-frequency lighting and largely static geometry.

**Spherical-harmonic irradiance** rests on the key result of Ramamoorthi & Hanrahan ("An Efficient Representation for Irradiance Environment Maps," SIGGRAPH 2001): the irradiance of a diffuse surface under distant lighting is so smooth that, in their words, "one needs to compute and use only **9 coefficients**, corresponding to the lowest-frequency modes of the illumination, in order to achieve **average errors of only 1%**," and "the irradiance can be procedurally represented simply as a quadratic polynomial in the cartesian components of the surface normal." This is why almost every probe format below stores 9 SH coefficients (or a cheaper equivalent) per sample.

**Irradiance volumes / probes** (Greger, Shirley, Hubbard & Greenberg, "The Irradiance Volume," IEEE CG&A 1998) place a 3D grid of irradiance samples through a volume so that *dynamic* objects moving through static geometry can interpolate indirect light from the nearest probes. This is the seminal idea behind every modern probe-grid GI method, and — decisively for this project — it produces a **spatial field of irradiance that can be sampled at an arbitrary point**, which is exactly what gameplay code wants.

**Mid-range/no-RT suitability: excellent.** This entire family is raster-friendly, needs no ray tracing at runtime, and has the lowest per-frame cost of any option. Its weakness — static-only — is precisely tolerable for the Backrooms.

### (b) Real-time global illumination — computing bounces live
**Intuition.** When lights or geometry move, precomputation is invalid; approximate the bounce integral each frame using a coarse proxy of the scene.

**Voxel cone tracing** (Crassin, Neyret, Sainz, Green & Eisemann, "Interactive Indirect Illumination Using Voxel Cone Tracing," I3D/CGF 2011) voxelizes the scene into a sparse voxel octree of pre-filtered radiance, then approximates the hemisphere integral by marching a handful of cones through progressively coarser voxels. Per the paper, it "can manage two light bounces for both Lambertian and glossy materials at interactive framerates (**25–70 FPS**)," "exhibits an almost scene-independent performance," and "can be used to efficiently estimate Ambient Occlusion." The trade-offs are large memory for the voxel structure, light leaking through thin walls, and re-voxelization cost for dynamic content. (Epic shipped a variant, "SVOGI," briefly before replacing it.)

**Radiance caching (GI-1.0)** (Boissé et al., "GI-1.0: A Fast and Scalable Two-level Radiance Caching Scheme for Real-Time Global Illumination," arXiv:2310.19855, 2023) is the modern production answer: a two-level cache with **screen-space probes** on primary surfaces (high fidelity because there are many) plus a **world-space hash-grid cache** for secondary bounces, using ray tracing to fill the caches at a few rays per frame. It explicitly positions itself between probe methods (cheap but low-detail and slow to react) and ReSTIR (detailed but noisy and expensive). It is fast, but it *assumes hardware ray tracing*.

**Screen-space GI (SSGI)** infers one indirect bounce from the depth/normal/color buffers alone — no ray tracing — but suffers the fundamental screen-space limitation: it cannot light from anything off-screen or occluded, producing view-dependent flicker.

**Mid-range/no-RT suitability: mixed.** VCT is viable without RT but heavy and leak-prone; GI-1.0 needs RT; SSGI is cheap but unreliable as a primary GI source.

### (c) Dynamic diffuse GI and light-probe methods — the queryable field (emphasis)
**Intuition.** Take Greger's irradiance volume, but *update it at runtime* by ray-tracing a few rays out of each probe every frame, and store the result compactly enough to sample cheaply on the GPU.

**DDGI** (Majercik, Guertin, Nowrouzezahrai & McGuire, "Dynamic Diffuse Global Illumination with Ray-Traced Irradiance Fields," JCGT 8(2), 2019) is the landmark. Each probe stores irradiance in an **octahedral parameterization** (a square texture covering all directions) plus a second octahedral map of depth and depth-squared — the "visibility-aware moment-based" term that suppresses the light leaking that plagued earlier probe methods (this is Chebyshev's inequality, the same statistic used by variance shadow maps). The paper reports full global illumination "visually comparable to offline path-traced results but several orders of magnitude faster: **6 ms/frame, versus 1 min/frame** in this scene (on GeForce RTX 2080 Ti at 1920×1080)"; NVIDIA's later shipping RTXGI figures were tighter still (~1 ms/frame at 1080p on the same GPU). DDGI's irradiance field is directly queryable at any world position by trilinearly interpolating the 8 surrounding probes with visibility weights.

**DDGI Resampling** (Majercik, Müller, Keller, Nowrouzezahrai & McGuire, CGF 2021) applies importance resampling to allocate probe rays where they matter, improving quality per ray.

**Efficient Light Probes** (Guo et al., 2022) and **Streaming Dynamic Light Probes to Thin Clients** (Stengel, Majercik et al., arXiv:2103.05875, 2021) push the format toward lower bandwidth/storage and remote streaming, demonstrating that world-space probes trivially support multi-viewer/serverside rendering because they are view-independent.

**Mid-range/no-RT suitability: excellent if baked, good if updated.** The representation is raster-cheap to sample. DDGI's *updates* need ray tracing, but the *format* does not — you can bake the same octahedral or SH probes offline and sample them identically at runtime. This dual nature (queryable field + raster-cheap sampling) makes it the backbone of the design in Part 2.

### (d) Many-light sampling and the ReSTIR lineage — brilliant, but RT-bound
**Intuition.** With thousands of lights (e.g., thousands of emissive fluorescent tubes), you can't evaluate them all per pixel; sample a few important ones and reuse good samples across neighbors and frames.

**Resampled Importance Sampling** (Talbot, Cline & Egbert, "Importance Resampling for Global Illumination," EGSR 2005) is the mathematical seed: draw candidate samples from a cheap distribution, then resample proportional to a better target, approximating samples from the good distribution.

**ReSTIR** (Bitterli, Wyman, Pharr, Shirley, Lefohn & Jarosz, "Spatiotemporal Reservoir Resampling for Real-Time Ray Tracing with Dynamic Direct Lighting," SIGGRAPH 2020) applies RIS in a reservoir that resamples across space and time. Per the paper, it renders "complex scenes containing **up to 3.4 million dynamic, emissive triangles in under 50 ms per frame while tracing at most 8 rays per pixel**," achieving "equal-error **6×–60× faster** than state-of-the-art methods" (a biased variant is 35×–65× faster). **ReSTIR GI** (Ouyang, Liu, Kettunen, Pharr & Pantaleoni, CGF 2021) extends reservoir reuse to multi-bounce indirect paths; **ReSTIR PT / GRIS** (Lin, Kettunen, Bitterli, Pantaleoni, Yuksel & Wyman, "Generalized Resampled Importance Sampling," SIGGRAPH 2022) generalizes the theory to full path reuse. **Grid-based reservoirs** (Boksansky, Jukarainen & Wyman, "Rendering Many Lights with Grid-Based Reservoirs," Ray Tracing Gems II, 2021) precompute per-cell reservoirs of important lights — conceptually close to what Bevy's own Solari author is now prototyping as world-space light grids.

**Mid-range/no-RT suitability: poor for the baseline.** Every member of this family requires ray queries against scene geometry. On a no-RT mid-range target the lineage is out of scope, with one caveat: the *idea* of grid-based reservoirs (precompute per-cell important-light lists) is directly borrowable as a raster-side light-culling structure, which is essentially what clustered forward shading already does.

### (e) Denoising and reconstruction — making 1 sample look like 1000
**Intuition.** If you can only afford one stochastic sample per pixel, the image is noisy; reconstruct a clean image by pooling samples across time and space where the surface is consistent.

**SVGF** (Schied et al., "Spatiotemporal Variance-Guided Filtering," HPG 2017) reconstructs a temporally stable image from **1 path-per-pixel** input: it reprojects previous frames to accumulate effective samples, estimates per-pixel luminance variance, and drives an edge-aware à-trous wavelet filter whose bandwidth adapts to that variance — reconstructing 1080p in ~10 ms comparable to a 2048-spp reference. **A-SVGF** (adaptive SVGF, Schied, Peters & Dachsbacher, 2018) adds temporal-gradient estimation to react faster to lighting changes and reduce ghosting. **Blue-noise sampling** distributes the residual error into a perceptually less objectionable high-frequency pattern.

**Mid-range/no-RT suitability: only relevant if you have a noisy stochastic source.** A baked/probe pipeline produces no per-pixel Monte-Carlo noise, so a full SVGF stage is unnecessary at baseline — but its temporal-reprojection and variance machinery is the correct reference if you ever add stochastic screen-space effects or an RT tier.

### (f) Shadows — the oldest hack, still essential
**Intuition.** A point is in shadow if something blocks the straight line to the light; render the scene from the light's view, store depths, and compare.

**Shadow mapping** (Williams, "Casting Curved Shadows on Curved Surfaces," SIGGRAPH 1978) is that idea; its eternal problems are resolution-driven aliasing, "shadow acne" (self-shadowing) and "peter-panning" (detached shadows) from depth bias. **Cascaded shadow maps** split the view frustum by depth so near geometry gets high shadow-map resolution — standard for directional lights and what Bevy uses. **Variance shadow maps** (Donnelly & Lauritzen, "Variance Shadow Maps," I3D 2006) store depth *and* depth-squared so the map can be pre-blurred and filtered as an ordinary texture (via Chebyshev's inequality), giving cheap soft shadows at the cost of light-bleeding artifacts; **moment shadow maps** extend this to four moments for better bleeding control. **Contact shadows / screen-space shadows** ray-march the depth buffer a short distance to recover fine contact detail cheaply — added to Bevy in 0.19. **SSAO** (see (h)) approximates the ambient occlusion that reads as soft contact shadowing under indirect light. **Neural shadow mapping** (Datta et al., "Neural Shadow Mapping," SIGGRAPH 2022) learns to produce filtered soft shadows from a shadow map, trading a small network inference for quality — research-frontier for shipping games.

**Mid-range/no-RT suitability: excellent.** Shadow maps are the raster shadow workhorse; CSM + contact shadows + AO is the standard mid-range recipe.

### (g) Physically based shading fundamentals — and the fluorescent-tube problem
**Intuition.** Model a surface's reflection as many microscopic mirror facets whose orientation distribution controls roughness.

**Cook–Torrance** (1982) established the microfacet specular BRDF; modern engines use the **GGX/Trowbridge–Reitz** normal-distribution function for its long, realistic highlight tails, combined with a Smith geometry term and Fresnel. All of this assumes a *point/directional* light so the integral collapses to a single direction.

A **fluorescent tube is an area (line) light**, and integrating a GGX BRDF over an extended emitter has no simple closed form. **Linearly Transformed Cosines** (Heitz, Dupuy, Hill & Neubelt, "Real-Time Polygonal-Light Shading with Linearly Transformed Cosines," SIGGRAPH 2016) solve this elegantly: a clamped-cosine distribution can be analytically integrated over an arbitrary spherical polygon, and a per-(roughness, view-angle) 3×3 matrix warps that cosine to approximate GGX — so an area light becomes a matrix transform plus a polygon integral, noise-free. The follow-up (Heitz & Hill, "Real-Time Line- and Disk-Light Shading with LTCs," 2017) extends it to **line, sphere and disk lights** — the line-light case being exactly a fluorescent tube. This is the correct reference for tube shading; the catch (Part 2) is that Bevy 0.19 ships no area-light type, so LTC would be custom-shader work.

**Mid-range/no-RT suitability: excellent.** GGX point/spot shading is the raster baseline; LTC area lights are analytic (no rays, no noise) and mid-range-affordable, just not built into Bevy.

### (h) Ambient occlusion — cheap contact darkening
**Intuition.** Creases and contacts receive less ambient light because nearby geometry blocks it; estimate that occlusion from the depth buffer.

**HBAO** (Horizon-Based Ambient Occlusion) marches the depth buffer to find the horizon angle occluding each point. **GTAO** (Jimenez et al., "Practical Real-Time Strategies for Accurate Indirect Occlusion," Activision, SIGGRAPH 2016) makes horizon-based AO match a ground-truth cosine-weighted occlusion integral and adds a multi-bounce approximation — this is the state of the art and **what Bevy implements** (its SSAO is GTAO, ported from Intel's XeGTAO, with an added visibility-bitmask mode). **Stochastic-Depth AO** (2021) addresses GTAO's single-layer limitation by sampling multiple depth layers to avoid over-darkening from thin occluders.

**Mid-range/no-RT suitability: excellent.** GTAO is a fixed screen-space cost and a near-universal mid-range choice.

### (i) Neural and radiance-field methods — the frontier
**Intuition.** Represent the scene's appearance (or its cached radiance) with a learned function instead of explicit geometry/probes.

**3D Gaussian Splatting** (Kerbl, Kopanas, Leimkühler & Drettakis, "3D Gaussian Splatting for Real-Time Radiance Field Rendering," SIGGRAPH 2023) represents a scene as millions of anisotropic Gaussians rasterized in real time (≥30 fps at 1080p), achieving state-of-the-art novel-view synthesis. But vanilla 3DGS bakes in *view-dependent appearance*, not relightable material — it captures a scene under fixed lighting and is not, out of the box, a dynamic lighting solution. **Neural radiance caching** learns the radiance cache that methods like GI-1.0 fill, and **neural GI/denoisers** (e.g., NVIDIA's ray-reconstruction) are shipping in RT titles.

**Mid-range/no-RT suitability: research frontier for shipping raster games.** For a fluorescent Backrooms game these are not the tool; note them as the horizon, not the plan.

### Which techniques give gameplay a queryable light field?
Only methods that store lighting in a **world-space spatial structure** can answer "how bright is this arbitrary point?" cheaply on the CPU:
- **Irradiance volumes / DDGI probe grids / SH irradiance probes — yes, natively.** This is their defining feature and the reason they anchor the design.
- **Lightmaps — only on surfaces** (per-texel, not in free space), awkward for an entity standing in a room.
- **Screen-space methods (SSGI, SVGF, ReSTIR as usually deployed) — no.** They live in screen space / GPU buffers and are view-dependent; reading them back for gameplay is both a stall and semantically wrong.
- **Voxel cone tracing — partially** (the voxel radiance grid is queryable but heavy and GPU-resident).

The conclusion is decisive: **a probe grid is both the best mid-range GI representation for a static maze and the natural gameplay light field.** Reuse it.

---

## PART 2 — LIGHTING SYSTEM DESIGN (Bevy 0.19, mid-range GPU, no mandatory RT)

### Note on the target repository
The provided repository (`github.com/Ladvien/foundation_vs_slop`) could not be inspected during research: direct fetches are blocked by `robots.txt` and the repo is not indexed by search (it appears newly created, private, or uncrawled), so the `Cargo.toml`, README, or any existing lighting/ECS code could not be verified from outside. The owner is a confirmed, active Bevy/Rust developer with other Bevy repositories, which makes a Bevy 0.19 game project plausible. **This design is therefore written to be self-contained against Bevy 0.19's documented API rather than assuming existing code.** Where it says "add a component/resource," verify first whether the repo already defines an equivalent and adapt names accordingly. This is the one hard gap; every other decision is grounded in verified Bevy 0.19 behavior and the cited literature. (Local inspection of the cloned repo — now available — should be the first concrete step to reconcile naming.)

### What Bevy 0.19 actually gives you (verified)
Bevy 0.19 shipped **19 June 2026** (261 contributors, 1,185 PRs). Relevant rendering facts, confirmed against the official release notes and `docs.rs` for `bevy 0.19.0`:
- **Renderer:** clustered forward (forward+) PBR renderer, WGSL shaders, a render-graph architecture, compute-shader support via wgpu. The BRDF is Filament-derived (GGX microfacet + Cook–Torrance lineage).
- **Light types:** `PointLight`, `SpotLight`, `DirectionalLight` only. **There is no area/rectangle/tube light** — the Area Lights request (GitHub issue #7662) remains open/unimplemented as of 0.19.
- **Light clustering:** Bevy **clusters lights on the GPU** as of 0.19, ~20× faster on the `many_lights` benchmark. Historically the uniform-buffer path caps at **256 point lights**; the storage-buffer path (native/WebGPU, not WebGL2) scales far higher (a prior benchmark sustained ~2,150 point lights at 60 fps on an M1 Max, cluster assignment being the bottleneck).
- **Irradiance volumes:** supported as a **baked** feature (`IrradianceVolume` component under a `LightProbe`). Bevy has **no built-in baker** — you bake in **Blender's Eevee** and export via the `bevy-baked-gi` tool (`export-blender-gi`) to a `.ktx2` 3D texture. Bevy stores irradiance as **Half-Life 2-style ambient cubes** (6 directional colors, 24 bytes/voxel) rather than SH, specifically because ambient cubes use 3 hardware-interpolated samples and the unsigned RGB9E5 texture format, which is faster to sample on the GPU than SH. Multiple overlapping volumes need `TEXTURE_BINDING_ARRAY`; on WebGL2/WebGPU only the single closest volume applies.
- **Reflection probes:** `EnvironmentMapLight` on a `LightProbe`, with **parallax correction on by default** in 0.19 (`ParallaxCorrection::None` to opt out); plus `GeneratedEnvironmentMapLight` for **runtime** environment-map filtering (procedural skies, runtime reflection probes).
- **Ambient occlusion:** `ScreenSpaceAmbientOcclusion` implementing **GTAO** (+ visibility-bitmask mode); Vulkan/DX12/Metal only (no WebGL); pairs well with TAA.
- **Shadows:** cascaded shadow maps for directional lights; shadow maps with PCF/Gaussian filtering (`ShadowFilteringMethod`) for point/spot; **contact shadows new in 0.19**.
- **Emissive materials do NOT cast light in the raster renderer.** An emissive `StandardMaterial` glows but contributes no illumination to other surfaces unless you (a) bake it, or (b) use **Solari**, Bevy's experimental real-time path tracer. Solari in 0.19 is explicitly **not production-ready**, is diffuse-focused in realtime mode, needs a ray-tracing GPU, and its author is prototyping world-space light grids for many-light next-event estimation.

The strategic implication is clear: **on a no-RT mid-range target you cannot rely on emissive geometry to light the scene, and you have no area lights. You must bake the fluorescent contribution into an irradiance volume and drive direct light with a limited set of clustered point/spot lights.** This maps almost perfectly onto the Backrooms' static geometry.

### 1. Light representation for fluorescent tubes
Model each fixture as a **three-part composite**:

1. **Emissive mesh (the visible tube).** A thin emissive quad/cylinder with a `StandardMaterial` whose `emissive` is set to a **cool fluorescent white with a slight green cast**. Aim for a correlated color temperature around the classic "cool white" halophosphate lamp — **~4100 K at a low CRI (~60–62)**. Per Don Klipstein's reference on fluorescent-lamp spectra, the standard halophosphate spectrum "is extra-rich in yellow and orange-yellow and low on red and green," which is what makes reds/greens look duller and skin tones pale — the uneasy, slightly-sick color that defines the Backrooms look. Reproduce this with a green-biased, magenta-deficient emissive tint rather than a neutral white. This is what the player sees and what bloom picks up. It casts no light by itself (Bevy raster), which is fine — its lighting is baked (below).
2. **Baked indirect contribution.** In Blender, place matching emissive tubes and **bake the irradiance volume** so the fluorescent wash — the flat, shadowless, everywhere-cool ambient that defines the Backrooms look — lives in the probe grid. This is the dominant light in the scene and costs nothing at runtime beyond a probe sample.
3. **A small number of real direct lights for the "live" fixtures near the player.** Represent the tube's soft directional pool of light with a `SpotLight` (wide cone, low-ish range) or short `PointLight` aimed downward, one per *active* fixture within a culling radius of the camera. These give real-time soft shadows (CSM/PCF + contact shadows) and specular response on the floor. Because Bevy has no tube light, approximate the tube's extent by (a) using a soft PCF/Gaussian shadow filter to fake penumbra, and (b) optionally splitting a long tube into 2–3 collinear spot/point lights to widen the highlight. Justify this against **LTC** (Heitz 2016; Heitz & Hill 2017 line lights): LTC is the correct analytic tube model, and if you later want physically accurate elongated highlights and soft area shadows without RT, implement an LTC line-light term in a **custom WGSL material/shader** as an "enhanced" tier — but it is not required for the baseline and is not built into Bevy.

**Color, flicker, buzz, failing tubes.** Drive per-fixture behavior from an ECS component (below) that modulates *both* the emissive material and the paired direct light's intensity:
- **Steady state:** subtle sinusoidal ripple (a few percent, a mains-hum shimmer approximated at frame rate) to suggest the ~100–120 Hz flicker of AC ballasts.
- **Failing tube:** stochastic flicker — random dropouts, the classic two-blink-then-off pattern, occasional strobe.
- **Buzz** is audio, triggered from the same component so light and sound stay correlated.

Because the flickering fixtures are exactly the ones near the player (the active direct lights), flicker is a direct-light intensity animation and does **not** require re-baking the probe grid every frame. Reserve probe updates for rare, large lighting changes (see §2).

### 2. Global illumination strategy
**Recommendation: baked irradiance volume(s) as the primary GI, full stop.** The Backrooms aesthetic — static maze, uniform overhead grids, flat diffuse carpet/wallpaper/ceiling tiles, no daylight — is the ideal case for baked diffuse GI and the worst case for runtime GI's strengths (dynamism). This choice is justified directly by **Greger's Irradiance Volume** (queryable field for dynamic objects in static geometry), **Ramamoorthi–Hanrahan** (diffuse irradiance is low-frequency, so a coarse grid suffices), and **DDGI** (the octahedral/visibility-aware encoding and interpolation that kills leaks). Runtime DDGI/GI-1.0 are rejected for the baseline because they need ray tracing; VCT is rejected for leak-proneness and memory on flat-walled mazes.

**Probe placement/density.** Backrooms rooms are boxy with ~2.5–3 m ceilings. Use a **regular grid at roughly 1–2 m spacing horizontally and ~1–1.5 m vertically** (2 layers per storey: knee height and head height), densifying to ~0.5–1 m near doorways and thin walls where irradiance gradients and leak risk are highest. Because Bevy irradiance volumes are axis-aligned cuboids scaled by a `Transform`, **tile the maze with several volumes** (e.g., one per room/corridor segment) rather than one giant volume — this keeps each `.ktx2` small, bounds leaks to local regions, and lets you stream them. Note the WebGL2/WebGPU limitation (only the closest volume samples); on native desktop with `TEXTURE_BINDING_ARRAY` you get multiple overlapping volumes blended.

**Storage format.** For the *rendered* GI, use Bevy's native format: **HL2 ambient cubes in a `.ktx2` 3D texture**, produced by Blender Eevee + `export-blender-gi`. For the *gameplay* field (§3), store a parallel CPU-side representation you control — either the same ambient-cube data or a single scalar/RGB irradiance per cell — because you should not read GPU textures back on the CPU per frame. (If you prefer SH for the gameplay side, order-2 SH per cell is 9 RGB coefficients per Ramamoorthi–Hanrahan; ambient cubes at 6 colors are cheaper and adequate for a "how bright here" query.)

**Updating probes when lights flicker.** Do **not** re-bake for ordinary flicker (handled as direct-light animation). For rare global changes (a whole section's power fails, a scripted blackout), you have two options: (a) pre-bake **two or more lighting states** (all-on, section-off, all-off) as separate `.ktx2` volumes and **cross-fade** between them — cheap, deterministic, no runtime GI; or (b) store a per-fixture influence weight per probe cell at bake time and recombine on the fly. Option (a) is strongly recommended for a shipping mid-range game.

### 3. The queryable light field for gameplay (first-class)
This is the crux. Design a **CPU-side light grid as the single gameplay source of truth**, synchronized with — but not read back from — the rendered probes.

**Data structure (resource).**
```rust
#[derive(Resource)]
struct GameplayLightField {
    origin: Vec3,          // world-space min corner
    cell_size: Vec3,       // e.g. (1.0, 1.25, 1.0) meters
    dims: UVec3,           // grid resolution
    // One irradiance sample per cell. Store either a scalar "illuminance"
    // (lux-like) for cheap stealth queries, or RGB for colored logic.
    cells: Vec<f32>,       // or Vec<[f32;3]> / Vec<AmbientCube>
    // Baked "lights-on" values; a parallel buffer holds current values
    // after applying dynamic fixture states.
    baked: Vec<f32>,
    version: u32,
}

impl GameplayLightField {
    fn sample(&self, p: Vec3) -> f32 {
        // trilinear interpolation of the 8 surrounding cells
    }
}
```

**Why a parallel CPU structure rather than reusing the GPU probe texture.** Three reasons: (1) **No GPU readback.** Reading a GPU texture to the CPU forces a pipeline stall and adds a frame of latency — unacceptable for per-frame AI/stealth. (2) **Semantic control.** Gameplay wants "illuminance at a point," a scalar you define (and can bias for design), not exactly the render's directional ambient cube. (3) **Cadence independence.** The gameplay field updates when *fixtures* change state, not every render frame. The cost is keeping two representations consistent, which you solve by driving both from the *same* fixture state and the *same* bake data.

**Synchronization.** At bake time, also export (or compute) the per-cell irradiance into `baked`. At runtime, a system recomputes `cells` from `baked` **only when a fixture within influence range changes state** (on/off/flicker-average), by adding/subtracting that fixture's precomputed per-cell contribution. For flicker specifically, use the *running average* brightness of a fixture (not its instantaneous strobe value) so AI perception doesn't jitter at frame rate — a design choice, but it makes "is the monster able to see me" stable and fair.

**Update cadence & interpolation.** Run the gameplay-field update at a **fixed low rate** (e.g., 10–20 Hz, or event-driven on fixture-state change), decoupled from render. `sample()` uses **trilinear interpolation** of the 8 surrounding cells; for stealth you may also want a small max-over-neighborhood or a visibility check (raycast to the nearest bright fixture) to avoid "lit through a wall" false positives — the same leak problem DDGI solves with visibility moments, here solved with a cheap physics raycast.

**Bevy ECS design.**
- **Components:** `FluorescentFixture { base_color, base_intensity, state: FixtureState }`, `FixtureState { Steady, Flickering{seed,phase}, Failing, Off }`, `CastsGameplayLight` marker, `LightSensitive { threshold }` (on entities/AI that react to light), `Illuminated { current: f32 }` (written per entity for reaction logic).
- **Resources:** `GameplayLightField` (above); `LightFieldConfig` (grid params, update rate).
- **Systems & schedules:**
  - `Startup`: `load_light_field` (load baked grid + per-fixture contribution tables).
  - `FixedUpdate` (or a custom `LightFieldSet`): `animate_fixtures` (updates emissive material + paired direct light intensity, sets running-average brightness) → `update_gameplay_light_field` (recompute dirty cells) → `sample_illumination_for_entities` (writes `Illuminated` for `LightSensitive` entities via `field.sample(transform.translation)`).
  - `Update`: AI/stealth systems read `Illuminated`/`sample()` — e.g., an entity that only moves in darkness checks `illuminated.current < threshold`; a "hunts in the dark" entity accelerates when the player's sampled illuminance is low.
- Order these in an explicit **system set** so gameplay reads a consistent field within a frame. Keep the field update off the hot render path.

This gives designers a clean API: `light_field.sample(pos)` returns a stable, physically-motivated brightness that is guaranteed consistent with what the player sees, because both derive from the same bake and the same fixture states.

### 4. Shadows and contact/ambient occlusion
**Direct shadows for the few dominant lights.** Enable **cascaded shadow maps** on any `DirectionalLight` (if you use one as a faint fill) and shadow-mapped `SpotLight`/`PointLight` shadows only on the **handful of active fixtures nearest the camera** (shadow-casting lights are expensive; cap them, e.g., 4–8 shadow casters). Use a soft **PCF/Gaussian `ShadowFilteringMethod`** to approximate the penumbra a real tube would cast, referencing the soft-shadow goal of variance/moment shadow maps (Donnelly & Lauritzen 2006) without paying for a VSM pipeline Bevy doesn't natively expose.

**Contact shadows.** Turn on Bevy 0.19's new **contact shadows** to reattach objects to the floor and recover fine detail cheaply — this is the screen-space contact-shadow technique the release explicitly added "without the cost of full raytracing."

**Ambient/contact occlusion for everything else.** Enable **GTAO** (`ScreenSpaceAmbientOcclusion`) to darken the corners, baseboards, and ceiling-tile grid that the flat baked ambient would otherwise leave too uniform — this is exactly Jimenez et al.'s use case (accurate indirect occlusion), and it's the single highest-impact cheap effect for making a flat diffuse Backrooms room read as three-dimensional. Pair with TAA per Bevy's guidance for noise reduction.

**The many-lights problem.** A Backrooms level has hundreds to thousands of fixtures. Solve it by **tiering**:
- **Baked-only (the vast majority):** every fixture's *indirect* light is in the irradiance volume; it needs no runtime light at all. Its emissive mesh still glows for free.
- **Active direct lights (a few dozen at most, near camera):** promoted to real clustered `SpotLight`/`PointLight` sources within a radius, demoted back to baked-only when far. Bevy's GPU clustering (20× faster in 0.19) and the storage-buffer path handle this comfortably — you are nowhere near the 256-light uniform cap because you actively cull to the camera neighborhood.
- **Shadow casters (a handful):** only the closest active lights cast real-time shadows.

This "bake the many, light the few" strategy is the raster analogue of what ReSTIR/grid-based reservoirs do stochastically, and it keeps the frame budget bounded regardless of level size.

### 5. Performance plan (mid-range GPU)
Target **60 fps (16.6 ms)** at 1080p on a mid-range discrete GPU (think a card without meaningful RT throughput). Rough budget:
- **G-buffer/forward opaque pass:** the bulk; flat Backrooms geometry is light on triangles, so this is texture/overdraw-bound, not geometry-bound.
- **Irradiance-volume sampling:** effectively free per fragment (3 hardware-interpolated texture samples per HL2 ambient cube).
- **GTAO:** a fixed screen-space cost, on the order of ~1–2 ms at 1080p on mid-range hardware (depth/normal prepass + AO pass); the dominant fixed lighting cost.
- **Shadow maps:** cost scales with **shadow-casting** light count and resolution — this is why you cap casters to a handful and use modest map resolutions.
- **Contact shadows + TAA + bloom:** small fixed post costs.

**Precomputed vs per-frame.** Precomputed: irradiance volumes (`.ktx2`), per-fixture per-cell contribution tables, lighting-state variants. Per-frame: direct shading for active lights, shadow maps for casters, GTAO, post. The gameplay light-field update is **not** per-frame (10–20 Hz / event-driven).

**Memory footprint.** Irradiance volumes at 24 bytes/voxel (HL2 ambient cubes): a 32×16×32 volume ≈ 16 K voxels ≈ **~400 KB**; tile a large level with dozens of such volumes for a few tens of MB total — trivial. The CPU gameplay grid at 4 bytes/cell (scalar) is a few hundred KB even for large levels. Shadow maps (a few 1–2 K² depth maps) dominate lighting VRAM, on the order of tens of MB.

**LOD for lighting.** Distant regions: baked-only (no direct lights, no shadow casters), lower-resolution irradiance volumes, disabled contact shadows. Near the player: full treatment. Promote/demote per the active-light culling radius.

**Expected bottlenecks,** in likely order: (1) **shadow-caster count/resolution** — cap aggressively; (2) **GTAO + TAA** fixed cost — tune sample counts/resolution scale; (3) **overdraw** on transparent/emissive tube meshes and any fog — keep fill down; (4) **cluster light assignment** if you let too many active lights in — keep the active set small. If you overshoot budget, cut shadow-caster count and GTAO quality first; the baked GI and probe sampling are already near-free.

### 6. Implementation roadmap in Bevy 0.19
**Phase 0 — Baseline raster look (uses only built-in Bevy).** Static maze meshes with `StandardMaterial`; cool-green **emissive** tube meshes; a faint `AmbientLight` + a few `SpotLight`/`PointLight` fixtures with shadows; **GTAO** on; **contact shadows** on; TAA + mild bloom. Ship-quality Backrooms look with zero custom rendering. *Gotcha:* remember emissive ≠ light in raster — the scene will look flat until Phase 1.

**Phase 1 — Baked irradiance volume (built-in feature + external bake).** Model the level in Blender, bake Eevee irradiance volumes, export with `bevy-baked-gi`'s `export-blender-gi` to `.ktx2`, and spawn `LightProbe` + `IrradianceVolume` entities tiling the maze. Now the fluorescent wash is real GI. *Gotchas:* Bevy has no built-in baker (the Blender round-trip is mandatory today); ambient-cube packing layout must match Bevy's `(Rx, 2Ry, 3Rz)` texture convention; multiple volumes need `TEXTURE_BINDING_ARRAY` (native only). Add reflection probes (`EnvironmentMapLight`, parallax correction default-on) for faint floor gloss if desired.

**Phase 2 — Gameplay light field (custom ECS, no rendering changes).** Implement `GameplayLightField` resource, `FluorescentFixture`/`LightSensitive`/`Illuminated` components, and the `FixedUpdate` systems from §3. Load per-cell baked irradiance alongside the render `.ktx2`. Validate `sample()` against the visible scene. This is pure ECS/CPU code — no render graph work.

**Phase 3 — Flicker/failing tubes (custom ECS).** `animate_fixtures` modulates emissive material + paired direct-light intensity; feed running-average brightness into the gameplay field; correlate audio buzz. Pre-bake lighting-state variants for scripted blackouts and cross-fade.

**Phase 4 (optional) — LTC tube lights (custom WGSL).** For physically accurate elongated tube highlights and soft area shadows without RT, implement a **line-light LTC** term (Heitz & Hill 2017) in a custom material/shader, replacing the multi-point-light tube approximation for hero fixtures. Custom WGSL + LTC lookup textures; a render-pipeline change, hence optional.

**Phase 5 (optional) — RT-enhanced tier (Bevy Solari).** On RT-capable hardware, enable **Solari** for real-time emissive-mesh lighting and dynamic GI, letting fixtures light the scene without baking. Keep it strictly optional and feature-gated: Solari is experimental/not production-ready in 0.19, needs an RT GPU, and would replace Phase 1's runtime role, not the gameplay field (which you still want for cheap CPU queries). If you add stochastic RT effects, that is where **SVGF/A-SVGF-style** denoising (Schied 2017/2018) becomes relevant.

### Cross-referenced design-decision summary
- **Baked irradiance volume as primary GI** ⟶ Greger (Irradiance Volume), Ramamoorthi–Hanrahan (low-frequency diffuse ⇒ coarse grid), Majercik DDGI (octahedral + visibility-aware interpolation to prevent leaks); Bevy 0.19 `IrradianceVolume`.
- **Probe grid doubles as gameplay light field** ⟶ Greger/DDGI queryable irradiance field; implemented as a CPU-side mirror to avoid GPU readback.
- **Fluorescent tube shading** ⟶ Cook–Torrance/GGX baseline; Heitz LTC line lights (2016/2017) as the analytic area-light reference for the optional custom tier (Bevy has no area light).
- **SSAO choice = GTAO** ⟶ Jimenez et al. (Practical Real-Time Strategies); Bevy's SSAO *is* GTAO.
- **Shadows** ⟶ Williams (shadow mapping), CSM, Donnelly–Lauritzen (soft-shadow goal); Bevy CSM + PCF + 0.19 contact shadows.
- **Many-lights "bake the many, light the few"** ⟶ raster analogue of Bitterli ReSTIR / Boksansky grid reservoirs; Bevy GPU clustering.
- **Denoising deferred to RT tier** ⟶ Schied SVGF/A-SVGF, only if stochastic sampling is introduced.
- **RT tier** ⟶ GI-1.0 (Boissé) / DDGI updates / ReSTIR as the frontier that Bevy Solari is heading toward; kept optional.

## Recommendations
1. **Start at Phase 0–1 immediately:** emissive tubes + a few shadowed spot/point lights + GTAO + contact shadows, then the baked Blender irradiance volume. This alone delivers the signature Backrooms look on mid-range hardware. **Threshold to proceed:** stable 60 fps at 1080p with the target scene visible.
2. **Treat the CPU light grid (Phase 2) as a core gameplay system, not an afterthought** — it is what makes stealth/entity-in-the-dark mechanics possible and is cheap. Build it before flicker so AI perception is defined early.
3. **Do not depend on emissive-casts-light or area lights** — neither exists in raster Bevy 0.19. Bake instead. **Revisit only if** you adopt the Solari RT tier (Phase 5) and can require an RT GPU.
4. **Cap shadow casters (4–8) and cull active direct lights to a camera radius** from day one; this is the primary knob for hitting frame budget.
5. **Escalate to LTC (Phase 4) only if** playtests show the point-light tube approximation looks wrong on hero fixtures, and **to Solari (Phase 5) only if** you decide to require RT hardware — otherwise the baked pipeline is the shipping target.
6. **Inspect the actual repository** (`foundation_vs_slop`) as the first concrete step and reconcile component/resource names; this design assumes a clean Bevy 0.19 baseline because the repo could not be verified remotely.

## Caveats
- **The target repository could not be inspected remotely** (robots-blocked, unindexed). All repo-specific claims are unverified; reconcile the design with any existing code in the now-available local clone.
- **Bevy moves fast.** All API facts are pinned to the **0.19.0** release (19 June 2026) and its docs; irradiance-volume, light-probe, and Solari APIs have changed every few releases and may shift again. Verify against the exact crate version in the repo's `Cargo.toml` before implementing.
- **No built-in GI baker** in Bevy 0.19 — the Blender Eevee + `bevy-baked-gi` round-trip is a real pipeline dependency and a maintenance cost; confirm `bevy-baked-gi` supports 0.19 (it has historically lagged Bevy releases) or budget for a custom baker/exporter.
- **Performance numbers are estimates.** The frame-budget figures are reasoned from Bevy benchmark history and the cited papers' reported timings (which are on different, often higher-end, hardware — e.g., DDGI's 6 ms and ReSTIR's 50 ms are on an RTX 2080 Ti / modern GPU), not measured on the target GPU/scene. Profile early.
- **Web targets are degraded:** WebGL2/WebGPU support only the single closest irradiance volume and lack GTAO (no compute in WebGL); a browser build needs a fallback lighting path.
- **Solari is experimental** and not production-ready in 0.19; do not plan a shipping title around it as the baseline.
- **LTC and any RT/stochastic path are custom or feature-gated work** beyond Bevy's built-ins; treat their effort/risk as separate from the raster baseline.

---

*Sources: Bevy 0.19 release notes (bevy.org/news/bevy-0-19), `docs.rs/bevy` 0.19 (`bevy::pbr::irradiance_volume`), Bevy GitHub PRs #3153/#3989/#7402 and issue #7662, jms55 "Realtime Raytracing in Bevy 0.19 (Solari)"; Kajiya 1986; Cook & Torrance 1982; Ramamoorthi & Hanrahan 2001; Greger et al. 1998; Sloan/Kautz/Snyder 2002; Crassin et al. 2011; Boissé et al. GI-1.0 2023; Majercik et al. DDGI 2019 / DDGI Resampling 2021; Bitterli et al. ReSTIR 2020; Ouyang et al. ReSTIR GI 2021; Lin et al. GRIS 2022; Boksansky et al. Ray Tracing Gems II 2021; Schied et al. SVGF 2017 / A-SVGF 2018; Williams 1978; Donnelly & Lauritzen 2006; Datta et al. Neural Shadow Mapping 2022; Heitz et al. LTC 2016 / Heitz & Hill 2017; Jimenez et al. GTAO 2016; Kerbl et al. 3DGS 2023; Stengel/Majercik et al. 2021; Guo et al. 2022.*
