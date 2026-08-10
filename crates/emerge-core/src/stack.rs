//! **What rests on what, and how high that puts it.**
//!
//! A lamp on a table stands at the table's height. That sentence was expressible in the schema —
//! [`Mount::OnSurface`] has been there since the descriptor was designed, and ten shipped descriptors
//! declare it — and it was implemented nowhere. `origin_of` matched `OnWall` and `OnCeiling` and sent
//! everything else to a `_ => 0.0` arm, so every lamp, plant, television and globe in the library was
//! placed on the floor. The editor drew it on the floor and the game agreed, which is the failure mode
//! a shared spawner produces when the thing it shares is wrong: consistent and wrong beats
//! inconsistent, but not by much.
//!
//! # A reference, not a height
//!
//! [`crate::map::Placed::on`] names the placement underneath rather than storing a Y. Move the table
//! and the lamp moves with it. A stored height is correct until someone drags the host and then wrong
//! in a way that reads as bad authoring rather than as stale data — the same argument
//! [`crate::map::Placed::id`] makes for being a string instead of an index.
//!
//! This is Tutenel et al.'s *semantic class* relation rather than geometry: a piece declares the
//! surface **class** it needs (`"worktop"`), a host declares the classes it **offers**, and matching
//! is by class. `emerge_core::vocab` already turns both sides into bitmasks — Game AI Pro 4 ch.4's
//! *"comparing these bitmasks is a very efficient way to filter out invalid links"* — so the check
//! costs one `&`.
//!
//! # Height is measured, not authored
//!
//! The top of a piece is its origin plus its drawn height, and *drawn* is the operative word:
//! `align.scale` and `align.stretch_y` are applied to the entity, so a table measured at 0.796 m and
//! scaled 1.2 presents its surface at 0.955 m. Reading `extent.height` alone would put the lamp inside
//! the tabletop on every scaled piece, and nothing downstream would say so.
//!
//! That rule is [`crate::descriptor::placed_height`]. It used to be `drawn_height`, defined here —
//! which was fine for stacking and wrong for everyone else, because `divisions` and `adjacency` wrote
//! the product out by hand and dropped the `scale` factor. One definition now, in the schema layer
//! both can reach.

use crate::descriptor::{placed_footprint, placed_height, Descriptor, Mount, OverlayHost};
use crate::library::Library;
use crate::map::{Map, Placed};

/// The surface class a descriptor needs under it, if it needs one.
pub fn needs_surface(d: &Descriptor) -> Option<&str> {
    match &d.mount {
        Some(Mount::OnSurface { class }) => Some(class.as_str()),
        _ => None,
    }
}

/// Whether `host` offers the surface `guest` asks for.
///
/// String comparison here rather than the resolved masks because the editor works with a candidate
/// that has not been through `Vocabularies::masks` yet. The vocabulary check still happens — at
/// library load, where a misspelled class is refused for everyone at once rather than by failing to
/// stack.
pub fn offers_for(host: &Descriptor, guest: &Descriptor) -> bool {
    match needs_surface(guest) {
        Some(class) => host.offers.surfaces.iter().any(|s| s == class),
        None => false,
    }
}

/// Whether a point in map space falls inside a placed piece's footprint.
///
/// Yaw-aware: the probe is rotated into the piece's own frame before the half-extent test, so a table
/// turned 90° is tested as the 0.8 × 1.6 rectangle it now occupies rather than the 1.6 × 0.8 one it
/// was measured as. The flood fill learned this the expensive way — it used `max(w, d)` on both axes
/// and striped the floor.
///
/// A piece with no measured footprint covers nothing. It is unmeasured, not flat, and treating unknown
/// as "everywhere" would let a lamp land on a mystery.
pub fn covers(d: &Descriptor, placed_at: (f32, f32), yaw: f32, probe: (f32, f32)) -> bool {
    let Some((w, depth)) = placed_footprint(d) else {
        return false;
    };
    let (dx, dz) = (probe.0 - placed_at.0, probe.1 - placed_at.1);
    // Into the piece's frame: rotate by -yaw. Bevy's yaw is about +Y, so a positive yaw turns +X
    // toward -Z; the inverse used here is the transpose of that rotation.
    let (s, c) = (-yaw).to_radians().sin_cos();
    let local_x = dx * c + dz * s;
    let local_z = -dx * s + dz * c;
    local_x.abs() <= w * 0.5 && local_z.abs() <= depth * 0.5
}

/// The placement under `probe` that can hold `guest` up, and the world Y of its surface.
///
/// **The highest one wins**, so a lamp dropped over a shelf standing on a table lands on the shelf.
/// Ties break on the placement id — unique within a map, so the answer is total and does not depend on
/// authoring order. This project's determinism lint exists because a "stable enough" key is how the
/// same map came out two ways.
pub fn host_under<'a>(
    map: &'a Map,
    library: &Library,
    y: &[f32],
    guest: &Descriptor,
    probe: (f32, f32),
) -> Option<(&'a Placed, f32)> {
    needs_surface(guest)?;
    let mut best: Option<(&Placed, f32)> = None;
    for (i, p) in map.placements.iter().enumerate() {
        let (Some(d), Some(&py)) = (library.get(&p.descriptor), y.get(i)) else {
            continue;
        };
        // A tipped piece offers no surface: "where is the tabletop of a table on its side" has no
        // answer worth inventing, and its recorded footprint no longer describes what it covers.
        if p.tip != (0, 0) || !offers_for(d, guest) || !covers(d, p.at, p.yaw, probe) {
            continue;
        }
        let Some(top) = placed_height(d).map(|h| py + h) else {
            continue;
        };
        let better = match best {
            None => true,
            Some((bp, by)) => (top, p.id.as_str()) > (by, bp.id.as_str()),
        };
        if better {
            best = Some((p, top));
        }
    }
    best
}

/// The world Y of every placement's origin, in map order.
///
/// Resolved together rather than one at a time because a guest's height is its host's height, and the
/// host may be authored anywhere in the file. [`Map::validate`] has already refused cycles and dangling
/// hosts, so the walk below terminates; it re-checks anyway, because a caller that skipped validation
/// deserves an error rather than a hang.
pub fn resolve_y(map: &Map, library: &Library) -> Result<Vec<f32>, String> {
    let mut out = vec![f32::NAN; map.placements.len()];
    let mut done = vec![false; map.placements.len()];
    for i in 0..map.placements.len() {
        resolve_one(map, library, i, &mut out, &mut done, &mut Vec::new())?;
    }
    Ok(out)
}

/// One placement's Y, resolving its host first. `path` carries the chain being walked so a loop names
/// itself rather than recursing until the stack runs out.
fn resolve_one(
    map: &Map,
    library: &Library,
    i: usize,
    out: &mut [f32],
    done: &mut [bool],
    path: &mut Vec<usize>,
) -> Result<f32, String> {
    if done[i] {
        return Ok(out[i]);
    }
    if path.contains(&i) {
        return Err(format!(
            "map: placement `{}` rests on itself through a loop — nothing in the chain reaches the \
             floor",
            map.placements[i].id
        ));
    }
    let p = &map.placements[i];
    let d = library.get(&p.descriptor).ok_or_else(|| {
        format!(
            "map: placement `{}` names descriptor `{}`, which this library does not define",
            p.id, p.descriptor
        )
    })?;

    // The surface underneath, resolved first — the only part of the datum that depends on another
    // placement, and so the only part that can recurse.
    let host_top = match &d.mount {
        Some(Mount::OnSurface { class }) => {
            let host_id = p.on.as_ref().ok_or_else(|| {
                format!(
                    "map: placement `{}` mounts on a `{class}` surface but records nothing under it. \
                     Placing it at floor level is how a lamp ends up inside the floor — put it on a \
                     piece that offers `{class}`, or change the descriptor's layer.",
                    p.id
                )
            })?;
            let hi = map
                .placements
                .iter()
                .position(|q| &q.id == host_id)
                .ok_or_else(|| {
                    format!(
                        "map: placement `{}` rests on `{host_id}`, which does not exist",
                        p.id
                    )
                })?;
            let host = &map.placements[hi];
            let host_d = library.get(&host.descriptor).ok_or_else(|| {
                format!(
                    "map: host `{}` names descriptor `{}`, which this library does not define",
                    host.id, host.descriptor
                )
            })?;
            let lift = surface_of(host_d, d, &host.id, &p.id)?;
            path.push(i);
            let host_y = resolve_one(map, library, hi, out, done, path)?;
            path.pop();
            Some(host_y + lift)
        }
        _ => None,
    };

    // The authored lift rides on top of whatever the layer decided — and because it lands in
    // `out`, a guest resolving through its host inherits the host's lift for free: raise the
    // table and the lamp comes with it.
    out[i] = datum(map, d, host_top, &p.id)? + p.lift;
    done[i] = true;
    Ok(out[i])
}

/// How far above its own origin a host presents its surface — after checking it is entitled to.
fn surface_of(host_d: &Descriptor, guest: &Descriptor, host_id: &str, guest_id: &str) -> Result<f32, String> {
    let class = needs_surface(guest).unwrap_or("");
    if !offers_for(host_d, guest) {
        return Err(format!(
            "map: `{guest_id}` needs a `{class}` surface and rests on `{host_id}`, which offers {}. \
             A piece that does not offer the class cannot hold it up.",
            if host_d.offers.surfaces.is_empty() {
                "none".to_owned()
            } else {
                host_d.offers.surfaces.join(", ")
            }
        ));
    }
    placed_height(host_d).ok_or_else(|| {
        format!(
            "map: `{guest_id}` rests on `{host_id}`, whose descriptor records no height. An \
             unmeasured piece cannot say where its surface is — measure `{}` in the tiles tab.",
            host_d.id
        )
    })
}

/// **The layer decides the height.** One function, so the editor's ghost and the game's spawner cannot
/// come to different answers about where a piece goes.
///
/// Exhaustive on purpose: the arm that used to be `_ => 0.0` is what silently floored every
/// `OnSurface` piece in the library, and a wildcard here means the next mount variant repeats it
/// without a compile error.
///
/// `host_top` is the world Y of the surface underneath, and is required by exactly one layer.
pub fn datum(
    map: &Map,
    d: &Descriptor,
    host_top: Option<f32>,
    who: &str,
) -> Result<f32, String> {
    let base = match &d.mount {
        // The map's own floor. A door fills its hole from the floor up, and the hole starts there.
        None | Some(Mount::OnFloor) | Some(Mount::Tiled) | Some(Mount::InOpening { .. }) => {
            map.origin.1
        }
        Some(Mount::OnWall { height }) => map.origin.1 + height,
        // **The map's ceiling, not a constant.** This was hardcoded 2.4 m, which hung the lights of a
        // 3.5 m room in mid-air. The map states its own height; that is the only number entitled to
        // answer this.
        Some(Mount::OnCeiling) => map.origin.1 + map.bounds.1,
        Some(Mount::OnSurface { class }) => host_top.ok_or_else(|| {
            format!("`{who}` needs a `{class}` surface and there is none under it")
        })?,
        // A decal lies on the plane it names. The floor and the ceiling are the map's to state; a
        // wall's height is nobody's, so `OverlayHost::Wall` carries it — this used to be the one arm
        // with no answer, and it returned an error rather than invent a number.
        Some(Mount::Overlay { on }) => match on {
            OverlayHost::Floor => map.origin.1,
            OverlayHost::Ceiling => map.origin.1 + map.bounds.1,
            OverlayHost::Wall { height } => map.origin.1 + height,
        },
    };
    // A geometric correction on the mesh, applied on top of whatever the layer decided rather than
    // instead of it: a floor grate is 6 cm into its floor, and into a tabletop too if it sat on one.
    Ok(base + d.align.y_offset.unwrap_or(0.0))
}

/// Where a piece **would** go if it were dropped at `probe` right now, and what it would rest on.
///
/// The ghost's question and the click's question are the same question, so they ask it once. A piece
/// that needs a surface and finds none is an `Err` rather than a piece at floor level — the editor
/// shows the sentence and declines to place it.
pub fn placement_at<'a>(
    map: &'a Map,
    library: &Library,
    y: &[f32],
    d: &Descriptor,
    probe: (f32, f32),
) -> Result<(f32, Option<&'a Placed>), String> {
    let host = host_under(map, library, y, d, probe);
    if needs_surface(d).is_some() && host.is_none() {
        let class = needs_surface(d).unwrap_or("");
        return Err(format!(
            "`{}` goes on a `{class}` surface — there is none here. Put it on something that offers \
             one.",
            d.id
        ));
    }
    let at = datum(map, d, host.map(|(_, top)| top), &d.id)?;
    Ok((at, host.map(|(p, _)| p)))
}

/// **How deep two footprints must interpenetrate before they are "overlapping"**, metres.
///
/// Not zero, and the size is the point: kitbashing wants pieces laid *flush* — wall segment against
/// wall segment, exactly end to end — and flush pieces touch. An exact test would refuse the very
/// gesture the overlap rule exists to protect, and float noise would refuse it intermittently,
/// which is worse. A centimetre of tolerated lap is invisible at authoring scale and makes
/// touching unambiguous.
pub const OVERLAP_EPS: f32 = 0.01;

/// A placed piece's plan rectangle: centre, its two axis directions at this yaw, and half-extents —
/// tips folded in via [`crate::descriptor::tipped_extents`], so a wall lying on its side reserves
/// the long low box it actually fills. `None` when unmeasured, on [`covers`]' own rule: unknown is
/// not flat, and unknown is not everywhere either.
fn plan_box(d: &Descriptor, at: (f32, f32), yaw: f32, tip: (u8, u8)) -> Option<((f32, f32), [(f32, f32); 2], (f32, f32))> {
    let (w, _h, depth) = crate::descriptor::tipped_extents(d, tip)?;
    let (s, c) = yaw.to_radians().sin_cos();
    Some((at, [(c, -s), (s, c)], (w * 0.5, depth * 0.5)))
}

/// Do two plan rectangles interpenetrate by more than [`OVERLAP_EPS`]?
///
/// Separating-axis test over the four face normals — exact for any yaw, which matters because yaw
/// is free degrees here, not the quarter turns a fill rounds to.
fn plans_overlap(
    a: ((f32, f32), [(f32, f32); 2], (f32, f32)),
    b: ((f32, f32), [(f32, f32); 2], (f32, f32)),
) -> bool {
    let (ac, aa, ah) = a;
    let (bc, ba, bh) = b;
    let d = (bc.0 - ac.0, bc.1 - ac.1);
    let dot = |u: (f32, f32), v: (f32, f32)| u.0 * v.0 + u.1 * v.1;
    for n in [aa[0], aa[1], ba[0], ba[1]] {
        let dist = dot(d, n).abs();
        let ra = ah.0 * dot(aa[0], n).abs() + ah.1 * dot(aa[1], n).abs();
        let rb = bh.0 * dot(ba[0], n).abs() + bh.1 * dot(ba[1], n).abs();
        if dist >= ra + rb - OVERLAP_EPS {
            return false;
        }
    }
    true
}

/// Do two pieces contest the same space at all?
///
/// The layers are the schema's own, not judgement: two floor-standing pieces share the floor; two
/// wall pieces share a wall only at the same height; two surface pieces contest only the same host
/// (`on`); [`Mount::Tiled`] is its own stratum, which is what keeps a floor tile from "blocking"
/// the crate standing on it — laying floor under a dressed room is `box_fill`'s documented
/// ordinary case. `Overlay` never participates: *"claims no volume"* is its own doc's promise.
fn same_layer(a: &Descriptor, a_on: Option<&str>, b: &Descriptor, b_on: Option<&str>) -> bool {
    use Mount::*;
    match (a.mount.as_ref(), b.mount.as_ref()) {
        (None | Some(OnFloor), None | Some(OnFloor)) => true,
        (Some(Tiled), Some(Tiled)) => true,
        (Some(OnCeiling), Some(OnCeiling)) => true,
        (Some(InOpening { .. }), Some(InOpening { .. })) => true,
        (Some(OnWall { height: h1 }), Some(OnWall { height: h2 })) => (h1 - h2).abs() < 1e-3,
        (Some(OnSurface { .. }), Some(OnSurface { .. })) => match (a_on, b_on) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        _ => false,
    }
}

/// **The placement already occupying the space `d` would take at `at`** — the overlap rule.
///
/// A placement is refused, not layered, when its plan rectangle interpenetrates an existing
/// placement's on the same layer: kitbashing wants pieces flush, and a mesh hidden inside another
/// is the kind of authoring accident that is only found by counting draw calls. Flush is legal by
/// [`OVERLAP_EPS`]; different layers pass each other by [`same_layer`]'s table.
///
/// The first blocker **in map order** is returned — file order, stable for a status line. `on` is
/// the host the new piece would rest on, so two lamps contest one table but not two tables.
pub fn blocking<'a>(
    map: &'a Map,
    library: &Library,
    d: &Descriptor,
    at: (f32, f32),
    yaw: f32,
    tip: (u8, u8),
    on: Option<&str>,
) -> Option<&'a Placed> {
    let guest = plan_box(d, at, yaw, tip)?;
    map.placements.iter().find(|p| {
        let Some(pd) = library.get(&p.descriptor) else {
            return false;
        };
        if !same_layer(d, on, pd, p.on.as_deref()) {
            return false;
        }
        match plan_box(pd, p.at, p.yaw, p.tip) {
            Some(b) => plans_overlap(guest, b),
            None => false,
        }
    })
}

/// What a move changed, so the caller can respawn exactly those entities and undo exactly that edit.
///
/// One record per placement that actually moved — the piece plus everything that was riding on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    /// `(index, where it was, what it rested on)`, in the order they were written. Restoring these in
    /// order puts the map back exactly.
    pub was: Vec<(usize, (f32, f32), Option<String>)>,
}

/// **Move a placement to `to`, taking everything resting on it along.**
///
/// Move the table and the lamp goes with it. That is already the promise [`crate::map::Placed::on`]
/// makes — it names the host rather than storing a height *"so move the table and the lamp moves with
/// it"* — but nothing had ever moved a table, so the promise had never been kept by anything.
///
/// # All of it, or none of it
///
/// Every member is re-seated through [`placement_at`], the same call the click path makes, so a move
/// obeys the same mount rules as a placement. If any member cannot be seated — a lamp dragged somewhere
/// its host no longer covers, a shelf pushed off the wall — **the whole move is refused and the map is
/// untouched**. A partial move would leave a `Placed::on` pointing at a piece that is no longer under
/// it, which is exactly the dangling reference [`Map::validate`] exists to refuse, written by the
/// editor rather than by an author.
///
/// That is why the work happens on a clone. Applying in place and rolling back on failure is two code
/// paths that have to agree about what "back" means, and the rollback is the one that never gets run.
///
/// # Riders are found transitively, in a total order
///
/// A mug on a tray on the table moves with the table. Collection is breadth-first over `Placed::on`
/// and the result is sorted by `Placed::id` — unique within a map — so the write order does not depend
/// on authoring order. `Map::validate` has already refused cycles, and the `seen` set means a
/// malformed map still terminates here rather than hanging.
pub fn move_placement(
    map: &mut Map,
    library: &Library,
    index: usize,
    to: (f32, f32),
) -> Result<Moved, String> {
    let anchor = map.placements.get(index).ok_or_else(|| {
        format!(
            "move: no placement at index {index}; the map has {}",
            map.placements.len()
        )
    })?;
    let from = anchor.at;
    let delta = (to.0 - from.0, to.1 - from.1);
    let group = group_of(map, index);

    // Proposed, on a copy. Nothing below can leave `map` half-written.
    let mut next = map.clone();
    for &i in &group {
        let p = &mut next.placements[i];
        p.at = (p.at.0 + delta.0, p.at.1 + delta.1);
    }

    // **Only the anchor is re-seated; the riders keep their host.**
    //
    // The group moves rigidly, so every rider is at the same offset from the same host it was already
    // on and its mount holds for exactly the reason it held before. Re-asking `placement_at` for them
    // would be worse than redundant: `host_under` does not know to skip the piece it is seating, so a
    // tray would find *itself* under itself, and a mug would hop onto whatever the group happened to
    // pass over. The anchor is the only member whose surroundings actually changed.
    let anchor_d = library.get(&next.placements[index].descriptor).ok_or_else(|| {
        format!(
            "move: `{}` names descriptor `{}`, which the library does not have",
            next.placements[index].id, next.placements[index].descriptor
        )
    })?;
    // **Asked against a map the group is not in.** "What would be under this if the things travelling
    // with it were not there" is the honest question — otherwise a table lands on its own mug, which
    // is the cycle `resolve_y` then refuses.
    let mut probe = next.clone();
    let mut drop_order = group.clone();
    drop_order.sort_unstable();
    for &i in drop_order.iter().rev() {
        probe.placements.remove(i);
    }
    let probe_ys = resolve_y(&probe, library)?;
    let (_, host) = placement_at(
        &probe,
        library,
        &probe_ys,
        anchor_d,
        next.placements[index].at,
    )?;
    let anchor_on = host.map(|h| h.id.clone());

    // Everything held. Record what it was, then commit.
    let mut was = Vec::with_capacity(group.len());
    for &i in &group {
        was.push((i, map.placements[i].at, map.placements[i].on.clone()));
        map.placements[i].at = next.placements[i].at;
        if i == index {
            map.placements[i].on = anchor_on.clone();
        }
    }
    Ok(Moved { was })
}

/// **A placement and everything riding on it**, transitively — the set a move carries.
///
/// A mug on a tray on the table is in here, found by walking `Placed::on` breadth-first. Shared with
/// the editor, which needs the same set *before* the move so it can take the whole group out of the
/// map while it is in hand; computing it twice is how the thing you see picked up comes to differ
/// from the thing that lands.
///
/// The result is sorted by `Placed::id` — unique within a map, which `pick_at` already leans on — so
/// it does not depend on which order the walk happened to pop. `Map::validate` has already refused
/// cycles, and the `seen` set means a malformed map terminates here rather than hanging.
///
/// Empty for an index the map does not have, rather than a panic: the editor holds a placement across
/// frames in which an undo can remove it.
pub fn group_of(map: &Map, index: usize) -> Vec<usize> {
    if index >= map.placements.len() {
        return Vec::new();
    }
    let mut group = vec![index];
    let mut seen: Vec<usize> = vec![index];
    let mut frontier = vec![index];
    while let Some(host) = frontier.pop() {
        let host_id = map.placements[host].id.clone();
        for (i, p) in map.placements.iter().enumerate() {
            if seen.contains(&i) || p.on.as_deref() != Some(host_id.as_str()) {
                continue;
            }
            seen.push(i);
            group.push(i);
            frontier.push(i);
        }
    }
    group.sort_by(|a, b| map.placements[*a].id.cmp(&map.placements[*b].id));
    group
}

/// Put back exactly what [`move_placement`] recorded. The inverse, and the editor's undo.
pub fn restore_moved(map: &mut Map, moved: &Moved) {
    for (i, at, on) in &moved.was {
        if let Some(p) = map.placements.get_mut(*i) {
            p.at = *at;
            p.on = on.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every `Mount` variant contests itself, unless it is exempt on purpose.**
    ///
    /// [`same_layer`] ends in `_ => false`, so a variant added tomorrow contests **nothing —
    /// including another of itself** until somebody remembers to add an arm. It fails open, in the
    /// direction of "the edit is allowed", and silently.
    ///
    /// The hole is invisible without this test because the one variant that *should* fall through
    /// does: `Overlay` claims no volume by design, so the fall-through looks deliberate for every
    /// variant. Naming the exemption is what separates "decided" from "not yet written".
    ///
    /// Exhaustive by construction — the `match` below has no wildcard, so a new variant does not
    /// compile until its author has answered the question.
    #[test]
    fn every_mount_variant_contests_itself_or_says_why_not() {
        use crate::descriptor::{Extent, Mount, OverlayHost};

        let every = [
            Mount::OnFloor,
            Mount::OnWall { height: 1.8 },
            Mount::OnCeiling,
            Mount::InOpening { clear: None },
            Mount::OnSurface { class: "worktop".to_owned() },
            Mount::Overlay { on: OverlayHost::Floor },
            Mount::Tiled,
        ];

        for mount in every {
            // Answered per variant, with no wildcard: adding a `Mount` breaks this line first.
            let (exempt, why) = match &mount {
                Mount::Overlay { .. } => (
                    true,
                    "claims no volume — two decals may share a wall, which is the point of them",
                ),
                Mount::OnFloor
                | Mount::OnWall { .. }
                | Mount::OnCeiling
                | Mount::InOpening { .. }
                | Mount::OnSurface { .. }
                | Mount::Tiled => (false, ""),
            };

            let d = Descriptor {
                id: "probe".to_owned(),
                extent: Extent { footprint: Some((1.0, 1.0)), height: Some(1.0) },
                mount: Some(mount.clone()),
                ..Descriptor::default()
            };
            // `OnSurface` contests only pieces on the SAME host, so the probe names one.
            let host = matches!(mount, Mount::OnSurface { .. }).then_some("table@1");
            let contests = same_layer(&d, host, &d, host);

            if exempt {
                assert!(
                    !contests,
                    "`{mount:?}` is exempt because it {why}, but it contests itself"
                );
            } else {
                assert!(
                    contests,
                    "`{mount:?}` does not contest another of itself, so two of them may occupy one \
                     space and nothing will refuse it. Add an arm to `same_layer`, or exempt it here \
                     with a reason."
                );
            }
        }
    }

    use super::*;
    use crate::descriptor::{Align, Extent, Offers};
    use crate::library::LIBRARY_VERSION;
    use crate::map::Placed;

    fn table() -> Descriptor {
        Descriptor {
            id: "table".into(),
            extent: Extent {
                footprint: Some((1.6, 0.8)),
                height: Some(0.8),
            },
            offers: Offers {
                surfaces: vec!["worktop".into()],
                sockets: Vec::new(),
            },
            mount: Some(Mount::OnFloor),
            ..Descriptor::default()
        }
    }

    fn lamp() -> Descriptor {
        Descriptor {
            id: "lamp".into(),
            extent: Extent {
                footprint: Some((0.3, 0.3)),
                height: Some(0.5),
            },
            mount: Some(Mount::OnSurface {
                class: "worktop".into(),
            }),
            ..Descriptor::default()
        }
    }

    fn lib(descriptors: Vec<Descriptor>) -> Library {
        Library {
            version: LIBRARY_VERSION,
            note: None,
            descriptors,
        }
    }

    fn at(id: &str, descriptor: &str, on: Option<&str>) -> Placed {
        Placed {
            id: id.into(),
            descriptor: descriptor.into(),
            at: (0.0, 0.0),
            on: on.map(str::to_owned),
            ..Placed::default()
        }
    }

    fn map(placements: Vec<Placed>) -> Map {
        Map {
            name: "test_map".into(),
            placements,
            ..Map::default()
        }
    }

    /// A table with a lamp and a mug on it, and a second table nowhere near them.
    fn stacked() -> (Map, Library) {
        let mut m = map(vec![
            at("t1", "table", None),
            at("l1", "lamp", Some("t1")),
            at("m1", "lamp", Some("t1")),
            at("t2", "table", None),
        ]);
        m.placements[3].at = (10.0, 10.0);
        // Both guests sit within the table's 1.6 x 0.8 footprint, offset so the move can be seen to
        // preserve the offsets rather than to collapse them onto the host.
        m.placements[1].at = (0.4, 0.2);
        m.placements[2].at = (-0.4, -0.2);
        (m, lib(vec![table(), lamp()]))
    }

    /// **Move the table and the lamp moves with it** — the promise `Placed::on`'s own doc makes, which
    /// nothing had ever kept because nothing could move a table.
    #[test]
    fn moving_a_host_carries_everything_resting_on_it() {
        let (mut m, l) = stacked();
        let moved = move_placement(&mut m, &l, 0, (3.0, 0.0)).unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(m.placements[0].at, (3.0, 0.0), "the table went where it was sent");
        // Offsets preserved, not collapsed.
        assert_eq!(m.placements[1].at, (3.4, 0.2));
        assert_eq!(m.placements[2].at, (2.6, -0.2));
        // Still resting on the same host, which is what makes them still land at its height.
        assert_eq!(m.placements[1].on.as_deref(), Some("t1"));
        assert_eq!(m.placements[2].on.as_deref(), Some("t1"));
        let y = resolve_y(&m, &l).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y[1], 0.8, "the lamp is still on the tabletop");

        // The untouched table stayed untouched, and is not in the record.
        assert_eq!(m.placements[3].at, (10.0, 10.0));
        assert_eq!(moved.was.len(), 3, "table plus two riders: {:?}", moved.was);
    }

    /// **All of it or none of it.** A piece dragged where its mount cannot hold refuses the whole
    /// move, rather than landing at floor level and reading as an authoring mistake — the same
    /// refusal the click path makes, and the reason the work happens on a clone.
    #[test]
    fn a_move_that_cannot_be_seated_changes_nothing() {
        let (mut m, l) = stacked();
        let before = m.clone();

        // The lamp is `OnSurface { worktop }`. Off the table there is no worktop, so there is nowhere
        // for it to go.
        let err = move_placement(&mut m, &l, 1, (40.0, 40.0))
            .expect_err("a piece with nowhere to rest must refuse the move");
        assert!(err.contains("worktop"), "the refusal names the surface: {err}");
        assert_eq!(m, before, "a refused move must leave the map byte-identical");
    }

    /// **A host does not come to rest on its own guest.** Re-seating the anchor asks what would be
    /// under it if the things travelling with it were not there; asking against the moved map instead
    /// let a table find the mug standing on it, and `resolve_y` then refused the cycle the move had
    /// just written.
    #[test]
    fn a_moved_host_does_not_land_on_the_things_it_is_carrying() {
        let (mut m, l) = stacked();
        move_placement(&mut m, &l, 0, (3.0, 0.0)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            m.placements[0].on, None,
            "the table stands on the floor, not on its own lamp"
        );
        // And the map still resolves, which is the property a cycle would have destroyed.
        resolve_y(&m, &l).unwrap_or_else(|e| panic!("{e}"));
    }

    /// Moving a *rider* off its host and onto another one re-seats it — the anchor is the member
    /// whose surroundings changed, so it is the one that gets a new `on`.
    #[test]
    fn moving_a_guest_onto_another_host_repoints_it() {
        let (mut m, l) = stacked();
        // `t2` is the far table; the lamp lands on it.
        move_placement(&mut m, &l, 1, (10.0, 10.0)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            m.placements[1].on.as_deref(),
            Some("t2"),
            "the lamp now rests on the table it was dropped on"
        );
        let y = resolve_y(&m, &l).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y[1], 0.8, "and stands at that table's height");
    }

    /// The recorded inverse puts every member back, which is what the editor's undo replays.
    #[test]
    fn restoring_a_move_puts_the_whole_group_back() {
        let (mut m, l) = stacked();
        let before = m.clone();
        let moved = move_placement(&mut m, &l, 0, (3.0, 0.0)).unwrap_or_else(|e| panic!("{e}"));
        assert_ne!(m, before, "the move did something");
        restore_moved(&mut m, &moved);
        assert_eq!(m, before, "restoring must be exact, not approximate");
    }

    /// A mug on a tray on a table moves with the table — riders are found transitively.
    #[test]
    fn riders_are_collected_through_the_whole_chain() {
        let mut tray = table();
        tray.id = "tray".into();
        tray.extent.footprint = Some((0.6, 0.6));
        tray.extent.height = Some(0.05);
        tray.mount = Some(Mount::OnSurface {
            class: "worktop".into(),
        });
        let mut m = map(vec![
            at("t1", "table", None),
            at("tray1", "tray", Some("t1")),
            at("mug1", "lamp", Some("tray1")),
        ]);
        let l = lib(vec![table(), lamp(), tray]);

        let moved = move_placement(&mut m, &l, 0, (5.0, 0.0)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(moved.was.len(), 3, "the mug two levels up must come too");
        assert_eq!(m.placements[1].at, (5.0, 0.0));
        assert_eq!(m.placements[2].at, (5.0, 0.0));
        assert_eq!(m.placements[2].on.as_deref(), Some("tray1"));
    }

    /// An index the map does not have is refused by name rather than panicking.
    #[test]
    fn moving_a_placement_that_is_not_there_is_refused() {
        let (mut m, l) = stacked();
        let err = move_placement(&mut m, &l, 99, (1.0, 1.0)).expect_err("no such placement");
        assert!(err.contains("99"), "{err}");
    }

    /// **The bug this module exists for.** Ten shipped descriptors declare `OnSurface`; every one of
    /// them was placed at floor level, in the editor and in the game alike.
    #[test]
    fn a_lamp_on_a_table_stands_at_the_tables_height() {
        let m = map(vec![
            at("t1", "table", None),
            at("l1", "lamp", Some("t1")),
        ]);
        let y = resolve_y(&m, &lib(vec![table(), lamp()])).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y[0], 0.0);
        assert_eq!(y[1], 0.8);
    }

    /// The host may be authored below its guest — file order is not the author's problem.
    #[test]
    fn the_host_may_appear_after_the_piece_that_rests_on_it() {
        let m = map(vec![
            at("l1", "lamp", Some("t1")),
            at("t1", "table", None),
        ]);
        let y = resolve_y(&m, &lib(vec![table(), lamp()])).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y[0], 0.8);
        assert_eq!(y[1], 0.0);
    }

    /// Stacks compose, and the whole tower rides the map's own floor.
    #[test]
    fn stacks_compose_and_ride_the_maps_floor() {
        let mut shelf = table();
        shelf.id = "shelf".into();
        shelf.mount = Some(Mount::OnSurface {
            class: "worktop".into(),
        });
        shelf.extent.height = Some(0.4);

        let m = Map {
            name: "test_map".into(),
            origin: (0.0, 10.0, 0.0),
            placements: vec![
                at("t1", "table", None),
                at("s1", "shelf", Some("t1")),
                at("l1", "lamp", Some("s1")),
            ],
            ..Map::default()
        };
        let y = resolve_y(&m, &lib(vec![table(), shelf, lamp()])).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y, vec![10.0, 10.8, 11.2]);
    }

    /// **The extent already carries the scale**, so the surface plane is `height × stretch_y` and
    /// nothing else. A previous version asserted `height × scale × stretch_y` — the double-application
    /// [`crate::descriptor::placed_footprint`]'s contract note describes, pinned as if it were the
    /// rule. `extent.height` IS the drawn height (`site/books`: raw mesh 0.297 m × scale 0.6 = the
    /// recorded 0.178 m); multiplying by scale again put the surface plane somewhere nothing draws.
    #[test]
    fn the_surface_plane_is_the_extent_and_scale_does_not_move_it() {
        let mut big = table();
        big.align = Align {
            scale: Some(2.0),
            ..Align::default()
        };
        let m = map(vec![at("t1", "table", None), at("l1", "lamp", Some("t1"))]);
        // The table's recorded height is 0.8 — as placed, whatever render scale reached it.
        let y = resolve_y(&m, &lib(vec![big, lamp()])).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y[1], 0.8);

        // `stretch_y` is game policy layered on top of the extent, the one factor that DOES move it.
        let mut stretched = table();
        stretched.align = Align {
            scale: Some(2.0),
            stretch_y: Some(1.5),
            ..Align::default()
        };
        let y = resolve_y(&m, &lib(vec![stretched, lamp()])).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y[1], 1.2);
    }

    /// A surface mount with nothing under it is refused rather than floored — the exact silence this
    /// module replaces.
    #[test]
    fn a_surface_mount_with_no_host_is_refused() {
        let m = map(vec![at("l1", "lamp", None)]);
        let err = resolve_y(&m, &lib(vec![table(), lamp()]))
            .err()
            .unwrap_or_default();
        assert!(err.contains("records nothing under it"), "{err}");
    }

    /// Resting on a piece that does not offer the class is refused. Every token is spelled correctly,
    /// so nothing else in the stack would notice.
    #[test]
    fn a_host_that_does_not_offer_the_class_is_refused() {
        let mut crate_ = table();
        crate_.id = "crate".into();
        crate_.offers.surfaces.clear();
        let m = map(vec![at("c1", "crate", None), at("l1", "lamp", Some("c1"))]);
        let err = resolve_y(&m, &lib(vec![crate_, lamp()]))
            .err()
            .unwrap_or_default();
        assert!(err.contains("cannot hold it up"), "{err}");
    }

    /// An unmeasured host cannot say where its surface is, and guessing zero would stack a lamp at
    /// floor level on top of a bookcase.
    #[test]
    fn an_unmeasured_host_is_refused() {
        let mut vague = table();
        vague.extent.height = None;
        let m = map(vec![at("t1", "table", None), at("l1", "lamp", Some("t1"))]);
        let err = resolve_y(&m, &lib(vec![vague, lamp()]))
            .err()
            .unwrap_or_default();
        assert!(err.contains("records no height"), "{err}");
    }

    /// The ceiling is the map's, not a constant — the hardcoded 2.4 hung a 3.5 m room's lights in
    /// mid-air.
    #[test]
    fn a_ceiling_mount_hangs_from_the_maps_own_ceiling() {
        let mut light = lamp();
        light.mount = Some(Mount::OnCeiling);
        let m = Map {
            name: "test_map".into(),
            bounds: (10.0, 3.5, 10.0),
            placements: vec![at("l1", "lamp", None)],
            ..Map::default()
        };
        let y = resolve_y(&m, &lib(vec![light])).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y[0], 3.5);
    }

    /// **Every layer has an answer.** A wall overlay carries its own height — the one arm that used
    /// to return an error, because `OnWall` had a height and `Overlay` had nowhere to put one. The
    /// floor and ceiling decals take theirs from the map, since that is whose business they are.
    #[test]
    fn a_decal_lies_on_the_plane_it_names() {
        let m = Map {
            name: "test_map".into(),
            origin: (0.0, 10.0, 0.0),
            bounds: (8.0, 3.0, 8.0),
            placements: vec![at("d", "decal", None)],
            ..Map::default()
        };
        let decal = |on| {
            let mut d = lamp();
            d.id = "decal".into();
            d.mount = Some(Mount::Overlay { on });
            d
        };

        let floor = resolve_y(&m, &lib(vec![decal(OverlayHost::Floor)]))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(floor[0], 10.0);

        let ceiling = resolve_y(&m, &lib(vec![decal(OverlayHost::Ceiling)]))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ceiling[0], 13.0);

        // A poster at 1.6 m on the wall of a room whose floor is at 10.
        let wall = resolve_y(&m, &lib(vec![decal(OverlayHost::Wall { height: 1.6 })]))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(wall[0], 11.6);
    }

    /// A cycle is caught by the resolver as well as by `Map::validate`, so a caller that skipped
    /// validation gets an error rather than a hang.
    #[test]
    fn a_loop_is_an_error_rather_than_a_hang() {
        // Both ends offer what the other needs, so the loop is the FIRST thing wrong with this map
        // rather than something the surface-class check would have caught on the way past.
        let mut shelf = lamp();
        shelf.id = "shelf".into();
        shelf.offers.surfaces = vec!["worktop".into()];
        let m = map(vec![
            at("a", "shelf", Some("b")),
            at("b", "shelf", Some("a")),
        ]);
        let err = resolve_y(&m, &lib(vec![shelf]))
            .err()
            .unwrap_or_default();
        assert!(err.contains("loop"), "{err}");
    }

    /// The footprint test is in the piece's own frame, so turning a table turns the area it covers.
    /// The flood fill learned this the expensive way and striped a floor over it.
    #[test]
    fn a_turned_piece_covers_the_area_it_now_occupies() {
        let t = table(); // 1.6 wide × 0.8 deep
        assert!(covers(&t, (0.0, 0.0), 0.0, (0.7, 0.0)));
        // Along the long axis at rest, outside it once turned a quarter turn.
        assert!(covers(&t, (0.0, 0.0), 0.0, (0.7, 0.3)));
        assert!(!covers(&t, (0.0, 0.0), 90.0, (0.7, 0.3)));
        // And the reverse: what was outside the short axis is inside it now.
        assert!(!covers(&t, (0.0, 0.0), 0.0, (0.0, 0.7)));
        assert!(covers(&t, (0.0, 0.0), 90.0, (0.0, 0.7)));

        // An unmeasured piece covers nothing — unknown is not "everywhere".
        let mut vague = table();
        vague.extent.footprint = None;
        assert!(!covers(&vague, (0.0, 0.0), 0.0, (0.0, 0.0)));
    }

    /// **The highest surface wins**, so a lamp dropped over a shelf standing on a table lands on the
    /// shelf rather than inside it.
    #[test]
    fn the_topmost_surface_is_the_one_a_piece_lands_on() {
        let mut shelf = table();
        shelf.id = "shelf".into();
        shelf.mount = Some(Mount::OnSurface {
            class: "worktop".into(),
        });
        shelf.extent.height = Some(0.4);

        let m = map(vec![at("t1", "table", None), at("s1", "shelf", Some("t1"))]);
        let l = lib(vec![table(), shelf, lamp()]);
        let ys = resolve_y(&m, &l).unwrap_or_else(|e| panic!("{e}"));

        let (host, top) = host_under(&m, &l, &ys, &lamp(), (0.0, 0.0))
            .unwrap_or_else(|| panic!("nothing under the cursor"));
        assert_eq!(host.id, "s1");
        assert_eq!(top, 1.2);
    }

    /// Away from any table there is nothing to stand on, and the answer is the sentence rather than
    /// a piece at floor level.
    #[test]
    fn a_surface_piece_over_bare_floor_is_refused_with_a_reason() {
        let m = map(vec![at("t1", "table", None)]);
        let l = lib(vec![table(), lamp()]);
        let ys = resolve_y(&m, &l).unwrap_or_else(|e| panic!("{e}"));

        // Over the table: on it.
        let (y, host) = placement_at(&m, &l, &ys, &lamp(), (0.0, 0.0)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y, 0.8);
        assert_eq!(host.map(|h| h.id.as_str()), Some("t1"));

        // Ten metres away: nothing, and it says which class it wanted.
        let err = placement_at(&m, &l, &ys, &lamp(), (10.0, 10.0))
            .err()
            .unwrap_or_default();
        assert!(err.contains("worktop"), "{err}");

        // A floor piece needs no host anywhere.
        let (y, host) = placement_at(&m, &l, &ys, &table(), (10.0, 10.0)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(y, 0.0);
        assert!(host.is_none());
    }

    /// **A map that is not at the origin.** Every bug in this area has had the same shape: two spaces
    /// that coincide for a map at `(0, 0, 0)` — the only kind the editor authors — so the confusion
    /// was invisible until something moved. The floor plan is the map's own, the cursor answers in
    /// world metres, and the conversion between them is stated once.
    #[test]
    fn a_moved_map_keeps_its_own_floor_plan() {
        let m = Map {
            name: "test_map".into(),
            origin: (100.0, 0.0, -50.0),
            bounds: (32.0, 4.0, 32.0),
            ..Map::default()
        };
        // The plan is centred on zero wherever the map stands — it describes the map, not the world.
        assert_eq!(m.floor_rect(), (-16.0, -16.0, 16.0, 16.0));
        // A cursor over the map's centre is `at = (0, 0)`, not `at = (100, -50)`.
        assert_eq!(m.to_map_space((100.0, -50.0)), (0.0, 0.0));
        assert_eq!(m.to_map_space((104.0, -50.0)), (4.0, 0.0));
        // And the corner of the plan is the corner of the map on the ground.
        let (min_x, min_z, ..) = m.floor_rect();
        assert_eq!(
            m.to_map_space((m.origin.0 + min_x, m.origin.2 + min_z)),
            (min_x, min_z)
        );
    }

    /// `Map::validate` refuses the same shapes at the door, so nothing reaches the resolver.
    #[test]
    fn validate_refuses_a_dangling_host_and_a_loop() {
        let dangling = map(vec![at("l1", "lamp", Some("nothing"))]);
        let err = dangling.validate().err().unwrap_or_default();
        assert!(err.contains("does not exist"), "{err}");

        let looped = map(vec![at("a", "lamp", Some("b")), at("b", "lamp", Some("a"))]);
        let err = looped.validate().err().unwrap_or_default();
        assert!(err.contains("loop"), "{err}");

        let itself = map(vec![at("a", "lamp", Some("a"))]);
        let err = itself.validate().err().unwrap_or_default();
        assert!(err.contains("rests on itself"), "{err}");
    }

    /// **Flush is legal; inside is not.** The overlap rule's whole bargain: kitbashing lays wall
    /// segments exactly end to end, so touching must pass while interpenetration refuses.
    #[test]
    fn flush_passes_and_overlap_blocks() {
        let (m, l) = (map(vec![at("t1", "table", None)]), lib(vec![table(), lamp()]));
        // The table is 1.6 wide at (0,0): a second table exactly flush starts at x = 1.6.
        assert!(
            blocking(&m, &l, &table(), (1.6, 0.0), 0.0, (0, 0), None).is_none(),
            "flush end-to-end must not read as overlap"
        );
        let hit = blocking(&m, &l, &table(), (1.0, 0.0), 0.0, (0, 0), None);
        assert_eq!(hit.map(|p| p.id.as_str()), Some("t1"), "a lapped table is blocked");
        // And the test is yaw-honest: turned 90 the second table presents 0.8 along X, so at
        // x = 1.3 its near edge (0.9) clears the first's far edge (0.8).
        assert!(
            blocking(&m, &l, &table(), (1.3, 0.0), 90.0, (0, 0), None).is_none(),
            "the turned footprint is the one that counts"
        );
    }

    /// Layers pass each other: the floor tile a crate stands on is not in its way, two lamps
    /// contest one table but not two tables, and a decal claims no volume at all.
    #[test]
    fn other_layers_do_not_block() {
        let tiled = Descriptor {
            id: "floor_tile".into(),
            extent: Extent {
                footprint: Some((0.5, 0.5)),
                height: Some(0.05),
            },
            mount: Some(Mount::Tiled),
            ..Descriptor::default()
        };
        let l = lib(vec![table(), lamp(), tiled.clone()]);
        let m = map(vec![at("f1", "floor_tile", None), at("t1", "table", None)]);
        // A floor-standing table over a Tiled tile: different strata.
        assert!(blocking(&m, &l, &table(), (0.0, 0.0), 0.0, (0, 0), None).is_some());
        assert!(blocking(&m, &l, &tiled, (1.6, 0.0), 0.0, (0, 0), None).is_none());

        // Two lamps on the same host contest it; on different hosts they never meet.
        let stacked = map(vec![
            at("t1", "table", None),
            at("l1", "lamp", Some("t1")),
        ]);
        assert!(
            blocking(&stacked, &l, &lamp(), (0.0, 0.0), 0.0, (0, 0), Some("t1")).is_some(),
            "two lamps on one table at one spot is an overlap"
        );
        assert!(
            blocking(&stacked, &l, &lamp(), (0.0, 0.0), 0.0, (0, 0), Some("t2")).is_none(),
            "a different host is a different layer"
        );
    }

    /// A tipped piece reserves the box it actually fills: a 0.8 m-tall table tipped about X lies
    /// 0.8 m deep in plan, so a probe its upright depth would have cleared now collides.
    #[test]
    fn a_tip_swaps_the_reserved_footprint() {
        let l = lib(vec![table(), lamp()]);
        let mut m = map(vec![at("t1", "table", None)]);
        m.placements[0].tip = (1, 0); // height (0.8) now lies along depth
        // Upright depth is 0.8 (half 0.4); at z = 0.7 a 0.8-deep guest (near edge 0.3) clears an
        // upright table but laps a tipped one only if the tip really swapped nothing... it does
        // not change depth here (0.8 == 0.8), so test the OTHER tip: about Z, width becomes 0.8.
        m.placements[0].tip = (0, 1); // width (1.6) -> height; height (0.8) -> width
        assert!(
            blocking(&m, &l, &table(), (1.2, 0.0), 0.0, (0, 0), None).is_none(),
            "tipped about Z the table is only 0.8 wide, so x = 1.2 clears it"
        );
        m.placements[0].tip = (0, 0);
        assert!(
            blocking(&m, &l, &table(), (1.2, 0.0), 0.0, (0, 0), None).is_some(),
            "upright it is 1.6 wide again and x = 1.2 laps it"
        );
    }
}
