- Use home still and lookup research on whatever you are implementing BEFORE you implement it.  We want SOTA and best practices.
- Do not use unwrap() or anything that'd lead to a panic.  Code safe.  Handle errors.
- Leave academic paper references in comments, if a paper was used in writing the code.
- Rember compilation cost time; try to bunch changes and use `cargo check` to spot issues
- Always run the full test suite (including determinism and headless behavioral tests) after modifying gameplay/simulation code, and verify determinism before shipping.
- Do NOT assume design decisions on my behalf. When a design or scope choice is ambiguous (colors, coverage %, approach), stop and ask before implementing. Prefer focused/concrete changes over global post-process filters or over-engineered solutions.
- When investigating whether an issue is fixed, actually inspect the underlying data/code first before offering explanations; do not assume a file is broken or blame viewport/version.
- Ensure every feature added is correctly included in the RL/QD systems for evolving.
- Keep the file /mnt/codex_fs/game_assets/projects/scp_characters/BEVY_GAME_INFO.md up to date with game info, to ensure our 3D artists are able to make assets that fit the game well. (The old `game_assets/SCP_Characters/` path no longer exists — the project moved under `projects/`.)
- When items are complete, move them from `BACKLOG.md` to `BACKLOG_ARCHIVE.md`.  DO NOT DELETE `BACKLOG_ARCHIVE.md`.
- **Read the vendored Bevy 0.19 source before writing Bevy code** — not bevy.org, which tracks `main`. See "Bevy 0.19" below.

## Bevy 0.19 — read the vendored source, not the web

We are pinned to **`bevy 0.19.0`**. bevy.org documents `main`, and it has been wrong for us more than
once. Every Bevy question this project has answered *correctly* was answered by reading the local copy;
every one answered from memory or the web cost a build-and-run cycle. **Check it before writing Bevy
code, not after a failed build.**

The authoritative copies are all local. `$BEVY` below is
`~/.cargo/registry/src/index.crates.io-*/bevy-0.19.0`:

| Source | Use it for |
|---|---|
| **`$BEVY/examples/` — 411 examples at exactly our version** | *"How do I use X?"* Grep here **first**. `3d/render_to_texture.rs`, `asset/asset_saving.rs`, `ui/`, `gizmos/`, `scene/`, `picking/` cover most of what this project reaches for. |
| **Crate source** — `~/.cargo/registry/src/*/bevy_{ecs,camera,ui,ui_widgets,gizmos,scene,light,image,picking}-0.19.0/src/` | Settles any API question in seconds and cannot be stale. Field vs component, required components, exact signatures. |
| **Rustdoc** — `cargo doc -p bevy --no-deps` → `target/doc/bevy/index.html` | Browsable index of the pinned API. `target/` is gitignored; rebuild after `cargo clean`. |
| **`$BEVY/_release-content/migration_guides.md`** | What moved since 0.18. |

### Traps already paid for

Each of these cost real time. They are 0.19 facts, verified in the source above.

- **A missing `Res<T>` panics the system**; it does not skip. Take `Option<Res<T>>`, or have the plugin
  that registers the reader `init_resource` it.
- **All run conditions are evaluated — there is no short-circuit.** A bare `Res<T>` in a `.run_if(..)`
  closure panics whenever that resource is absent, even behind an earlier condition that returned
  false. This shipped and crashed every launch that did not use the feature. Use `Option<Res<T>>`.
- **`Single<..>` silently skips its system** on a non-unique match. So **any second camera breaks every
  `Single<.., With<Camera3d>>`** — the audio listener, every billboard, `selection`, `drive_camera`.
  Filter positively on `crate::MainCamera`, never on `With<Camera3d>` alone.
- **A bundle containing two of the same component panics.** `button_visual()` already carries a `Node`;
  `.insert()` afterwards to override, do not pass a second one in the tuple.
- **`TransformGizmoPlugin` is unusable here.** Its overlay camera blanks an HDR main camera's output
  (measured: 13,343 → 183 distinct colours) *and* spawns the second `Camera3d` above.
- `Camera::clear_color` is a **field**; `RenderTarget` is a **separate component** (one of `Camera`'s
  `#[require]`s). `AmbientLight` is a **component** in 0.19 and applies per-camera.
- `add_plugins` tuples cap at **15** (`all_tuples!(impl_plugins_tuples, 0, 15, P, S)`,
  `bevy_app-0.19.0/src/plugin.rs:186`). Nest to go further.
- `Resource` is now a subtrait of `Component`; you cannot derive both.

`docs/ui.md` §5 carries the UI-specific traps (grid placement panics, the 95-codepoint default font,
`Pickable::IGNORE` on full-screen containers, layer ordering).

## Testing

**Read `TESTING.md` before writing or running tests** — it documents the whole system (what exists, how to
run it, how to add to it). The one-liners:

- `cargo test` — deterministic-core layer (RNG/WFC/utility/ORCA/laser). Fast, GPU-free, the CI hard gate.
- `cargo test --features test-harness -- --test-threads=1` — headless replay / liveness / SSIM. Boots the
  real game with no window; **needs a GPU**.

Non-negotiables (details in `TESTING.md`): exact-hash only the **physics-off** core
(`SimConfig::deterministic_core()`) — the Avian solver is not bit-reproducible, so physics-on runs use
**liveness** oracles; hold `serial_guard()` in every harness test; new systems go on `FixedUpdate` if they
touch pinned state (would appear in `snapshot_hash`), else `Update`. Strategy, oracle rules, and the full
invariant list live in `TESTING.md` (see its "Strategy" and "Invariants & determinism rules" sections).

## Animation

**Read `docs/animation.md` before touching `src/anim/`, squad/crab/manca clip wiring, or a character
GLB's clips.** It's the engineering guide; `docs/artist_guide.md` §4 holds the per-asset clip tables and
the authoring contract. The one idea: **no transitions.** Every clip stays resident on the
`AnimationPlayer` and is never rewound — each frame the shared `anim::PoseBlender` only eases weights and
advances one shared gait phase. So **never add `AnimationTransitions` to anything the blender drives**
(its `PostUpdate` pass would stomp the weights).

Non-negotiables (details in `docs/animation.md`): the animation layer is **cosmetic** — `Update` only
(never `FixedUpdate`), `PoseBlender` rides the **model child** not the sim entity (issue #18), and it is
the **deliberate exception** to the "wire every feature into RL/QD" rule above — it is invisible to
`snapshot_hash` by construction, so a genome gene pointed at it would never move the fitness. Cosmetic
tuning goes in the `src/anim`/`src/squad` constants and `docs/artist_guide.md`, not the evolving systems.
Gait clips must be authored **in-place** (zero root motion) with an honest per-cycle ground distance;
`tests/valkyrie_asset.rs` pins the GLB contract.

## Determinism: ECS query order decides nothing

Query order is **not stable across `App` instances**. Anything it could decide — a shared RNG draw or
counter, a `take(n)` budget, a clamped accumulate, a last-writer-wins write, a lethal pick — needs a stable
**total** key: `sort_total!` (panics on a tie, naming the site), `util::sort_value_canonical` (ties
interchangeable → sort the WHOLE value), or `// SORT-OK: <why>`. `tests/determinism_lint.rs` enforces it.

Four sites documented the exact trap they then fell into, so don't trust a comment claiming a total order.
Both shapes: a key that is a **prefix of the value** (`(pos)` when the element is `(pos, payload)`), and a
**tiebreak derived from the tied quantity** — `GibKey` hashed the position it existed to disambiguate.

A determinism probe on an idle box proves nothing: run it under load.

## Additional Game Assets
- Additional games assets are cataloged at /mnt/codex_fs/game_assets/CATALOG.md, feel free to use any of these.

## Screenshots

To capture a frame from the running game, use the **`screenshots` skill** — the game screenshots
itself from inside the render pipeline (`src/devshot.rs`); never reach for the macOS `screencapture`
tool.

- **Player region captures live in `debug_screenshots/`.** When the player runs the game and presses **Ctrl+P**, they drag a box and release to save *just that region* to `debug_screenshots/region_<timestamp>.png` — a deliberate "look here" pointer at whatever they're asking about. If the player references something visual, **check `debug_screenshots/` (newest first) and read `debug_screenshots/CLAUDE.md`.** Produced by `src/region_capture.rs` (dev-only, stripped from release).
