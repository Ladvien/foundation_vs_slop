# Credits and third-party attribution

Third-party material bundled in this repository, and the terms it ships under. **Notices marked
*required* must travel with any distributed build** — they are licence conditions, not courtesies.

## Required attribution

### Human ear geometry — embedded in `assets/scp1048a/scp-1048-a.glb`

> **"Human Ear Model"** by **ssavish274** on Sketchfab
> (UID `e1f0f4b6fae54d59bdc3c6b2534eb411`), licensed
> **Creative Commons Attribution 4.0 International (CC-BY 4.0)**.

SCP-1048-A — the bear built out of human ears — embeds this geometry, processed (uniform-scaled to a
0.065 m ear length, solidified, voxel-remeshed and decimated 8064 → 120 triangles) and instanced
across the body. Because the geometry itself is embedded in the shipped `.glb`, **CC-BY's attribution
condition applies to any build that includes that asset.** The full notice, including the processing
steps, is `assets/scp1048a/ATTRIBUTION.md`; keep that file beside the asset and carry this credit into
any packaged build.

## Courtesy attribution (CC0 — not required)

These are public-domain (CC0 1.0) and impose no conditions; they are listed because saying where
things came from is the decent thing to do.

| Material | Source | Used by |
|---|---|---|
| Backrooms wall / carpet diffuse textures | [amini-allight/backrooms-textures](https://github.com/amini-allight/backrooms-textures) | `assets/textures/` (see its own `CREDITS.md`) |
| "Curly Teddy Natural" fabric normal map | Poly Haven | SCP-1048's plush surface |
| "rust_coarse_01" normal map | Poly Haven | SCP-1048-C's corroded scrap |
| `leather_white` / `leather_red_02` normal maps | Poly Haven | SCP-1048-B's skin and torn seam |
| Prototype kit | Kenney | `assets/kenney_prototype-kit` |

## Setting and fiction

SCP-1048 and the wider SCP Foundation setting are collaborative fiction published under
**CC-BY-SA 3.0** at [scpwiki.com](https://scpwiki.com). The 3D assets in this repository are
**clean-room reproductions** — original geometry built after studying reference material, never
derived mesh data — but the *characters and setting* remain the SCP community's work under that
licence. Per-asset provenance and the reference-gating rules are recorded in each asset's `README.md`
and in the `SCP_Characters` pipeline's `references/README.md`.

## This project

The game's own code and original assets are licensed under the **GNU GPL v3** — see `LICENSE`.
