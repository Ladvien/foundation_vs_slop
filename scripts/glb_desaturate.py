#!/usr/bin/env python3
"""Pull a `.glb`'s flat material colours toward institutional neutral, **in the container only**.

    scripts/glb_desaturate.py assets/ozea/mug.glb --out assets/ozea/mug.glb
    scripts/glb_desaturate.py in.glb --amount 0.9 --out out.glb

# Why this exists

`docs/lore/2026-07-12-scp-color-language.md` §0 is the strongest visual rule this project has:

    The Foundation has no color language. That is the point. Its visual identity is deliberately
    anti-color: black redaction bars, manila folders, clinical white, grayscale photographs...
    **Grayscale is contained. Color is anomalous. Saturation is a readout.**

§7 adds the two that bite here: *"Keep the Foundation grayscale. The restraint is the brand."* and
*"Make color mean deviation."* `src/ui/theme.rs` already enforces it on the UI side (bounded chroma,
`red >= blue`, tested by `the_foundation_has_no_house_palette`); the world side was still open.

The dressing pass on 2026-08-02 walked straight into it. `assets/low_poly_furniture/` is a stylised
kit authored for a different game: `Books A.glb` ships saturated red / blue / green covers, and
`Mug.glb` is `(0.80, 0.46, 0.07)` — **orange**, which in this setting is the single most legible
signal there is (`color-language.md` §4: D-Class). A bright orange mug on a console reads as a test
subject's, and a red-white-blue book stack in a war room reads as anomalous. Both were found by
rendering the scene and looking at it, not by a test.

# Why a container rewrite rather than a Blender round-trip

Identical reasoning to `scripts/glb_recompress_texture.py`, which this is modelled on: a glTF exporter
is free to reorder animations, retriangulate, rename morph targets or drop a shape key, any of which
silently breaks a contract some test pins. **Every byte of geometry, skin, animation and image data is
copied through untouched** — only `materials[].pbrMetallicRoughness.baseColorFactor` differs, and only
its RGB (alpha is preserved so glass stays glass).

⚠️ **Only affects flat-factor materials.** glTF writes `baseColorFactor = (1,1,1,1)` whenever a
base-colour *texture* is linked, so a textured material's colour does not live here and this script
refuses to pretend otherwise — it reports how many it skipped.

# The transform

Blend each colour toward its own **relative luminance** (Rec. 709), then apply a small warm bias, so
the result is desaturated rather than merely darkened and sits in the same warm-neutral family
`theme.rs` uses ("baseline reality is slightly warm — like a photocopied document"). A fully
achromatic result would be *colder* than the intent, which is why the bias is there and why it is
asymmetric: red is nudged up and blue down, preserving the `red >= blue` invariant the UI palette
tests assert.
"""

from __future__ import annotations

import argparse
import json
import struct
import sys

# Rec. 709 relative luminance — the same weights `palette.rs` and `theme.rs` reason in.
LUMA = (0.2126, 0.7152, 0.0722)

# How far a channel is pushed off neutral to keep the result warm rather than dead grey. Small: the
# rule is "slightly warm", not "sepia".
WARM_BIAS = 0.020


def desaturate(rgb: list[float], amount: float) -> list[float]:
    """Blend toward luminance by `amount`, then bias warm. `amount = 1.0` is fully achromatic."""
    lum = sum(c * w for c, w in zip(rgb, LUMA))
    out = [c + (lum - c) * amount for c in rgb]
    out[0] = min(1.0, out[0] + WARM_BIAS)  # red up
    out[2] = max(0.0, out[2] - WARM_BIAS)  # blue down  => red >= blue, always
    return out


def load(path: str):
    with open(path, "rb") as f:
        magic, version, total = struct.unpack("<III", f.read(12))
        if magic != 0x46546C67:
            sys.exit(f"{path} is not a binary glTF container")
        js, chunks = None, []
        while f.tell() < total:
            length, kind = struct.unpack("<II", f.read(8))
            data = f.read(length)
            if kind == 0x4E4F534A:
                js = json.loads(data.decode("utf-8"))
            chunks.append((kind, data))
        if js is None:
            sys.exit(f"{path} has no JSON chunk")
        return js, chunks


def write(path: str, js: dict, chunks: list) -> None:
    raw = json.dumps(js, separators=(",", ":")).encode("utf-8")
    raw += b" " * ((4 - len(raw) % 4) % 4)  # glTF requires 4-byte chunk alignment
    out = [(0x4E4F534A, raw)] + [(k, d) for k, d in chunks if k != 0x4E4F534A]
    body = b"".join(struct.pack("<II", len(d), k) + d for k, d in out)
    with open(path, "wb") as f:
        f.write(struct.pack("<III", 0x46546C67, 2, 12 + len(body)))
        f.write(body)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("glb")
    ap.add_argument("--amount", type=float, default=0.85, help="0 = unchanged, 1 = fully achromatic")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    js, chunks = load(args.glb)
    changed, textured = 0, 0
    for m in js.get("materials", []):
        pbr = m.setdefault("pbrMetallicRoughness", {})
        if pbr.get("baseColorTexture") is not None:
            textured += 1
            continue
        f = pbr.get("baseColorFactor", [1.0, 1.0, 1.0, 1.0])
        new = desaturate(f[:3], args.amount) + [f[3] if len(f) > 3 else 1.0]
        print(f"  {m.get('name', '?'):22} "
              f"({f[0]:.3f}, {f[1]:.3f}, {f[2]:.3f}) -> ({new[0]:.3f}, {new[1]:.3f}, {new[2]:.3f})")
        pbr["baseColorFactor"] = new
        changed += 1

    write(args.out, js, chunks)
    print(f"{args.glb}: {changed} material(s) neutralised at amount={args.amount}"
          + (f", {textured} skipped (base colour lives in a texture)" if textured else ""))


if __name__ == "__main__":
    main()
