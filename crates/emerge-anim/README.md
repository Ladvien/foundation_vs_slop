# emerge-anim

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

Clip-weight blending on a skinned rig's `AnimationPlayer`. Bevy 0.19.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. It depends on sibling crates by workspace path, so it builds *inside* that workspace, not on its own. Issues and PRs belong upstream.

## The one idea: no transitions

Every clip in a rig's blend set stays resident on the `AnimationPlayer` forever and is never restarted. Each frame only two things move: the eased clip **weights**, and one shared normalised **gait phase**.

That is what keeps feet planted through a walk→run crossover — a transition would restart a clip and the contact would slide. It also means you must never add `AnimationTransitions` to anything this drives: its `PostUpdate` pass would stomp the weights.

## Cosmetic by construction

This layer runs on `Update`, never `FixedUpdate`, and touches no simulation state. In the parent project that is load-bearing: it is invisible to the snapshot hash by design.

## Examples

```sh
cargo run -p emerge-anim --example blend_weights
```

Prints the whole locomotion weight vector across the speed × heading domain — no engine, no rig, no GLB. Watch idle → walk → run hand off while the weights sum to exactly 1.000 the entire way, which is the crate's one idea: nothing is ever started or stopped, only eased.

## License

GPL-3.0-only, with the game it was carved out of.
