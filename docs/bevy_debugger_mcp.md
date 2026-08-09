# Agent debugging over BRP — `bevy_debugger_mcp`

[`Ladvien/bevy_debugger_mcp`](https://github.com/Ladvien/bevy_debugger_mcp) is our MCP server that lets an agent inspect a **running** instance of this game: query entities, components and resources live, instead of reasoning about them from source.

It is a **separate process**. Nothing in this workspace links it, and the game gains no HTTP client. The shape is:

```
agent  ──MCP──▶  bevy-debugger-mcp  ──BRP (JSON-RPC/HTTP :15702)──▶  the running game
```

## Why it is a Cargo feature, not a debug_assertions gate

`devshot` and `region_capture` are `#[cfg(debug_assertions)]`. BRP is not, and the difference is deliberate: BRP is not only an observation channel, it can **mutate a live `World`**. It has no business in a shipped binary, and it must never be on during a determinism run — an external writer into pinned state is precisely what the goldens exist to catch.

A Cargo feature is the only gate that removes it from the *resolved dependency graph* rather than merely from the plugin list. Measured with the feature off, building the game alone: `cargo tree -i bevy_remote` matches no package, and the resolved `bevy_math`/`glam` feature sets are byte-identical to before the feature existed.

**That holds for the game's own build, and not for `--workspace`.** Since the debugger was vendored in as a workspace member, `cargo tree --workspace -i bevy_remote` *does* match — `bevy_debugger_bevy` declares `bevy` with `bevy_remote` and `serialize` non-optionally, and Cargo unifies features across everything a single build compiles, so `bevy_internal` gains `bevy_remote` for the duration of a `--workspace` build. The determinism gate runs exactly that way.

Two things follow, and neither is hidden:

- **The shipped game is unaffected.** `cargo build`, `cargo build --release` and `cargo build --profile dist` compile only the root package, where the gate still holds exactly as measured.
- **The `--workspace` build compiles a differently-featured `bevy` than the goldens were pinned under.** Nothing *adds* `RemotePlugin` — that still requires `--features debugger` — so no extra system runs; the difference is which code is compiled, not which code executes. Treat a golden movement after touching this crate's dependencies as a signal to re-measure rather than a mystery.

## Running it

```sh
# 1. The game, with the remote protocol on (default port 15702).
cargo run --features debugger

# 2. The MCP server, once, from the copy in this repo.
cargo install --path crates/bevy_debugger_mcp --locked

# 3. Register it with Claude Code. NOTE: `--stdio`, not `stdio` — the vendored setup-claude.sh
#    prints the wrong form. `BEVY_MCP_DEV_PASSWORD` seeds the dev users; without it the password is
#    random per start and only written to stderr, which the client cannot read.
claude mcp add bevy-debugger --scope user \
  -e BEVY_BRP_HOST=127.0.0.1 -e BEVY_BRP_PORT=15702 -e BEVY_MCP_DEV_PASSWORD=<pick-one> \
  -- ~/.cargo/bin/bevy-debugger-mcp --stdio
```

**Restart Claude Code afterwards** — MCP tools do not appear until it reconnects.

Then ask the agent to look at the running game. The observation tools ride Bevy's **built-in** BRP methods (`world.query`, `world.get_components`, `world.list_components`, `registry.schema`, …), so they work with nothing in this repo beyond the two plugins.

## The companion plugin, and what it adds

The debugger ships `bevy_debugger_bevy`, which registers two **custom** BRP methods on top of Bevy's built-in ones. It lives at `crates/bevy_debugger_mcp/crates/bevy_debugger_bevy` and is reached as a path dependency behind the same `debugger` feature, so it is absent from every default, release and determinism build. Confirm both halves of that with:

```sh
cargo tree -i bevy_debugger_bevy                      # error: did not match any packages
cargo tree -i bevy_debugger_bevy --features debugger  # matches, and so does bevy_remote
```

**`bevy_debugger/screenshot`** — captures the primary window, optionally crops to a region, optionally scales, and writes a PNG.

```sh
curl -s -X POST http://127.0.0.1:15702 -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"bevy_debugger/screenshot","params":{"path":"shot.png","region":{"x":760,"y":400,"width":900,"height":560},"zoom":0.5}}'
```

Capture is asynchronous: the method replies *before* the image exists, so it always writes to a path and reports that path back rather than returning bytes. Read the file once it appears.

**`bevy_debugger/input`** — headless keyboard and mouse injection, writing straight into Bevy's input resources without touching the OS input stack. This is the one that matters here: the project forbids driving a real keyboard or display, and this is the sanctioned version of exactly that.

### `DebuggerPlugin` owns `RemotePlugin` — do not add a second one

`DebuggerPlugin::build` adds `RemotePlugin::default().with_method_main(..)` to register its two methods. Bevy rejects a duplicate plugin by name, so adding `RemotePlugin` alongside it panics the moment the feature is switched on. `src/lib.rs` therefore adds `DebuggerPlugin` plus **only** the HTTP transport.

### Capture is offscreen, and never touches your desktop

`bevy_debugger/screenshot` captures an `Image` a camera renders to — not the window. That distinction is the whole point: reading the window surface only works while the window is actually on screen, so it needs the window raised, which steals focus and can switch Spaces. Measured here with one variable changed: **7,188 distinct colours** with the game focused, **1** with another app in front.

`src/debug_capture.rs` owns the mirror camera and its target image, spawned from `camera::setup` with the *same* environment map, exposure and bloom the player's camera gets — parity by construction, because the first version guessed at the lighting and rendered the squad against black. `bevy_debugger_bevy` never captures the window and has no fallback that would; without the `DebugCaptureTarget` resource it fails loudly.

Verified writing 11,721-colour frames of live gameplay with a different application frontmost throughout. No `osascript`, no raise, no `screencapture(1)`.

Two things to know:

- **The UI layer is not in the capture.** Bevy draws UI through the window camera, so a mirror 3D camera does not receive the HUD or menus. The 3D scene is faithful; the interface is not there.
- **The scene renders twice per frame** while the feature is on. That is the cost of never touching the window, and it is why this lives behind an opt-in feature.

`bevy_devshot` remains the path for a capture that must include the UI — it reads the window, so it needs the window visible.

### Input goes to the game, never through the OS

`bevy_debugger/input` writes into `ButtonInput<KeyCode>` and `ButtonInput<MouseButton>` — resources inside the game process. It cannot leak into whatever window you are actually using, and this is structural rather than careful: the crate depends only on `bevy`, `bevy_remote`, `serde`, `serde_json` and `image`, none of which can synthesise an OS event, and its sources contain no `enigo`, `CGEvent`, `core-graphics`, `xdotool`, `SendInput`, `winit` or `unsafe`. Keys injected while another application held focus produced nothing in that application.

`key` and `button` deserialize straight into Bevy's `KeyCode`/`MouseButton`, so every key Bevy knows is accepted under Bevy's own variant spelling — `KeyW`, `ArrowLeft`, `F11`, `Numpad7`, `ShiftLeft`. (An earlier note here described a hand-written twelve-key table and a scroll stub that wrote nothing; both are gone.)

### Injected input is queued, and lands in `PreUpdate` — this was a real bug

**BRP handlers registered with `with_method_main` run in the `Last` schedule.** Bevy's `keyboard_input_system` runs at the top of `PreUpdate` and clears the just-pressed and just-released sets.

So a handler that wrote straight into `ButtonInput` had its `just_pressed` flag wiped by the very next `PreUpdate`, *before* any `Update` system could read it. **Every `just_pressed`-based action was unreachable by injected input**, while the method cheerfully answered `success: true`. Held `pressed` state survived the clear, which is what made it look intermittent — some actions responded and some silently did not.

Measured against this game: `bevy_debugger/input` reported success for `KeyQ` (`Action::CameraRotateLeft`) and the camera's rotation quaternion was bit-identical before and after, across `Press`, hold, and `Tap`.

The handler now validates and **queues**; `input::apply_pending_input` runs in `PreUpdate` `.after(bevy::input::InputSystems)` and does the write on the far side of the clear. A `Tap` presses on one frame and releases on the next, because pressing and releasing within a single frame produces a state no physical key can — `pressed` false all frame while both edges are true — which silently skips every `pressed`-based reader. Verified after the fix: the same `Tap` of `KeyQ` rotated the camera 90°.

If you change that ordering, re-verify against a running game. A unit test on `ButtonInput` cannot see this class of bug, because the bug is entirely in *when* the write happens.

## Hacking on the debugger

It is in this repo — `crates/bevy_debugger_mcp/` — so edit it and rebuild. There is no pin to bump and no sibling checkout to keep in sync; both were deleted along with `scripts/sync_debugger.sh` when the crate was vendored in. Changes flow **monorepo → mirror**, like every other crate: `scripts/mirror_crates.sh` pushes `crates/bevy_debugger_mcp/` out to `Ladvien/bevy_debugger_mcp` with `git subtree split`, and nothing is ever pulled back.

`crates/bevy_debugger_mcp/CLAUDE.md` carries the crate's own non-negotiables. The one that matters most: **nothing in `bevy_debugger_bevy` may be able to touch the OS**, enforced by `crates/bevy_debugger_mcp/crates/bevy_debugger_bevy/tests/leaf.rs`, which pins the five-crate dependency list and fails on any source mentioning `enigo`, `xdotool`, `CGEvent`, `SendInput`, `winit` or `unsafe`.

## Upstream notes

`bevy_debugger_mcp` moved from Bevy 0.16 to 0.19 in PR #3. That upgrade replaced the WebSocket transport with HTTP JSON-RPC and renamed every BRP method (`bevy/query` → `world.query`, `bevy/get` → `world.get_components`, and so on).

The repo was vendored in at `f428b85`, which is upstream `main`. It was **relicensed GPL-3.0 → MIT OR Apache-2.0** on the way in, matching the other `bevy_*` crates here: a GPL crate in the Bevy ecosystem is unadoptable, and adoption is the reason these are mirrored out at all. An older note in this tree claiming ~43 errors in `src/observability/` is stale — the server builds clean.
