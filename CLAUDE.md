- Use home still and lookup research on whatever you are implementing BEFORE you implement it.  We want SOTA and best practices.
- Do not use unwrap() or anything that'd lead to a panic.  Code safe.  Handle errors.
- Leave academic paper references in comments, if a paper was used in writing the code.
- Rember compilation cost time; try to bunch changes and use `cargo check` to spot issues
- Add under a ## Testing section (create one if absent).
- This is a Rust game project (ECS-based). Always run the full test suite (including determinism and headless behavioral tests) after modifying gameplay/simulation code, and verify determinism before shipping.
- Do NOT assume design decisions on my behalf. When a design or scope choice is ambiguous (colors, coverage %, approach), stop and ask before implementing. Prefer focused/concrete changes over global post-process filters or over-engineered solutions.
- When investigating whether an issue is fixed, actually inspect the underlying data/code first before offering explanations; do not assume a file is broken or blame viewport/version.
- Ensure every feature added is correctly included in the RL/QD systems for evolving.

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

## Taking screenshots (do NOT use the macOS `screencapture` tool)

The game screenshots **itself** from inside the render pipeline via the `devshot` dev module
(`src/devshot.rs`) — no macOS screen-recording permission, no window/Space juggling. To grab a frame
while the game is running (working dir = project root):

```bash
touch screenshot.request      # sentinel; devshot consumes it next frame
sleep 1.5                      # give it a frame or two to render + write
# then Read screenshot.png
```

- Output is `screenshot.png` in the project root (gitignored, overwritten each time).
- Mechanism: `Screenshot::primary_window()` + `save_to_disk` (bevy 0.19), triggered by the sentinel file so it can be driven headlessly from the shell.
- **Caveat:** a *fully hidden/occluded* macOS window releases its Metal drawable, so a capture then comes back **black** (~57 KB PNG). A real frame is >150 KB. If you get black, the game window is hidden — retry once it's visible (even unfocused is fine; `WinitSettings` renders continuously).
- Keystroke injection into the window is blocked this environment, so to verify input-driven behavior (movement, selection, fog reveal) drive it with a **temporary** auto-input/self-test system, screenshot, then revert the temp code.
- `devshot` is dev-only; strip the module + its plugin registration for release builds.
