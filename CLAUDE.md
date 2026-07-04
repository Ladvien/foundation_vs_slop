- Use home still and lookup research on whatever you are implementing BEFORE you implement it.  We want SOTA and best practices.
- Do not use unwrap() or anything that'd lead to a panic.  Code safe.  Handle errors.
- Leave academic paper references in comments, if a paper was used in writing the code.

## Additional Game Assets
- Additional games assets are cataloged at /mnt/codex_fs/game_assets/CATALOG.md, feel free to use any of these.

## Taking screenshots (do NOT use the macOS `screencapture` tool)

The game screenshots **itself** from inside the render pipeline via the `devshot` dev module
(`src/devshot.rs`) — no macOS screen-recording permission, no window/Space juggling. To grab a frame
while the game is running (working dir = project root):

```bash
touch screenshot.request      # sentinel; devshot consumes it next frame
sleep 1.5                      # give it a frame or two to render + write
# then Read screenshot.png
```

- Output is `screenshot.png` in the project root (gitignored, overwritten each time).
- Mechanism: `Screenshot::primary_window()` + `save_to_disk` (bevy 0.19), triggered by the sentinel
  file so it can be driven headlessly from the shell.
- **Caveat:** a *fully hidden/occluded* macOS window releases its Metal drawable, so a capture then
  comes back **black** (~57 KB PNG). A real frame is >150 KB. If you get black, the game window is
  hidden — retry once it's visible (even unfocused is fine; `WinitSettings` renders continuously).
- Keystroke injection into the window is blocked this environment, so to verify input-driven
  behavior (movement, selection, fog reveal) drive it with a **temporary** auto-input/self-test
  system, screenshot, then revert the temp code.
- `devshot` is dev-only; strip the module + its plugin registration for release builds.
