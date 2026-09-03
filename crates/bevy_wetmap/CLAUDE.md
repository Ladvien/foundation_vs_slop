# bevy_wetmap — the non-negotiables

Read this before editing anything under `crates/bevy_wetmap/`. This directory **is** the root of a
public mirror ([`Ladvien/bevy_wetmap`](https://github.com/Ladvien/bevy_wetmap)), split out of
`Ladvien/foundation_vs_slop` with `git subtree split`. Changes flow **one way**: monorepo → mirror.
Nothing is ever edited on the far side and nothing is pulled back.

## The CPU is the authority. The GPU is write-only.

This is the crate's entire reason to exist. Texture-space blood accumulation elsewhere is a GPU render
target, which is why nobody can hash one; this one keeps `(amount, age)` on the CPU in integers and
treats the two `Image`s as output. `crates/bevy_carnage/src/vfx.rs:6-19` states the rule and the
reason.

So: **nothing reads `Assets<Image>`.** A future idea of the form "sample the canvas in a shader and
feed the result to gameplay", or "read the uploaded pixels back to place decals", must be refused —
the CPU buffer is already there and is already the authority. `digest()` folds the buffer, never the
images, and that is not an implementation detail: it is the claim.

## Row-major is the canonical order

One `Vec<(u8, u16)>`, row-major. That order **is** the order the tick pass walks, so no sort is needed
and none may be added. A sort here would be a second answer to a question the layout already answers,
and the digest would then depend on which answer ran.

## The moving passes read a snapshot and write disjoint slots

Both the drip and the spread pass copy `wet` into `prev`, then compute each texel's new value from
`prev` alone. That is what makes the result independent of traversal order, and it is what makes the
digest mean anything. Two unit tests defend it directly —
`the_drip_pass_moves_a_parcel_exactly_one_texel` and
`the_spread_pass_reaches_only_the_four_neighbourhood_in_one_call` — because a pass that read its own
writes would cascade in one call and both would go red.

**Both passes conserve mass to the byte, and each has a separate argument for it.** The drip leaves the
threshold behind, so a wet destination's residue is at most `threshold` while a parcel is at most
`255 − threshold`: they sum to exactly 255, so an arrival between two wet texels cannot overflow and
there is no clamp on that path. The spread is an antisymmetric flux on the coverage *difference*, and
`f32::round` is odd, so `flux(i→j) == −flux(j→i)` exactly. If you rewrite either as a "give a fraction
away" loop, both arguments die and the conservation tests go red — that is the tests doing their job,
not a table to update.

The single lossy path is blood running onto a **saturated dry crust**, which does not shed and so has
no room. That loss is the model: `amount` is normalised coverage, a texel at 255 is fully covered, and
more blood on it would not be visible. Do not "fix" it by letting the crust move.

## Dry paint does not move

Wetness gates every change to `amount`. A texel past `dry_ticks` neither drips, nor spreads, nor soaks,
and its age has stopped — so a dried canvas is a **fixed point** of `tick`, and asks for no upload.
That is what makes a run stop where it stopped. Do not add a "dry blood slowly fades" path; a second
rule for the same quantity is how a run would start creeping again three months later.

## `flush` is the only place `Assets<Image>` is touched

One function, and it uploads only when dirty. The per-frame budget across canvases is the plugin's, and
it is ordered `(dirty_since, Entity)` — an integer tick first, so it is reproducible, with `Entity`
breaking a tie only between canvases that went dirty on the same tick, where the only thing at stake is
which write-only image gets its bytes first.

## Ticks, not seconds. No clocks.

`tick` takes the tick *number*. Nothing here reads a clock, virtual or real, and nothing may. A float
accumulator large enough stops advancing at all, which is a recorded failure in this family of crates.
`dry_ticks` is quoted at 60 Hz; a caller on another rate re-derives it in data.

`dry_ticks` is also the **single authority** for the drying timeline: a texel's age is rescaled onto
`bloodstain::dry::DRY_REF_TICKS` before `appearance` is asked anything, so moving the dial moves the
whole curve rather than only the wet/dry gate.

## The caller owns the schedule

`WetmapPlugin` registers exactly one system, the upload budget, because uploading is the only part with
no gameplay opinion in it. It does **not** tick canvases: a tick number is gameplay state, and
inventing one would mean reading a clock.

Both of that system's resources are `Option`, deliberately. Bevy 0.19 *panics* a system with a missing
`Res<T>` rather than skipping it, and `Assets<Image>` belongs to a plugin this crate does not add.

## No shader, no asset, no material

The crate hands back two `Handle<Image>` and never names a material — which is why it does not depend
on `bevy_pbr`. The dry surface is composited *into* the canvas on the CPU, so there is nothing left to
blend in WGSL. If a caller's blood looks flat, the cause is almost always that their
`StandardMaterial` left `perceptual_roughness` and `metallic` at the shipped scalars, which Bevy
multiplies by the texture (`bevy_pbr-0.19.0/src/pbr_material.rs:157-163`); both must be `1.0`.

Roughness is the **green** channel and metallic the **blue** one, stated at
`bevy_pbr-0.19.0/src/pbr_material.rs:153-154`. The metallic-roughness image is `Rgba8Unorm`, **not**
sRGB: it carries material data, not colour.

## Rules

- **Bevy 0.19 is pinned.** Read the vendored source, not bevy.org — that documents `main` and has been
  wrong for this pin more than once. `bevy_render` is in the feature list for exactly three type names
  (`Extent3d`, `TextureDimension`, `TextureFormat`); see the comment in `Cargo.toml`.
- **No `unwrap()`, no `expect`, no `panic!`, no panicking index in library code.** The hot loops use
  `get`/`get_mut` with a `continue` guard even where the index is in bounds by construction, so the
  crate contains none at all. A mesh that cannot carry a wetmap gets `false` and one `warn!`.
- **One path per feature.** No fallbacks, no legacy shims, no stub placeholders, no setting that picks
  between two implementations of one thing.
- `tests/leaf.rs` is the ratchet: allowed dependencies are `bevy` and `bloodstain`. Widening it is a
  design decision and should cost a deliberate edit there, not a passing build.

## Furniture the mirror script enforces

`scripts/mirror_crates.sh` refuses to mirror a crate missing `README.md` (opening with the **Vibe
Coded** warning, then the mirror notice, then an `## Examples` section), `CLAUDE.md`, `Cargo.toml`, a
licence, or `examples/*.rs`. `examples/canvas_digest.rs` is **terminal only** — no window, no GPU — and
that is deliberate: the crate's headline claim is reproducibility, and a claim like that has to be
checkable over ssh.
