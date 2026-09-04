# bevy_flaymap — the non-negotiables

Read this before editing anything under `crates/bevy_flaymap/`. This directory **is** the root of a
public mirror ([`Ladvien/bevy_flaymap`](https://github.com/Ladvien/bevy_flaymap)), split out of
`Ladvien/foundation_vs_slop` with `git subtree split`. Changes flow **one way**: monorepo → mirror.
Nothing is ever edited on the far side and nothing is pulled back.

## The CPU is the authority. The GPU is write-only.

This is the crate's entire reason to exist. Texture-space damage masking is shipped technology and
everywhere it exists it is a GPU render target — Frostbite 2 revealed a layered material out of one
(Kihl, SIGGRAPH 2010 Advances course) — which is why nobody can hash one. This crate keeps the removed
depth on the CPU in integers and treats the two `Image`s as output.
`crates/bevy_carnage/src/vfx.rs:6-19` states the rule and the reason.

So: **nothing reads `Assets<Image>`.** A future idea of the form "sample the flaymap in a shader and
feed the result to gameplay", or "read the uploaded pixels back to place a decal", must be refused —
the depth buffer is already there and is already the authority. `digest()` folds that buffer, never the
images, and that is not an implementation detail: it is the claim.

## Depth is one `u16` per texel, monotone, and row-major

Hundredths of a millimetre removed, saturating at `Layers::span_mm()`. Row-major **is** the canonical
order every pass walks, so no sort is needed and none may be added: a sort would be a second answer to
a question the layout already answers, and the digest would then depend on which answer ran.

**Tissue does not grow back.** Every write either adds to a texel or leaves it alone. Do not add a
"wounds close over time" path — a second rule for the same quantity is how a monotone buffer starts
oscillating three months later, and it would put a clock in a crate that has none.

Integers rather than an `f32` because the buffer is the thing the digest folds: a float accumulation
would make the wound depend on the order the hits were summed in.

## `shade` is derived, and deliberately outside the digest

`shade` is a pure function of the depth buffer, the layer table and `FlaySettings`. That is why
`digest()` does not fold the pixels: it would hash the same information twice, and a palette tweak
would then read as a simulation divergence. If you ever need the pixels in a hash, the depth buffer is
what you actually want.

Every peeled texel's colour and roughness is `bevy_cross_section::texel_at` — **the identical
per-texel rule that bakes a cut face's strip**. Do not author a flaymap palette here. A flayed patch
and the stump beside it drifting apart is exactly what one shared function prevents.

## A hit peels a crater, not a cylinder

`paint_uv` adds its full depth at the centre and smoothstep-falls to zero at the radius, quantised to a
byte so the depth a texel receives is an integer multiply. A flat disc would stack into a bore with a
vertical wall and the layer bands would never show; the falloff is what makes the rim readable, and it
is the crate's whole visual. The stamp's edge length is forced **odd** so the footprint is symmetric
about its centre texel — an even one would drift a crater by half a texel per hit.

## The bone handoff fires exactly once

`Handoff::bone_reached` is true on the first paint call in which any texel crosses
`Layers::starts_mm()[3]`, and false forever after; `bone_handed_off` is the whole mechanism and
`BoneExposed::from_handoff` is the whole gate. A flag that stayed true would make a consumer spawn a
fracture proxy, or a bone-scrape sound, once per shot for the rest of the fight.

`at` and `normal` are `Some` from `paint_world` and `None` from `paint_uv`, and that asymmetry is not
an oversight to tidy up: a UV names a point on an *atlas*, and a seam maps one UV to several places on
a body, so a `paint_uv` that invented a mesh position would be guessing.

## A UV off the canvas is refused, not clamped

This is the one place the crate deliberately differs from `bevy_wetmap`, which clamps to the edge.
Blood on the wrong texel is cosmetic; peeling a body's edge texels to bone because a ray came back with
a UV of `1.4` is a gameplay error, and the caller would then get a bone handoff for a hit that never
landed. One `warn!` per canvas, then silence.

## No clock, no RNG

`paint_uv` takes the tick *number* and `shade` is called by the caller. Nothing here reads a clock,
virtual or real, and nothing may. The only random source anywhere in the crate is
`bloodstain::hash_f32`, reached through `texel_at`, keyed by integer lattice coordinates and
`FlaySettings::seed` — so a wound is a pure function of the hits that made it, and
`the_scripted_wound_is_frozen` can pin a digest.

## The caller owns the schedule and the message

`FlaymapPlugin` registers exactly one system, the upload budget, because uploading is the only part
with no gameplay opinion in it. It registers `BoneExposed` and **writes it nowhere**: only the caller
knows whether the thing it peeled has a skeleton something else owns.

Both of that system's resources are `Option`, deliberately. Bevy 0.19 *panics* a system with a missing
`Res<T>` rather than skipping it, and `Assets<Image>` belongs to a plugin this crate does not add.

## No shader, no asset, no material

The crate hands back two `Handle<Image>` and never names a material — which is why it does not depend
on `bevy_pbr`. The intact surface is written *into* the canvas on the CPU, so there is nothing left to
blend in WGSL. If a caller's wound looks flat, the cause is almost always that their `StandardMaterial`
left `perceptual_roughness` at the shipped scalar, which Bevy multiplies by the texture
(`bevy_pbr-0.19.0/src/pbr_material.rs:157-163`); it must be `1.0`.

Roughness is the **green** channel and metallic the **blue** one, stated at
`bevy_pbr-0.19.0/src/pbr_material.rs:153-154`. The metallic-roughness image is `Rgba8Unorm`, **not**
sRGB: it carries material data, not colour.

## Rules

- **Bevy 0.19 is pinned.** Read the vendored source, not bevy.org — that documents `main` and has been
  wrong for this pin more than once. `bevy_render` is in the feature list for exactly three type names
  (`Extent3d`, `TextureDimension`, `TextureFormat`); see the comment in `Cargo.toml`.
- **No `unwrap()`, no `expect`, no `panic!`, no panicking index in library code.** The hot loops use
  `get`/`get_mut` with a `continue` guard even where the index is in bounds by construction, so the
  crate contains none at all — and `tests/leaf.rs::the_library_holds_no_panicking_call` sweeps `src/`
  for them. Tests may panic; that is what an assertion is.
- **One path per feature.** No fallbacks, no legacy shims, no stub placeholders, no setting that picks
  between two implementations of one thing. A non-finite `scale.mm_per_unit` collapses the noise onto
  one phase rather than selecting a second scale.
- **`bevy_wetmap` is a sibling, not a base.** `src/uv.rs` reimplements Möller–Trumbore rather than
  importing its copy, so an actor that only ever gets flayed does not resolve a blood-drip model it
  never calls. `tests/leaf.rs` is the ratchet: allowed dependencies are `bevy`, `bevy_cross_section`,
  `bloodstain` and `serde`. Widening it is a design decision and should cost a deliberate edit there.

## Furniture the mirror script enforces

`scripts/mirror_crates.sh` refuses to mirror a crate missing `README.md` (opening with the **Vibe
Coded** warning, then the mirror notice, then an `## Examples` section), `CLAUDE.md`, `Cargo.toml`, a
licence, or `examples/*.rs`. `examples/flay_digest.rs` is **terminal only** — no window, no GPU — and
that is deliberate: the crate's headline claim is reproducibility, and a claim like that has to be
checkable over ssh. `examples/flaymap_peel.rs` touches no `std::fs` and reads no clock, so it builds
for `wasm32-unknown-unknown` as-is.
