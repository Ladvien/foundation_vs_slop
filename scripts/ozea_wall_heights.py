#!/usr/bin/env -S blender --background --factory-startup --python
"""Author true 2.4 m Site-kit variants of the Ozea wall family — instead of scaling them at runtime.

    blender --background --factory-startup --python scripts/ozea_wall_heights.py -- \\
        --src /mnt/codex_fs/game_assets/models/scifi/ozea_ultimate_library \\
        --out assets/ozea

# Why this exists

`site::kit::y_scale` is `target / authored`, applied as a **Y scale** in `site::visuals::place`. The
Ozea wall family is authored 2.00 m against a `WALL_HEIGHT` of 2.40, so the Site was stretching every
wall, corner cap and window by 1.2 — and the column, authored 1.10, by **2.18**. `assets/config/
config.ron` already records why that is not acceptable: a 1.2x vertical scale "would stretch the panel
detailing", which is exactly why the Ozea *walls* were never promoted into the dungeon furniture kit.
The Site was doing what the dungeon refused.

Scaling is the wrong tool because the distortion is not uniform in what it damages: skirting, panel
trim and bolt detail are all authored at a real size, and multiplying their height by 1.2 (or 2.18)
reads as smeared geometry, while the *plain* section between them could grow indefinitely and nobody
would know.

# The edit

For each piece: **cut at a height inside a plain band and translate everything above it up.** The
detail above and below keeps its authored proportions; only the featureless mid-section lengthens.
Cut heights were chosen by profiling the actual vertex distribution — e.g. `wall` has detail at
0.00-0.10, 0.40-0.47, 1.53-1.60 and 1.90-2.00, so anywhere in 0.47..1.53 is safe and 1.00 is the
middle of it. They are not guesses; re-derive them if the source pack is ever updated.

Faces that span the cut are the plain band's **vertical** side faces, whose normals are horizontal and
therefore unchanged by a vertical translation — so normals and tangents stay valid without a rebuild.
UVs on that band stretch, which is the one real cost and is invisible on flat-colour low-poly panels.

# Why it re-converts from the FBX rather than editing the shipped `.glb`

Round-tripping a `.glb` back through Blender re-parents everything under a Y-up conversion empty and
re-exports materials and tangents, which risks changing bytes that have nothing to do with height.
Importing the source once and doing re-origin + height in a single session keeps one conversion path,
which is the same reason `fbx_to_glb.py` exists at all.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Vector

# Blender runs this as `--python scripts/ozea_wall_heights.py`, which does NOT put `scripts/` on path.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from mesh_origin import reorigin_group_to_base  # noqa: E402 — must follow the path insert above

# `(dest name, pack, fbx, cut height m, target height m)`.
#
# `cut` is measured from the piece's base AFTER re-origin, and sits inside a plain band — see the
# module docstring. `target` is what `site::pieces::target_height` asks of the piece, so that
# `kit_ozea.ron`'s `height` can equal it and `y_scale` falls to a no-op 1.0.
PIECES = [
    ("wall.glb",        "Pack_SciFi_HS_004_V1.0", "SM_Wall_1x2.fbx",               1.00, 2.40),
    ("wall_corner.glb", "Pack_SciFi_HS_004_V1.0", "SM_Wall_CornerCap.fbx",         1.00, 2.40),
    ("wall_window.glb", "Pack_SciFi_A_003_V2.0",  "SM_WallPanel_Large_Window.fbx", 1.00, 2.40),
    ("column.glb",      "Pack_SciFi_HS_001_V2.0", "SM_Pylonne.fbx",                0.55, 2.40),
]

# `(dest name, pack, fbx, height m)` — pieces built by CROPPING a wall rather than lengthening one.
#
# The header course over a doorway is, architecturally, just the top of the wall: `WALL_HEIGHT` 2.40
# minus `DOORWAY_HEIGHT` 2.00 leaves a 0.40 m band, and without a piece in it the Site's perimeter has
# a slot straight through it above the ASYNC door. `DOORWAY_HEIGHT`'s own doc comment says "the wall
# runs continuous above it" — the dungeon does that, the Site never had.
#
# Cropping is the honest operation here, and it is not the same as shortening. `raise_above` cannot
# help (it only lengthens) and a Y scale would squash 2.00 m of authored trim into 0.40. Keeping the
# TOP 0.40 m of the wall gives a piece whose top trim is the wall's own top trim, at its authored
# size — so the header lines up with the wall tops either side of the door by construction rather
# than by tuning. The cut lands at `authored - 0.40` = 1.60 m, which is exactly the top edge of the
# wall's 1.53-1.60 detail band, so the crop follows an existing edge loop instead of slicing through
# geometry: what is kept is the plain 1.60-1.90 section plus the 1.90-2.00 trim.
HEADERS = [
    ("wall_header.glb", "Pack_SciFi_HS_004_V1.0", "SM_Wall_1x2.fbx", 0.40),
]

# `(dest name, pack, fbx)` — the half-length **corner leg**, cropped along the panel's LENGTH.
#
# A junction cell used to carry two FULL 1 m panels crossed at its centre. The corner point is that
# same centre, so each panel ran 0.50 m PAST the other: the two runs met in a plus, not an L, and each
# left a half-panel stub jutting into open space. The player saw it as "the walls, where two of them
# meet, overlap by about 1/3". Two adjacent junction cells made it worse — two parallel stubs with a
# doubled section between them.
#
# A leg is half a panel, so a junction places one leg per direction that actually continues instead of
# two panels that always overshoot. Cut at the panel's midpoint (`z = 0` in the exported mesh), which
# is plain: the length detail sits at |z| 0.18-0.25 and 0.40-0.50, so the cut misses every groove and
# the half kept keeps its authored END trim for the side that butts the next full panel.
#
# It also takes the SAME height re-authoring `wall.glb` does (cut 1.00, +0.40 to reach 2.40) — a leg
# that skipped it would be a 2.00 m stub against 2.40 m runs, which is the very defect this file
# exists to remove.
LEGS = [
    ("wall_leg.glb", "Pack_SciFi_HS_004_V1.0", "SM_Wall_1x2.fbx", 1.00, 2.40),
]


def raise_above(objs, cut: float, delta: float) -> None:
    """Translate every vertex above `cut` up by `delta`, in Blender's Z-up.

    Guards against a shared mesh datablock — translating one twice would double-move it (the same
    trap `mesh_origin._shift_meshes` documents).
    """
    seen: set[int] = set()
    for obj in objs:
        mesh = obj.data
        if id(mesh) in seen:
            continue
        seen.add(id(mesh))
        for v in mesh.vertices:
            if v.co.z > cut:
                v.co.z += delta
        mesh.update()


def crop_above(objs, cut: float) -> None:
    """Keep only the geometry above `cut`, capping the plane it leaves open (Blender Z-up).

    `bisect_plane` with `clear_inner` removes everything on the negative side of the plane normal —
    below the cut — and `holes_fill` closes the boundary loops it exposes, so the result is a closed
    solid rather than a shell with an open underside. Guards a shared mesh datablock for the same
    reason `raise_above` does: cutting one twice would cut the already-cut result.
    """
    seen: set[int] = set()
    for obj in objs:
        mesh = obj.data
        if id(mesh) in seen:
            continue
        seen.add(id(mesh))
        bm = bmesh.new()
        bm.from_mesh(mesh)
        res = bmesh.ops.bisect_plane(
            bm,
            geom=list(bm.verts) + list(bm.edges) + list(bm.faces),
            plane_co=Vector((0.0, 0.0, cut)),
            plane_no=Vector((0.0, 0.0, 1.0)),
            clear_inner=True,
        )
        edges = [e for e in res["geom_cut"] if isinstance(e, bmesh.types.BMEdge)]
        if edges:
            bmesh.ops.holes_fill(bm, edges=edges)
        bm.to_mesh(mesh)
        bm.free()
        mesh.update()


def crop_half(objs, axis: int, keep_negative: bool) -> None:
    """Keep half the mesh along `axis`, cutting at 0 and capping the face it opens.

    Same `bisect_plane` + `holes_fill` shape as `crop_above`; the only difference is which plane. The
    normal is flipped to choose a side, because `clear_inner` always removes what lies on the plane
    normal's negative side.
    """
    n = [0.0, 0.0, 0.0]
    n[axis] = -1.0 if keep_negative else 1.0
    seen: set[int] = set()
    for obj in objs:
        mesh = obj.data
        if id(mesh) in seen:
            continue
        seen.add(id(mesh))
        bm = bmesh.new()
        bm.from_mesh(mesh)
        res = bmesh.ops.bisect_plane(
            bm,
            geom=list(bm.verts) + list(bm.edges) + list(bm.faces),
            plane_co=Vector((0.0, 0.0, 0.0)),
            plane_no=Vector(n),
            clear_inner=True,
        )
        edges = [e for e in res["geom_cut"] if isinstance(e, bmesh.types.BMEdge)]
        if edges:
            bmesh.ops.holes_fill(bm, edges=edges)
        bm.to_mesh(mesh)
        bm.free()
        mesh.update()


def build_leg(
    src_root: str, out_dir: str, dest: str, pack: str, fbx: str, cut: float, target: float
) -> str:
    """Crop a wall panel to half its LENGTH, for a corner leg — see `LEGS`."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    path = os.path.join(src_root, pack, "02_EXPORT", "FBX", fbx)
    if not os.path.isfile(path):
        return f"{dest}: SOURCE MISSING {path}"
    bpy.ops.import_scene.fbx(filepath=path)
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if not meshes:
        return f"{dest}: no mesh objects in {fbx}"
    for obj in bpy.context.scene.objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)

    before = reorigin_group_to_base(meshes)
    # Height first, on exactly the terms `build` uses, so a leg and a run are the same wall.
    delta = target - before[2]
    if delta < 0 or not (0.0 < cut < before[2]):
        return f"{dest}: cut {cut} / target {target} outside the piece (0..{before[2]:.3f})"
    raise_above(meshes, cut, delta)
    # `export_yup` maps Blender (x, y, z) to glTF (x, z, -y), so the panel's exported LENGTH axis (Z)
    # is Blender's Y. Keeping Blender y <= 0 keeps the exported +Z half.
    crop_half(meshes, 1, keep_negative=True)
    ext = reorigin_group_to_base(meshes)

    out = os.path.join(out_dir, dest)
    bpy.ops.export_scene.gltf(
        filepath=out,
        export_format="GLB",
        export_apply=True,
        export_yup=True,
        export_tangents=True,
        export_animations=False,
        use_selection=False,
    )
    return f"{dest}: length {before[1]:.3f} -> {ext[1]:.3f} m (height {ext[2]:.3f} m)"


def build_header(src_root: str, out_dir: str, dest: str, pack: str, fbx: str, height: float) -> str:
    """Crop the top `height` metres off a wall to make a header course — see `HEADERS`."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    path = os.path.join(src_root, pack, "02_EXPORT", "FBX", fbx)
    if not os.path.isfile(path):
        return f"{dest}: SOURCE MISSING {path}"
    bpy.ops.import_scene.fbx(filepath=path)

    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if not meshes:
        return f"{dest}: no mesh objects in {fbx}"
    for obj in bpy.context.scene.objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)

    # Re-origin first, so the cut is measured from the piece's own base and not from the source pivot.
    authored = reorigin_group_to_base(meshes)[2]
    if not (0.0 < height < authored):
        return f"{dest}: header {height} m is outside the piece (0..{authored:.3f})"
    crop_above(meshes, authored - height)
    # ...and again, because the crop left the geometry floating `authored - height` up.
    ext = reorigin_group_to_base(meshes)

    out = os.path.join(out_dir, dest)
    bpy.ops.export_scene.gltf(
        filepath=out,
        export_format="GLB",
        export_apply=True,
        export_yup=True,
        export_tangents=True,
        export_animations=False,
        use_selection=False,
    )
    return f"{dest}: cropped top {height:.3f} m off {authored:.3f} m -> {ext[2]:.3f} m"


def build(src_root: str, out_dir: str, dest: str, pack: str, fbx: str, cut: float, target: float) -> str:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    path = os.path.join(src_root, pack, "02_EXPORT", "FBX", fbx)
    if not os.path.isfile(path):
        return f"{dest}: SOURCE MISSING {path}"
    bpy.ops.import_scene.fbx(filepath=path)

    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if not meshes:
        return f"{dest}: no mesh objects in {fbx}"

    # Same centimetre-authoring bake `fbx_to_glb.convert` does, for the same reason: make "the numbers
    # in the file are metres" true before anything measures them.
    for obj in bpy.context.scene.objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)

    # Re-origin FIRST so `cut` is measured from the piece's own base rather than from wherever the
    # source pack happened to put its pivot.
    ext = reorigin_group_to_base(meshes)
    authored = ext[2]  # Blender Z-up: index 2 is the height that becomes Bevy's +Y
    delta = target - authored
    if delta < 0:
        return f"{dest}: authored {authored:.3f} m already exceeds target {target:.3f} m — refusing to shrink"
    if not (0.0 < cut < authored):
        return f"{dest}: cut {cut} m is outside the piece (0..{authored:.3f})"
    raise_above(meshes, cut, delta)

    out = os.path.join(out_dir, dest)
    bpy.ops.export_scene.gltf(
        filepath=out,
        export_format="GLB",
        export_apply=True,
        export_yup=True,
        export_tangents=True,
        export_animations=False,
        use_selection=False,
    )
    return f"{dest}: {authored:.3f} -> {target:.3f} m (cut {cut:.2f}, +{delta:.3f})"


def main() -> int:
    argv = sys.argv
    argv = argv[argv.index("--") + 1 :] if "--" in argv else []
    p = argparse.ArgumentParser(prog="ozea_wall_heights")
    p.add_argument("--src", required=True, help="Ozea library root")
    p.add_argument("--out", required=True, help="output directory (assets/ozea)")
    a = p.parse_args(argv)

    for dest, pack, fbx, cut, target in PIECES:
        print("ozea_wall_heights:", build(a.src, a.out, dest, pack, fbx, cut, target))
    for dest, pack, fbx, height in HEADERS:
        print("ozea_wall_heights:", build_header(a.src, a.out, dest, pack, fbx, height))
    for dest, pack, fbx, cut, target in LEGS:
        print("ozea_wall_heights:", build_leg(a.src, a.out, dest, pack, fbx, cut, target))
    return 0


if __name__ == "__main__":
    sys.exit(main())
