# Agent debugging over BRP — `bevy_debugger_mcp`

[`Ladvien/bevy_debugger_mcp`](https://github.com/Ladvien/bevy_debugger_mcp) is our MCP server that lets an agent inspect a **running** instance of this game: query entities, components and resources live, instead of reasoning about them from source.

It is a **separate process**. Nothing in this workspace links it, and the game gains no HTTP client. The shape is:

```
agent  ──MCP──▶  bevy-debugger-mcp  ──BRP (JSON-RPC/HTTP :15702)──▶  the running game
```

## Why it is a Cargo feature, not a debug_assertions gate

`devshot` and `region_capture` are `#[cfg(debug_assertions)]`. BRP is not, and the difference is deliberate: BRP is not only an observation channel, it can **mutate a live `World`**. It has no business in a shipped binary, and it must never be on during a determinism run — an external writer into pinned state is precisely what the goldens exist to catch.

A Cargo feature is the only gate that removes it from the *resolved dependency graph* rather than merely from the plugin list. Measured with the feature off: `bevy`'s `bevy_remote` feature is not enabled, and the resolved `bevy_math`/`glam` feature sets are byte-identical to before the feature existed. `Cargo.lock` records nine extra packages (`bevy_remote`, `hyper`, `tokio`, …) because a lockfile records every possible resolution — none of them build unless you ask for them.

## Running it

```sh
# 1. The game, with the remote protocol on (default port 15702).
cargo run --features debugger

# 2. The MCP server, once, from its own checkout or an install.
cargo install --git https://github.com/Ladvien/bevy_debugger_mcp   # or: brew, or ./install.sh
./setup-claude.sh                                                  # registers it with Claude Code
```

Then ask the agent to look at the running game. The observation tools ride Bevy's **built-in** BRP methods (`world.query`, `world.get_components`, `world.list_components`, `registry.schema`, …), so they work with nothing in this repo beyond the two plugins.

## The companion plugin, and what it adds

The debugger ships `crates/bevy_debugger_bevy`, which registers two **custom** BRP methods on top of Bevy's built-in ones. It is adopted here as a git dependency pinned to a rev, behind the same `debugger` feature, so it is absent from every default, release and determinism build. Confirm that with `cargo tree -i bevy_debugger_bevy`, which fails to match a package unless the feature is on.

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

`key` and `button` deserialize straight into `KeyCode` and `MouseButton`, so **every variant Bevy knows works by name** — `KeyW`, `Space`, `ArrowLeft`, `F5`, `ShiftLeft`, `Numpad7`. There is no hand-maintained key table to fall behind the engine. `InputKind::Scroll` writes a real `MouseWheel` message (`unit` is `"Line"` or `"Pixel"`) and fails loudly when there is no primary window to address it to.

## Hacking on the debugger locally

Clone it as a sibling and redirect the dependency with a `[patch]` in the **gitignored** `.cargo/config.toml` — the same file that carries the `cargo fvs` alias, so this stays machine-local and CI and fresh clones keep the reproducible pinned rev:

```toml
# .cargo/config.toml  (gitignored)
[patch."https://github.com/Ladvien/bevy_debugger_mcp"]
bevy_debugger_bevy = { path = "../bevy_debugger_mcp/crates/bevy_debugger_bevy" }
```

Edits in the sibling checkout are picked up on the next `cargo build`, and they commit to the debugger's own repo where they belong.

## Upstream notes

`bevy_debugger_mcp` moved from Bevy 0.16 to 0.19 in PR #3, already merged to its `main`. That upgrade replaced the WebSocket transport with HTTP JSON-RPC and renamed every BRP method (`bevy/query` → `world.query`, `bevy/get` → `world.get_components`, and so on).

`crates/bevy_debugger_bevy` did not compile against that `main`, and its screenshot handler was a skeleton that parsed `region` and `zoom` and then discarded the captured image. Both are fixed in `61ed2cb` — see that commit for the reasoning. The pin has since moved forward to **`f3c2347`**, which adds the offscreen capture and the full key coverage described above; `scripts/sync_debugger.sh` reports the pin against upstream. The MCP server binary itself builds clean; an earlier note in this tree claiming ~43 errors in `src/observability/` is stale.

### The MCP server's own tools do not load in Claude Code

The two BRP methods above are reached by POSTing to `:15702` and need no MCP server at all — that is the sanctioned agent path and it works. The **separate** convenience surface, the 13 MCP tools (`observe`, `experiment`, `hypothesis`, `detect_anomaly`, `stress_test`, `time_travel_replay`, plus user/security management), is currently rejected by the client:

```
tools fetch failed — Invalid input: expected "object" (at tools.0.inputSchema.type)
```

Every tool in `src/secure_mcp_tools.rs` takes `Parameters(req): Parameters<Value>`, and `schemars` renders `serde_json::Value` as `{"$schema": …, "title": "AnyValue"}` — a schema with no `type` and no `properties`. MCP requires `inputSchema` to be an object schema. Fixing it means giving those tools real parameter structs upstream; until then, use BRP directly.
