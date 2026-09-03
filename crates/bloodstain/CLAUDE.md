# bloodstain — the non-negotiables

Read this before editing anything under `crates/bloodstain/`. This directory **is** the root of a
public mirror ([`Ladvien/bloodstain`](https://github.com/Ladvien/bloodstain)), split out of
`Ladvien/foundation_vs_slop` with `git subtree split`. Changes flow **one way**: monorepo → mirror.
Nothing is ever edited on the far side and nothing is pulled back.

## No math library. Ever.

The public API is `[f32; 3]`. The workspace this crate lives in resolves **two** `glam` versions at
once, so a leaf naming either could collide with a consumer naming the other — naming neither is the
only choice that cannot. `tests/leaf.rs` forbids `glam`, `nalgebra` and every `bevy*` crate by name,
and `crate::vec` mirrors `glam::Vec3` **operation for operation** because its output feeds a frozen
golden that was blessed against glam. If you add a vector helper, name the glam function it mirrors.

Conversion is the consumer's job and it has exactly one home: `bevy_carnage/src/v3.rs`. Per wound,
not per vertex.

## One math path: `libm`, unconditionally

Not `std`'s math behind a feature. A second math path is a second set of bits, and this crate's
product is a frozen model. This was **measured before it was adopted**: at the spatter golden's own
inputs, `libm::{sinf, cosf, sqrtf}` are bit-identical to the platform libm the model was blessed
against. `powf`/`expf` differ by one ULP at some inputs and are read only by code written here, never
by a moved golden. Every transcendental goes through `crate::m`.

## Four goldens are locks, not snapshots

`hash_f32_is_frozen`, `the_spatter_model_is_frozen`, `the_stain_placement_is_frozen`,
`the_pool_model_is_frozen`. If one goes red while the build profile is held fixed, **the model
moved** — that is the finding, not a table to update. Re-bless only for a profile change, and say
which profile in the doc comment (both spatter tables record the one re-blessing they have had).

`the_spatter_model_is_frozen` and `the_stain_placement_is_frozen` came out of `bevy_carnage::spatter`
**with their bits unchanged**. That is the evidence the extraction was a move rather than a rewrite,
and it is why a future refactor must not touch them.

## Ticks, not seconds. No clocks.

Every function that involves time takes `tick: u32` and `hz: u32`. Nothing reads a clock, virtual or
real. A float accumulator large enough stops advancing at all, which is a recorded failure in this
family of crates. Tick counts are quoted for 60 Hz; a caller on another rate re-derives them in data.

## Seeds are places, never histories

`wound_seed` and `patterns::site_seed` hash a position **quantised onto `WELD`**, mixed with an enum
discriminant. Never an accumulator, an entity id, an asset id or a clock: an arena slot is assigned by
load order, and a drain counter desynchronises permanently after any single difference.

**`WoundKind` and `PatternClass` are append-only.** Their discriminants are mixed into seeds and
travel in saved data. A new variant goes on the end with the next number; nothing is ever renumbered.

## No fallbacks, and no fabricated answers

A function that cannot answer returns `None`, `0`, or `false` — never a plausible-looking guess.
`landing` refuses a droplet that never crosses the plane; `area_of_origin` refuses an
underdetermined scene rather than averaging it into a point; `rasterise` refuses a wrong-sized buffer
rather than half-filling it; a zero normal gets a zero basis rather than "straight up".

No `unwrap`, no `expect`, no panicking index in library code. `#![forbid(unsafe_code)]`.

## Cessation has one predicate

`rheo::flows(driving, yield)`. A clot, a rivulet arresting on a wall, and a pool that stopped creeping
are **one mechanism at three ages**. There is deliberately no `clotted` boolean beside it and no
second `f <= 0.0` guard in `pulse_wound` — a second guard would be a second answer to the same
question.

## Tuned constants say so

`SPINE_WE_MIN`, `PERFUSION_STRESS_PA`, `hct_exponent`, `DRY_REF_TICKS` are tuned or compressed rather
than measured, and each says so in its own doc comment **and** in `docs/citations.md`. A tuned
constant that admits it is honest; one dressed as a measurement is not. When adding a constant: name
the paper, or name the fact that there isn't one.

## Furniture the mirror script enforces

`scripts/mirror_crates.sh` refuses to mirror a crate missing `README.md` (opening with the **Vibe
Coded** warning, then the mirror notice, then an `## Examples` section), `CLAUDE.md`, `Cargo.toml`, a
licence, or `examples/*.rs`. Both examples are **terminal only** — no window, no GPU — so they run
anywhere, and that is deliberate: a crate whose examples need a display cannot be judged over ssh.
