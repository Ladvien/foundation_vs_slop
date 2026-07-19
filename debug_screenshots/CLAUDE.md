# debug_screenshots/ — the player is pointing at something

**If you are a Claude Code session, read this first.**

Each capture is a **pair** sharing one base name:

- `region_<timestamp>.png` — a **screenspace region the player deliberately cropped** while the game was running (dev tool: **Ctrl+P**, drag a rectangle, release). Not a full screenshot — *just the area the player boxed*, because that box **is the message**: it marks exactly the thing they want you to look at (a visual bug, artifact, UI element, creature, or question about what's on screen).
- `region_<timestamp>.md` — the player's **typed note** describing the issue, plus **auto-gathered debugging metadata** (selection rect, camera position/zoom, the ground world-point + dungeon cell under the box, and the **gameplay entities that fall inside the box**: type, source asset, world position, screen position, distance). Read this alongside the PNG.

**Read the `.md` and open the `.png` together, then plan the fix.** The note says *what's wrong*, the image shows *it*, and the metadata says *where in the world / which entities* — that is exactly the (image, text, structured-context) triple that multimodal bug-localization wants (region focusing per Xiao et al. 2026 "VisualRepair"; structured spatial context per Liu et al. 2026 "GALA").

Treat these as a **human visual-attention signal** — a region-of-interest / "look here" pointer, the same idea as a highlighted crop handed to a vision model. Combined with the player's typed message, the box tells you *where* on screen the words are about.

## How to use them

- **Read the newest file first.** Filenames sort chronologically: `region_YYYY-MM-DD_HH-MM-SS-mmm.png` (capture time, millisecond-unique).
- **Open the image** (the Read tool renders PNGs). Look at what's inside the crop — that is the subject. If the player's request is vague ("this looks wrong", "fix that"), the crop is what "this"/"that" refers to.
- Multiple recent files usually mean the player pointed at several things in one session — line them up with their message in order.

## Notes

- The PNGs are **gitignored** (ephemeral debug artifacts); only this `CLAUDE.md` is committed.
- Produced by `src/region_capture.rs` (`RegionCapturePlugin`), a dev-only tool stripped from release builds. The capture plays a short "snap" and never touches the simulation.
- Older captures may be stale — a region the player pointed at earlier may already be resolved. When in doubt, ask which capture they mean.
