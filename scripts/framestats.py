#!/usr/bin/env python3
"""**Measure a frame instead of glancing at it.**

Three of the Site editor's bugs were invisible to a green test suite and were found only by rendering
a frame and *measuring* it. A glance called the blanked framebuffer "dark"; the numbers called it 183
distinct colours at median luminance 0, against 13,343 and 57 for a healthy frame. That is the
difference between an opinion and a finding.

    ./scripts/framestats.py shot.png                 # stats for one frame
    ./scripts/framestats.py before.png after.png     # and what changed between two

Reports distinct colours, the luminance distribution, and — for a pair — the fraction of pixels that
differ and where they are. "Where" matters: a change confined to the left 22% of the screen is the
editor panel redrawing; a change spread over the whole frame is the world.

Uses `image` via PIL if present, else a small pure-Python PNG reader, so it works with no environment
setup. Downsamples before counting, because the point is a signal, not an exact census.
"""

import sys
import zlib
from pathlib import Path


def read_png(path: Path):
    """Decode a PNG to (width, height, rgb bytes). Handles the 8-bit RGB/RGBA non-interlaced forms
    that `devshot` writes; anything else is an error rather than a guess."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path}: not a PNG")
    pos, idat, w, h, depth, color = 8, bytearray(), 0, 0, 0, 0
    while pos < len(data):
        ln = int.from_bytes(data[pos : pos + 4], "big")
        typ = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + ln]
        if typ == b"IHDR":
            w = int.from_bytes(body[0:4], "big")
            h = int.from_bytes(body[4:8], "big")
            depth, color = body[8], body[9]
            if body[12] != 0:
                raise SystemExit(f"{path}: interlaced PNGs are not supported")
        elif typ == b"IDAT":
            idat += body
        elif typ == b"IEND":
            break
        pos += 12 + ln
    if depth != 8 or color not in (2, 6):
        raise SystemExit(f"{path}: expected 8-bit RGB or RGBA, got depth={depth} color={color}")
    nch = 3 if color == 2 else 4
    raw = zlib.decompress(bytes(idat))
    stride = w * nch
    out = bytearray(w * h * 3)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        filt = raw[p]
        p += 1
        line = bytearray(raw[p : p + stride])
        p += stride
        if filt == 1:
            for i in range(nch, stride):
                line[i] = (line[i] + line[i - nch]) & 0xFF
        elif filt == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif filt == 3:
            for i in range(stride):
                a = line[i - nch] if i >= nch else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 0xFF
        elif filt == 4:
            for i in range(stride):
                a = line[i - nch] if i >= nch else 0
                b = prev[i]
                c = prev[i - nch] if i >= nch else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        elif filt != 0:
            raise SystemExit(f"{path}: unknown row filter {filt}")
        for x in range(w):
            out[(y * w + x) * 3 : (y * w + x) * 3 + 3] = line[x * nch : x * nch + 3]
        prev = line
    return w, h, bytes(out)


def load(path: Path):
    try:
        from PIL import Image  # noqa: PLC0415  — optional fast path

        im = Image.open(path).convert("RGB")
        return im.width, im.height, im.tobytes()
    except ImportError:
        return read_png(path)


def stats(w, h, px, step=4):
    colours, lums = set(), []
    for y in range(0, h, step):
        base = y * w * 3
        for x in range(0, w, step):
            i = base + x * 3
            r, g, b = px[i], px[i + 1], px[i + 2]
            colours.add((r, g, b))
            lums.append(0.2126 * r + 0.7152 * g + 0.0722 * b)
    lums.sort()
    n = len(lums)
    return {
        "size": f"{w}x{h}",
        "distinct": len(colours),
        "lum_min": round(lums[0], 1),
        "lum_p25": round(lums[n // 4], 1),
        "lum_med": round(lums[n // 2], 1),
        "lum_p75": round(lums[3 * n // 4], 1),
        "lum_max": round(lums[-1], 1),
        "sampled": n,
    }


def diff(a, b, step=4):
    (w1, h1, p1), (w2, h2, p2) = a, b
    if (w1, h1) != (w2, h2):
        raise SystemExit(f"frames differ in size: {w1}x{h1} vs {w2}x{h2}")
    changed = total = 0
    cols = [0] * 10
    rows = [0] * 10
    for y in range(0, h1, step):
        base = y * w1 * 3
        for x in range(0, w1, step):
            i = base + x * 3
            total += 1
            d = (
                abs(p1[i] - p2[i])
                + abs(p1[i + 1] - p2[i + 1])
                + abs(p1[i + 2] - p2[i + 2])
            )
            # 24/765 ≈ 3% of the channel range: above encoder noise, below "a shadow moved".
            if d > 24:
                changed += 1
                cols[min(9, x * 10 // w1)] += 1
                rows[min(9, y * 10 // h1)] += 1
    return {
        "changed_pct": round(100.0 * changed / max(total, 1), 2),
        "by_column_tenth": cols,
        "by_row_tenth": rows,
    }


def main():
    args = [Path(a) for a in sys.argv[1:]]
    if not args:
        raise SystemExit(__doc__)
    frames = [load(p) for p in args]
    for path, f in zip(args, frames):
        s = stats(*f)
        print(f"{path.name}: " + "  ".join(f"{k}={v}" for k, v in s.items()))
    if len(frames) == 2:
        d = diff(frames[0], frames[1])
        print(f"\nchanged: {d['changed_pct']}% of sampled pixels")
        print(f"  by column tenth (left→right): {d['by_column_tenth']}")
        print(f"  by row tenth    (top→bottom): {d['by_row_tenth']}")
        if d["changed_pct"] < 0.05:
            print("\n  VERDICT: the frames are effectively identical — the input did nothing.")
        else:
            print("\n  VERDICT: the frame changed.")


if __name__ == "__main__":
    main()
