# `assets/site_dressing/` — the human half of Site-67's dressing

Ozea is a **facility** kit. Checked mesh by mesh across all 418 of its FBX on 2026-08-02, it has no
mugs, bottles, food, books, bags, blankets, laundry, stains or noticeboards anywhere — so the objects
that say *a person was here* have to come from somewhere else. They come from
`assets/low_poly_furniture/`, which was converted for the dungeon's furniture manifest and is 92/114
unused.

Two reasons these are copies rather than references into that tree:

1. **They are recoloured.** `docs/lore/2026-07-12-scp-color-language.md` §0: *"The Foundation has no
   color language… Grayscale is contained. Color is anomalous. Saturation is a readout."* The source
   meshes are a stylised kit — `Mug.glb` ships `(0.80, 0.46, 0.07)`, i.e. **orange**, which in this
   setting means D-Class, and `Books A.glb` ships saturated red/blue/green covers. Both were caught by
   rendering the Site and looking at it. `scripts/glb_desaturate.py` pulls each flat `baseColorFactor`
   toward its own luminance with a small warm bias, so `red >= blue` holds exactly as
   `src/ui/theme.rs` requires of the UI palette. Geometry, skins and animations are copied through
   byte-for-byte; only the colour factors differ.

2. **They cannot live in `assets/ozea/`.** `tests/ozea_asset.rs::every_ozea_mesh_is_base_origined_and_xz_centred`
   globs that directory and requires XZ-centring within 5 mm. `Mug.glb` measures `cx = -0.026`. It is
   base-origined (`y` starts at 0.000), which is the half the Site's `place()` actually needs, but it
   would fail that glob — so it gets its own directory rather than a weakened test.

Regenerate:

    python3 scripts/glb_desaturate.py "assets/low_poly_furniture/glb/Miscellaneous/Mug.glb" \
        --out assets/site_dressing/mug.glb
