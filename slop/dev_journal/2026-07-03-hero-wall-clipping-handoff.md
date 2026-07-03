# Handoff: "hero clipping into walls" investigation (unresolved)

**Date:** 2026-07-03. See `2026-07-03-wfc-dungeon-onboarding.md` for full project onboarding.

## The complaint

In the isometric view the hero (Kenney `figurine`, blue) **looks like it clips into / is
embedded in walls** when standing at wall corners. User has raised this ~4 times and is
frustrated by fixes that don't address the true root cause. User screenshots consistently
show the hero at an **inside corner**: part of its body occluded by a wall, and its arm
appearing to hang over an adjacent black void.

## ⚠️ FIRST: the working tree has TEMP DIAGNOSTIC CODE — revert before anything else

- `src/player.rs`:
  - `player_movement` currently **auto-wanders at 100× speed** (8-way pseudo-random
    direction) instead of reading WASD. Restore the real input block (WASD →
    `SCREEN_FORWARD`/`SCREEN_RIGHT`, `dt = time.delta_secs().min(MAX_FRAME_DT)`, no
    `stress_speed`).
  - `stress_probe` system + its registration in `PlayerPlugin` (`(player_movement,
    stress_probe).chain()`) must be removed → back to `.add_systems(Update, player_movement)`.
- `src/dungeon.rs`: `is_solid` was made `pub` for the probe — revert to private `fn`.
- `WALL_HEIGHT` is **1.0** (correct — user wants tall walls; do NOT lower it).
- `camera.rs` `VIEWPORT_HEIGHT` is 12.0 (normal). No temp zoom left.

The game currently is NOT playable (auto-wanders); reverting the above restores it.

## What is PROVEN (evidence, not assumption)

Used the home-still fault-localization method (instrument → trace → systematically
eliminate). Instrumented the hero's 4 collision-box corners (`±half.x, ±half.y` around its
position) against `Dungeon::is_solid` every frame:

- **Smooth wander:** 0 frames with any corner inside a wall; `on_floor=true` always.
- **100× stress test** (8-way wall-ramming, ~1200 frames): only **6** flagged frames, all at
  ONE flush-against-wall position `pos=(15.05,39.16)`, `corners_solid=[true,false,true,false]`.
  The west edge = 15.05 − 0.35 = 14.70 = the west wall's inner face exactly; float rounds it
  to 14.6999 so `is_solid` tips true by ~**1e-5**. i.e. the hero is stopped **flush** against
  the wall — collision is correct; the "clip" is a floating-point boundary artifact, invisible.

**Conclusion: the collision is correct. The hero's footprint is never meaningfully inside a
wall.** The visible problem is therefore in RENDERING, plus the flush-contact:

1. **Isometric occlusion** — walls are 1.0 tall, hero ~0.98 (figurine 0.7 × scale 1.4). A
   wall between the camera and the hero (or just taller behind it) correctly depth-occludes
   it → looks embedded. This is the dominant cause.
2. **Flush contact + arm projection** — collision box half-extents `(0.35, 0.14)` exactly
   match the figurine footprint (arms span ±0.25 × scale). So the arm tip stops *exactly* on
   the wall face (0 gap); at arm height + iso projection it visually overlaps the wall / hangs
   over adjacent void.

## What the user has REJECTED (do not repeat)

- **Depth-bias "render hero on top"** (set figurine materials' `depth_bias` high) — rejected;
  makes the hero show through walls it's genuinely behind.
- **Shorter walls** (`WALL_HEIGHT` 0.6) — rejected ("stupid"); walls must stay tall (1.0) for
  the Backrooms look.

## Recommended next steps

The user wants **tall walls kept** AND the hero to not look clipped, with a real fix. Options:

1. **Wall cutaway / fade (recommended)** — each frame, fade or hide the wall tiles that sit
   between the camera and the hero (the standard iso solution). Complication: walls currently
   share one `wall_mat` and their `Visibility` is owned by the fog system (`fog.rs`), so this
   needs per-wall material handling or a combined visibility rule (`revealed && !occluding`).
   Walls carry `Tile { cell }`; the occluding set is the walls just "camera-side" of the hero.
2. **Small collision margin** — inflate the collision half-extents by ~0.08–0.1 so the hero
   stops a hair *before* walls (visual gap; also kills the 1e-5 epsilon-clip and reduces the
   arm-over-void). Cheap, complements #1. Watch corridor fit: corridors are 2-wide (clear 1.6,
   half 0.8), so keep half.x < 0.8.
3. Precisely reproduce the **user's exact screenshot** (inside corner + arm over void) and
   confirm via a screenshot whether cutaway+margin resolves it. Screen capture works this
   session; **keystroke injection is blocked** (System Events 1002), so drive with a temp
   auto-mover, then revert it.

## Meta / process notes for the next agent

- The user explicitly wants **research-referenced, fault-localized debugging** — instrument
  and prove root cause before changing code; don't slap on speculative fixes.
- Verify visually: `screencapture -o -x` works; bring window forward via System Events
  `set frontmost`; keystrokes are blocked, so use a temp auto-mover for repro then revert it.
- Watch for **stale game instances**: earlier a `target/release` build ran for 33 min and was
  screenshotted by mistake. Kill by bare name: `pkill -9 -f foundation_vs_slop`.
- Nothing is committed. Reverting the temp code returns to a clean, collision-correct state
  (WFC dungeon, fog, Backrooms textures, tall walls, non-clipping collision).
