#!/usr/bin/env python3
"""Derive a tangent-space **normal** map and a packed **ORM** map from a diffuse-only texture.

    scripts/derive_surface_maps.py assets/textures/backrooms-wall-diffuse.png \\
        --out assets/textures --name backrooms-wall --strength 2.2 --rough-lo 0.82 --rough-hi 0.98

# Why this exists

`assets/textures/` shipped exactly two dungeon textures, both **diffuse only** — no normal, roughness,
metallic, AO or emissive map existed anywhere in the project. With the flat `GlobalAmbientLight` that
used to light the game, that did not matter: a uniform ambient term is added identically to every
surface whatever its normal, so a normal map would have changed nothing. `src/world.rs` replaced that
fill with an irradiance environment map, which *does* depend on the normal — so surface relief now
reads, and these are the maps that supply it.

The asset library has 1.8 GB of real scanned PBR sets, but none of them are *this wallpaper* and *this
carpet*. Restyling those two textures to fit a scanned set would change authored art (the Backrooms
yellow is a deliberate look). Deriving from the diffuse keeps the art and adds only the channels it
was missing.

# The honest caveat

Luminance is **not** height. This treats a dark pixel as a low one, which is a heuristic, not a
measurement — it is right for grout lines, fabric weave, wallpaper embossing and grime, and wrong
wherever the albedo is dark because the material is dark rather than recessed. It is the standard
fallback when no height map was authored, and it is worth stating plainly rather than pretending the
output is a scan. If a real height map ever ships for these, feed it via `--height` and this
assumption drops out.

# What it guards against, each of which is a real failure and not a hypothetical

* **Seams.** Both inputs are *seamless* and tile every metre. A gradient computed with clamped edges
  puts a visible bright/dark line at every tile boundary — the exact grid the relight was meant to
  stop advertising. Every neighbour lookup here is `np.roll`, so the derivative wraps.
* **sRGB.** Diffuse PNGs are sRGB-encoded. Taking a gradient of sRGB values weights midtones wrongly;
  luminance is computed in **linear** light and only the final normal/ORM bytes are written back raw.
* **Colour-managed output.** A normal map is a vector field, not a picture. The outputs are written
  with no colour profile, and `dungeon::render` loads them with `is_srgb: false` — sRGB-decoding a
  normal map tilts every vector toward +Z and quietly flattens the relief you just generated.
* **Metallic.** Wallpaper and carpet are dielectrics. B is written as a hard 0 rather than left to
  whatever the diffuse happened to contain.

No Pillow on this box, so I/O goes through ImageMagick's raw `RGB:` pipe, which is dependency-free and
exact (no re-encode between read and write).
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

import numpy as np

# Rec. 709 luminance weights, applied in LINEAR light (see the sRGB note in the module docstring).
LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)


def run(cmd: list[str], stdin: bytes | None = None) -> bytes:
    """Run a command, returning stdout. Raises with the tool's own stderr on failure."""
    p = subprocess.run(cmd, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if p.returncode != 0:
        raise RuntimeError(f"{cmd[0]} failed ({p.returncode}): {p.stderr.decode(errors='replace')[:400]}")
    return p.stdout


def read_rgb(path: Path) -> np.ndarray:
    """Read an image as float32 RGB in [0,1], shape (h, w, 3)."""
    dims = run(["magick", "identify", "-format", "%w %h", str(path)]).decode()
    w, h = (int(v) for v in dims.split())
    raw = run(["magick", str(path), "-alpha", "off", "-depth", "8", "RGB:-"])
    expected = w * h * 3
    if len(raw) != expected:
        raise RuntimeError(f"{path}: expected {expected} bytes for {w}x{h} RGB, got {len(raw)}")
    return np.frombuffer(raw, dtype=np.uint8).reshape(h, w, 3).astype(np.float32) / 255.0


def write_rgb(path: Path, img: np.ndarray) -> None:
    """Write float32 RGB in [0,1] as an 8-bit PNG with no colour profile."""
    h, w, _ = img.shape
    raw = (np.clip(img, 0.0, 1.0) * 255.0 + 0.5).astype(np.uint8).tobytes()
    run(
        ["magick", "-size", f"{w}x{h}", "-depth", "8", "RGB:-", "-strip", f"PNG24:{path}"],
        stdin=raw,
    )


def srgb_to_linear(c: np.ndarray) -> np.ndarray:
    """Exact sRGB EOTF (not the 2.2 approximation — the toe matters for dark grout/weave)."""
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def box_blur_wrap(a: np.ndarray, radius: int, passes: int = 2) -> np.ndarray:
    """Separable box blur with wrap-around edges. Repeated passes approximate a Gaussian."""
    out = a
    for _ in range(passes):
        acc = np.zeros_like(out)
        for d in range(-radius, radius + 1):
            acc += np.roll(out, d, axis=0)
        out = acc / (2 * radius + 1)
        acc = np.zeros_like(out)
        for d in range(-radius, radius + 1):
            acc += np.roll(out, d, axis=1)
        out = acc / (2 * radius + 1)
    return out


def sobel_wrap(h: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Sobel gradients with wrap-around, so a seamless input yields a seamless normal map."""
    def sh(dy: int, dx: int) -> np.ndarray:
        return np.roll(np.roll(h, dy, axis=0), dx, axis=1)

    # Standard 3x3 Sobel. dx is +right, dy is +down in image space.
    dx = (sh(-1, -1) + 2 * sh(0, -1) + sh(1, -1)) - (sh(-1, 1) + 2 * sh(0, 1) + sh(1, 1))
    dy = (sh(-1, -1) + 2 * sh(-1, 0) + sh(-1, 1)) - (sh(1, -1) + 2 * sh(1, 0) + sh(1, 1))
    return dx, dy


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("diffuse", type=Path, help="source diffuse texture")
    ap.add_argument("--out", type=Path, required=True, help="output directory")
    ap.add_argument("--name", required=True, help="output basename: <name>-normal.png, <name>-orm.png")
    ap.add_argument("--height", type=Path, default=None,
                    help="optional real height map; overrides the luminance-as-height heuristic")
    ap.add_argument("--strength", type=float, default=2.0, help="normal-map relief strength")
    ap.add_argument("--height-blur", type=int, default=0,
                    help="blur the height field by this radius before differentiating. The point is to "
                         "separate PRINTED detail from PHYSICAL relief: a wallpaper's printed pattern is "
                         "flat ink, but luminance-as-height reads it as geometry, and the result is a wall "
                         "that looks like sandpaper. Blurring first keeps the embossing and drops the print.")
    ap.add_argument("--rough-lo", type=float, default=0.80,
                    help="roughness of the BRIGHTEST pixels (clean, slightly smoother)")
    ap.add_argument("--rough-hi", type=float, default=0.98,
                    help="roughness of the DARKEST pixels (grime and recesses scatter more)")
    ap.add_argument("--ao-radius", type=int, default=6, help="cavity radius in texels for the AO term")
    ap.add_argument("--ao-strength", type=float, default=0.6, help="0 = no AO, 1 = full cavity darkening")
    args = ap.parse_args(argv)

    if not args.diffuse.is_file():
        print(f"error: no such file: {args.diffuse}", file=sys.stderr)
        return 1
    args.out.mkdir(parents=True, exist_ok=True)

    lin = srgb_to_linear(read_rgb(args.diffuse))
    if args.height is not None:
        height = srgb_to_linear(read_rgb(args.height))[..., 0]
    else:
        height = lin @ LUMA

    # --- Normal map -------------------------------------------------------------------------------
    # Differentiate the (optionally smoothed) height, but keep the ORM terms below on the FULL-detail
    # height: grime should still darken and roughen at print resolution even where it is not embossed.
    relief = box_blur_wrap(height, args.height_blur, passes=1) if args.height_blur > 0 else height
    dx, dy = sobel_wrap(relief)
    # Tangent-space, OpenGL convention (+Y up) — glTF's and Bevy's. The DirectX flavour negates G, and
    # feeding one where the other is expected inverts every bump into a dent, which reads as "the
    # lighting is wrong" rather than as an obviously flipped channel.
    nx = -dx * args.strength
    ny = dy * args.strength
    nz = np.ones_like(nx)
    inv_len = 1.0 / np.sqrt(nx * nx + ny * ny + nz * nz)
    normal = np.stack([nx * inv_len, ny * inv_len, nz * inv_len], axis=-1) * 0.5 + 0.5
    write_rgb(args.out / f"{args.name}-normal.png", normal)

    # --- ORM: R = occlusion, G = roughness, B = metallic (glTF/Bevy packing) -----------------------
    # Cavity AO: how far below its neighbourhood a texel sits. A pixel level with its surroundings is
    # unoccluded (1.0); one in a groove darkens. This is a local contrast measure, NOT ray-traced
    # occlusion — it cannot know about geometry, only about the texture's own relief.
    cavity = height - box_blur_wrap(height, args.ao_radius)
    scale = np.abs(cavity).max()
    cavity = cavity / scale if scale > 1e-6 else np.zeros_like(cavity)
    ao = np.clip(1.0 + np.minimum(cavity, 0.0) * args.ao_strength, 0.0, 1.0)

    # Roughness: darker (dirtier, more recessed) scatters more. A narrow band — the whole surface is
    # matte wallpaper and carpet, and a wide band would read as wet patches.
    lo, hi = np.percentile(height, 2.0), np.percentile(height, 98.0)
    t = np.clip((height - lo) / (hi - lo), 0.0, 1.0) if hi - lo > 1e-6 else np.full_like(height, 0.5)
    rough = args.rough_hi + (args.rough_lo - args.rough_hi) * t

    orm = np.stack([ao, rough, np.zeros_like(ao)], axis=-1)  # B = 0: both surfaces are dielectric
    write_rgb(args.out / f"{args.name}-orm.png", orm)

    print(f"{args.name}: normal + orm written to {args.out}  "
          f"(relief x{args.strength}, roughness {args.rough_lo:.2f}..{args.rough_hi:.2f})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
