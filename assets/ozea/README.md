# `assets/ozea/` — the Site-67 art kit

Seventeen meshes from the **OZEA Studio Ultimate Sci-Fi Asset Library**, staged at
`/mnt/codex_fs/game_assets/models/scifi/ozea_ultimate_library/` (37 packs, 415+ assets).

**Licence:** commercial and non-commercial use permitted; **redistribution or resale of the source
files is not**. The converted `.glb`s here are derived game assets, which the licence allows; the
source `.fbx`/`.blend`/`.obj` tree stays out of the repo and lives only on `codex_fs`.

Consumed by `assets/site/kit_ozea.ron` (the shipped Site kit) — plus `floor_grate` and `floor_light`,
which `assets/config/config.ron` also lands as `Tiled` decor props in the **dungeon**. Those two are
the only pieces here with a consumer outside the Site; changing them touches both.

## Origin convention — the reason this file exists

Every mesh in this directory is **XZ-centred with its base at `y = 0`** (`docs/artist_guide.md` §3).

This was not true until 2026-08-01. The kit was originally converted **without** re-origining, so it
inherited whatever pivot each source pack happened to carry — and the packs disagree with each other:
`SM_Wall_Corner` in HS_002 is off-centre by 15 mm in *two* axes, walls and floors are centre-origined,
props are base-origined. Eleven of the sixteen were wrong.

That is not cosmetic. `site::kit::y_scale` is `target / authored`, applied as a Y scale **about the
entity origin**, so a wall reaches `WALL_HEIGHT` only if it grows upward from its base. A
centre-origined 2.0 m wall scaled by 1.2 became `Y[-1.2, +1.2]`: half of it underground, 1.17 m
standing against 2.4 m intended. The player reported it as *"the north and south wall doesn't square
into the east-and-west walls… looks like the floors point of origin may be off too."*

`tests/ozea_asset.rs` now pins the convention against the bytes. It previously computed each mesh's
bounding box and used only `hi - lo`, discarding the minimum — which *is* the origin — which is exactly
how eleven files drifted without a single test noticing.

## Reproducing this directory

```sh
blender --background --factory-startup --python scripts/fbx_to_glb.py -- \
    --src  /mnt/codex_fs/game_assets/models/scifi/ozea_ultimate_library \
    --pack Pack_SciFi_HS_001_V2.0 --pack Pack_SciFi_HS_002_V3.0 \
    --pack Pack_SciFi_HS_004_V1.0 --pack Pack_SciFi_E_001_V1.0 \
    --pack Pack_SciFi_A_002_V1.0 --pack Pack_SciFi_A_003_V2.0 \
    --out  /tmp/ozea_staging --reorigin-base
```

`--reorigin-base` is **required** — without it the staged meshes carry the source pivots and the Site
breaks as described above. Then copy the staged files to the names below (the converter writes
`<pack-slug>__<fbx-basename>.glb`; promotion to a short, role-shaped name is deliberately manual, per
`scripts/fbx_to_glb.py`'s header).

`INVENTORY.md`, written beside the staged output, carries each mesh's measured W/H/D — those are the
numbers `kit_ozea.ron`'s `height` fields must match. Never hand-guess them.

## Name mapping

The source of every file, recovered from the glTF node names still embedded in each `.glb`. Note the
object names inside are frequently **French** and do not match their file names — `SM_Floor_Plain.fbx`
contains an object called `SM_Sol_Simple` — so map by node name, not by filename.

| `assets/ozea/` | source pack | source FBX | node name inside |
|---|---|---|---|
| `wall.glb` | HS_004 | `SM_Wall_1x2.fbx` | `SM_Wall_1x2` |
| `wall_header.glb` | HS_004 | `SM_Wall_1x2.fbx` | `SM_Wall_1x2` (top 0.40 m) |
| `wall_corner.glb` | HS_004 | `SM_Wall_CornerCap.fbx` | `SM_Wall_CornerCap` |
| `wall_low.glb` | A_003 | `SM_WallPanel_Low_Solid.fbx` | `SM_WallPanel_Low_Solid` |
| `wall_window.glb` | A_003 | `SM_WallPanel_Large_Window.fbx` | `SM_WallPanel_Large_Window` |
| `floor.glb` | HS_002 | `SM_Floor_Plain.fbx` | `SM_Sol_Simple` |
| `floor_grate.glb` | HS_002 | `SM_Floor_Grate.fbx` | `SM_Sol_Grille.001` |
| `floor_light.glb` | HS_002 | `SM_Floor_Light_01.fbx` | `SM_Sol_Light` |
| `floor_line_cross.glb` | A_003 | `SM_Floor_Line_Cross.fbx` | `SM_Floor_Line_Cross.001` |
| `floor_line_straight.glb` | A_003 | `SM_Floor_Line_Straight.fbx` | `SM_Floor_Line_Straight.001` |
| `doorframe_single.glb` | A_002 | `SM_DoorFrame_Single.fbx` | `SM_DoorFrame_Single` |
| `doorframe_double.glb` | A_002 | `SM_DoorFrame_Double.fbx` | `SM_DoorFrame_Double` |
| `column.glb` | HS_001 | `SM_Pylonne.fbx` | `Pylonne` |
| `crate.glb` | HS_001 | `SM_Caisse.fbx` | `Caisse` |
| `pipe.glb` | HS_001 | `SM_Tuyau_Droit.fbx` | `Tuyau_Droit.001` |
| `pipe_corner.glb` | HS_001 | `SM_Tuyau_Coude.fbx` | `SM_Tuyau_Coude` |
| `cryo_pod.glb` | E_001 | `SM_Cryogenic_Stasis_Chamber.fbx` | `_Body` + `_Door` |

`wall_header.glb` is the one piece that is not a straight promotion: it is `SM_Wall_1x2` **cropped to
its top 0.40 m** by `scripts/ozea_wall_heights.py`'s `HEADERS` table, to fill the band between
`DOORWAY_HEIGHT` and `WALL_HEIGHT` above the ASYNC door. A header course over a doorway *is* the top
of the wall, so cropping — rather than squashing a 2 m panel — keeps the wall's own top trim at its
authored size, and the header lines up with the wall tops either side of the door by construction.

`cryo_pod` is the one **two-object** asset, which is why the converter re-origins as a rigid *group*
(`mesh_origin::reorigin_group_to_base`): re-origining each object to its own base would stack the
door's base onto the body's and shear the pod apart.

⚠️ **Do not map containment cells from `SM_Cellule`.** French *cellule* is a **battery** cell — it
measures 0.30 × 0.50 × 0.30 m. The article wanted is `SM_Cryogenic_Stasis_Chamber`, promoted here as
`cryo_pod.glb`.
