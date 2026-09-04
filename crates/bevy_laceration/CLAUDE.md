# bevy_laceration — the non-negotiables

Read this before editing anything under `crates/bevy_laceration/`. This directory **is** the root of a public mirror ([`Ladvien/bevy_laceration`](https://github.com/Ladvien/bevy_laceration)), split out of `Ladvien/foundation_vs_slop` with `git subtree split`. Changes flow **one way**: monorepo → mirror. Nothing is ever edited on the far side and nothing is pulled back.

## The tear is a pure function, and `digest` is how we know

`tear(mesh, path, normal, shape, region, layers, scale)` reads nothing but its arguments and `bloodstain::hash_f32`. No clock, no RNG crate, no global, no query order. `the_tear_is_frozen` pins both output digests; if a change moves either one, that change re-blesses the constants deliberately, in the same commit, with the reason — and `examples/laceration_curve.rs` prints the same two numbers, so a reader can check the claim without the suite.

The example's half-width is spelled `CELL * 1.2`, not `0.06`, because those are different `f32` bit patterns and a digest is a digest of bits. Keep it that way.

## The gape only ever opens

The time curve is monotone by construction and there is no closing half. That is not a simplification — it is O'Brien, Bargteil & Hodgins (2002), `doi:10.1145/566570.566579`: plastic deformation is *retained* ahead of separation, so a laceration cannot spring back. Adding a heal, a close or an elastic recoil is a different model and needs a different name.

## Every retear starts from the intact source

`Laceration::source` is never written. The gape is a function of `(clock - opened_at)` and nothing else, so re-running the system twice on the same tick produces the same mesh — an accumulating edit would drift, and a drifting wound cannot be hashed or rewound. The plugin refuses, with one warning, when the entity's own `Mesh3d` handle *is* the source, because that would destroy the intact copy on the first frame; `a_laceration_that_draws_its_own_source_is_refused_rather_than_destroying_it` pins it.

## The vertex buffer keeps its length

Only positions and indices are rewritten. Everything else — normals, `UV_0`, `ATTRIBUTE_JOINT_INDEX`, `ATTRIBUTE_JOINT_WEIGHT`, vertex colours, anything custom — arrives untouched because the skin mesh is a clone of the input, not a rebuild. Do not "optimise" this by compacting the buffer: a re-index is a copy per attribute per retear, and getting it wrong is a limb following the wrong bone.

## No number without a source, and a made-up number says so

Three papers carry this crate, cited in `src/curve.rs` and `src/tear.rs` module docs with their DOIs. Everything else is flagged in the doc comment as this crate's own: `Gape::open_ticks` (no paper gives a *rate*), the `3` in the exponent (the 95 %-at-`open_ticks` choice), `RAIL_WANDER` and `WANDER_MM` (nothing tabulates the raggedness of a wound margin), and `ALONG_LANGER_FACTOR`, which is a **stiffness** ratio used as a **gape** proxy. A new number goes in with its DOI or with the flag; never silently.

## Nothing panics, and refusals are loud once

No `unwrap`, no `expect`, no indexing that can fail, anywhere in `src/`. `tear` returns `Option` and every refusal is a `warn_once!` naming what was wrong. Read meshes through `try_attribute_option` / `try_indices_option`, **never** `attribute()` / `indices()`: Bevy 0.19's plain accessors `expect` when a mesh's vertex data has been extracted to the render world, and that is reachable from a caller who authored `RenderAssetUsages::RENDER_WORLD`.

Every resource in a system signature is `Option<Res<..>>` or `Option<ResMut<..>>`, including ones this plugin inits, because in 0.19 a missing `Res<T>` *panics* the system rather than skipping it — and `CrossSectionSettings` being absent is a supported configuration, not an error.

## Depth belongs to `bevy_cross_section`

The bed's `UV_1` comes from `uv1_at`, never from a fraction computed here. One definition of depth-below-skin, shared with every cut face in the family; `the_bed_floor_sits_at_the_authored_depth` pins that a rail reads 0 and the floor reads `bed_depth_mm / span_mm`.

## Verify

```sh
cargo test                  # unit tests + tests/{tear,plugin,leaf}.rs
cargo build --release       # the trimmed `bevy` feature list has to build without the dev umbrella
cargo build --examples
cargo run --example laceration_curve   # twice: the two digest lines must match
```
