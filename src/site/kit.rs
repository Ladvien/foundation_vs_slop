//! **The Site's art kit, as data** — which GLB each [`SitePiece`] wears.
//!
//! # Why this is a file and not a `match`
//!
//! `SitePiece::glb()` used to be a `match` returning `&'static str` literals pointing at the Kenney
//! prototype kit. That made the Site's entire look a **code** property: re-skinning it meant editing
//! Rust, and there was no way to hold two kits at once (greybox for tests, dressed for play).
//!
//! The rest of the game already treats a kit as data — `placement::manifest` loads the furniture
//! library from RON, and `assets/config/furniture_kenney.ron` exists purely to prove that swapping the
//! whole library is *authoring one file, zero code changes*. The Site was the exception. This makes it
//! the same shape, so there is **one** kit mechanism in the game rather than two.
//!
//! # A struct, not a map
//!
//! Every piece is a named field with `deny_unknown_fields`, so a kit that forgets one fails at
//! **parse** time naming the field, and a kit with a typo'd key fails instead of silently falling back
//! to a default. A `HashMap<SitePiece, String>` would have pushed both of those to runtime — and the
//! failure mode of a missing structural piece is an invisible wall, which is exactly the kind of bug
//! that survives a playtest.

use serde::{Deserialize, Serialize};

use super::pieces::SitePiece;

/// The shipped greybox kit. Kenney prototype blocks — deliberately readable rather than pretty, and
/// the fallback every test builds against.
pub const SITE_KIT_PATH: &str = "assets/site/kit_greybox.ron";

/// One mesh in a kit: where it lives, and how tall the artist made it.
///
/// **`height` is why this is a struct and not a bare path.** The Site's pieces are scaled to game
/// heights (`WALL_HEIGHT` 2.4 m, `DOORWAY_HEIGHT` 2.0 m), and the scale factor is
/// `target / authored`. The authored height is an **art** fact that differs per kit — the Kenney
/// prototype kit is a uniform 1 m module, while Ozea is mixed: `SM_Wall_1x1` is 1.00 m but
/// `SM_DoorFrame_Single` is already 2.01 m and `SM_Cryogenic_Stasis_Chamber` 2.41 m. A single
/// `KIT_MODULE_HEIGHT` constant in code cannot describe both, and using Kenney's would have scaled
/// the Ozea doorframe to 4 m. Measured at conversion time by `scripts/ozea_to_glb.py`, which prints
/// every bounding box for exactly this reason.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KitPiece {
    pub glb: String,
    /// The mesh's authored height in metres, as it sits in the file.
    pub height: f32,
}

/// One [`KitPiece`] per [`SitePiece`].
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SiteKit {
    pub floor: KitPiece,
    pub wall: KitPiece,
    pub wall_corner: KitPiece,
    pub wall_doorway: KitPiece,
    pub wall_doorway_wide: KitPiece,
    pub wall_window: KitPiece,
    pub wall_low: KitPiece,
    pub column: KitPiece,
    pub crate_: KitPiece,
    pub pipe: KitPiece,
    pub pipe_corner: KitPiece,
    pub floor_button: KitPiece,
    pub area_decal: KitPiece,
    pub arrow_decal: KitPiece,
    pub specimen_standin: KitPiece,
}

impl SiteKit {
    /// The GLB this kit dresses `piece` in.
    pub fn glb(&self, piece: SitePiece) -> &str {
        &self.piece(piece).glb
    }

    /// Vertical scale for `piece` in THIS kit: the game's target height over the artist's.
    ///
    /// Game policy (what a wall must reach) stays in `pieces::target_height`; art fact (how tall the
    /// mesh is) lives in the kit. Neither belongs in the other.
    pub fn y_scale(&self, piece: SitePiece) -> f32 {
        super::pieces::target_height(piece)
            .map_or(1.0, |target| target / self.piece(piece).height)
    }

    /// The kit entry for `piece`.
    pub fn piece(&self, piece: SitePiece) -> &KitPiece {
        use SitePiece::*;
        match piece {
            Floor => &self.floor,
            Wall => &self.wall,
            WallCorner => &self.wall_corner,
            WallDoorway => &self.wall_doorway,
            WallDoorwayWide => &self.wall_doorway_wide,
            WallWindow => &self.wall_window,
            WallLow => &self.wall_low,
            Column => &self.column,
            Crate => &self.crate_,
            Pipe => &self.pipe,
            PipeCorner => &self.pipe_corner,
            FloorButton => &self.floor_button,
            AreaDecal => &self.area_decal,
            ArrowDecal => &self.arrow_decal,
            SpecimenStandin => &self.specimen_standin,
        }
    }

    /// Every (piece, glb) pair — for preloading and for the validators.
    pub fn entries(&self) -> Vec<(SitePiece, &str)> {
        SitePiece::ALL.iter().map(|p| (*p, self.glb(*p))).collect()
    }
}

/// Parse a kit. Loud on a missing or unknown field (`deny_unknown_fields`).
pub fn parse_site_kit(text: &str) -> Result<SiteKit, String> {
    ron::from_str::<SiteKit>(text).map_err(|e| format!("site kit parse error: {e}"))
}

/// Reject a kit that would produce invisible geometry.
///
/// Both checks are about failure modes that are silent in play rather than loud:
/// an empty path loads nothing and leaves a hole in a wall, and a duplicated GLB means two pieces the
/// layout treats as different render identically — which defeats the point of a hand-authored,
/// *learnable* hub.
pub fn validate_site_kit(kit: &SiteKit) -> Result<(), String> {
    for (piece, glb) in kit.entries() {
        if glb.trim().is_empty() {
            return Err(format!("site kit: {piece:?} has an empty GLB path"));
        }
        if !glb.ends_with(".glb") {
            return Err(format!("site kit: {piece:?} -> {glb:?} is not a .glb (artist_guide.md §3)"));
        }
    }
    for piece in SitePiece::ALL {
        let h = kit.piece(*piece).height;
        if !(h.is_finite() && h > 0.0) {
            return Err(format!(
                "site kit: {piece:?} has authored height {h} — the scale is target/authored, so a \
                 zero or negative height is a divide-by-zero or an inside-out mesh"
            ));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for (piece, glb) in kit.entries() {
        if !seen.insert(glb) {
            return Err(format!(
                "site kit: {piece:?} reuses {glb:?} — two pieces the layout distinguishes would render \
                 identically, and the hub is supposed to be learnable"
            ));
        }
    }
    Ok(())
}

/// Read + validate a kit from disk. One path: a bad kit is a loud startup failure, never a default.
pub fn load_site_kit(path: &str) -> Result<SiteKit, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("site kit {path} is unreadable: {e}"))?;
    let kit = parse_site_kit(&text)?;
    validate_site_kit(&kit)?;
    Ok(kit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_kit_parses_and_validates() {
        let kit = load_site_kit(SITE_KIT_PATH).expect("the shipped greybox kit must load");
        assert_eq!(kit.entries().len(), SitePiece::ALL.len(), "every piece is dressed");
    }

    #[test]
    fn a_kit_missing_a_piece_is_refused_at_parse_time() {
        // The whole reason this is a struct rather than a map: forgetting a piece must not be a
        // runtime hole in a wall.
        let text = r#"( floor: "a.glb", wall: "b.glb" )"#;
        assert!(parse_site_kit(text).is_err(), "a partial kit must not parse");
    }

    #[test]
    fn a_kit_that_reuses_one_mesh_for_two_pieces_is_refused() {
        let mut kit = load_site_kit(SITE_KIT_PATH).expect("shipped kit loads");
        kit.wall_corner = kit.wall.clone();
        let err = validate_site_kit(&kit).expect_err("duplicate GLBs must be refused");
        assert!(err.contains("learnable"), "the message must say WHY: {err}");
    }

    /// **The whole point of the seam**, and the Site's version of the furniture library's
    /// asset-swap contract: re-skinning the Site is authoring ONE FILE, zero code changes.
    ///
    /// Before 2026-08-01 this test could not have been written — `SitePiece::glb()` was a `match`
    /// returning `&'static str`, so the Site's look was a property of the binary.
    #[test]
    fn the_site_kit_is_swappable_by_authoring_one_file() {
        let greybox = load_site_kit(SITE_KIT_PATH).expect("greybox kit loads");
        let swapped =
            load_site_kit("assets/site/kit_ozea_partial.ron").expect("the swap fixture loads");
        assert_ne!(greybox, swapped, "the fixture must actually differ, or it proves nothing");
        // Named pieces really did change kit — not merely "the structs differ somewhere".
        assert!(swapped.glb(SitePiece::Floor).contains("ozea"), "floor should be Ozea now");
        // The scale really is kit-derived, not a constant: Ozea's wall is authored at 2.00 m and
        // Kenney's at 1.00 m, so the SAME piece must scale differently in each kit.
        // Compare a piece the fixture actually swaps: the Ozea doorframe is authored at 1.98 m and
        // the Kenney one at 1.0 m, so the SAME piece must scale differently in each kit. (Comparing
        // `Wall` proved nothing — the partial fixture still uses the Kenney wall, so both scales were
        // identical and the assertion was vacuous.)
        assert!(
            (swapped.y_scale(SitePiece::WallDoorwayWide) - greybox.y_scale(SitePiece::WallDoorwayWide))
                .abs()
                > 0.5,
            "a 1.98 m doorframe and a 1.0 m one cannot want the same scale — the kit is not driving it"
        );
        assert!(greybox.glb(SitePiece::Floor).contains("kenney"), "greybox floor is Kenney");
        // And the swapped kit is a VALID kit, not just a different one.
        validate_site_kit(&swapped).expect("a swapped kit must satisfy every rule the shipped one does");
    }

    #[test]
    fn a_non_glb_path_is_refused() {
        let mut kit = load_site_kit(SITE_KIT_PATH).expect("shipped kit loads");
        kit.pipe = KitPiece { glb: "site/ozea/pipe.fbx".into(), height: 1.0 };
        assert!(validate_site_kit(&kit).is_err(), "artist_guide.md §3 is glTF-binary only");
    }
}
