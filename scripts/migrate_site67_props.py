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
    # v1 (the 2026-08-02 enlargement) -> v2 (rooms expanded to close the voids, cells doubled).
    (6, 2, 14, 10,    0,  0, "async door hall — grew south over its own approach"),
    (10, 12, 4, 2,    0,  0, "the approach — swallowed by the hall"),
    (50, 2, 4, 10,   15,  0, "monitoring"),
    (21, 6, 24, 3,    0,  3, "the cell row"),
    (46, 2, 3, 12,   -1, -1, "cell spur — the block now meets monitoring directly"),
    (5, 14, 54, 4,    0,  6, "main spine"),
    (6, 19, 8, 8,     0,  6, "records"),
    (20, 19, 12, 10,  3,  6, "research"),
    (36, 19, 8, 6,    4,  6, "requisition"),
    (48, 19, 10, 8,   9,  6, "briefing"),
    (5, 30, 54, 4,    0,  6, "south spine"),
    (6, 35, 5, 8,     0,  6, "quarters"),
    (15, 35, 5, 8,    8,  6, "galley"),
    (24, 35, 5, 8,   16,  6, "activities"),
    (33, 35, 5, 8,   24,  6, "war room"),
    (2, 44, 60, 3,    0,  6, "ring — south leg"),
    (2, 2, 3, 42,     0,  0, "ring — west leg"),
    (59, 2, 3, 42,   14,  0, "ring — east leg"),
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
            out.append(f"        // RELOCATED (its corridor was absorbed): {s.strip()}")
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
