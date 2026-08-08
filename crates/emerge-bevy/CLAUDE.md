# emerge-bevy — notes for agents

The runtime half of emerge: take an `emerge-core` library plus a map, and put it in a Bevy world.

## Source of truth

The source of truth for this crate is [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop)
at `crates/emerge-bevy/`. If you are reading this in a standalone `Ladvien/emerge-bevy` checkout, that is
a read-only `git subtree split` mirror — **changes made here cannot be pulled back**. Make them upstream.

## Build and test

**Not a leaf.** It path-depends on the sibling `emerge-core`, so it builds *inside* the workspace, not on
its own: `cargo check -p emerge-bevy`.

This is the **only** crate in the emerge set that knows what a renderer is, on purpose. `emerge-core` is
the schema, the solvers and the validation with no engine in it, and
`crates/emerge-core/tests/engine_free.rs` fails the build if that stops being true. This is the other
side of that line.

## The non-negotiable: one spawner

`spawn_descriptor` is deliberately the **single** shared spawner. `emerge-mapper` shows you a map and the
game plays it, and they must agree about what a placement looks like down to the last degree of yaw. The
way to guarantee that is not care — it is having one function.

**Do not add a second spawner**, not for the editor, not for a test, not behind a flag. The alternative
was tried elsewhere in this tree and is on the record: `bake.rs` and `site_editor::source_map`
independently grew the same RON writer, and one of them had a bug the other did not.

Masks, stacked Y, role masks and seat positions all resolve **once at load**, before anything spawns.
Keep that ordering; resolving per-spawn is how two callers start disagreeing.

## Rules

- **Bevy 0.19 is pinned.** Read the vendored source
  (`~/.cargo/registry/src/index.crates.io-*/bevy-0.19.0/`, and its `examples/`), not bevy.org — that
  documents `main` and has been wrong for this pin more than once.
- **A missing `Res<T>` panics its system in 0.19**; it does not skip. Take `Option<Res<T>>`, or have the
  plugin that registers the reader `init_resource` it.
- **All run conditions are evaluated — there is no short-circuit.** A bare `Res<T>` in a `.run_if(..)`
  closure panics whenever that resource is absent, even behind an earlier condition that returned false.
- **A bundle containing two of the same component panics.** Insert afterwards to override; do not pass a
  second one in the tuple.
- **No `unwrap()`.** A map is a file somebody authored — a missing asset, an unknown role or a bad
  transform is an error to report, not a panic mid-spawn.
- **One path per feature.** No fallbacks, no legacy shims, no stub placeholders. Never spawn a
  substitute for a descriptor that failed to resolve; fail loudly instead.

## In the monorepo

The game loads what `emerge-mapper` writes through `src/emerge_map.rs`, which uses `EmergePlugin` /
`EmergeWorld` — that is the step that makes the editor's output real. Root `CLAUDE.md` and `TESTING.md`
carry the project-wide rules; neither is part of this mirror.
