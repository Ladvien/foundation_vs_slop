# 2026-08-01 — the Ozea kit's origins, the wall corners, and the ASYNC door

Resolves two player captures, both `App state: Site`:

- `region_2026-08-01_21-19-22-273` — *"This is what the site looks like."* (baseline)
- `region_2026-08-01_22-09-10-430` — *"Here is the skinned facility. Still needs work. There re white
  squares z-fighting with some grey ones. The corners at the walls are not connected, looks you are
  using a poll instead of a corner piece."*

An earlier capture had already reported *"the north and south wall doesn't square into the
east-and-west walls… looks like the floors point of origin may be off too."*

## What the player saw, and what it actually was

**"Looks like the floor's point of origin may be off too"** was right, and it went much further than
the floor. **Eleven of the sixteen** Ozea meshes carried whatever pivot their source pack happened to
have — walls and floors centre-origined, props base-origined, `SM_Wall_Corner` off-centre by 15 mm in
*two* axes. `site::kit::y_scale` is `target / authored` applied about the entity origin, so a
centre-origined 2.00 m wall asked to reach `WALL_HEIGHT` rendered `Y[-1.2, +1.2]`: half of it
underground, **1.17 m standing against 2.40 m intended**.

Every mesh in `assets/ozea/` is now XZ-centred with its base at `y = 0`, and `wall`, `wall_corner`,
`wall_window` and `column` are re-authored to a true 2.40 m by cutting at a plain band and lengthening
only that — so trim and skirting keep their proportions. Their `y_scale` is now a no-op 1.0, replacing
a 1.2× stretch on the walls and a **2.18×** stretch on the column.

**"White squares z-fighting with some grey ones"** was the floor inlays. The line decals and the
threshold pad are the same 0.05–0.06 m thickness as the floor plate, so with everything seated at
`y = 0` their top faces were *exactly* coplanar and the depth winner was undefined. Fixed
geometrically rather than with a depth bias — coplanar faces are not a precision problem a bias papers
over, they are genuinely the same plane. The plate is sunk (`y_offset: -0.06`) so its top lands on
`y = 0`, and the inlays clear that plane by 2 mm.

**"A poll instead of a corner piece"** was literally true, and the substitution was the cause.
`SM_Wall_CornerCap` is a 0.22 m **post**, not a 2 m corner panel; dropping it in place of the crossed
wall slabs deleted a full metre of wall from each run and left a pole standing in the gap. The cap is
now placed **additively** — the two slabs stay and the cap covers the seam where they meet — at every
junction *derived* from the layout, so a future layout edit cannot forget one.

## The thing nobody had reported: the ASYNC door

Not in either capture, but found while measuring the kit against the layout. Four faults were stacked
on the game's signature image:

| fault | measured |
|---|---|
| Frame turned across its wall run | `doorframe_double.glb` is thin along X and spans Z, exactly like `SM_Wall_1x2`, but the door was authored `yaw: 0.0` while every wall in the `z=1` row it fills is `yaw: 90.0` |
| Frame off the wall line | placed at the *trigger's* position, `(6.5, 2.5)` — a metre out on the hall floor and 0.5 m off the gap's centre |
| Gap left open | 4 m of perimeter held clear for a 2.003 m frame, with nothing in the remaining 2 m |
| Portal quad wrong size and plane | `Rectangle::new(hx*2.0, 2.0)` = **3.2 × 2.0**, sized from the gameplay trigger volume, against a **measured 1.600 × 1.626** clear opening — and 90° out of the frame's plane for *any* yaw, since Bevy's `Rectangle` is XY-plane/+Z-normal while the frame faces ±X |
| No header course | the frame reaches `DOORWAY_HEIGHT` 2.0 and the walls beside it `WALL_HEIGHT` 2.4, leaving a **0.40 × 2.00 m slot straight through the perimeter** above the lintel. Found by screenshotting the fixed door and noticing the wall tops either side sat above the frame |

The aperture material is `AlphaMode::Opaque` **by design** (it must occlude), so the 0.8 m-per-side
overhang was not a soft artifact — it punched an opaque hole through the wall either side of the door.

The header gap is the oldest of the five: `DOORWAY_HEIGHT`'s own doc comment says *"the door tucks
under the header with no gap; the wall runs continuous above it"*. The dungeon honours that. The Site
never had — and `site::pieces` carried a comment claiming a spawner placed a `WallLow` header course
at `y = 2.0`, describing a fix nobody had written. `assets/ozea/wall_header.glb` is now a real piece:
`SM_Wall_1x2` **cropped** to its top 0.40 m, because a header course over a doorway *is* the top of
the wall, so it keeps the wall's own trim at authored size and lines up with the runs either side by
construction rather than by tuning. Nothing is squashed; `y_scale` is 1.0.

## What stops each of these recurring

- `tests/ozea_asset.rs` asserts the base-at-0 / XZ-centred convention across **every** mesh in the
  directory. It previously computed each bounding box and used only `hi - lo`, discarding the minimum
  — and the minimum *is* the origin. That is precisely how eleven files drifted with a green suite.
- `docs/artist_guide.md` §3 gains rule 7 (origin at the base, centred in XZ). It had rules for format,
  axis, scene index and tangents, and nothing about origins.
- The doorway's **clear opening** is now an art fact in the kit (`kit::DoorPiece::opening`), measured
  from the mesh's POSITION accessors, required on both doorway pieces in both kits. The aperture quad
  is built from it instead of from a trigger volume.
- `SiteLayout::validate` refuses a perimeter gap that does not match the frame — wrong width, or a
  frame not centred in it — so the layout and the spawner cannot drift apart again.
- `DoorPlacement` now carries `pos` (trigger, must be reachable floor) and `frame_pos` (the frame, in
  the gap, which is deliberately *not* floor) as separate fields. One field could never have served
  both, and it served the trigger.

## The second pass: what a full visual tour found

The fixes above were verified from two camera angles. That was not enough. A deliberate tour —
seventeen vantage points across every area, plus the floor at maximum zoom — found three more, none
of which any test could have caught.

**The corner fix was half a fix.** `corner_cells` caps a cell carrying *both* a yaw-0 and a yaw-90
wall, which is a **concave** junction — there were 12. Every **convex** corner (each room's outside
corner) had nothing at all: `gen_site_perimeter.py` only walled cells *orthogonally* adjacent to
floor, and a run passes THROUGH those and turns at a *diagonal* one. So the two runs stopped 0.5 m
short of where their centrelines cross, leaving an open notch you could see straight through — **16
of them**, which is the player's "the corners at the walls are not connected" still true at every
outside corner. The generator now also emits crossed slabs at cells touching floor only diagonally,
and because such a cell then carries both yaws, `corner_cells` caps it for free: **28 junctions, up
from 12.**

⚠️ The obvious criterion — "a wall arrives on each axis" — is a trap. It is satisfied by cells the
fix itself creates, so iterating to a fixed point walls in every void *between* rooms: 18 cells
becomes 129 and keeps climbing. Keying on the **floor** instead is a fact about the layout and
terminates in one pass by construction.

**The aperture was single-sided.** From the Q/E detents that look at the Site from outside, the ASYNC
door was a see-through hole in the exterior wall with the hall floor visible through it. `Material`
has no `cull_mode` hook, so `specialize` now clears `primitive.cull_mode`.

**The operatives were in the bind pose.** Each carried what looked like a 4 m lance run through their
chest. The mesh was never wrong — `rifle` measures **0.902 m** composed through its entire node chain
— it is a non-skinned child of `hand_r`, and in the rest pose that arm is straight out, so the rifle
sat level at chest height a metre to the side. Nothing animated Site avatars at all. They now carry
`anim::BlendSource` on the model child and a `drive_avatar_animation` driver, the same seam the squad
uses, so they idle and walk.

## The third pass: the two content gaps

**Containment cells were bare panes.** `site67.ron`'s `cells:` authors one `WallWindow` and nothing
around it, so a wing holding nothing read as six sheets of glass on an open deck — which undercuts
FVS-D-4, since "a rack of the things you brought home" only lands if a cell looks like one when it is
empty. `enclose_containment_cell` now derives a booth (two sides and a back) from the cell's own
placement, the same way `corner_cells` derives its caps, so a seventh cell gets one for free. The
occupant also moved: it was spawned at the cell entity's origin, which is exactly where the GLASS is,
so a held specimen stood inside the pane it is meant to be seen through.

**The research wing had no slab.** `research::lab::StudySubject` is documented as "the specimen
currently on the slab" and the HUD says *NO SPECIMEN ON THE SLAB — CONTAIN ONE FIRST*, but no slab
existed anywhere in the world and the wing was bare floor. `assets/ozea/slab.glb`
(`SM_MedPod_Treatment_Bed`, from the same pack as the cryo pod) is now its centrepiece, and
`lay_out_the_study_subject` puts the studied specimen on it — bed platform measured at y = 0.60, not
guessed. It adds no gameplay rule: the subject is still chosen by `keep_a_study_subject`; this only
shows what that already decided.

## The fourth pass: the corners were still wrong, in the other direction

The player, on the closed corners: *"where two of them meet, it feels like they overlap by about
1/3."* They were right, and it is the older half of the corner design rather than anything the convex
fix introduced.

A junction cell carried **two full 1 m panels crossed at its centre** — and the corner point IS that
centre, so each panel ran **0.50 m past the other**. The runs met in a **plus, not an L**, and each
left a half-panel stub jutting into open space: outward at a convex corner, straight into the walkable
opening at a concave one. Two adjacent junctions made it worse — two parallel stubs with a doubled
section between them, which is exactly what "overlap" describes.

Full panels cannot build this corner. The corner point is a cell CENTRE, so a piece that stops there
has to be half a cell long. `assets/ozea/wall_leg.glb` is that piece: `SM_Wall_1x2` cropped along its
LENGTH at the panel's plain midpoint (the length detail sits at |z| 0.18–0.25 and 0.40–0.50, so the
cut misses every groove), and given the same 2.40 m re-authoring the full panel gets so a leg and a
run are the same wall.

A junction now places **one leg per direction the perimeter actually continues in** — two for an L,
three for a T, four for a crossing — each reaching from the corner point to the cell edge where the
next full panel begins. Nothing overshoots and nothing is left out. `site67.ron` still records a wall
on those cells, because that is what `is_walkable` and the validator read; only the *rendering*
changed.

## Knowingly left

- **`wall_low` is still squashed** 2.00 → 0.9 (0.45×). Shortening a panel properly is an authoring
  decision, not a conversion flag — `scripts/ozea_wall_heights.py` only lengthens. It is now actually
  *placed*, though: it was dressed, target-heighted, validated and spawned nowhere, so the Records desk
  and Requisition counter its own kit entry describes had no author until now.
- **The aperture's shader** (FVS-G-5). Its uniforms were authored blind and are still guesses. Worth
  noting the geometry fix helps it: the shader marches on `uv.x` with no aspect uniform, so the old
  3.2 × 2.0 quad stretched the corridor illusion 1.6:1 and the corrected one is nearly square.
- **The Ozea doorways do not clear a 1.82 m operative** — the wide frame's hole is 1.64 m rendered.
  Accepted rather than overlooked: it is a portal whose trigger fires before an avatar reaches the
  frame, and the alternative was scaling the frame 23% in Y alone, which stretches its trim. The test
  that claimed to check this was measuring the frame's *outline*, not the hole, and now measures the
  hole and states the real number.
