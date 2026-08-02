#!/usr/bin/env python3
"""Derive Site-67's perimeter wall ring from its floor rects.

A boundary cell is any non-floor cell orthogonally adjacent to floor. Walls there cannot change
walkability (`is_walkable = is_floor && !wall`, and these are not floor), so the layout validator's
flood-fill is untouched by construction.

Orientation: the promoted `SM_Wall_1x2` is 0.10 m thin along X and 1.00 m long along Z, so a wall
separating along X wants yaw 0 and one separating along Z wants yaw 90.
"""

# ⚠️ THIS IS A HAND-DUPLICATED COPY of `site67.ron`'s `floor:` list, and nothing checks that the two
# agree. If they drift, this script generates the perimeter of a building that does not exist: the
# new floor keeps the OLD room's wall cells sitting on top of it as unwalkable holes, and the walls
# the new floor needs are never emitted. Change one, change the other, in the same commit.
FLOOR = [
    (0, 2, 12, 10),    # async door hall
    (14, 2, 16, 10),   # containment wing
    (30, 2, 4, 10),    # monitoring — beside the cells it watches
    (0, 12, 34, 2),    # the spine
    (0, 14, 8, 8),     # records office
    (8, 14, 2, 10),    # connector: spine -> south spine
    (10, 14, 12, 10),  # research wing
    (24, 14, 8, 6),    # requisition
    (26, 20, 2, 2),    # connector: requisition -> briefing
    (24, 22, 10, 8),   # briefing room
    (0, 24, 24, 2),    # the SOUTH spine
    (0, 26, 5, 4),     # quarters
    (6, 26, 5, 4),     # galley
    (12, 26, 5, 4),    # activities
    (18, 26, 5, 4),    # war room
]

# Cells the ASYNC aperture occupies — its own frame is placed by the spawner, so no wall may sit on
# top of it or the doorway is bricked up.
#
# TWO cells, not four. `doorframe_double.glb` is 2.003 m along its span, so centred on the gap it
# covers exactly x=6..7; holding x=5..8 clear (as this did until 2026-08-01) left a metre of open
# perimeter either side of the frame with nothing standing in it. The flanks are walled by falling
# out of this set, and `SiteLayout::validate` now checks the frame's span against it — see
# `the_async_doorway_gap_matches_the_frame`.
DOOR_KEEP_OUT = {(x, 1) for x in range(6, 8)}

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


walled = {c for c in boundary if c not in DOOR_KEEP_OUT}

# THE OUTSIDE OF EVERY CORNER.
#
# `boundary` only holds cells ORTHOGONALLY adjacent to floor, which is every cell a run passes
# through — and none of the cells a run TURNS at. At a room's outer (convex) corner the diagonal cell
# touches floor only diagonally, so it got no wall, and the two runs stopped 0.5 m short of the point
# where their centrelines cross: an open notch you could see straight through, at all 18 of them.
# Screenshotting the perimeter on 2026-08-01 is what found it; the earlier corner-cap work had capped
# only the 12 CONCAVE junctions, which are the cells that do sit in `boundary`.
#
# A convex corner is a cell that touches floor ONLY diagonally: the runs on both sides of it are
# enclosing the same room, and the cell is the point they turn around.
#
# These diagonal-only cells emit no panel of their own — `site::visuals::wall_panels` keys on floor
# EDGES, and a cell touching floor only diagonally shares no edge with any floor cell. They are kept
# because the perimeter is a statement about which cells are wall (what `is_walkable` and
# `SiteLayout::validate` read), and leaving a room's outside corner out of it would say the corner is
# open when it is not. The visible corner is capped by `site::visuals::corner_vertices`, which finds
# the lattice point where two perpendicular panels already meet — so the cap follows the panels, not
# this list.
#
# **Defined from the FLOOR, never from the walls**, and that is load-bearing. Deriving it instead from
# "a wall arrives on each axis" looks equivalent and is not: adding a wall creates fresh cells that
# satisfy it, and iterating to a fixed point walls in every void BETWEEN rooms — 18 cells becomes 129
# and still climbing. Keying on floor is a fact about the layout, so it is a single pass by
# construction. It deliberately leaves the notches where two DIFFERENT rooms' rings pass each other
# across a void (e.g. the containment wing against the spine); those are outside any room and read as
# the gap between two buildings, which is what they are.
#
# Every cell added here is NON-floor, so `is_walkable = is_floor && !wall` is untouched and the
# layout validator's connectivity flood-fill cannot change.
corners = set()
for (x, z) in cells:
    for dx, dz in ((1, 1), (1, -1), (-1, 1), (-1, -1)):
        c = (x + dx, z + dz)
        if c in cells or c in walled or c in DOOR_KEEP_OUT:
            continue
        # Only diagonal contact — an orthogonal neighbour means a run passes through, not turns.
        if any((c[0] + ox, c[1] + oz) in cells for ox, oz in ((1, 0), (-1, 0), (0, 1), (0, -1))):
            continue
        corners.add(c)

rows = []
for cell in sorted(set(boundary) | corners):
    if cell in DOOR_KEEP_OUT:
        continue
    if cell in corners:
        rows.append(row("Wall", cell, 0.0))
        rows.append(row("Wall", cell, 90.0))
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
