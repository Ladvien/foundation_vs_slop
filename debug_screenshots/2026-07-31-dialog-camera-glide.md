# 2026-07-31 — "move the game window where the dialog is happening"

Resolved. Capture `region_2026-07-31_12-17-26-384` (note + PNG) deleted; this is its write-up.

## What was asked

> When a dialog event happens, we should move the game window where the dialog is happening.
> Don't jar the player, though.

The capture showed five squad members mid-conversation at cell (57, 149) with the camera parked
elsewhere — the speech bubble was on screen, but only because the squad happened to be near the
camera's spawn framing. Nothing in the game aimed the view at a speaker.

## What shipped

`CameraRig::glide_to: Option<Vec3>` (`src/camera.rs`). When set, `drive_camera` eases `focus`
toward it and clears it on arrival; `dialogue::runtime::present_current` sets it to the current
speaker's position on **every node**, so a mid-conversation speaker change re-aims.

The three decisions the note's second sentence forced:

- **Gentler than the rotation ease.** Same frame-rate-independent `1 − exp(−k·dt)` construction the
  yaw uses (Holmér 2023), but `GLIDE_SMOOTHING = 4.0` against the yaw's `9.0` — a 9.0 pull across
  half a map reads as a yank. That is the "don't jar" half of the request, and it is the only
  number here tuned by feel rather than derived.
- **Real time, not virtual.** A conversation freezes the sim (`MenuState::Conversation` is
  `is_blocking()`), so a glide on the virtual clock would never move at all.
- **The player always wins.** Any WASD pan or middle-mouse drag clears the glide the same frame, and
  `snap_camera_to` clears it too — a glide aimed at the previous place must not drag the camera back
  across a newly-entered one. Note that `allow_pan` is false while the conversation overlay is up, so
  during a conversation the escape hatch is specifically the middle-drag.

**Conversations only.** Ambient barks — squad chatter during normal play — deliberately do not move
the camera; that would take the view away mid-combat.

## Verified

Not just unit-tested. Temporarily displacing the camera 30.8 world units at conversation start, the
real game traced `dist 28.71 → 19.07 → 12.06 → 5.65 → 0.00 arrived=true`, landing exactly on the
speaker, with a later node re-aiming to a different squad member. Probe removed after.

`glide_step` is also split out as a pure function with unit tests: no single frame teleports, the
approach is monotone, it lands exactly (so the caller can clear it), and 4 frames at 240 fps travel
the same distance as 1 frame at 60 fps.

## Fixed in passing

`Action::CameraRecenter`'s comment claimed it was "a smooth pull rather than a teleport" while
writing `rig.focus` directly — which the camera transform is rebuilt from the same frame, i.e. a
teleport. It routes through the glide now, so the comment is true.
