# bevy_devshot

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

A frame on demand. `touch screenshot.request` and the next frame writes `screenshot.png`, rendered straight from the GPU.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. Issues and PRs belong upstream.

## Why a file and not a key binding

Because the thing that wants the screenshot usually isn't a person at the keyboard. A sentinel file works over SSH, from a Makefile, inside CI, from a capture script, and from an agent driving your game in a window it does not own. No OS screen-capture, no accessibility permission prompt, no compositor.

```rust
#[cfg(debug_assertions)]
app.add_plugins(bevy_devshot::DevShotPlugin);
```

```sh
touch screenshot.request     # -> screenshot.png in the working directory
```

That's the whole API. The plugin polls one `Path::exists` per frame and does nothing else.

## Gate it

Register it behind `#[cfg(debug_assertions)]`. A shipped game has no reason to watch the filesystem for a screenshot request.

## The `png` feature is load-bearing at runtime

This crate takes `bevy` with `default-features = false` and four features, one of which is `png`. Bevy's `save_to_disk` picks its encoder from the file extension via the `image` crate — so dropping `png` compiles perfectly and then fails with an unsupported-format error at the exact moment you wanted a screenshot. It is listed explicitly for that reason.

## Examples

```sh
cargo run -p bevy_devshot --example capture
# then, from another terminal:
touch screenshot.request
```

Opens a window with a spinning cube and writes `screenshot.png` the next frame after the sentinel file appears. This is the only example here that needs a GPU.

## License

MIT OR Apache-2.0, at your option.
