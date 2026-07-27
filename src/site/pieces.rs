//! **The greybox kit** — which `.glb` each authored Site piece maps to, and how it must be scaled.
//!
//! Site-67 is greyboxed from `assets/kenney_prototype-kit/`, which is already in the repo, already
//! licensed, and already proven through the loader (`assets/config/furniture_kenney.ron` is the
//! asset-swap fixture that exercises exactly this path). **FVS-N-10 is therefore not a prerequisite**;
//! see the correction in `BACKLOG.md` §3. Converting the Ozea sci-fi library is an art upgrade to do
//! *after* the layout settles, so it targets only the meshes the Site actually uses rather than all 411.
//!
//! ## The one thing that will bite you: the kit is 1 m TALL
//!
//! Kenney's prototype pieces are a **1 m module in every axis**, including height — `wall.glb` measures
//! 0.2 × **1.0** × 1.0 and `floor-square.glb` is a zero-thickness plane at y = 0. The game's contract
//! (`docs/artist_guide.md` §1) is `WALL_HEIGHT = 2.4` with a `DOORWAY_HEIGHT` of 2.0 and a 1.82 m
//! operative. So a wall placed at native scale is **knee-high** and the Site reads as a model railway.
//!
//! Every piece therefore declares its own Y scale here rather than at each call site, because "scale
//! the walls but not the crates" is precisely the kind of rule that gets applied to four of five
//! spawners and then debugged as a rendering bug.
//!
//! ## Why an enum and not a path string
//!
//! `placement::manifest::ManifestItem` carries `glb: String`, and that is right for *it*: the furniture
//! manifest exists to be kit-swapped at runtime, which is the whole point of the `furniture_kenney.ron`
//! fixture. The Site's kit swap is a different event — a one-off art upgrade in code, not a runtime
//! choice — so the cost/benefit inverts: an enum makes a typo'd path a **compile error** instead of a
//! silent missing prop, and makes the eventual Ozea swap a single-table edit.

/// Vertical scale that lifts a 1 m Kenney wall to the game's `WALL_HEIGHT` of 2.4 m.
///
/// Not `dungeon::WALL_HEIGHT / 1.0` written out, because the `1.0` is a *property of the kit* and the
/// 2.4 is a property of the game; naming both keeps the swap honest when the kit changes.
pub const KIT_MODULE_HEIGHT: f32 = 1.0;

/// A piece the Site layout may place.
///
/// Ordered by role rather than alphabetically, so the table reads as a kit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub enum SitePiece {
    // ── Structure ──
    Floor,
    Wall,
    WallCorner,
    /// A 2 m opening between areas. Pair with [`SitePiece::Lintel`] to fill the header course.
    WallDoorway,
    /// The wide opening the ASYNC aperture sits in.
    WallDoorwayWide,
    /// Glazing for a containment cell front.
    WallWindow,
    /// Waist-high run: counters, records desks, the requisition bar.
    WallLow,
    Column,
    // ── Dressing ──
    Crate,
    Pipe,
    PipeCorner,
    /// Floor pad marking the door threshold.
    FloorButton,
    /// Flat floor decal — the wing colour-code that makes the hub learnable without signage.
    AreaDecal,
    /// Directional wayfinding on the floor.
    ArrowDecal,
    /// Stands in for a held specimen inside a containment cell (FVS-D-4).
    ///
    /// A neutral shape on purpose: `Specimen` records only `captured: Entity`, and that entity is
    /// despawned with the expedition — so the Site genuinely does not know what species it is holding.
    /// Recording the species is FVS-E-1's job (the research posterior needs it anyway); until then a
    /// stand-in is honest and a guessed model would not be.
    SpecimenStandin,
}

impl SitePiece {
    /// The GLB this piece loads.
    pub fn glb(self) -> &'static str {
        use SitePiece::*;
        match self {
            Floor => "kenney_prototype-kit/Models/GLB format/floor-square.glb",
            Wall => "kenney_prototype-kit/Models/GLB format/wall.glb",
            WallCorner => "kenney_prototype-kit/Models/GLB format/wall-corner.glb",
            WallDoorway => "kenney_prototype-kit/Models/GLB format/wall-doorway.glb",
            WallDoorwayWide => "kenney_prototype-kit/Models/GLB format/wall-doorway-wide.glb",
            WallWindow => "kenney_prototype-kit/Models/GLB format/wall-window-large.glb",
            WallLow => "kenney_prototype-kit/Models/GLB format/wall-low.glb",
            Column => "kenney_prototype-kit/Models/GLB format/column.glb",
            Crate => "kenney_prototype-kit/Models/GLB format/crate-color.glb",
            Pipe => "kenney_prototype-kit/Models/GLB format/pipe.glb",
            PipeCorner => "kenney_prototype-kit/Models/GLB format/pipe-corner.glb",
            FloorButton => "kenney_prototype-kit/Models/GLB format/button-floor-square.glb",
            AreaDecal => "kenney_prototype-kit/Models/GLB format/indicator-square-a.glb",
            ArrowDecal => "kenney_prototype-kit/Models/GLB format/indicator-special-arrow.glb",
            SpecimenStandin => "kenney_prototype-kit/Models/GLB format/shape-cylinder-detailed.glb",
        }
    }

    /// Vertical scale to apply at spawn.
    ///
    /// `1.0` means "the kit's native height is already right for this piece's job" — a crate is a crate
    /// at 0.5 m, a floor plane has no height, and a floor decal must stay flat. Anything that stands up
    /// as architecture is scaled to the game's wall contract.
    pub fn y_scale(self) -> f32 {
        use SitePiece::*;
        match self {
            // Full-height architecture -> WALL_HEIGHT (2.4 m).
            Wall | WallCorner | WallWindow | Column => {
                crate::dungeon::WALL_HEIGHT / KIT_MODULE_HEIGHT
            }
            // A doorway must leave a 2.0 m opening (`DOORWAY_HEIGHT`); the header course above it is a
            // separate `WallLow` placed at y = 2.0 by the spawner.
            WallDoorway | WallDoorwayWide => crate::dungeon::DOORWAY_HEIGHT / KIT_MODULE_HEIGHT,
            // Waist-high: a 0.5 m Kenney low wall -> ~0.9 m counter.
            WallLow => 1.8,
            // Native height is the intent.
            Floor | Crate | Pipe | PipeCorner | FloorButton | AreaDecal | ArrowDecal
            | SpecimenStandin => 1.0,
        }
    }

    /// Every piece the kit defines — so a test can walk the whole table.
    pub const ALL: &'static [SitePiece] = &[
        SitePiece::Floor,
        SitePiece::Wall,
        SitePiece::WallCorner,
        SitePiece::WallDoorway,
        SitePiece::WallDoorwayWide,
        SitePiece::WallWindow,
        SitePiece::WallLow,
        SitePiece::Column,
        SitePiece::Crate,
        SitePiece::Pipe,
        SitePiece::PipeCorner,
        SitePiece::FloorButton,
        SitePiece::AreaDecal,
        SitePiece::ArrowDecal,
        SitePiece::SpecimenStandin,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_maps_to_a_glb_that_exists_on_disk() {
        // No precedent in the repo for this check, and it is the single most likely greybox failure:
        // a mistyped path is a silently missing prop at runtime, discovered by squinting at a screenshot.
        // Costs nothing to catch at `cargo test` instead. Paths are cwd-relative like `GAME_CONFIG_PATH`.
        let missing: Vec<&str> = SitePiece::ALL
            .iter()
            .map(|p| p.glb())
            .filter(|rel| !std::path::Path::new("assets").join(rel).exists())
            .collect();
        assert!(missing.is_empty(), "Site pieces name GLBs that are not in assets/: {missing:?}");
    }

    #[test]
    fn architecture_is_scaled_up_and_dressing_is_not() {
        // The kit is a 1 m module in EVERY axis, so anything that stands up as architecture must be
        // scaled or the Site renders knee-high. This pins the rule so a new piece cannot quietly ship
        // at native height.
        assert!(SitePiece::Wall.y_scale() > 2.0, "a wall must reach WALL_HEIGHT");
        assert!(SitePiece::Column.y_scale() > 2.0);
        assert_eq!(SitePiece::WallDoorway.y_scale(), crate::dungeon::DOORWAY_HEIGHT);
        // Dressing keeps its authored size.
        assert_eq!(SitePiece::Crate.y_scale(), 1.0);
        assert_eq!(SitePiece::Floor.y_scale(), 1.0);
        // A floor decal scaled in Y would z-fight the floor it sits on.
        assert_eq!(SitePiece::AreaDecal.y_scale(), 1.0);
    }

    #[test]
    fn a_doorway_opening_clears_an_operative() {
        // The Valkyrie renders at ~1.82 m. A doorway scaled below that is a wall with a mouse hole.
        assert!(
            SitePiece::WallDoorway.y_scale() * KIT_MODULE_HEIGHT > 1.82,
            "the doorway opening must clear a 1.82 m operative"
        );
    }

    #[test]
    fn no_two_pieces_share_a_glb() {
        // Not a correctness requirement, but a duplicate is almost always a copy-paste in the table
        // that silently makes one piece render as another.
        let mut seen = std::collections::HashSet::new();
        for p in SitePiece::ALL {
            assert!(seen.insert(p.glb()), "{p:?} duplicates another piece's GLB");
        }
    }
}
