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

Source: **ambientCG**, "Concrete 028" (https://ambientcg.com/view?id=Concrete028), 1K-JPG release.
License: **CC0 1.0** — public domain; ambientCG publishes its whole library CC0.

Board-formed architectural concrete: vertical plank seams, panel joints and pour staining. Unlike the
two sets above this one ships **authored** AO, roughness and normal maps, so `concrete-normal.png` is
its `NormalGL` (OpenGL/glTF convention, which is what Bevy wants) converted to PNG, and
`concrete-orm.png` packs its real AO + roughness rather than relief inferred from luminance. No use of
`scripts/derive_surface_maps.py` here — that script exists for diffuse-only sources. `-orm` is the
glTF channel packing: R = occlusion, G = roughness, B = metallic. Metallic is a hard 0 — concrete is
a dielectric.

The diffuse is **grey-world channel-balanced** (each channel scaled to the common mean) to strip a
faint warm cast, taking chroma 0.031 → 0.001. A channel balance rather than a saturation crush,
because crushing saturation would also flatten the pour staining that makes it read as concrete.

**Replaced Ground 0046 on 2026-08-01.** Player: *"I still have backrooms carpet with 'concrete' walls
(looks more like marble)."* They were right, and measurement backs it: Ground 0046 shipped at mean
srgb 0.599/0.595/0.579 with an albedo standard deviation of just **0.019** — a pale, almost featureless
wash, which is what reads as polished stone. Concrete 028 is 0.395/0.395/0.394 with sd **0.030**: 33%
darker and 58% more surface variation, with real board structure instead of a smooth field.

> Note for whoever measures next: the numbers this file previously quoted for Ground 0046
> (0.518/0.518/0.517, chroma ~0.001) do **not** match the shipped 1024² JPEG, which measures
> 0.599/0.595/0.579, chroma 0.020. They were presumably taken from the 2K source before the downscale
> and JPEG pass. The figures above are measured on the files as they ship.

The desaturation rationale still holds and is why the balance step exists: per
`docs/lore/2026-07-12-scp-color-language.md` §6 the concrete zone is the **desaturated counterweight**
to the Backrooms yellow (wallpaper 0.591/0.584/0.362, chroma 0.229; carpet chroma 0.317), so the less
chroma it carries the better it does its job. At chroma 0.001 this set is *more* neutral than the one
it replaces, so the biome contrast is preserved while the value contrast improves.

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
