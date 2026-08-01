//! **Site navigation** — a small walkable mask, deliberately *not* a second [`crate::dungeon::Dungeon`].
//!
//! ## Why this exists at all
//!
//! The obvious move — let real squad `Unit`s stand at the Site — does not work, and the reason is
//! concrete rather than stylistic: `squad::unit_movement` and `fog::update_los` both take
//! `Res<Dungeon>`, and while `RunState::Idle` that resource is **absent** (first boot) or **stale**
//! (post-run, describing a world that has been despawned). Operatives at the Site would collide with a
//! ghost dungeon and repaint a ghost fog grid.
//!
//! **This argument is now conditional, and the condition is new.** Since `input::Action::VisitSite`
//! the player can stand at the Site *during* a live expedition, and in that case `Dungeon` is neither
//! absent nor stale — it is the world the squad is currently walking around in
//! (`docs/2026-08-01-two-live-layers.md` §5). So the reasoning above holds only for the Site between
//! expeditions. Anyone using it to justify a new decision must say which of the two cases they mean.
//!
//! ## Why it is not a `Dungeon`
//!
//! `Dungeon` is a single global resource that FVS-A-5 regenerates per run. A second one would make
//! "which world does this system mean?" ambiguous in every system that reads it. The design doc is
//! explicit: **the Site is entities, not a `Dungeon`.** So this carries a different type name on
//! purpose — no system can confuse a [`SiteNav`] for the expedition world, and none of the
//! dungeon-indexed subsystems (fog, cutaway, light field, mould, almond water) can accidentally consume
//! it.
//!
//! ## Cosmetic, and it must stay that way
//!
//! Nothing here runs on `FixedUpdate` or touches `(Transform, Health)` on a hashed entity. Avatars are
//! [`crate::site::SiteAvatar`], never `squad::Unit`, so they contribute no rows to `snapshot_hash`.

use bevy::prelude::*;

use super::layout::SiteLayout;

/// Longest single collision step, so a fast mover cannot tunnel through a wall cell. Mirrors the
/// dungeon mover's sub-stepping for the same reason.
const MAX_STEP: f32 = 0.2;

/// The Site's walkable mask, baked once from the authored layout.
///
/// A resource rather than a re-read of [`SiteLayout`] per query: the mask is a pure function of the
/// layout and the layout never changes at runtime, so paying for the rect scan on every movement step
/// would be work with no possible different answer.
#[derive(Resource, Debug, Clone)]
pub struct SiteNav {
    origin: Vec3,
    min: IVec2,
    dims: IVec2,
    walkable: Vec<bool>,
}

impl SiteNav {
    /// Bake the mask from the layout's floor runs minus its wall cells.
    pub fn bake(layout: &SiteLayout) -> Self {
        // Bounds from the floor runs; walls outside the floor cannot matter (they are not walkable
        // either way), so the mask only needs to span the floor.
        let mut min = IVec2::new(i32::MAX, i32::MAX);
        let mut max = IVec2::new(i32::MIN, i32::MIN);
        for r in &layout.floor {
            min = min.min(IVec2::new(r.x, r.z));
            max = max.max(IVec2::new(r.x + r.w, r.z + r.h));
        }
        // An empty layout cannot happen (`validate` rejects it), but a zero-size grid would panic on
        // indexing, so clamp rather than trust.
        if min.x > max.x || min.y > max.y {
            return Self { origin: Vec3::ZERO, min: IVec2::ZERO, dims: IVec2::ZERO, walkable: Vec::new() };
        }
        let dims = max - min;
        let mut walkable = vec![false; (dims.x * dims.y).max(0) as usize];
        for z in 0..dims.y {
            for x in 0..dims.x {
                let c = min + IVec2::new(x, z);
                walkable[(z * dims.x + x) as usize] = layout.is_walkable(c);
            }
        }
        Self {
            origin: Vec3::new(layout.origin.0, layout.origin.1, layout.origin.2),
            min,
            dims,
            walkable,
        }
    }

    /// Can an avatar stand on this cell?
    pub fn is_walkable(&self, c: IVec2) -> bool {
        let l = c - self.min;
        if l.x < 0 || l.y < 0 || l.x >= self.dims.x || l.y >= self.dims.y {
            return false;
        }
        self.walkable[(l.y * self.dims.x + l.x) as usize]
    }

    /// World point → cell.
    pub fn world_to_cell(&self, p: Vec3) -> IVec2 {
        IVec2::new((p.x - self.origin.x).floor() as i32, (p.z - self.origin.z).floor() as i32)
    }

    /// Cell → world centre.
    pub fn cell_center(&self, c: IVec2) -> Vec3 {
        Vec3::new(
            self.origin.x + c.x as f32 + 0.5,
            self.origin.y,
            self.origin.z + c.y as f32 + 0.5,
        )
    }

    /// Is a box of half-extents `half` centred at `p` entirely on walkable ground?
    fn box_clear(&self, p: Vec3, half: f32) -> bool {
        // Four corners is sufficient for an axis-aligned box against a 1 m grid as long as the box is
        // smaller than a cell, which every avatar is (`AVATAR_HALF` well under 0.5).
        for (dx, dz) in [(-half, -half), (half, -half), (-half, half), (half, half)] {
            if !self.is_walkable(self.world_to_cell(p + Vec3::new(dx, 0.0, dz))) {
                return false;
            }
        }
        true
    }

    /// Move `pos` by `delta`, sliding along walls rather than stopping dead.
    ///
    /// **Axis-separated, and that is the whole point.** Rejecting the move outright whenever the
    /// combined step is blocked makes an avatar stick to any wall it brushes, which reads as a bug even
    /// though nothing is wrong. Trying each axis independently is what produces the wall-slide players
    /// expect. Same shape and same reasoning as `Dungeon::resolve_move`, on a different mask.
    pub fn resolve_move(&self, pos: Vec3, delta: Vec3, half: f32) -> Vec3 {
        let steps = (delta.length() / MAX_STEP).ceil().max(1.0) as u32;
        let d = delta / steps as f32;
        let mut p = pos;
        for _ in 0..steps {
            let both = p + Vec3::new(d.x, 0.0, d.z);
            if self.box_clear(both, half) {
                p = both;
                continue;
            }
            // Blocked diagonally — slide along whichever single axis is still clear.
            let only_x = p + Vec3::new(d.x, 0.0, 0.0);
            if self.box_clear(only_x, half) {
                p = only_x;
                continue;
            }
            let only_z = p + Vec3::new(0.0, 0.0, d.z);
            if self.box_clear(only_z, half) {
                p = only_z;
            }
            // Both blocked: stay put for this sub-step. Not an error — it is a corner.
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav() -> (SiteLayout, SiteNav) {
        let l = SiteLayout::load().expect("shipped layout");
        let n = SiteNav::bake(&l);
        (l, n)
    }

    #[test]
    fn the_mask_agrees_with_the_layout_on_every_floor_cell() {
        // The bake is a cache of `layout::is_walkable`; if the two ever disagree, movement and
        // validation are reasoning about different Sites.
        let (l, n) = nav();
        for r in &l.floor {
            for c in r.cells() {
                assert_eq!(n.is_walkable(c), l.is_walkable(c), "mask disagrees at {c:?}");
            }
        }
    }

    #[test]
    fn walls_are_not_walkable_but_the_floor_under_them_still_is_floor() {
        // The distinction that replaced the "no wall on floor" rule: a column occupies a floor cell and
        // is not walkable. Both halves matter — floor is architecture, walkable is navigation.
        let (l, n) = nav();
        for w in &l.walls {
            let c = IVec2::new(w.cell.0, w.cell.1);
            assert!(!n.is_walkable(c), "wall cell {c:?} must not be walkable");
        }
    }

    #[test]
    fn an_avatar_cannot_walk_out_of_the_site() {
        // Push hard in every direction from each spawn and assert we never end up off the mask. This is
        // the property that makes the hub a room rather than a plane.
        let (l, n) = nav();
        for s in &l.spawns {
            let start = Vec3::new(l.origin.0 + s.0, l.origin.1, l.origin.2 + s.1);
            for dir in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z, Vec3::new(1.0, 0.0, 1.0)] {
                let mut p = start;
                for _ in 0..200 {
                    p = n.resolve_move(p, dir.normalize() * 0.5, 0.25);
                }
                assert!(
                    n.is_walkable(n.world_to_cell(p)),
                    "walking {dir:?} from {s:?} left the walkable mask at {p:?}"
                );
            }
        }
    }

    #[test]
    fn an_avatar_slides_along_a_wall_instead_of_sticking() {
        // Push diagonally into a wall and assert real progress along the free axis. Without the
        // axis-separated retry this returns ~0 and every brush against a wall reads as a freeze.
        let (_l, n) = nav();
        // The spine's south edge: walkable at z=12, void below (the async hall ends at z=11 only for
        // x<12, so pick a spine cell east of the hall).
        let start = n.cell_center(IVec2::new(20, 12));
        assert!(n.is_walkable(n.world_to_cell(start)), "precondition: the probe starts on floor");
        let moved = n.resolve_move(start, Vec3::new(1.0, 0.0, -1.0), 0.25);
        assert!(
            (moved.x - start.x).abs() > 0.1,
            "an avatar pushed diagonally into a wall must still slide along it (moved {:?})",
            moved - start
        );
    }

    #[test]
    fn a_fast_step_cannot_tunnel_through_a_wall() {
        // Sub-stepping, pinned. A single 50 m delta must not teleport an avatar across the map.
        let (l, n) = nav();
        let s = l.spawns[0];
        let start = Vec3::new(l.origin.0 + s.0, l.origin.1, l.origin.2 + s.1);
        let end = n.resolve_move(start, Vec3::new(50.0, 0.0, 0.0), 0.25);
        assert!(n.is_walkable(n.world_to_cell(end)), "a huge step tunnelled off the mask");
    }
}
