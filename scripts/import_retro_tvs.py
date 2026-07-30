#!/usr/bin/env -S blender --background --factory-startup --python
"""Import the retro-TV FBX pack and export one GLB per set, **satisfying the `"screen"` contract**.

    blender --background --factory-startup --python scripts/import_retro_tvs.py -- \\
        --src /mnt/codex_fs/game_assets/models/retro-tvs-3-variations-2k \\
        --out /tmp/retro_tvs_glb

Pack-specific rather than generic (unlike its siblings `fbx_to_glb.py` / `blend_to_glb.py`) because
the work here is **material surgery against a runtime contract**, not a format change.

# The contract

`light::glow_screens` finds a TV's screen by walking the spawned scene for a sub-mesh whose
`StandardMaterial` satisfies, in **linear** space:

    c.green + c.blue > 3.0 * c.red + 0.05

and swaps that sub-mesh's material for the animated `TvStaticMaterial`. Miss it and the TV renders as
an inert box: no CRT snow, no `ScreenLight` — and **no error**, because nothing failed, the walk just
found nothing. That silence is why this script asserts the predicate on its own output instead of
trusting the export.

# Why the source pack does not satisfy it, and what is changed

The pack ships the right *structure* — every TV object carries two material slots, `Main_TV_Mat`
(chassis) and `Main_TV_Glass_Mat` (screen) — which is the hard part and the reason this pack was
chosen. But its glass base colour is neutral grey `(0.8, 0.8, 0.8)`: `1.6 > 2.45` is false, so the
screen would never be found.

Two changes, both to the glass material only:

1. **Unlink its base-colour texture.** This is not cosmetic tidying — glTF writes
   `baseColorFactor = (1,1,1,1)` whenever a base-colour *texture* is linked, discarding the socket
   value. So with the texture attached, no colour set here would survive the export at all.
2. **Set a dark, cool base colour.** It is a *marker* first: the runtime swap replaces this material
   within a frame of the scene streaming in, so what it looks like matters only for that frame and
   for any build where `LightingPlugin` is absent (the headless harness) — where "dead CRT glass" is
   exactly the right read anyway.

The chassis is forced to neutral white so it cannot be mistaken for the screen (`2.0 > 3.05` is
false). Its texture is left linked and intact.
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

import bpy

# A dead CRT: near-black, cool, and chromatic enough to clear the predicate with margin.
# Linear. G+B = 0.26 against 3R + 0.05 = 0.11.
SCREEN_BASE = (0.020, 0.100, 0.160, 1.0)
CHASSIS_BASE = (1.0, 1.0, 1.0, 1.0)
GLASS_MAT = "Main_TV_Glass_Mat"
CHASSIS_MAT = "Main_TV_Mat"


def principled(mat):
    if not mat.use_nodes:
        return None
    return next((n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)


def satisfies_screen_predicate(r: float, g: float, b: float) -> bool:
    """The exact test from `light::glow_screens`, kept in one place so drift is visible."""
    return g + b > 3.0 * r + 0.05


def verify_glb(path: Path) -> tuple[bool, list[tuple[float, float, float]]]:
    """Read back the exported GLB's `baseColorFactor`s and check one of them is a screen.

    Verifying the *artifact* rather than the Blender scene is the point: the failure this guards
    against (the exporter dropping a factor because a texture is linked) happens during export.
    """
    import json

    with open(path, "rb") as f:
        _magic, _ver, _len = struct.unpack("<III", f.read(12))
        clen, _ctype = struct.unpack("<II", f.read(8))
        gltf = json.loads(f.read(clen))
    factors: list[tuple[float, float, float]] = []
    for m in gltf.get("materials", []):
        pbr = m.get("pbrMetallicRoughness", {})
        f4 = pbr.get("baseColorFactor", [1.0, 1.0, 1.0, 1.0])
        factors.append((f4[0], f4[1], f4[2]))
    return any(satisfies_screen_predicate(*f) for f in factors), factors


def main(argv: list[str]) -> int:
    argv = argv[argv.index("--") + 1 :] if "--" in argv else []
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--src", type=Path, required=True, help="pack root (containing source/*.fbx)")
    ap.add_argument("--out", type=Path, required=True, help="staging output directory")
    ap.add_argument("--tex-size", type=int, default=1024,
                    help="downscale embedded maps to this edge. The pack ships 2K, but a TV is ~0.5 m "
                         "and never exceeds ~150 px on screen at this game's ortho iso zoom, so 2K is "
                         "~13x oversampled and every GLB embeds its own copy (5 TVs x 2K = 26 MB).")
    args = ap.parse_args(argv)

    fbx = sorted(p for p in (args.src / "source").glob("*.fbx") if not p.name.startswith("._"))
    if not fbx:
        print(f"error: no .fbx under {args.src / 'source'}", file=sys.stderr)
        return 1
    args.out.mkdir(parents=True, exist_ok=True)

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.fbx(filepath=str(fbx[0]))

    # --- Material surgery (see module docs) ---
    #
    # The FBX references its maps as siblings of `source/`, so the importer resolves none of them and
    # the chassis would export as flat white — worse than the greybox kit it is replacing. They are
    # linked explicitly here, from the pack's own `textures/` directory.
    tex_dir = args.src / "textures"

    def image(name: str):
        p = tex_dir / name
        if not p.is_file():
            print(f"  warning: missing texture {p.name}", file=sys.stderr)
            return None
        return bpy.data.images.load(str(p), check_existing=True)

    def link_tex(mat, node, socket: str, filename: str, non_color: bool):
        img = image(filename)
        if img is None:
            return
        if args.tex_size and max(img.size) > args.tex_size:
            img.scale(args.tex_size, args.tex_size)
            # PACK after scaling, or the downscale is silently discarded: the glTF exporter copies the
            # image's source FILE when one exists, not the resized in-memory buffer. Measured — without
            # this, 2K -> 1K only moved a GLB from 5.32 MB to 4.93 MB (the delta was recompression, not
            # resolution).
            img.pack()
        if non_color:
            # A normal or roughness map is data, not a picture — the same distinction that
            # `dungeon::render::linear_texture` enforces on the Bevy side.
            img.colorspace_settings.name = "Non-Color"
        tex = mat.node_tree.nodes.new("ShaderNodeTexImage")
        tex.image = img
        if socket == "Normal":
            nm = mat.node_tree.nodes.new("ShaderNodeNormalMap")
            mat.node_tree.links.new(tex.outputs["Color"], nm.inputs["Color"])
            mat.node_tree.links.new(nm.outputs["Normal"], node.inputs["Normal"])
        else:
            mat.node_tree.links.new(tex.outputs["Color"], node.inputs[socket])

    for mat in bpy.data.materials:
        node = principled(mat)
        if node is None:
            continue
        if mat.name.startswith(GLASS_MAT):
            # Deliberately textureless — it is a detection marker, and a linked base-colour texture
            # would make glTF discard the factor entirely (module docs).
            for link in list(node.inputs["Base Color"].links):
                mat.node_tree.links.remove(link)
            node.inputs["Base Color"].default_value = SCREEN_BASE
        elif mat.name.startswith(CHASSIS_MAT):
            for link in list(node.inputs["Base Color"].links):
                mat.node_tree.links.remove(link)
            node.inputs["Base Color"].default_value = CHASSIS_BASE
            link_tex(mat, node, "Base Color", "Main_TV_Mat_albedo.jpg", non_color=False)
            link_tex(mat, node, "Roughness", "Main_TV_Mat_roughness.jpg", non_color=True)
            link_tex(mat, node, "Normal", "Main_TV_Mat_normal.png", non_color=True)

    # SORTED BY NAME, and that is load-bearing: `names` below is applied POSITIONALLY. Blender does not
    # guarantee the order of `bpy.data.objects`, so an importer or version change could silently pair
    # `retro_tv_large.glb` with a different mesh — and the manifest rows carry *measured* footprints
    # keyed to those filenames, so the failure lands as wrong collision extents on the wrong model, with
    # no error anywhere. Sorting makes the pairing a property of the source names, not of iteration luck.
    meshes = sorted((o for o in bpy.data.objects if o.type == "MESH"), key=lambda o: o.name)
    # The pack's five TVs, in the source's own order: one large, two mid, two small.
    names = ["retro_tv_large", "retro_tv_a", "retro_tv_a_small", "retro_tv_b", "retro_tv_b_small"]

    failures = 0
    for i, obj in enumerate(meshes):
        slug = names[i] if i < len(names) else f"retro_tv_{i}"

        bpy.ops.object.select_all(action="DESELECT")
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

        # Re-origin to base-centre, same convention as `blend_to_glb.py` — the placement grammar
        # seats a prop by its transform, so a TV whose origin is elsewhere floats or sinks.
        corners = [tuple(c) for c in obj.bound_box]
        xs, ys, zs = ([c[a] for c in corners] for a in (0, 1, 2))
        cx, cy, base_z = (min(xs) + max(xs)) * 0.5, (min(ys) + max(ys)) * 0.5, min(zs)
        for v in obj.data.vertices:
            v.co.x -= cx
            v.co.y -= cy
            v.co.z -= base_z
        obj.data.update()

        dst = args.out / f"{slug}.glb"
        bpy.ops.export_scene.gltf(
            filepath=str(dst),
            export_format="GLB",
            use_selection=True,
            export_yup=True,
            export_apply=True,
            export_materials="EXPORT",
        )

        ok, factors = verify_glb(dst)
        dims = (max(xs) - min(xs), max(ys) - min(ys), max(zs) - min(zs))
        mark = "screen OK " if ok else "NO SCREEN"
        print(f"{slug:22s} {dims[0]:.3f}x{dims[1]:.3f}x{dims[2]:.3f} m  {mark}  factors={[tuple(round(c,3) for c in f) for f in factors]}")
        if not ok:
            failures += 1

    if failures:
        print(
            f"\nFAILED: {failures} GLB(s) carry no sub-mesh matching light::glow_screens' predicate "
            f"(green + blue > 3*red + 0.05, linear). Those TVs would render as inert boxes with no "
            f"error at runtime.",
            file=sys.stderr,
        )
        return 1
    print(f"\n{len(meshes)} TV(s) -> {args.out}; every one carries a detectable screen.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
