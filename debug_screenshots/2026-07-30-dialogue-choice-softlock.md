# 2026-07-30 — "I can't click on these options" (FVS-L-7): fixed, and the limits of that claim

Status note for one player region-capture, `region_2026-07-30_21-06-20-790`, now deleted per this
directory's `CLAUDE.md`. What it showed: the expedition-start conversation over the squad — prompt
*"How do we play this?"* with options *"1. Push forward, weapons hot."* and *"2. Hold and sweep the
room first."* — with `Menu/overlay: Conversation`, `Sim: frozen`.

**This was a soft-lock, not a cosmetic bug.** A modal conversation freezes the sim and a choice never
auto-advances (`active.advance_at = f32::INFINITY`), so an unanswerable choice is an unrecoverable
run, on the game's opening beat.

---

## Cause

`MeshPickingSettings::require_markers` defaults to **`false`**, so the picking backend ray-casts
*every* mesh in the world, and `Pickable` defaults to `should_block_lower: true`.
`containment::extraction`'s beacon — a decorative, unlit, 10%-alpha `Cylinder` light shaft — stands
exactly where the squad starts, which is exactly where the leader's choice bubbles float. It was
swallowing the pointer. The player found it: *"whatever the beam of light is at that the squad starts
in, that intercepts the mouse."*

## Fix (shipped `d52c7d4`)

- **Picking is opt-in**: `require_markers: true` + `MeshPickingCamera` on the camera. This kills the
  class rather than patching one mesh — `Pickable::IGNORE` on the beacon would have left the trap
  armed for the next decorative mesh anyone puts near a bubble.
- **`1`–`9` (and numpad) pick an option; `Enter`/`Space` advance a line.** The bubbles were already
  *labelled* "1." / "2." — the keys a player would guess were being advertised and not accepted. This
  writes the same `ChoicePicked` message the click observer writes, so it is a second input *device*,
  not a second resolution path.

## What is verified, and what is not — read this before trusting the item as closed

**Verified (2026-07-30, `cargo test`):** five tests in `src/dialogue/runtime.rs::tests` drive the
conversation state machine on the keyboard alone — a digit picks that option, the numpad matches the
number row, an out-of-range digit is ignored rather than clamped, a digit typed during a *line* does
not answer the next choice, and a full walk ends with the cursor dropped, `ConversationLock` released
and `MenuState::Closed` set. `advance_at` is held at infinity throughout, so none of them can pass on
the auto-advance timeout. **That is the property which made this a soft-lock rather than an
annoyance: a player who cannot click can always press `1`, and the run continues.**

**NOT verified: that the bubbles can be clicked.** Mesh picking needs a window, a pointer and a hand;
the dialogue plugin is never registered in the headless harness (`src/dialogue/mod.rs:6-7`), so no
automated test in this repo can reach it. `require_markers` + `MeshPickingCamera` remain justified by
source reading only. The user chose to close FVS-L-7 on the keyboard evidence — recorded here so the
next reader inherits *"the run can no longer soft-lock"* and not *"picking is confirmed working."*

**If picking is ever silently dead, check `MeshPickingCamera` on the camera first** — that marker is
the cost of making picking opt-in, and a second camera that needs picking must opt in too.

**Files:** `src/dialogue/mod.rs`, `src/dialogue/runtime.rs`, `src/camera.rs`.

---

## Two unrelated things this capture also showed, kept because they are evidence

1. **FVS-Q-8, plainly.** The room in the frame has **yellow Backrooms carpet on the floor and grey
   concrete walls**, which is the per-cell biome bug the player reported separately (*"I don't like
   backrooms carpets and concrete walls. It should be one or the other."*). The fork was decided the
   same day: **option (b)**, corridors inherit one endpoint room. See `BACKLOG.md` FVS-Q-8.
2. **The still-open capture pair.** `region_2026-07-31_00-47-45-138` and `_00-47-58-994` are
   deliberately **not** deleted: 13 s apart, identical camera and identical scene census, 45.5 fps →
   27.3 fps with a 224 ms worst frame. That is FVS-N-24's fixed-camera decay caught twice in one
   pair, and it is the reproducer.
