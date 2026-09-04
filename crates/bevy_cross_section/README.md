# bevy_cross_section

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

Anatomical cross-sections for cut faces: the skin, subcutaneous fat, muscle, cortical bone and marrow a cut through a limb, a torso or a head actually crosses, at the depths they were measured at on living adults — baked once into a procedural strip texture and painted onto any cap through a second UV channel.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. Issues and PRs belong upstream.

![A cut cylinder whose cap shows skin, fat, muscle, cortex and marrow bands](https://raw.githubusercontent.com/Ladvien/bevy_cross_section/main/docs/cross_section.gif)

## Why this exists

Every runtime-fracture system paints its cut faces with one "inside" material, and a limb sliced through reads as a statue: flat pink, edge to centre. A real cut is bands, and the bands have widths. A dermis is two millimetres; the fat under it is seven on a thigh and sixteen on an abdomen; the quadriceps is a centimetre and the hamstring two and a half; then a shell of cortical bone, then marrow. Those numbers exist because needle lengths, insulin injections and body-composition estimates depend on them, so they have been measured with ultrasound on thousands of people. This crate carries them, with the papers, and turns them into pixels.

The whole thing is CPU-side and deterministic: a strip is a pure function of the thickness table, a size and a seed, so two machines bake the same bytes and a test can freeze the digest.

## The model

**Depth below skin is exact and cheap.** A cut face belongs to a convex cell whose supplied faces are the subject's own surface. For a point inside a convex polytope the distance to the boundary is the minimum over its faces of the distance to that face's plane — a handful of dot products per vertex. `depth_below_skin` does that, and `annotate_cap` writes the result into `ATTRIBUTE_UV_1` as `(depth / span, along / tile)` without touching `UV_0`, so nothing that hashed a cap before this crate existed moves.

**The bands are a table with sources.** `Layers::for_region` carries, per region:

| Region | Skin | Fat | Muscle | Cortex | Sources |
|---|---|---|---|---|---|
| Limb | 1.9 mm | 7.2 mm | 18.7 mm | 5.0 mm* | Akkus 2012, Derraik 2014, Abe 2015 |
| Torso | 2.2 mm | 16.0 mm | 7.3 mm | 2.0 mm* | Akkus 2012, Derraik 2014, Abe 2015 |
| Head | 2.0 mm | 2.6 mm | 2.2 mm | 6.0 mm* | Dimitrova 2018 (6.8 mm total to bone; the split is this crate's) |

\* Cortical bone thickness is **not corpus-sourced** — no open-access long-bone cortex table was reachable when this was written — and is stated as this crate's own. Override it through `CrossSectionSettings::layers`.

**The strip is anatomy at physical scale.** Fat is lobules ~2 mm across in a septal net (Worley cells); muscle in cross-section is fascicles ~0.7 mm wrapped in perimysium at ~3 mm; cortex is ivory pierced by ~0.1 mm Haversian canals; the marrow cavity opens through a thinning trabecular lattice into yellow marrow with red patches. Because the strip is parameterised in millimetres, a lobule is 2 mm on a thigh and 2 mm on a finger. The muscle band's colour is not authored: it is a 0.3 mm venous film from `bloodstain::spectral` over a mid-grey substrate, the same optics that colour a pool.

## Compatibility

| bevy | bevy_cross_section |
|---|---|
| 0.19 | 0.1 |

## What it exposes

- `CrossSectionPlugin` — bakes one strip per `Region` on `Startup` into `CrossSectionAtlas`, as two images and a `StandardMaterial` that samples them through `UvChannel::Uv1`. System set: `CrossSectionSystems`.
- `CrossSectionSettings` — the per-region `Layers`, strip size, seed and `Scale`. Insert it before the plugin to override.
- `Layers`, `Layer`, `Region` — the table and the query `Layers::at(depth_mm)`.
- `SkinPlane`, `depth_below_skin`, `annotate_cap`, `uv1_at` — the geometry side.
- `strip`, `Strip`, `Band` — the bake, engine-free apart from `bevy::math`.

No components. The caller decides which region a subject's cells belong to, hands `annotate_cap` the cell's supplied-face planes, and puts `CrossSectionAtlas::material(region)` on the cap.

## References

- Akkus, Oguz, Uzunlulu & Kizilgul, *Evaluation of skin and subcutaneous adipose tissue thickness for optimal insulin injection*, J. Diabetes Metab. 3:8 (2012). `doi:10.4172/2155-6156.1000216`
- Derraik et al., *Effects of age, gender, BMI, and anatomical site on skin thickness in children and adults with diabetes*, PLoS ONE 9(1) (2014). `doi:10.1371/journal.pone.0086637`
- Abe, Loenneke & Thiebaud, *Morphological and functional relationships with ultrasound measured muscle thickness of the lower extremity*, Ultrasound 23(3) (2015). `doi:10.1177/1742271X14554678`
- Dimitrova et al., *Facial soft tissue thicknesses in Bulgarian adults*, Folia Morphol. 77(3) (2018). `doi:10.5603/fm.a2017.0114`
- Bosschaart et al., *A literature review and novel theoretical approach on the optical properties of whole blood*, Lasers Med Sci 29 (2014). `doi:10.1007/s10103-013-1446-7` — through `bloodstain::spectral`.

## Examples

```sh
cargo run --example cross_section_strip    # terminal only — bakes the three strips, prints bands and digests
cargo run --example cross_section          # a cut cylinder per region, caps banded  (needs a GPU)
```

## License

MIT OR Apache-2.0, at your option.
