# bevy_cross_section — the non-negotiables

Read this before editing anything under `crates/bevy_cross_section/`. This directory **is** the root of a public mirror ([`Ladvien/bevy_cross_section`](https://github.com/Ladvien/bevy_cross_section)), split out of `Ladvien/foundation_vs_slop` with `git subtree split`. Changes flow **one way**: monorepo → mirror. Nothing is ever edited on the far side and nothing is pulled back.

## The table has sources, and a number without one says so

`Layers::for_region` is the crate. Every value in it is either a number from a paper cited in `src/layers.rs`'s module docs, a stated derivation from one (the head split), or flagged as this crate's own (cortical bone). A new number goes in with its DOI or with the flag; never silently. The test `the_measured_rows_are_the_papers` pins the sourced ones to the tenth of a millimetre they were reported at, so retuning a band for looks fails a test — which is the point.

## Depth is measured, never guessed

`depth_below_skin` is exact for a point inside a convex cell with the cell's supplied faces as planes. Do not replace it with a distance to the mesh, a raycast, or a per-vertex "inset" — those are approximations of a quantity this crate can compute exactly, and they would cost a mesh query where this costs a dot product.

## `UV_0` is not this crate's to touch

Caps arrive with planar cross-section UVs in `UV_0` that other crates and their goldens depend on. This crate writes `UV_1` and nothing else. A material that wants the bands samples through `UvChannel::Uv1`.

## The strip is a pure function

`strip(layers, width, height, seed)` reads nothing but its arguments and `bloodstain::hash_f32`. No clock, no RNG crate, no global. `the_strips_are_frozen` pins the digest of all three regions; if a change moves it, that change re-blesses the golden deliberately, in the same commit, with the reason.

## Verify

```sh
cargo test                  # unit + tests/leaf.rs + doctests
cargo build --release       # the trimmed `bevy` feature list has to build without the dev umbrella
cargo build --examples
```
