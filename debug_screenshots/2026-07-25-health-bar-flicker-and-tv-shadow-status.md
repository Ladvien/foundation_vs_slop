# 2026-07-25 — health bar flicker (fixed) and TV shadow artifacts (mitigated, unconfirmed)

Status note for three player region-captures (`region_2026-07-25_16-52-37-220`, `_16-53-25-103`,
`_16-53-51-436`). **Not deleted yet** — per this directory's `CLAUDE.md`, captures get removed once a
fix is confirmed; the shadow half of this isn't confirmed live, so the raw captures stay until you can
re-test and say either way.

---

## 1. "Flickering wall sconce shadows on the floor and all of the squad members' health bars" — the
##    health-bar half is fixed and confirmed

**Cause.** `HealthBarMaterial` is `AlphaMode::Blend` with no `depth_bias` override, and every bar
floats at the identical fixed height (`BAR_Y = 2.0`) above its owner. A tightly clustered squad — every
one of these three captures shows one — puts several bars' AABB centres within millimetres of each
other and the camera, so their sort keys tie. Bevy then resolves the tie by ECS extraction order, which
isn't stable frame to frame (this project's own rule: "ECS query order decides nothing"). Same bug
class already fixed for `BloodPoolMaterial` decals in `gore.rs`.

**Fix.** Added a stable per-bar `depth_bias` tiebreak (`impl Material for HealthBarMaterial`), keyed on
each owner's spawn position (hashed into the otherwise-unused `_pad0` uniform slot, not read by the
shader). Confirmed visually: a live devshot of a 5-unit squad cluster near a TV shows all five bars
rendering cleanly stacked with no overlap/z-fight artifacts.

**Files:** `src/health.rs`.

---

## 2. "Flickering squares under the squad" / "Wtf are these shadow artifacts?" — mitigated, not
##    confirmed

**Investigation.** Ruled out, with source-level certainty: Bevy shadow maps (every light except one
explicitly sets `shadow_maps_enabled: false`) and Bevy contact shadows (`ContactShadows` is attached to
the camera, but every light's `contact_shadows_enabled` — a separate per-light flag — defaults `false`
and is never set `true` anywhere in this project, so the raymarch code path is structurally dead; the
earlier same-day fix that bumped its step count was very likely a no-op misdiagnosis). Also ruled out:
`autogib` (only bakes to a cache resource, never spawns visible geometry outside an actual death event;
these captures all show `units=5` alive), the VHS post-process (correctly pause-gated, not
geometry-anchored), `gore.rs` decals (all death-triggered, all maroon not black), `psi_vision.rs`
(always a saturated hue, never neutral/black).

**Strongest remaining lead:** the TV's `SpotLight` (`attach_screen_lights`, `src/light.rs`) is the
*only* shadow-casting light in the entire game — a deliberate exception, added because "a real TV
throws shadows of whatever stands in front of it" (player request). It has no shadow softening and its
intensity continuously flickers 11 Hz + a slow roll. Hard, unfiltered shadow edges pulsing through a
large intensity swing every frame is a strong theoretical match for "jagged," "elongated," "flickering"
— but I could not get a clean live on/off repro this session (kept landing on an unexplored area / the
title screen not cooperating with a scripted repro), so this is circumstantial, not confirmed.

**Mitigation applied** (keeps the player-requested feature, doesn't remove it):
- `shadow_normal_bias` on the TV spotlight bumped from Bevy's default (1.8) to 3.0 — the standard lever
  against self-shadowing "acne" reading as jagged noise, relevant here because a TV sits close to a
  wall in a small room and the squad can stand right up against it.
- `flicker_screens`' intensity swing narrowed from `[0.62, 1.0]` to `[0.80, 1.0]` — halves how hard the
  shadow's contrast against the lit floor pulses per frame, while keeping the "restless CRT" character.

**Files:** `src/light.rs`.

**What would confirm or rule this out:** next time this happens, Ctrl+P it *while standing near a TV*
specifically (the note field can just say "near TV" or "not near TV") — that's the one piece of
information these three captures didn't happen to pin down.
