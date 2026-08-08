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

### `bevy_devshot` is still the screenshot path this project uses

BRP capture now works — it was verified writing real frames, cropping to the pixel, and scaling — but `bevy_devshot` needs no running MCP server, no HTTP port, and no feature flag, and the standing rule is one path per job. BRP screenshots are for an agent already talking to a live game over BRP; the sentinel file is for everything else.

**Either way the window must be frontmost.** A capture taken while another application holds focus comes back a single flat colour, through *both* paths — it is the window surface, not the handler. Raise the game by its unix id and verify the raise stuck before capturing, because the raise can silently lose to whatever steals focus next:

```sh
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $PID) to true"
osascript -e 'tell application "System Events" to get name of first process whose frontmost is true'   # must say foundation_vs_slop
```

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

`crates/bevy_debugger_bevy` did not compile against that `main`, and its screenshot handler was a skeleton that parsed `region` and `zoom` and then discarded the captured image. Both are fixed in `61ed2cb`, which is the rev pinned here — see that commit for the reasoning. The MCP server binary itself builds clean; an earlier note in this tree claiming ~43 errors in `src/observability/` is stale.
