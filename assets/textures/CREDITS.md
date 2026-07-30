# Texture credits

- `backrooms-wall-diffuse.png`, `backrooms-carpet-diffuse.png`

Source: **amini-allight/backrooms-textures** (https://github.com/amini-allight/backrooms-textures)
License: **Creative Commons Zero (CC0 1.0)** — public domain. Attribution not required;
credited here as a courtesy.

- `backrooms-wall-normal.png`, `backrooms-wall-orm.png`,
  `backrooms-carpet-normal.png`, `backrooms-carpet-orm.png`

**Derived** from the two CC0 diffuse maps above by `scripts/derive_surface_maps.py` — the source set
is diffuse-only and no height map was ever authored, so relief is inferred from linear-light
luminance (that script's docstring states the caveat). Same CC0 terms as their source. `-orm` is the
glTF channel packing: R = occlusion, G = roughness, B = metallic.

- `concrete-diffuse.jpg`, `concrete-normal.png`, `concrete-orm.png`

Source: **texturecan.com**, "Damaged Concrete with Deep Cracks" (Ground 0046), via
`/mnt/codex_fs/game_assets/textures/pbr/`. License: **CC0** (texturecan publishes its library CC0).

Replaced Concrete 0031 ("Concrete Wall of Stacked Rectangular Stones") on 2026-07-30. That one worked
as a *zone* — clearly grey, clearly not the motel — but its stacked-stone pattern read as medieval
cobblestone rather than a Foundation facility. Ground 0046 is a poured slab with deep cracks, and it is
also near-perfectly neutral (mean srgb 0.518/0.518/0.517, chroma ~0.001) against the wallpaper's
0.59/0.58/0.36. That maximises the *saturation* contrast between the two biomes, which is what
`docs/lore/2026-07-12-scp-color-language.md` 6 actually asks of the reality layer — the concrete zone
is the desaturated counterweight, so the less chroma it carries the better it does its job.
Downscaled from the 2K release to 1024² to match the Backrooms set, and the separate AO + roughness
maps packed into one ORM. Metallic is a hard 0 — concrete is a dielectric.

## `assets/barrels/*.glb`

Source: **acid-barrel-pack**, via `/mnt/codex_fs/game_assets/models/acid-barrel-pack/`
(Sketchfab-sourced glTF). Split per-object and re-origined to base-centre by
`scripts/blend_to_glb.py --unit-scale`.

`--unit-scale` was needed, and finding out why is worth recording: the pack's node chain scales by
0.01 (an `.fbx` wrapper, cm→m) and then by 334.94, netting 3.3494. Applied faithfully that yields a
2.87 m barrel. The raw POSITION accessor reads 0.5525 × 0.5525 × 0.8573, and a real 55-gallon drum is
0.851 m tall — so the mesh is authored correct and the node scale is round-trip noise. Ignoring the
parent entirely is *not* the fix: that same chain carries glTF's Y-up → Z-up rotation, and dropping it
exports the barrels lying on their side.
