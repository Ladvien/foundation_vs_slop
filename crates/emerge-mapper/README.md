# emerge-mapper

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

A standalone world-building editor: open a project directory, author a map from an asset library, save something any engine can load. A Bevy application in its own right, not a mode of a game.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. It depends on sibling crates by workspace path, so it builds *inside* that workspace, not on its own. Issues and PRs belong upstream.

## Four tabs

- **Map** — palette, ghost, place, fill, undo.
- **Tiles** — mesh import, measurement review, descriptor authoring, and a vision-LLM labeler whose suggestions stage behind the same commit door a human edit goes through.
- **Compose** — reusable groups a map holds a *reference* to rather than a copy of: members, the edge tokens their boundary derives, where those members disagree, and which were built against a descriptor that has since changed.
- **Anim** — a rig bench: measure GLB clips, compare against `rigs.ron`, plot diagnostics, and scrub a staged figure driven by the **real** `emerge-anim` blender rather than a preview copy of it. Last, because the other three are three views of building a level and this is a different job.

## Every refusal sticks, and can be copied

Each tab keeps a list of what has gone wrong on it. The newest is a filled block under the title; the
run is a bulleted log at the bottom of the panel. Severity is declared at the ~210 write sites rather
than guessed at render time — which is how `NOT WRITTEN:` used to draw in the same grey as a receipt.
`Cmd/Ctrl+C` copies the live tab's text, problems first, for handing to somebody else; `Esc` clears
them.

## Driveable by an agent, without taking your screen

`--features debugger` adds the companion BRP plugin and an offscreen mirror camera, so an agent can
press keys and take framed captures of the map with no window raised and no OS screen capture. The
mirror renders the map only — Bevy draws a UI tree to one camera — so `bevy_devshot`'s whole-frame
sentinel capture (`touch screenshot.request`) remains the way to see a panel.

The same feature lets an agent **guide you** rather than the other way round. `bevy_debugger/guide`
posts a walkthrough, the editor shows one step at a time over the map, and `bevy_debugger/guide+watch`
waits on a named condition — *the tile has two pieces*, *the tile is saved* — advancing itself and
recording how many attempts each step took. `src/guided.rs` holds the editor's ten conditions;
`guides/` holds the scripts. A step with no checkpoint (*"does this look right?"*) waits for you to
say so, which is the half no machine can answer and the reason a person is in the loop at all.

## Testable without a GPU

`harness::build_headless` builds the same plugin graph with `WgpuSettings { backends: None }` and no window, so `tests/headless.rs` can step frames and assert. That matters more than usual here: in Bevy 0.19 a missing `Res<T>` **panics its system**, and no unit test can answer "does this app survive its first frame".

## Examples

```sh
cargo run -p emerge-mapper --example boot_headless -- <project-root> [map-name]
```

Boots the shipped editor plugin graph with no window and no wgpu device, steps frames, and reports that nothing panicked. That question has teeth in Bevy 0.19, where a missing `Res<T>` panics its system rather than skipping it. A real project directory is required — inventing one here would be a second source of truth for what a project looks like.

## License

GPL-3.0-only, with the game it was carved out of.
