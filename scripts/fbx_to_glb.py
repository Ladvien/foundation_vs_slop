#!/usr/bin/env -S blender --background --factory-startup --python
"""**FVS-N-10** — batch FBX → glTF 2.0 binary, against `docs/artist_guide.md` §3.

The Ozea "Ultimate SciFi Asset Library" ships `.fbx` + `.obj` + `.blend` and **zero `.glb`**, while
the artist guide's format rule is a hard requirement: glTF 2.0 binary only. This is the repeatable
conversion, run headless.

    blender --background --factory-startup --python scripts/fbx_to_glb.py -- \\
        --src  /mnt/codex_fs/game_assets/models/scifi/ozea_ultimate_library \\
        --pack Pack_SciFi_A_001+_V2.0 \\
        --out  /tmp/ozea_glb

`--factory-startup` matters: a user's enabled add-ons can register importers that change FBX
interpretation, and this must produce the same bytes on any machine.

# It converts to a STAGING directory, never straight into `assets/`

`docs/artist_guide.md` §2: the share is the *library*; `assets/` holds only what the game loads,
**converted and named for its use**. A bulk copy would put 411 meshes named `SM_Wall_A_01` in the
tree, of which the game loads a handful. So this writes to `--out` and a human promotes and renames
what Site-67 actually uses. That is the same raw→curated split `/mnt/codex_fs/game_assets/models/`
already enforces for generative assets.

# What it guards against, each measured rather than assumed

* **macOS resource forks.** The library carries **418** `._*.fbx` siblings (plus ~1500 more beside the
  other file types). They are AppleDouble metadata, not models; a naive `glob('*.fbx')` returns them
  as phantom duplicates and Blender's importer either fails or produces garbage. Skipped by name.
* **Colliding basenames.** 411 distinct basenames across 418 files — **7 collide across packs** and
  would silently overwrite each other in a flat output directory. Every output is therefore prefixed
  with its pack, so the collision is impossible rather than merely unlikely.
* **Empty imports.** An FBX that yields no mesh is reported and skipped, not exported as an empty
  `.glb` that fails later at a spawn site with no clue where it came from.
* **Stray geometry.** §3 rule 6 exists because a 10-metre shelf FBX forced a kit swap. Anything whose
  bounding box exceeds `--max-extent` is reported loudly; it is still written, because "unusually
  large" is a judgement a human makes, but it cannot pass unnoticed.

# The export contract

Mirrors `scp_characters`'s `export/gltf.py`, which is the existing in-repo precedent:
`export_format=GLB`, `export_yup=True` (§3 rule 2 — no axis conversion is applied at load),
`export_apply=True` (modifiers baked, or they silently vanish), `export_tangents=True` (Bevy does not
regenerate them for normal maps), and textures embedded.

Animations are **off**: these are static props. A prop carrying an empty animation list is noise in
the asset, and the clip contract in `docs/artist_guide.md` §4 applies to characters, not scenery.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

import bpy

# Blender runs this as `--python scripts/fbx_to_glb.py`, which does NOT put `scripts/` on `sys.path`.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from mesh_origin import reorigin_group_to_base  # noqa: E402 — must follow the path insert above


def cli_args() -> argparse.Namespace:
    """Blender passes script args after a bare `--`."""
    argv = sys.argv
    argv = argv[argv.index("--") + 1 :] if "--" in argv else []
    p = argparse.ArgumentParser(prog="fbx_to_glb")
    p.add_argument("--src", required=True, help="library root")
    p.add_argument(
        "--pack",
        action="append",
        default=None,
        help="pack directory name; repeatable. Omit to convert every pack under --src.",
    )
    p.add_argument("--out", required=True, help="staging output directory")
    p.add_argument("--limit", type=int, default=0, help="stop after N models (0 = no limit)")
    p.add_argument(
        "--max-extent",
        type=float,
        default=12.0,
        help="report any model whose bounding box exceeds this many metres (artist guide §3 rule 6)",
    )
    p.add_argument(
        "--reorigin-base",
        action="store_true",
        help=(
            "seat each asset's pivot XZ-centred with its base at y=0 (artist guide §3 rule 7). "
            "Use for anything destined for a KIT — `site::kit::y_scale` scales about the entity "
            "origin, so a centre-origined wall buries half of itself. Off by default so a bulk "
            "staging pass stays faithful to what the artist authored."
        ),
    )
    return p.parse_args(argv)


def find_fbx(src: str, packs: list[str] | None) -> list[tuple[str, str]]:
    """`(pack, path)` for every real `.fbx`, resource forks excluded.

    `Pack_SciFi_A_001+_V2.0` is nested one level deeper than its siblings and carries a `+` in the
    path, so the walk is by directory rather than by a fixed depth or a glob pattern.
    """
    out: list[tuple[str, str]] = []
    roots = [os.path.join(src, p) for p in packs] if packs else [
        os.path.join(src, d) for d in sorted(os.listdir(src)) if os.path.isdir(os.path.join(src, d))
    ]
    for root in roots:
        pack = os.path.basename(root.rstrip("/"))
        for dirpath, _dirnames, filenames in os.walk(root):
            for fn in sorted(filenames):
                # AppleDouble forks are named `._<original>` and are NOT models.
                if fn.startswith("._") or not fn.lower().endswith(".fbx"):
                    continue
                out.append((pack, os.path.join(dirpath, fn)))
    # Sorted so a run is reproducible and a partial `--limit` run is a stable prefix.
    out.sort(key=lambda pair: (pair[0], pair[1]))
    return out


def slug(pack: str) -> str:
    """`Pack_SciFi_A_001+_V2.0` → `scifi_a_001__v2_0`. Filesystem- and asset-path-safe.

    **Each** non-alphanumeric becomes an underscore; runs are NOT collapsed, so `+_` becomes `__`.
    This docstring said `scifi_a_001_v2_0` — one underscore — which is what a collapsing rule would
    produce and is not what the files are called. `scripts/manifest_from_staging.py` restates this
    rule (it cannot import this module, which needs `bpy`), and matched 20 files short until it was
    read off the code rather than the sentence above it.
    """
    s = pack.lower().replace("pack_", "")
    return "".join(c if c.isalnum() else "_" for c in s).strip("_")


def scene_extent() -> float:
    """Largest bounding-box dimension across every mesh, in metres."""
    lo = [float("inf")] * 3
    hi = [float("-inf")] * 3
    for obj in bpy.context.scene.objects:
        if obj.type != "MESH":
            continue
        for corner in obj.bound_box:
            world = obj.matrix_world @ __import__("mathutils").Vector(corner)
            for i in range(3):
                lo[i] = min(lo[i], world[i])
                hi[i] = max(hi[i], world[i])
    if lo[0] == float("inf"):
        return 0.0
    return max(hi[i] - lo[i] for i in range(3))


def convert(
    pack: str,
    path: str,
    out_dir: str,
    max_extent: float,
    dims_out: dict | None = None,
    reorigin_base: bool = False,
) -> tuple[bool, str]:
    """Import one FBX into an empty scene and export it as `.glb`. Returns `(ok, note)`.

    `dims_out`, when given, receives `basename -> (w, h, d, pack)` for the inventory. The HEIGHT is
    the number `assets/site/kit_*.ron` needs: `site::kit` scales a piece by `target / authored`, and
    the authored height is per-mesh because Ozea is not a uniform module (walls 2.00 m, doorframes
    ~2.0 m, floors 0.05 m, stasis chamber 2.41 m). Hand-measuring 400 meshes is how a kit ends up
    subtly wrong, so the converter that already has the mesh open writes it down.
    """
    bpy.ops.wm.read_factory_settings(use_empty=True)
    try:
        bpy.ops.import_scene.fbx(filepath=path)
    except Exception as exc:  # noqa: BLE001 — the importer raises a bare RuntimeError
        return False, f"import failed: {exc}"

    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if not meshes:
        # Loud and skipped: an empty `.glb` would fail at a spawn site with nothing pointing back here.
        return False, "no mesh objects in the file"
    if dims_out is not None:
        dims_out[os.path.splitext(os.path.basename(path))[0]] = (
            max(o.dimensions[0] for o in meshes),
            max(o.dimensions[1] for o in meshes),
            max(o.dimensions[2] for o in meshes),
            pack,
        )

    # BAKE THE OBJECT TRANSFORM INTO THE MESH DATA.
    #
    # The FBX importer represents this library's centimetre authoring as a node `scale` of 0.01 over
    # 100x vertex data. That is valid glTF and Bevy renders it correctly — the node transform applies —
    # but it leaves a footgun: anything reading the mesh AABB *directly* (a bbox check, a collider, a
    # placement heuristic) sees centimetres and is off by 100x with no error. Measured on
    # `SM_DoorFrame_Double`: accessor bounds 200.3 units, node scale 0.01, true height 2.003 m.
    #
    # Applying the scale makes "the numbers in the file are metres" true, which is what
    # `docs/artist_guide.md` §3 rule 2 actually promises.
    for obj in bpy.context.scene.objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)

    # SEAT THE PIVOT (opt-in, `--reorigin-base`).
    #
    # `location=False, rotation=False` above is deliberate for a bulk staging pass — it keeps the file
    # faithful to what the artist authored. But a mesh destined for a KIT needs one shared datum, and
    # this library does not have one: measured across the 16 promoted Ozea pieces, walls and floors are
    # centre-origined while props are base-origined, because the source packs themselves disagree
    # (`SM_Wall_Corner` is even off-centre by 15 mm in two axes).
    #
    # `site::kit::y_scale` is `target / authored` applied about the entity origin, so a centre-origined
    # wall scaled to reach `WALL_HEIGHT` grows half its gain DOWNWARD and buries itself. See
    # `mesh_origin` for the full reasoning. Group-wise, not per-object: a two-object asset like the
    # cryo pod must move rigidly or it shears.
    if reorigin_base:
        reorigin_group_to_base(meshes)

    extent = scene_extent()
    note = ""
    if extent > max_extent:
        note = f"extent {extent:.1f} m > {max_extent} m — check the bbox (artist guide §3 rule 6)"

    base = os.path.splitext(os.path.basename(path))[0]
    # ALWAYS pack-prefixed: 7 basenames collide across packs, and a flat directory would silently
    # overwrite. Making it unconditional means the collision cannot come back with a new pack.
    dest = os.path.join(out_dir, f"{slug(pack)}__{base}.glb")

    bpy.ops.export_scene.gltf(
        filepath=dest,
        export_format="GLB",
        export_apply=True,      # bake modifiers, or they vanish silently
        export_yup=True,        # §3 rule 2 — no axis conversion at load
        export_tangents=True,   # Bevy does not regenerate them for normal maps
        export_animations=False,  # static scenery; §4's clip contract is for characters
        use_selection=False,
    )
    return True, note


def main() -> int:
    a = cli_args()
    os.makedirs(a.out, exist_ok=True)
    files = find_fbx(a.src, a.pack)
    if a.limit:
        files = files[: a.limit]
    if not files:
        print("fbx_to_glb: nothing to convert — check --src/--pack", file=sys.stderr)
        return 1

    print(f"fbx_to_glb: {len(files)} model(s) → {a.out}")
    ok = 0
    problems: list[str] = []
    dims: dict[str, tuple[float, float, float, str]] = {}
    for i, (pack, path) in enumerate(files, 1):
        good, note = convert(pack, path, a.out, a.max_extent, dims, a.reorigin_base)
        name = os.path.basename(path)
        if good:
            ok += 1
            print(f"  [{i}/{len(files)}] {name}" + (f"  ⚠ {note}" if note else ""))
            if note:
                problems.append(f"{pack}/{name}: {note}")
        else:
            print(f"  [{i}/{len(files)}] {name}  ✗ {note}")
            problems.append(f"{pack}/{name}: {note}")

    # INVENTORY — the authoring surface for the Site kit. Written beside the staged GLBs rather than
    # into `assets/`, because staging is exactly where a human picks what to promote.
    inv = os.path.join(a.out, "INVENTORY.md")
    with open(inv, "w") as fh:
        fh.write("# Converted mesh inventory\n\n")
        fh.write("*Generated by `scripts/fbx_to_glb.py`. Do not hand-edit — re-run it.*\n\n")
        fh.write(
            "The **H** column is what `assets/site/kit_*.ron` needs. `site::kit` scales a piece by\n"
            "`target / authored`; the authored height is per-mesh because these kits are not uniform\n"
            "modules. Promote a staged `.glb` into `assets/`, then paste its path and H into the kit.\n\n"
        )
        fh.write(f"{len(dims)} mesh(es).\n\n")
        fh.write("| staged file | W (m) | **H (m)** | D (m) | source pack |\n")
        fh.write("|---|---:|---:|---:|---|\n")
        for name in sorted(dims):
            w, h, d, pack = dims[name]
            fh.write(f"| `{name}.glb` | {w:.2f} | **{h:.2f}** | {d:.2f} | {pack} |\n")
    print(f"fbx_to_glb: inventory -> {inv}")

    print(f"\nfbx_to_glb: {ok}/{len(files)} converted")
    if problems:
        # Reported at the end as well as inline: a 400-model run scrolls, and the whole point of the
        # guards is that what they catch must not be lost in the noise.
        print("problems:")
        for p in problems:
            print(f"  - {p}")
    # Non-zero only if EVERYTHING failed — a partial batch is a normal outcome for a library this
    # varied, and the per-file report is the actionable output.
    return 0 if ok else 2


if __name__ == "__main__":
    sys.exit(main())
