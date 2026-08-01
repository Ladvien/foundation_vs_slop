#!/usr/bin/env python3
"""Derive Site-67's perimeter wall ring from its floor rects.

A boundary cell is any non-floor cell orthogonally adjacent to floor. Walls there cannot change
walkability (`is_walkable = is_floor && !wall`, and these are not floor), so the layout validator's
flood-fill is untouched by construction.

Orientation: the promoted `SM_Wall_1x2` is 0.10 m thin along X and 1.00 m long along Z, so a wall
separating along X wants yaw 0 and one separating along Z wants yaw 90.
"""

FLOOR = [
    (0, 2, 12, 10),    # async door hall
    (14, 2, 16, 10),   # containment wing
    (0, 12, 34, 2),    # the spine
    (0, 14, 8, 8),     # records office
    (10, 14, 12, 10),  # research wing
    (24, 14, 8, 6),    # requisition
    (26, 20, 2, 2),    # connector: requisition -> briefing
    (24, 22, 10, 8),   # briefing room
]

# Cells the ASYNC aperture occupies — its own frame is placed by the spawner, so no wall may sit on
# top of it or the doorway is bricked up.
DOOR_KEEP_OUT = {(x, 1) for x in range(5, 9)}

cells = set()
for x, z, w, h in FLOOR:
    for i in range(x, x + w):
        for j in range(z, z + h):
            cells.add((i, j))

boundary = {}
for (x, z) in cells:
    for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        n = (x + dx, z + dz)
        if n in cells:
            continue
        boundary.setdefault(n, set()).add((dx, dz))

def row(piece, cell, yaw):
    return f"        ( piece: {piece + ',':11s} cell: ({cell[0]:3d},{cell[1]:3d}), yaw: {yaw:5.1f} ),"


rows = []
for cell in sorted(boundary):
    if cell in DOOR_KEEP_OUT:
        continue
    dirs = boundary[cell]
    along_x = any(d[0] for d in dirs)   # floor lies east/west -> wall is thin in X
    along_z = any(d[1] for d in dirs)   # floor lies north/south -> wall is thin in Z
    if along_x and along_z:
        # A CORNER: two crossed 1 m panels, not a corner piece.
        #
        # The kit's `WallCorner` is Ozea's `SM_Wall_CornerCap`, whose footprint is 0.22 m — it is a
        # corner POST, so dropped into a 1 m cell it leaves a visible gap to the 1 m runs either side
        # and reads as a free-standing bollard rather than a join. Ozea's true corner is 2 m and does
        # not fit this grid at all. Two `Wall` panels crossing at the cell centre do meet both runs,
        # using only pieces that are already grid-native.
        rows.append(row("Wall", cell, 0.0))
        rows.append(row("Wall", cell, 90.0))
    elif along_x:
        rows.append(row("Wall", cell, 0.0))
    else:
        rows.append(row("Wall", cell, 90.0))

print(f"// {len(rows)} generated perimeter entries")
print("\n".join(rows))
