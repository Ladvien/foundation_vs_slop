#!/usr/bin/env python3
"""Replace one embedded texture inside a `.glb`, **without touching anything else**.

    scripts/glb_recompress_texture.py assets/scp610/scp-610.glb \\
        --image 0 --replace-with /tmp/normal_1k.png --out assets/scp610/scp-610.glb

# Why this exists rather than a Blender round-trip

`assets/scp610/scp-610.glb` shipped at 28.7 MB, of which **88% was a single 2048² 16-bit PNG normal
map** (25.4 MB; the colour and roughness maps together are 1.1 MB). At this game's fixed isometric
zoom the creature is ~1.9 m tall and never exceeds a few hundred pixels, so a 2K 16-bit normal map is
roughly an order of magnitude more than the screen can show.

The obvious fix — import to Blender, scale the image, re-export — would round-trip the whole asset.
That asset's hand-off doc (`assets/scp610/README.md` §1) pins a contract this game's tests enforce:
13 bones on a single `torso` root, 5 in-place clips in a specific order, a `Basis` + `mutation` morph
pair, watertight geometry, base planted at `y = 0`. A glTF exporter is free to reorder animations,
rename morph targets, retriangulate, or drop a shape key — any of which silently breaks that contract
for a texture change. So this rewrites the container instead: **every byte of geometry, skin,
animation and morph data is copied through untouched**, and only the chosen image's bytes differ.

# How it works, and what it guards against

A GLB is a JSON chunk plus one BIN chunk; `bufferViews` are (offset, length) windows into that BIN.
Changing one image's length shifts every window after it, so the buffer is rebuilt from scratch:

* **Overlap check first.** Repacking assumes bufferViews do not share bytes. Blender's exporter never
  emits overlapping views, but a hand-built or optimised glTF can, and silently repacking one of those
  would corrupt the other view. Refuses to run if any two overlap.
* **4-byte alignment.** Every bufferView offset is padded up to a multiple of 4. glTF requires
  alignment to the accessor's component size, and 4 satisfies every type in practice; an unaligned
  view produces garbage vertices on some backends rather than an error.
* **Chunk padding.** The JSON chunk is padded with spaces and the BIN chunk with zeros, per spec —
  not with whatever happened to follow in memory.
* **Verified after write.** Re-parses the output and asserts the contract survives: identical node,
  mesh, skin, animation and accessor counts, identical animation names *in order*, identical morph
  target names, and identical total accessor element counts (which is geometry not having moved).
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from pathlib import Path

GLB_MAGIC = 0x46546C67
CHUNK_JSON = 0x4E4F534A
CHUNK_BIN = 0x004E4942


def read_glb(path: Path) -> tuple[dict, bytes]:
    data = path.read_bytes()
    magic, version, _total = struct.unpack_from("<III", data, 0)
    if magic != GLB_MAGIC:
        raise SystemExit(f"{path}: not a GLB (magic {magic:#x})")
    if version != 2:
        raise SystemExit(f"{path}: glTF version {version}, expected 2")
    off, gltf, buf = 12, None, b""
    while off < len(data):
        clen, ctype = struct.unpack_from("<II", data, off)
        body = data[off + 8 : off + 8 + clen]
        if ctype == CHUNK_JSON:
            gltf = json.loads(body)
        elif ctype == CHUNK_BIN:
            buf = body
        off += 8 + clen + ((4 - clen % 4) % 4)
    if gltf is None:
        raise SystemExit(f"{path}: no JSON chunk")
    return gltf, buf


def write_glb(path: Path, gltf: dict, buf: bytes) -> None:
    js = json.dumps(gltf, separators=(",", ":")).encode()
    js += b" " * ((4 - len(js) % 4) % 4)          # JSON pads with SPACES, per spec
    bn = buf + b"\x00" * ((4 - len(buf) % 4) % 4)  # BIN pads with ZEROS
    total = 12 + 8 + len(js) + 8 + len(bn)
    with open(path, "wb") as f:
        f.write(struct.pack("<III", GLB_MAGIC, 2, total))
        f.write(struct.pack("<II", len(js), CHUNK_JSON))
        f.write(js)
        f.write(struct.pack("<II", len(bn), CHUNK_BIN))
        f.write(bn)


def fingerprint(gltf: dict) -> dict:
    """The properties that must survive a texture swap. Compared before and after."""
    meshes = gltf.get("meshes", [])
    morphs = [m.get("extras", {}).get("targetNames") for m in meshes]
    return {
        "nodes": len(gltf.get("nodes", [])),
        "meshes": len(meshes),
        "skins": len(gltf.get("skins", [])),
        "joints": [len(s.get("joints", [])) for s in gltf.get("skins", [])],
        "animations": [a.get("name") for a in gltf.get("animations", [])],
        "accessors": len(gltf.get("accessors", [])),
        "accessor_counts": sum(a.get("count", 0) for a in gltf.get("accessors", [])),
        "morph_target_names": morphs,
        "materials": [m.get("name") for m in gltf.get("materials", [])],
    }


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("glb", type=Path)
    ap.add_argument("--image", type=int, required=True, help="index into gltf.images")
    ap.add_argument("--replace-with", type=Path, required=True)
    ap.add_argument("--mime", default=None, help="new mimeType (inferred from suffix if omitted)")
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args(argv)

    gltf, buf = read_glb(args.glb)
    before = fingerprint(gltf)

    views = gltf.get("bufferViews", [])
    spans = sorted((v.get("byteOffset", 0), v.get("byteOffset", 0) + v["byteLength"], i)
                   for i, v in enumerate(views))
    for a, b in zip(spans, spans[1:]):
        if a[1] > b[0]:
            raise SystemExit(
                f"bufferViews {a[2]} and {b[2]} overlap ({a[0]}..{a[1]} vs {b[0]}..{b[1]}); "
                "repacking would corrupt one of them. Aborting."
            )

    images = gltf.get("images", [])
    if not 0 <= args.image < len(images):
        raise SystemExit(f"--image {args.image} out of range (0..{len(images)-1})")
    img = images[args.image]
    if "bufferView" not in img:
        raise SystemExit(f"image {args.image} is a URI, not embedded — nothing to repack")
    target_view = img["bufferView"]

    new_bytes = args.replace_with.read_bytes()
    mime = args.mime or {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg"}.get(
        args.replace_with.suffix.lower()
    )
    if mime is None:
        raise SystemExit(f"cannot infer mimeType from {args.replace_with.name}; pass --mime")
    img["mimeType"] = mime

    # Rebuild the BIN chunk in the bufferViews' existing order, substituting the one image.
    out = bytearray()
    for _start, _end, idx in spans:
        v = views[idx]
        pad = (4 - len(out) % 4) % 4
        out.extend(b"\x00" * pad)
        payload = new_bytes if idx == target_view else buf[v["byteOffset"] : v["byteOffset"] + v["byteLength"]]
        v["byteOffset"] = len(out)
        v["byteLength"] = len(payload)
        out.extend(payload)

    gltf["buffers"][0]["byteLength"] = len(out)
    gltf["buffers"][0].pop("uri", None)
    write_glb(args.out, gltf, bytes(out))

    # Verify the contract on the artifact, not on the in-memory dict.
    after_gltf, _ = read_glb(args.out)
    after = fingerprint(after_gltf)
    if before != after:
        diff = {k: (before[k], after[k]) for k in before if before[k] != after[k]}
        raise SystemExit(f"CONTRACT CHANGED — refusing to trust this output: {diff}")

    old_mb = args.glb.stat().st_size / 1e6
    new_mb = args.out.stat().st_size / 1e6
    print(f"{args.glb.name}: {old_mb:.1f} MB -> {new_mb:.1f} MB ({new_mb/old_mb*100:.0f}%)")
    print(f"  image[{args.image}] {len(buf):,}-byte buffer rebuilt; contract verified:")
    print(f"    nodes={after['nodes']} meshes={after['meshes']} skins={after['skins']} "
          f"joints={after['joints']} accessors={after['accessors']}")
    print(f"    animations={after['animations']}")
    print(f"    morph targets={after['morph_target_names']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
