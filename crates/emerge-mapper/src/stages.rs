//! **Every far-away staging point, in one place.**
//!
//! The editor stands things far from the map to look at them in isolation: a tile on the Tiles tab, a
//! rig on the Anim bench, a group on Compose, and two photo booths that stage subjects to photograph
//! them. Each needs a spot no other can reach, because a camera pointed at one must not see another's
//! contents.
//!
//! # Why this module exists
//!
//! These were five constants in five modules with nothing comparing them, and on 2026-08-09 the
//! Compose stage was given `(4096, 0, 4096)` — **the thumbnail booth's exact coordinates**. At
//! startup the booth cycles forty-five meshes through that point to photograph them, so switching to
//! Compose showed the palette's entire kit flashing past. Reported as *"all the meshes are still
//! flashing on the composure screen"*, and it took an author at the machine to see it: no test
//! renders, so nothing could have caught two constants being equal.
//!
//! [`distinct`] is that check, and it is why the constants live together rather than apart. A new
//! stage adds a row there and the test fails until it is somewhere of its own.

use bevy::prelude::Vec3;

/// The Tiles tab's single-piece stage.
pub const TILE: Vec3 = Vec3::new(-4096.0, 0.0, 4096.0);

/// The Anim tab's rig bench.
pub const BENCH: Vec3 = Vec3::new(-4096.0, 0.0, -4096.0);

/// The label booth, which photographs a piece from several angles for the vision labeller.
pub const LABEL_BOOTH: Vec3 = Vec3::new(4096.0, 0.0, -4096.0);

/// The palette booth, which bakes one thumbnail per library piece at startup.
pub const THUMB_BOOTH: Vec3 = Vec3::new(4096.0, 0.0, 4096.0);

/// **The Compose tab's group stage.** Off the corners the other four use, on purpose — see the
/// module note for what happened when it shared one.
pub const COMPOSE: Vec3 = Vec3::new(4096.0, 0.0, 0.0);

/// Every stage, with the name to report when two of them collide.
pub const ALL: &[(&str, Vec3)] = &[
    ("tiles", TILE),
    ("anim bench", BENCH),
    ("label booth", LABEL_BOOTH),
    ("thumbnail booth", THUMB_BOOTH),
    ("compose", COMPOSE),
];

/// How far apart two stages must be, metres.
///
/// Generous rather than tight: a stage holds a whole group, a booth holds whatever the largest piece
/// in the kit is, and both are looked at by a camera framing several metres. Anything closer than
/// this is a mistake even if nothing currently overlaps.
pub const CLEARANCE: f32 = 512.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// **No two stages may share a spot, or be close enough to see each other.**
    ///
    /// The test that would have caught the Compose/thumbnail collision the day it was written.
    #[test]
    fn distinct() {
        for (i, (name_a, a)) in ALL.iter().enumerate() {
            for (name_b, b) in ALL.iter().skip(i + 1) {
                let d = a.distance(*b);
                assert!(
                    d >= CLEARANCE,
                    "the `{name_a}` and `{name_b}` stages are {d} m apart, closer than the {CLEARANCE} m \
                     clearance — a camera framing one will see the other's contents. That is exactly \
                     how the Compose stage came to sit on the thumbnail booth and show the whole \
                     palette flashing past at startup."
                );
            }
        }
    }
}
