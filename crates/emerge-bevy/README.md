# emerge-bevy

The runtime half of emerge: take an `emerge-core` library plus a map, and put it in a Bevy world.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. It depends on sibling crates by workspace path, so it builds *inside* that workspace, not on its own. Issues and PRs belong upstream.

## What it does

Resolves masks, stacked Y, role masks and seat positions once at load, then spawns entities.
`spawn_descriptor` is deliberately the **single** shared spawner, so a map cannot look one way in the
editor and another in the game — the fork that happens the moment two spawners exist.

This is the only crate in the emerge set that knows what a renderer is, on purpose.

## License

GPL-3.0-only, with the game it was carved out of.
