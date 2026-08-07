# emerge-anim

Clip-weight blending on a skinned rig's `AnimationPlayer`. Bevy 0.19.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. It depends on sibling crates by workspace path, so it builds *inside* that workspace, not on its own. Issues and PRs belong upstream.

## The one idea: no transitions

Every clip in a rig's blend set stays resident on the `AnimationPlayer` forever and is never restarted.
Each frame only two things move: the eased clip **weights**, and one shared normalised **gait phase**.

That is what keeps feet planted through a walk→run crossover — a transition would restart a clip and
the contact would slide. It also means you must never add `AnimationTransitions` to anything this
drives: its `PostUpdate` pass would stomp the weights.

## Cosmetic by construction

This layer runs on `Update`, never `FixedUpdate`, and touches no simulation state. In the parent
project that is load-bearing: it is invisible to the snapshot hash by design.

## License

GPL-3.0-only, with the game it was carved out of.
