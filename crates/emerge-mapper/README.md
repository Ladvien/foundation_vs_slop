# emerge-mapper

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

A standalone world-building editor: open a project directory, author a map from an asset library, save something any engine can load. A Bevy application in its own right, not a mode of a game.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. It depends on sibling crates by workspace path, so it builds *inside* that workspace, not on its own. Issues and PRs belong upstream.

## Three tabs

- **Map** — palette, ghost, place, fill, undo.
- **Tiles** — mesh import, measurement review, descriptor authoring, and a vision-LLM labeler whose suggestions stage behind the same commit door a human edit goes through.
- **Anim** — a rig bench: measure GLB clips, compare against `rigs.ron`, plot diagnostics, and scrub a staged figure driven by the **real** `emerge-anim` blender rather than a preview copy of it.

## Testable without a GPU

`harness::build_headless` builds the same plugin graph with `WgpuSettings { backends: None }` and no window, so `tests/headless.rs` can step frames and assert. That matters more than usual here: in Bevy 0.19 a missing `Res<T>` **panics its system**, and no unit test can answer "does this app survive its first frame".

## Examples

```sh
cargo run -p emerge-mapper --example boot_headless -- <project-root> [map-name]
```

Boots the shipped editor plugin graph with no window and no wgpu device, steps frames, and reports that nothing panicked. That question has teeth in Bevy 0.19, where a missing `Res<T>` panics its system rather than skipping it. A real project directory is required — inventing one here would be a second source of truth for what a project looks like.

## License

GPL-3.0-only, with the game it was carved out of.
