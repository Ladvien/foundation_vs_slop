"""Shared mesh-origin maths for the Blender conversion scripts.

**The convention, stated once:** a converted mesh sits **XZ-centred with its base at the world
origin** — in Blender's Z-up that is `x = y = 0` at the bounding-box centre and `z = 0` at the
bottom, which the glTF exporter's `export_yup=True` turns into "centred in X/Z, `Y` minimum at 0".

# Why this file exists

The maths lived in two places and had already drifted into two shapes: `blend_to_glb.reorigin_to_base`
(a function) and `import_retro_tvs` (the same arithmetic inline, with a comment saying it was copied).
`scripts/fbx_to_glb.py` — the converter that produced the whole Ozea Site kit — had neither, so it
shipped 11 of 16 meshes on whatever pivot the source FBX happened to carry: walls and floors
centre-origined, props base-origined, one piece neither.

That is not a cosmetic difference. `site::kit::y_scale` is `target / authored` applied as a Y scale
about the entity origin, so a wall reaches `WALL_HEIGHT` only if it grows upward **from its base**. A
centre-origined 2.0 m wall scaled by 1.2 becomes `Y[-1.2, +1.2]`: half of it underground, 1.17 m
standing against 2.4 m intended. Every consumer downstream — the corner caps, the light fixtures at
`SITE_FIXTURE_Y`, the operatives' feet — is measured against a datum the meshes did not share.

One copy, imported by all three converters, is the fix for the *class* of bug rather than this instance.

# Importing this from a Blender script

These scripts run as `blender --background --factory-startup --python scripts/foo.py`, so Blender's
Python does not have `scripts/` on `sys.path`. Each consumer prepends its own directory:

    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from mesh_origin import reorigin_to_base
"""

from __future__ import annotations

import bpy


def isolate(obj) -> None:
    """Make `obj` the one selected, active object."""
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj


def _world_aabb(objs) -> tuple[list[float], list[float]]:
    """Combined world-space AABB over `objs`. Assumes transforms are already applied, so each
    object's `bound_box` is world-space."""
    lo = [float("inf")] * 3
    hi = [float("-inf")] * 3
    for obj in objs:
        for corner in obj.bound_box:
            for i in range(3):
                lo[i] = min(lo[i], corner[i])
                hi[i] = max(hi[i], corner[i])
    return lo, hi


def _shift_meshes(objs, dx: float, dy: float, dz: float) -> None:
    """Translate the MESH DATA of every object by one delta.

    Moves the mesh rather than the object because the object transform has already been applied —
    shifting that again would reintroduce exactly the offset being removed.

    **Guards against a shared mesh datablock.** Two objects can reference the same `obj.data` (the
    Ozea packs do this for repeated greebles), and translating its vertices once per *object* would
    move it twice. Keyed on the datablock's identity, not the object's.
    """
    seen: set[int] = set()
    for obj in objs:
        mesh = obj.data
        if id(mesh) in seen:
            continue
        seen.add(id(mesh))
        for v in mesh.vertices:
            v.co.x -= dx
            v.co.y -= dy
            v.co.z -= dz
        mesh.update()


def reorigin_to_base(obj, unit_scale: bool = False) -> tuple[float, float, float]:
    """Apply transforms, then move mesh data so base-centre sits at the world origin.

    Returns the object's `(x, y, z)` extent in metres *after* the move — Blender axes, so `z` is the
    height that becomes Bevy's `+Y`.
    """
    isolate(obj)
    # Detach from any parent FIRST, keeping the world transform. `transform_apply` bakes only the
    # object's OWN transform — a parented object keeps inheriting its parent's, so the "applied"
    # result is still rotated. glTF is the case that bites: Blender's importer parents everything to
    # a root empty carrying the Y-up -> Z-up conversion, so without this the acid barrels exported
    # lying on their side (height on Y, 0.86, instead of Z) and the base-at-origin move below then
    # planted a *side* face at y=0. A `.blend` with unparented objects never shows the bug.
    if obj.parent is not None:
        bpy.ops.object.parent_clear(type="CLEAR_KEEP_TRANSFORM")
    if unit_scale:
        # Keep the inherited ROTATION, discard the inherited SCALE. For packs whose mesh-local
        # coordinates are already the intended metres and whose node chain carries only junk.
        #
        # The acid-barrel pack is the worked example: node 1 (an `.fbx` wrapper) scales by 0.01, node 4
        # by 334.94, netting 3.3494 — a Sketchfab FBX round-trip artifact. Applied faithfully that
        # yields a 2.87 m barrel; the raw POSITION accessor reads 0.5525 x 0.5525 x 0.8573, and a real
        # 55-gallon drum is 0.851 m tall. So the mesh is authored correct and the chain is noise.
        # Ignoring the parent entirely is NOT the fix — that chain also carries glTF's Y-up -> Z-up
        # rotation, and dropping it exports the barrels lying on their side.
        obj.scale = (1.0, 1.0, 1.0)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

    # With transforms applied, local == world, so `bound_box` is the world AABB.
    lo, hi = _world_aabb([obj])
    _shift_meshes([obj], (lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, lo[2])
    return (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2])


def reorigin_group_to_base(objs) -> tuple[float, float, float]:
    """Re-origin a whole multi-object asset as **one rigid group**.

    Returns the group's combined `(x, y, z)` extent in metres, Blender axes.

    The difference from [`reorigin_to_base`] is the whole point: that one centres each object on its
    own base, which is right for a pack of independent props and **wrong for an asset built from
    several objects**. `cryo_pod.glb` is the worked example — a `_Body` and a `_Door` — where
    per-object re-origining would stack the door's base onto the body's and shear the asset apart.
    One combined AABB, one delta, applied to every mesh.

    Transforms are applied per object first (each bakes only its own), after which every `bound_box`
    is world-space and they can be compared to each other.
    """
    if not objs:
        return (0.0, 0.0, 0.0)
    for obj in objs:
        isolate(obj)
        if obj.parent is not None:
            bpy.ops.object.parent_clear(type="CLEAR_KEEP_TRANSFORM")
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

    lo, hi = _world_aabb(objs)
    _shift_meshes(objs, (lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, lo[2])
    return (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2])
