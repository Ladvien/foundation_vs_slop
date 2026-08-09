# bevy_light_grid — notes for agents

An illuminance grid your **creatures** can read: a CPU scalar field over cells that answers "how bright is it here, and which way is brighter", plus the photophobic / phototropic / photophilic taxis markers.

## Source of truth

The source of truth for this crate is [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) at `crates/bevy_light_grid/`. If you are reading this in a standalone `Ladvien/bevy_light_grid` checkout, that is a read-only `git subtree split` mirror — **changes made here cannot be pulled back**. Make them upstream.

## Build and test

A leaf — `bevy_math` plus `bevy_ecs` (the latter *only* for the three taxis marker components) — so it builds and tests on its own: `cargo test -p bevy_light_grid`.

## The non-negotiable: this is not a renderer

`tests/leaf.rs` is the ratchet. Allowed: `bevy_math`, `bevy_ecs`. Forbidden by name: `bevy_app`, `bevy_render`, `bevy_pbr`, `avian`, `wgpu`, `winit`, `emerge`, `foundation_vs_slop`.

`bevy_render`/`bevy_pbr` are forbidden for a reason worth stating out loud: **the day this crate reaches for a renderer is the day somebody has confused "how bright is this cell for the AI" with "what colour is this pixel".** Those are different questions with different answers. The GPU lighting pass already knows the second one, but the answer lives in a framebuffer, and a creature deciding whether to scuttle into shadow cannot read a framebuffer.

`bevy_app` is forbidden too, because a field is only reusable if the **caller** owns the schedule — when the bake runs, and where it sits relative to the agents reading it, are gameplay decisions. This crate registers no system and no plugin.

**Occlusion is the caller's.** Both passes take the line-of-sight test as a closure; this crate never learns what a wall is. Keep that signature monomorphised and `bool`-returning so it cannot perturb a float.

The two passes are split on cost, and that split is load-bearing: `bake` is expensive and event-driven (static fixtures, re-run when one changes), `compose` is cheap and per-tick (only the moving cones, re-added onto the cached base). Do not collapse them.

## Rules

- **Bevy 0.19 is pinned.** Read the vendored `bevy_math-0.19.0` / `bevy_ecs-0.19.0` source, not bevy.org — that documents `main` and has been wrong for this pin more than once.
- **No `unwrap()`**, no `expect` on caller data, no panicking index — cell coordinates come from gameplay.
- **One path per feature.** No fallbacks, no legacy shims, no stub placeholders.

## In the monorepo

The game wraps this as `LightField { core, dirty }` (`src/light.rs:341`). Note what stays on the game side: `dirty` is bake-gating for *this game's* fixtures, and a grid that tracked it would be guessing at a schedule it does not own. Root `CLAUDE.md` and `TESTING.md` carry the project-wide rules; neither is part of this mirror.
