# Setting up this build environment

Everything needed to clone, build, run and test this project on a new machine — plus the agent-debugging setup, which is optional but is what makes an AI assistant useful here.

Assets are committed (111 MB, no Git LFS), so a clone is self-contained. The `/mnt/codex_fs` paths in some source comments point at the *authoring* library for 3D work; nothing in the build reads them.

## 1. Prerequisites

**Rust, recent stable.** The workspace is edition 2024 (needs ≥ 1.85) and the agent debugger declares `rust-version = "1.95"`, so in practice: current stable. Known good here: `rustc 1.96.1`. There is no `rust-toolchain.toml`, deliberately — nothing pins you to a version.

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
```

**Platform libraries for Bevy.**

- **macOS** — Xcode Command Line Tools: `xcode-select --install`. Nothing else.
- **Linux** — the same set CI installs, needed even for the headless suite because the binary target still links them:

  ```sh
  # Debian / Ubuntu, which is what CI runs
  sudo apt-get install -y --no-install-recommends \
    libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev

  # Arch / CachyOS — the same libraries, and headers ship in the same packages
  sudo pacman -S --needed alsa-lib systemd-libs wayland libxkbcommon vulkan-icd-loader
  ```

**Disk — budget 60 GB.** `target/` reached 58 GB during heavy work on this machine and has filled a 460 GB volume twice. When it bites, `rm -rf target/debug/incremental` reclaims tens of GB (10 GB in one recent case) without throwing away the expensive Bevy dependency build; `cargo clean -p foundation_vs_slop` is the next step up.

**A GPU** for the windowed game, the editor, and the headless replay/SSIM harness. The `cargo test --workspace` layer is pure CPU and needs none.

Optional: **`gh`** (GitHub CLI) if you will push the extracted crates to their mirrors, and **`jq`** if you want the agent-debugging hook to work.

## 2. Clone and build

```sh
git clone git@github.com:Ladvien/foundation_vs_slop.git
cd foundation_vs_slop
cargo build            # first build compiles all of Bevy — expect 5–15 minutes
```

## 3. `.cargo/config.toml` — the one file that is not in the repo

**This is the step people miss, and it fails in a way that looks like a bug in the tests.**

`.cargo/config.toml` is gitignored on purpose: a hardcoded machine-specific `build.target-dir` in it once broke CI on every commit. So each machine keeps its own, and you have to write it once:

```toml
# .cargo/config.toml — machine-local, never committed

[alias]
fvs = "run --quiet -p fvs --"

# REQUIRED for the headless harness. It pins Bevy's IO task pool to one thread (that is what makes
# system order deterministic) and builds ~35 `App`s in one process, so every asset load funnels
# through a single stack. The default 2 MiB overflows partway through, reproducibly, at the same
# test. CI sets this in `env:`; a local shell does not.
[env]
RUST_MIN_STACK = "33554432"
```

Without the `[env]` block, `cargo test --features test-harness` aborts with `thread 'IO Task Pool (0)' has overflowed its stack` and a `SIGABRT`. It reads like a real determinism failure and is not.

## 4. Running things

`cargo fvs` is the dispatcher, available once the alias above is in place. Without it, spell it `cargo run -p fvs --`.

```sh
cargo fvs play                      # the game
cargo fvs play --map break_room     # a specific authored map
cargo fvs edit break_room           # the standalone world editor (emerge-mapper)
cargo fvs train behavior --generations 2
cargo fvs --help                    # everything it knows
```

Plain `cargo run` also launches the game — `default-run` points at it, since two binaries exist.

**Running a built binary directly needs `BEVY_ASSET_ROOT`.** Bevy resolves relative asset paths against the *executable's* directory, not your shell's, so `./target/debug/foundation_vs_slop` finds nothing on its own:

```sh
BEVY_ASSET_ROOT="$PWD" ./target/debug/foundation_vs_slop
```

`cargo run` does not need this.

## 5. Tests — two layers

```sh
cargo test --workspace                              # deterministic core: fast, GPU-free, the hard gate
cargo test --features test-harness -- --test-threads=1   # headless replay / liveness / SSIM; needs a GPU
```

`--workspace` is load-bearing: this workspace has a root package, so a bare `cargo test` compiles no test target under `crates/` at all.

Expect **1621 passed / 0 failed** on the first layer.

The harness layer has three tests CI skips deliberately, each guarded by a tripwire in `tests/skip_debt.rs` that fails when the underlying reason stops being true. To run it exactly as CI does:

```sh
cargo test --features test-harness --no-fail-fast -- --test-threads=1 \
  --skip parallel_search_reproduces_the_inline_archives_bit_for_bit \
  --skip batch_emitter_scales_past_opponents_deterministically \
  --skip search_rollouts_are_reproducible_under_load \
  --skip search_rollouts_of_mutants_are_reproducible_under_load \
  --skip watching_the_feed_makes_it_generate_and_ignoring_it_stops \
  --skip shipped_level_playtests_and_is_deterministic \
  --skip a_candidate_genome_actually_changes_the_simulation
```

That gives **1160 passed / 0 failed / 7 ignored** across 38 binaries. `TESTING.md` explains the strategy and the determinism rules; read it before adding tests.

**Goldens are per-platform.** Determinism is pinned on `ubuntu-latest` x86_64. Some field-hash tests have no golden pinned for aarch64 yet and fail *in isolation* on Apple Silicon for that reason alone — see each test's own message.

## 6. Agent debugging over BRP (optional, but it is the sanctioned way to drive the game)

This project forbids agents from taking your keyboard or screen. The replacement is [`bevy_debugger_mcp`](https://github.com/Ladvien/bevy_debugger_mcp) — a separate process speaking MCP to an agent and BRP to the running game, so screenshots come from an offscreen render target and keystrokes go straight into the game's own input resources. Neither touches the OS.

The debugger lives **in this repo** at `crates/bevy_debugger_mcp/`, so there is nothing else to clone.

```sh
# 1. The MCP server, built from the copy in this workspace.
cargo install --path crates/bevy_debugger_mcp --locked   # ~6 minutes, release build

# 2. Register it with Claude Code. NOTE: the flag is `--stdio`, not `stdio` —
#    the vendored setup-claude.sh prints the wrong form.
claude mcp add bevy-debugger --scope user \
  -e BEVY_BRP_HOST=127.0.0.1 -e BEVY_BRP_PORT=15702 -e BEVY_MCP_DEV_PASSWORD=<pick-one> \
  -- ~/.cargo/bin/bevy-debugger-mcp --stdio

# 3. Run the game with the protocol on.
cargo run --features debugger
```

Its tools require a JWT; `BEVY_MCP_DEV_PASSWORD` seeds the development users with a password you choose, because otherwise they are random per start and only ever written to stderr, which the client cannot read. **Restart Claude Code** after registering — MCP tools are not available until it reconnects.

Verify without any MCP client at all:

```sh
curl -s -X POST http://127.0.0.1:15702 -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"rpc.discover"}' | jq '.result.methods | length'   # 25
```

**To edit the debugger** rather than just use it, edit it — it is an ordinary crate in this workspace. `cargo build --features debugger` picks up plugin changes; `cargo install --path crates/bevy_debugger_mcp` reinstalls the server. It was a pinned git dependency until the pin turned a one-line bug into a cross-repo errand, and `docs/bevy_debugger_mcp.md` records both the bug and why the pin went away.

## 7. Pushing the extracted crates (only if you maintain them)

Eleven crates under `crates/` mirror to their own private `Ladvien/*` repos via `git subtree split`. `scripts/mirror_crates.sh` re-syncs them; it needs `gh` authenticated and a clean working tree, and refuses any crate missing a README, `CLAUDE.md`, a license, `examples/*.rs`, or the "Vibe Coded" label. Changes flow monorepo → mirror only.

`bevy_debugger_mcp` is the one that arrived by the opposite route — `git subtree add`, history intact — and its nested `crates/bevy_debugger_bevy` travels with it as part of the same mirror.

## 8. Things that will bite you

| Symptom | Cause |
|---|---|
| `IO Task Pool (0) has overflowed its stack`, SIGABRT | `RUST_MIN_STACK` not set — §3 |
| Meshes 404 when running a built binary | `BEVY_ASSET_ROOT` not set — §4 |
| `cargo test` passes but crate tests never ran | Missing `--workspace` — §5 |
| A screenshot comes back one flat colour | The window is not on screen. Use the BRP offscreen capture, which does not care — §6 |
| Disk full mid-build | `rm -rf target/debug/incremental` — §1 |
| Bevy API doesn't match the docs you found | bevy.org documents `main`; this is pinned to 0.19.0. Read the vendored source under `~/.cargo/registry/src/*/bevy-0.19.0/` — see `CLAUDE.md` |
| MCP tools missing after `claude mcp add` | Claude Code needs a restart — §6 |

`CLAUDE.md` is the working agreement for changes to this repo; `TESTING.md` covers the test system; `docs/bevy_debugger_mcp.md` covers the debugger in depth.
