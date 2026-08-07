# emerge-core

The engine-free half of world building: the asset descriptor and map schemas, a constraint IR with
three solver backends, WFC, Poisson/Delaunay geometry, a seeded ChaCha8 RNG, comment-preserving RON
surgery, a hand-rolled glTF reader, and animation-clip measurement.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. Issues and PRs belong upstream.

## Why "engine-free" is enforced, not just intended

Three consumers share this crate — a game, an offline search that fans out across worker subprocesses,
and a standalone editor — and none of them should have to agree on a renderer to share a schema.

That was a *comment* for months, and a comment cannot fail a build. `tests/engine_free.rs` is the
ratchet: it fails if a `bevy`/`avian`/`wgpu`/`winit` dependency appears in the manifest, and a second
test scans the sources as a backstop. Widening the dependency list should cost an argument and a
deliberate edit, not a passing `cargo build`.

## License

GPL-3.0-only, with the game it was carved out of.
