# Parked exercises

These six guides name pieces from the `site` kit — `site/floor`, `site/wall`, `site/wall_low`, `site/wall_doorway`, `site/tile_4`. That kit is not bound: `assets/emerge/kits.ron` says it *"was cleared on 2026-08-16 and is being re-authored against the ozea meshes"*, so an author walking one of these strands at the step that says "select `site/wall`".

They are parked rather than deleted because the kit is expected back, and rewriting a build-a-room walkthrough against a kit with no walls would be inventing an exercise rather than keeping one.

**Nothing loads this directory.** A guide reaches the editor over BRP (`bevy_debugger/guide`), and the two tests that scan `guides/` take only files whose extension is `json` — a directory has none, so parking one here is what stops it shipping. See `guided::PENDING_GUIDES_DIR`.

Four of them are still *driven* by `tests/headless.rs` against fixtures, so they cannot rot while they wait. Moving one back is a `git mv` and dropping the `pending/` from the path in its drive test, once `site` is bound in `kits.ron` again.
