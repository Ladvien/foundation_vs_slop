# emerge-anim — notes for agents

Clip-weight blending on a skinned rig's `AnimationPlayer`. Bevy 0.19.

## Source of truth

The source of truth for this crate is [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) at `crates/emerge-anim/`. If you are reading this in a standalone `Ladvien/emerge-anim` checkout, that is a read-only `git subtree split` mirror — **changes made here cannot be pulled back**. Make them upstream.

## Build and test

**Not a leaf.** It path-depends on the sibling `emerge-core`, so it builds *inside* the workspace, not on its own: `cargo test -p emerge-anim`.

**This is the one manifest in the workspace that does not write `bevy.workspace = true`**, deliberately: it needs `default-features = false`, and Cargo *ignores* a member-level `default-features` when the dependency is inherited — so `{ workspace = true, default-features = false }` would silently keep the defaults on. The version is the workspace's; keep them in step by hand when bumping.

## The non-negotiable: no transitions

Every clip in a rig's blend set stays resident on the `AnimationPlayer` forever and is **never restarted**. Each frame only two things move: the eased clip **weights**, and one shared normalised **gait phase**.

That is what keeps feet planted through a walk→run crossover — a transition would restart a clip and the contact would slide. It follows that you must **never add `AnimationTransitions` to anything this drives**: its `PostUpdate` pass would stomp the weights.

**Cosmetic by construction.** This layer runs on `Update`, never `FixedUpdate`, and touches no simulation state. In the parent project that is load-bearing: it is invisible to the snapshot hash *by design*, and it is the deliberate exception to that repo's "wire every feature into the RL/QD systems" rule — a genome gene pointed here would never move the fitness. Cosmetic tuning belongs in constants, not in the evolving systems.

**Gait clips are authored in place**, with zero root motion and an honest per-cycle ground distance. The blender derives speed from that distance; a clip that translates its root will read as the wrong speed and skate.

**A held phase reaches the free clips too, and only by their speed.** `hold_phase` is the editor bench's scrub, and it used to pin only the gait slots — those are *paused* clips whose seek time this module owns, so φ freezes them. A `Playback::Free` slot is the deliberate opposite: Bevy's `advance_animations` ticks it and this module touches only its weight. So the bench's `Space` froze the walk and left the idle running, measured as ~150,000 pixels of change per two seconds with the phase held. While `held`, a free slot's speed goes to zero and returns to its authored value on release. **Speed, never a seek** — a free clip has no φ to be held at, and rewinding one is the thing this module exists to never do.

## Rules

- **Bevy 0.19 is pinned.** Read the vendored source (`~/.cargo/registry/src/index.crates.io-*/bevy-0.19.0/`, and its `examples/`), not bevy.org — that documents `main` and has been wrong for this pin more than once.
- **A missing `Res<T>` panics its system in 0.19**; it does not skip. Take `Option<Res<T>>`.
- **All run conditions are evaluated — there is no short-circuit.** A bare `Res<T>` in a `.run_if(..)` closure panics whenever that resource is absent, even behind an earlier condition that returned false.
- **No `unwrap()`.** A missing clip index or an unrigged GLB is an error to report; this crate's code already says `error!`, which is why it declares `bevy_log` itself.
- **One path per feature.** No fallbacks, no legacy shims, no stub placeholders.

## In the monorepo

The game reaches this as `crate::anim` (`src/lib.rs:23`); `emerge-mapper`'s Anim tab drives the **real** blender rather than a preview copy, on purpose. `docs/animation.md` is the engineering guide and `docs/artist_guide.md` §4 holds the per-asset clip tables and the authoring contract — neither those, nor the root `CLAUDE.md`, nor `TESTING.md`, is part of this mirror.
