# Agent debugging over BRP — `bevy_debugger_mcp`

[`Ladvien/bevy_debugger_mcp`](https://github.com/Ladvien/bevy_debugger_mcp) is our MCP server that lets an
agent inspect a **running** instance of this game: query entities, components and resources live, instead
of reasoning about them from source.

It is a **separate process**. Nothing in this workspace links it, and the game gains no HTTP client. The
shape is:

```
agent  ──MCP──▶  bevy-debugger-mcp  ──BRP (JSON-RPC/HTTP :15702)──▶  the running game
```

## Why it is a Cargo feature, not a debug_assertions gate

`devshot` and `region_capture` are `#[cfg(debug_assertions)]`. BRP is not, and the difference is
deliberate: BRP is not only an observation channel, it can **mutate a live `World`**. It has no business in
a shipped binary, and it must never be on during a determinism run — an external writer into pinned state
is precisely what the goldens exist to catch.

A Cargo feature is the only gate that removes it from the *resolved dependency graph* rather than merely
from the plugin list. Measured with the feature off: `bevy`'s `bevy_remote` feature is not enabled, and the
resolved `bevy_math`/`glam` feature sets are byte-identical to before the feature existed. `Cargo.lock`
records nine extra packages (`bevy_remote`, `hyper`, `tokio`, …) because a lockfile records every possible
resolution — none of them build unless you ask for them.

## Running it

```sh
# 1. The game, with the remote protocol on (default port 15702).
cargo run --features debugger

# 2. The MCP server, once, from its own checkout or an install.
cargo install --git https://github.com/Ladvien/bevy_debugger_mcp   # or: brew, or ./install.sh
./setup-claude.sh                                                  # registers it with Claude Code
```

Then ask the agent to look at the running game. The observation tools ride Bevy's **built-in** BRP methods
(`world.query`, `world.get_components`, `world.list_components`, `registry.schema`, …), so they work with
nothing in this repo beyond the two plugins.

## What is NOT wired up yet, and why

The debugger ships a companion plugin, `crates/bevy_debugger_bevy`, which adds custom BRP methods for
**screenshots** and **headless input injection**. That second one is genuinely wanted here — this project
has a standing rule against driving a real keyboard or display, and BRP input injection is the sanctioned
version of exactly that.

It is **not adopted yet because it does not compile.** Measured against `main` (`a315d61`):

```
screenshot.rs:43  `?` couldn't convert the error to `BrpError`
screenshot.rs:61  expected `On<'_, '_, ScreenshotCaptured>`, found `&On<...>`
input.rs:62       `?` couldn't convert the error to `BrpError`
```

Its screenshot handler is also a skeleton — `screenshot.rs:58` is `// TODO: crop to region, scale by zoom`
and the captured image is discarded on the next line. So **`bevy_devshot` remains the one screenshot
path**; swapping to BRP capture would retire a working path for one that writes no file.

When those are fixed upstream, adopt it as a git dependency pinned to a rev:

```toml
bevy_debugger_bevy = { git = "https://github.com/Ladvien/bevy_debugger_mcp", rev = "<sha>" }
```

A pinned git dep rather than a vendored copy, for the reason `emerge-bevy`'s header gives about
`spawn_descriptor`: a second copy is a second source of truth, and the two drift.

## Hacking on the debugger locally

Clone it as a sibling and redirect the dependency with a `[patch]` in the **gitignored**
`.cargo/config.toml` — the same file that carries the `cargo fvs` alias, so this stays machine-local and
CI and fresh clones keep the reproducible pinned rev:

```toml
# .cargo/config.toml  (gitignored)
[patch."https://github.com/Ladvien/bevy_debugger_mcp"]
bevy_debugger_bevy = { path = "../bevy_debugger_mcp/crates/bevy_debugger_bevy" }
```

Edits in the sibling checkout are picked up on the next `cargo build`, and they commit to the debugger's
own repo where they belong.

## Upstream notes

`bevy_debugger_mcp` moved from Bevy 0.16 to 0.19 in PR #3, **already merged to `main`**. That upgrade
replaced the WebSocket transport with HTTP JSON-RPC and renamed every BRP method (`bevy/query` →
`world.query`, `bevy/get` → `world.get_components`, and so on). Two areas remain broken upstream and are
unrelated to us: `src/observability/` (~43 errors, an opentelemetry/prometheus version mismatch) and much
of its test suite.
