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

Source: **texturecan.com**, "Concrete Wall of Stacked Rectangular Stones" (Concrete 0031), via
`/mnt/codex_fs/game_assets/textures/pbr/`. License: **CC0** (texturecan publishes its library CC0).
Downscaled from the 2K release to 1024² to match the Backrooms set, and the separate AO + roughness
maps packed into one ORM. Metallic is a hard 0 — concrete is a dielectric.
