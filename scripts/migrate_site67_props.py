#!/usr/bin/env python3
"""Translate Site-67's authored props into the enlarged layout.

The rooms kept their footprints, so every prop keeps its position *within its room* — this applies
one (dx, dz) per room rather than re-authoring 200 coordinates by hand, which is how a chair ends up
facing a wall it used to face away from.

Anything that lands in no old room is reported rather than guessed at. That is the whole point: a
silent fallback here would scatter the containment wing's crates into a void and nobody would notice
until a screenshot.
"""
import re
import sys

# (old_x, old_z, old_w, old_h, dx, dz, name)
MOVES = [
    (0, 2, 12, 10,   6,  0, "async door hall"),
    (14, 2, 16, 10, -1, -1, "containment wing -> twelve cell rooms replace it"),
    (30, 2, 4, 10,  20,  0, "monitoring"),
    (0, 12, 34, 2,   5,  2, "spine -> main spine"),
    (0, 14, 8, 8,    6,  5, "records"),
    (8, 14, 2, 10,   2, -2, "spine connector -> the approach"),
    (10, 14, 12, 10, 10,  5, "research"),
    (24, 14, 8, 6,  12,  5, "requisition"),
    (26, 20, 2, 2,  12,  5, "requisition connector"),
    (24, 22, 10, 8, 24, -3, "briefing"),
    (0, 24, 24, 2,   5,  6, "south spine"),
    (0, 26, 5, 8,    6,  9, "quarters"),
    (6, 26, 5, 8,    9,  9, "galley"),
    (12, 26, 5, 8,  12,  9, "activities"),
    (18, 26, 5, 8,  15,  9, "war room"),
]

PROP = re.compile(r'^(\s*\(\s*piece:\s*)(\w+)(\s*,\s*pos:\s*\(\s*)([-\d.]+)(\s*,\s*)([-\d.]+)(\s*\))(.*)$')


def find(x, z):
    for (ox, oz, ow, oh, dx, dz, name) in MOVES:
        if ox <= x < ox + ow and oz <= z < oz + oh:
            return dx, dz, name
    return None


def main(path):
    out, moved, orphans, dropped = [], 0, [], 0
    in_props = False
    for line in open(path):
        s = line.rstrip("\n")
        if re.match(r'\s*props:\s*\[', s):
            in_props = True
            out.append(s)
            continue
        if in_props and re.match(r'\s*\],\s*$', s):
            in_props = False
            out.append(s)
            continue
        m = PROP.match(s) if in_props else None
        if not m:
            out.append(s)
            continue
        x, z = float(m.group(4)), float(m.group(6))
        hit = find(x, z)
        if hit is None:
            orphans.append((m.group(2), x, z))
            out.append(s)
            continue
        dx, dz, _name = hit
        if dx == -1 and dz == -1:
            dropped += 1
            out.append(f"        // RELOCATED (containment wing became twelve cell rooms): {s.strip()}")
            continue
        moved += 1
        out.append(f"{m.group(1)}{m.group(2)}{m.group(3)}{x + dx:.1f}{m.group(5)}{z + dz:.1f}{m.group(7)}{m.group(8)}")

    open(path, "w").write("\n".join(out) + "\n")
    print(f"moved {moved} props, commented out {dropped} from the old containment wing")
    if orphans:
        print(f"⚠️ {len(orphans)} props matched no old room — place these by hand:")
        for p in orphans:
            print(f"    {p[0]} at ({p[1]}, {p[2]})")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "assets/site/site67.ron")
