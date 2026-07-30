#!/usr/bin/env -S blender --background --factory-startup --python
"""**Per-object** `.blend` → glTF 2.0 binary, against `docs/artist_guide.md` §3.

    blender --background --factory-startup --python scripts/blend_to_glb.py -- \\
        --src  /mnt/codex_fs/game_assets/models/tinylivingpack/source/Sims4_TinyLivingPack_1_0.blend \\
        --out  /tmp/tinyliving_glb --prefix-strip TinyLiving_

Sibling of `scripts/fbx_to_glb.py`, which converts a *pack of FBX files*. This one splits a **single
`.blend` containing many props** into one GLB per object, which is the shape the furniture manifest
needs: `placement.furniture.items` addresses one model per row, and Bevy spawns it via
`GltfAssetLabel::Scene(0)`.

`--factory-startup` matters for the same reason it does in the FBX script: a user's enabled add-ons
can register exporters that change glTF output, and this must produce the same bytes on any machine.

# It converts to a STAGING directory, never straight into `assets/`

`docs/artist_guide.md` §2: the share is the *library*; `assets/` holds only what the game loads,
converted and named for its use. A 48-object pack yields 48 GLBs of which the game wants a handful,
so this writes to `--out` and a human promotes and renames what a room actually needs.

# What it guards against, each measured against this pack rather than assumed

* **Origin is not where the prop's base is.** The Sims-derived source leaves many objects with their
  origin metres away (`offset (-6.0892, -2.1998, +1.1559)` on the bookends, per the pack's README),
  because they were authored laid out in a showroom grid. The placement grammar seats a prop by its
  transform, so exporting as-authored puts furniture through walls and floating in the air. Every
  object is re-origined to **base-centre**: X/Y centred on the bounding box, Z at its minimum — the
  "base-at-origin (Z)" convention the pack's own README uses for the objects that already had it.
* **Un-applied transforms.** Object-level scale/rotation would otherwise bake into the export
  inconsistently with the re-origin maths, so transforms are applied first and the mesh data is moved
  directly.
* **Blender is Z-up, glTF is Y-up.** The exporter's `+Y up` conversion is on (the default, stated
  explicitly here because `BEVY_GAME_INFO.md` pins "Y-up, metres, no axis conversion applied at
  load" — the conversion has to happen at export or not at all).
* **Scale is left alone.** Furniture ships native 1:1 in real metres; the manifest measures it. A
  convenience `--scale` would be the first step toward a kit nothing agrees on the size of.
* **Name collisions and junk objects.** Output names are slugified and de-duplicated; non-mesh
  objects and Blender's `.001` duplicate suffixes are handled rather than silently overwriting.

Reports each object's exported dimensions in metres, so the numbers can be pasted into the manifest
rather than eyeballed.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import bpy


def slugify(name: str, strip_prefix: str | None) -> str:
    """`TinyLiving_SirCumferenceCoffeeTable` → `sir_cumference_coffee_table`."""
    if strip_prefix and name.startswith(strip_prefix):
        name = name[len(strip_prefix) :]
    name = re.sub(r"\.\d+$", "", name)  # Blender's duplicate suffix
    name = name.replace("-", "_").replace(" ", "_")
    # Split CamelCase without shattering acronyms.
    name = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name)
    name = re.sub(r"(?<=[A-Z])(?=[A-Z][a-z])", "_", name)
    return re.sub(r"_+", "_", name).strip("_").lower()


def isolate(obj) -> None:
    """Make `obj` the one selected, active object."""
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj


def reorigin_to_base(obj) -> tuple[float, float, float]:
    """Apply transforms, then move mesh data so base-centre sits at the world origin.

    Returns the object's `(x, y, z)` extent in metres *after* the move — Blender axes, so `z` is the
    height that becomes Bevy's `+Y`.
    """
    isolate(obj)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

    # With transforms applied, local == world, so `bound_box` is the world AABB.
    corners = [tuple(c) for c in obj.bound_box]
    xs = [c[0] for c in corners]
    ys = [c[1] for c in corners]
    zs = [c[2] for c in corners]
    cx = (min(xs) + max(xs)) * 0.5
    cy = (min(ys) + max(ys)) * 0.5
    base_z = min(zs)

    # Move the MESH, not the object: the object transform is already applied, and shifting it again
    # would reintroduce exactly the offset this is removing.
    for v in obj.data.vertices:
        v.co.x -= cx
        v.co.y -= cy
        v.co.z -= base_z
    obj.data.update()
    return (max(xs) - min(xs), max(ys) - min(ys), max(zs) - min(zs))


def main(argv: list[str]) -> int:
    argv = argv[argv.index("--") + 1 :] if "--" in argv else []
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--src", type=Path, required=True, help="source .blend (already opened by Blender)")
    ap.add_argument("--out", type=Path, required=True, help="staging output directory")
    ap.add_argument("--prefix-strip", default=None, help="drop this prefix from object names")
    ap.add_argument("--only", nargs="*", default=None, help="export only these object names")
    args = ap.parse_args(argv)

    args.out.mkdir(parents=True, exist_ok=True)

    # Sorted by name so the output set — and the `_2`, `_3` de-duplication suffixes below, which are
    # assigned in iteration order — are a property of the source, not of Blender's unspecified
    # `bpy.data.objects` ordering. Otherwise a re-run on another machine can hand the same slug to a
    # different mesh.
    meshes = sorted((o for o in bpy.data.objects if o.type == "MESH"), key=lambda o: o.name)
    if args.only:
        wanted = set(args.only)
        meshes = [o for o in meshes if o.name in wanted]
    if not meshes:
        print("error: no mesh objects matched", file=sys.stderr)
        return 1

    # Everything starts hidden-from-export; `use_selection` picks one object at a time.
    bpy.ops.object.select_all(action="DESELECT")

    seen: dict[str, int] = {}
    rows: list[tuple[str, tuple[float, float, float], int]] = []
    for obj in meshes:
        slug = slugify(obj.name, args.prefix_strip)
        if slug in seen:
            seen[slug] += 1
            slug = f"{slug}_{seen[slug]}"
        else:
            seen[slug] = 0

        extent = reorigin_to_base(obj)
        isolate(obj)
        dst = args.out / f"{slug}.glb"
        bpy.ops.export_scene.gltf(
            filepath=str(dst),
            export_format="GLB",
            use_selection=True,
            export_yup=True,  # Blender Z-up -> glTF/Bevy Y-up. See module docs.
            export_apply=True,
            export_materials="EXPORT",
        )
        tris = len(obj.data.loop_triangles) or sum(len(p.vertices) - 2 for p in obj.data.polygons)
        rows.append((slug, extent, tris))

    print("\n=== exported (metres, Blender XYZ; Z becomes Bevy +Y height) ===")
    print(f"{'name':38s} {'x':>7s} {'y':>7s} {'z(h)':>7s} {'tris':>7s}")
    for slug, (x, y, z), tris in sorted(rows):
        print(f"{slug:38s} {x:7.3f} {y:7.3f} {z:7.3f} {tris:7d}")
    print(f"\n{len(rows)} object(s) -> {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
