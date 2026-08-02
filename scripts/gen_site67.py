#!/usr/bin/env python3
"""Generate Site-67's geometry — areas, floor runs, the perimeter wall ring, and every doorway.

# Why this replaces `gen_site_perimeter.py`

That script carried a hand-duplicated copy of `site67.ron`'s `floor:` list with a warning on top of
it: *"nothing checks that the two agree. If they drift, this script generates the perimeter of a
building that does not exist."* Rewriting the layout wholesale is exactly when that drifts, so the
floor list moved HERE and both outputs are now derived from it in one pass. One source of truth.

# The layout, in one paragraph

Three bands joined by two long spines. North is the aperture hall and the containment block — twelve
individual cell ROOMS off their own corridor, six a side. Middle is the working half: records,
research, requisition, briefing. South is where people live. The spines are deliberately long: the
Director asked for a Site three times the size with the growth spent on circulation rather than on
bigger rooms, so every room keeps the footprint (and therefore the dressing density) it already had
and the walk between them is what got longer.

# Doorways are floor, not absence of floor

An interior doorway is ONE cell of floor punched through the wall row between two rects. That makes
the floor contiguous — so `SiteLayout::validate`'s flood-fill still reaches everything — while the
perimeter pass walls the cells either side of it, which is what turns a shared edge into a door
rather than an open side. The old layout had no wall rows between adjacent rects at all, so rooms
opened along their whole shared edge; that is the "the Site's rooms have no doors" property, and it
was inherited from the dungeon's Backrooms art direction (`placement/furnish.rs:531`) rather than
chosen for the hub.

Run from the repo root:  python3 scripts/gen_site67.py
"""

from collections import defaultdict

TILE = 1  # cells are 1 m; kept as a name so the arithmetic below reads as metres

# ── ROOMS ────────────────────────────────────────────────────────────────────────────────────────
# (AreaId, label, x, z, w, h). Sizes are UNCHANGED from the pre-2026-08-02 layout for every room that
# already existed — the growth is in the corridors, so a room's dressing stays as dense as it was.
ROOMS = [
    ("AsyncDoor",   "ASYNC APERTURE", 6,  2, 12, 10),
    ("Monitoring",  "MONITORING",    50,  2,  4, 10),
    ("Records",     "RECORDS",        6, 19,  8,  8),
    ("Research",    "RESEARCH",      20, 19, 12, 10),
    ("Requisition", "REQUISITION",   36, 19,  8,  6),
    ("Briefing",    "BRIEFING",      48, 19, 10,  8),
    ("Quarters",    "QUARTERS",       6, 35,  5,  8),
    ("Kitchen",     "GALLEY",        15, 35,  5,  8),
    ("Activities",  "ACTIVITIES",    24, 35,  5,  8),
    ("WarRoom",     "WAR ROOM",      33, 35,  5,  8),
]

# ── THE CONTAINMENT BLOCK ────────────────────────────────────────────────────────────────────────
# Twelve cell rooms, six either side of their own corridor. Each is 3x3 — big enough to walk into and
# stand beside what is held there, which a 2 m booth never was.
CELL_W = CELL_H = 3
CELL_PITCH = 4                     # 3 of room + 1 of wall
CELL_X0 = 22
CELL_COLS = 6
CELL_NORTH_Z = 2                   # rooms occupy z[2,5)
CELL_CORRIDOR_Z = 6                # corridor occupies z[6,9)
CELL_CORRIDOR_H = 3
CELL_SOUTH_Z = 10                  # rooms occupy z[10,13)

CELL_XS = [CELL_X0 + i * CELL_PITCH for i in range(CELL_COLS)]
# The corridor overhangs the cell run by one cell each end so its own end walls are not a cell's wall.
CELL_CORRIDOR_X0 = CELL_X0 - 1
CELL_CORRIDOR_W = CELL_COLS * CELL_PITCH  # last cell's trailing wall column, plus the leading one

CELLS = []   # (label, x, z, w, h, door_cell, frame_yaw, interior_yaw)
for i, cx in enumerate(CELL_XS):
    # North rank: door in the room's SOUTH wall row (z = CELL_NORTH_Z + 3), opening onto the corridor.
    CELLS.append((f"CELL {i + 1:02d}", cx, CELL_NORTH_Z, CELL_W, CELL_H,
                  (cx + 1, CELL_NORTH_Z + CELL_H), 90.0, 180.0))
for i, cx in enumerate(CELL_XS):
    # South rank: door in the room's NORTH wall row, which is the corridor's south wall row.
    CELLS.append((f"CELL {i + 7:02d}", cx, CELL_SOUTH_Z, CELL_W, CELL_H,
                  (cx + 1, CELL_SOUTH_Z - 1), 90.0, 0.0))

# ── CORRIDORS ────────────────────────────────────────────────────────────────────────────────────
# Every one is an `AreaId::Corridor`, which may be declared more than once — see `SiteLayout::validate`.
CORRIDORS = [
    # THE SERVICE RING, as a U rather than a loop. A facility you can only cross through its own rooms
    # is a comb; a ring means every wing has two ways out. It is open at the NORTH because that edge is
    # the containment block and the aperture hall — the ASYNC door has to sit on the building's outer
    # wall, and putting a corridor outside it would mean walking round the back of the way out.
    ("SERVICE RING",  2, 44, 60,  3),                    # south leg
    ("SERVICE RING",  2,  2,  3, 42),                    # west leg
    ("SERVICE RING", 59,  2,  3, 42),                    # east leg
    # The two spines, four cells wide — enough that two people pass without the camera reading a duct.
    ("SPINE",         5, 14, 54,  4),                    # main spine: north band <-> working half
    ("SPINE",         5, 30, 54,  4),                    # south spine: working half <-> living half
    ("APPROACH",     10, 12,  4,  2),                    # aperture hall down to the main spine
    ("CELL SPUR",    CELL_CORRIDOR_X0 + CELL_CORRIDOR_W + 1, 2, 3, 12),  # cell row + monitoring -> spine
]

# ── DOORWAYS ─────────────────────────────────────────────────────────────────────────────────────
# One cell of FLOOR through a wall row, plus the frame that stands in it. `clearance` is the level a
# person needs to pass; `None` is an unrestricted door.
#
# `yaw` follows the same convention every wall in the kit uses: a frame separating along X is 90.
DOORWAYS = []

def door(x, z, yaw, clearance=None, label=""):
    DOORWAYS.append({"cell": (x, z), "yaw": yaw, "clearance": clearance, "label": label})

# Every cell room. LEVEL 2 — canon puts working proximity to a contained anomaly at Level 2, which is
# the clearance Ito (the containment specialist) and the two researchers hold.
for (label, cx, cz, w, h, dcell, fyaw, _iyaw) in CELLS:
    door(dcell[0], dcell[1], fyaw, clearance="Level2", label=label)

# Onto the cell row at all. Monitoring's own door is unrestricted: Nowak is Level 1, which canon
# describes as working in proximity to contained anomalies with NO access to them — so he can reach
# his camera bank and not the cells, which is the distinction made spatial.
door(CELL_CORRIDOR_X0 + CELL_CORRIDOR_W, 7, 0.0, clearance="Level2", label="CONTAINMENT")
door(CELL_CORRIDOR_X0 + CELL_CORRIDOR_W + 4, 6, 0.0, clearance=None, label="MONITORING")  # x=49, the wall between the spur and the camera bank

# Off the main spine into the working half. Records is RAISA's: Level 2, "a ceiling on information".
door(9, 18, 90.0, clearance="Level2", label="RECORDS")
door(25, 18, 90.0, clearance=None, label="RESEARCH")
door(39, 18, 90.0, clearance=None, label="REQUISITION")
door(52, 18, 90.0, clearance=None, label="BRIEFING")
# Off the south spine into the living half, all unrestricted — a site where people cannot reach their
# own bunks is a prison.
door(8, 34, 90.0, clearance=None, label="QUARTERS")
door(17, 34, 90.0, clearance=None, label="GALLEY")
door(26, 34, 90.0, clearance=None, label="ACTIVITIES")
door(35, 34, 90.0, clearance=None, label="WAR ROOM")
# Research also opens onto the south spine, so the working half is not a dead end.
door(25, 29, 90.0, clearance=None, label="RESEARCH")
# NO door where the ring meets a spine: the west and east legs run flush against both spines' ends,
# so those junctions are already open floor. A doorway cell there would sit INSIDE the spine rect and
# `validate` rejects overlapping areas — which is the check doing its job, since a door through a
# wall that is not there is a door onto nothing. Corridor meeting corridor is an opening, not a door.
door(8, 43, 90.0, clearance=None, label="QUARTERS")
door(35, 43, 90.0, clearance=None, label="WAR ROOM")
# ...and the aperture hall onto the west leg, so arriving from an expedition you are already inside.
door(5, 6, 0.0, clearance=None, label="ASYNC APERTURE")

# ── ASSEMBLY ─────────────────────────────────────────────────────────────────────────────────────
FLOOR = []
AREAS = []
for (aid, label, x, z, w, h) in ROOMS:
    AREAS.append((aid, label, x, z, w, h))
    FLOOR.append((x, z, w, h, label.lower()))
for (label, cx, cz, w, h, _d, _f, _i) in CELLS:
    AREAS.append(("ContainmentCell", label, cx, cz, w, h))
    FLOOR.append((cx, cz, w, h, label.lower()))
# The cell row is the CONTAINMENT wing itself, not a corridor: it is a destination, it keeps the
# per-wing lighting and room tone that were authored for `AreaId::Containment`, and the twelve cells
# open off it.
AREAS.append(("Containment", "CONTAINMENT",
              CELL_CORRIDOR_X0, CELL_CORRIDOR_Z, CELL_CORRIDOR_W, CELL_CORRIDOR_H))
FLOOR.append((CELL_CORRIDOR_X0, CELL_CORRIDOR_Z, CELL_CORRIDOR_W, CELL_CORRIDOR_H, "the cell row"))
for (label, x, z, w, h) in CORRIDORS:
    AREAS.append(("Corridor", label, x, z, w, h))
    FLOOR.append((x, z, w, h, label.lower()))
for d in DOORWAYS:
    FLOOR.append((d["cell"][0], d["cell"][1], 1, 1, f"doorway -> {d['label'].lower()}"))
    AREAS.append(("Corridor", d["label"], d["cell"][0], d["cell"][1], 1, 1))
# Each doorway is one cell of floor, so the flood-fill passes through it — AND a 1x1 area, so no
# walkable cell in the Site belongs to nothing. `area_at` returning `None` means "nowhere", which is a
# real state (the voids between wings) and the wrong one for a threshold you are standing in.
#
# It is a `Corridor` — the one repeatable id, and the right meaning: a threshold is connective tissue.
# Its LABEL is what is on the far side, so the room-name readout announces where the door goes as you
# step through it, which is what a door is for.
cells = set()
for x, z, w, h, _ in FLOOR:
    for i in range(x, x + w):
        for j in range(z, z + h):
            cells.add((i, j))

# The ASYNC aperture's own gap — its frame is placed by the spawner, so no wall may sit on it.
# The aperture sits in the hall's NORTH wall — the building's outermost edge on that side.
ASYNC_FRAME_X = 12
DOOR_KEEP_OUT = {(x, 1) for x in range(ASYNC_FRAME_X, ASYNC_FRAME_X + 2)}

boundary = defaultdict(set)
for (x, z) in cells:
    for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        n = (x + dx, z + dz)
        if n not in cells:
            boundary[n].add((dx, dz))

walled = {c for c in boundary if c not in DOOR_KEEP_OUT}

# Convex corners: cells touching floor ONLY diagonally. Defined from the FLOOR, never from the walls —
# deriving it from "a wall arrives on each axis" walls in every void between rooms and does not
# terminate. See the long note this replaces in `gen_site_perimeter.py`.
corners = set()
for (x, z) in cells:
    for dx, dz in ((1, 1), (1, -1), (-1, 1), (-1, -1)):
        c = (x + dx, z + dz)
        if c in cells or c in walled or c in DOOR_KEEP_OUT:
            continue
        if any((c[0] + ox, c[1] + oz) in cells for ox, oz in ((1, 0), (-1, 0), (0, 1), (0, -1))):
            continue
        corners.add(c)


def wall_rows():
    out = []
    for cell in sorted(set(boundary) | corners):
        if cell in DOOR_KEEP_OUT:
            continue
        if cell in corners:
            yaw = 0.0
        else:
            dirs = boundary[cell]
            # Separating along X wants yaw 0; along Z wants 90. A cell reached on both axes is a
            # concave junction — either orientation caps it, and X is the arbitrary-but-stable pick.
            yaw = 0.0 if ((1, 0) in dirs or (-1, 0) in dirs) else 90.0
        out.append(f"        ( piece: Wall,        cell: ({cell[0]:3d},{cell[1]:3d}), yaw: {yaw:5.1f} ),")
    return out


def connectivity_report():
    """Flood-fill the floor and name anything the operative spawn cannot reach."""
    start = (52, 22)  # the briefing room, where the operatives idle
    if start not in cells:
        return [f"the spawn cell {start} is not floor at all"]
    seen, stack = {start}, [start]
    while stack:
        x, z = stack.pop()
        for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            n = (x + dx, z + dz)
            if n in cells and n not in seen:
                seen.add(n)
                stack.append(n)
    bad = []
    for (aid, label, x, z, w, h) in AREAS:
        if not any((i, j) in seen for i in range(x, x + w) for j in range(z, z + h)):
            bad.append(f"{aid} {label!r} at ({x},{z}) is unreachable")
    return bad


def main():
    area_rows = [
        f'        ( id: {aid + ",":16s} label: {chr(34) + label + chr(34) + ",":18s} '
        f'rect: (x: {x:2d}, z: {z:2d}, w: {w:2d}, h: {h:2d}) ),'
        for (aid, label, x, z, w, h) in AREAS
    ]
    floor_rows = [
        f"        (x: {x:2d}, z: {z:2d}, w: {w:2d}, h: {h:2d}),   // {note}"
        for (x, z, w, h, note) in FLOOR
    ]
    door_rows = [
        f'        ( cell: ({d["cell"][0]:3d},{d["cell"][1]:3d}), yaw: {d["yaw"]:5.1f}, '
        f'clearance: {("Some(" + d["clearance"] + ")") if d["clearance"] else "None":13s}, '
        f'label: {chr(34) + d["label"] + chr(34)} ),'
        for d in DOORWAYS
    ]
    walls = wall_rows()

    xs = [c[0] for c in cells]
    zs = [c[1] for c in cells]
    print(f"// cells: {len(cells)}  walls: {len(walls)}  areas: {len(AREAS)}  doors: {len(DOORWAYS)}")
    print(f"// extent: x[{min(xs)},{max(xs)}]  z[{min(zs)},{max(zs)}]")

    with open("/tmp/site67_gen.txt", "w") as f:
        f.write("AREAS\n" + "\n".join(area_rows))
        f.write("\n\nFLOOR\n" + "\n".join(floor_rows))
        f.write("\n\nDOORS\n" + "\n".join(door_rows))
        f.write("\n\nWALLS\n" + "\n".join(walls) + "\n")
    for line in connectivity_report():
        print(f"// UNREACHABLE: {line}")
    print("// wrote /tmp/site67_gen.txt")


if __name__ == "__main__":
    main()
