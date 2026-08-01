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

/// One GLB path per [`SitePiece`].
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SiteKit {
    pub floor: String,
    pub wall: String,
    pub wall_corner: String,
    pub wall_doorway: String,
    pub wall_doorway_wide: String,
    pub wall_window: String,
    pub wall_low: String,
    pub column: String,
    pub crate_: String,
    pub pipe: String,
    pub pipe_corner: String,
    pub floor_button: String,
    pub area_decal: String,
    pub arrow_decal: String,
    pub specimen_standin: String,
}

impl SiteKit {
    /// The GLB this kit dresses `piece` in.
    pub fn glb(&self, piece: SitePiece) -> &str {
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
        assert!(greybox.glb(SitePiece::Floor).contains("kenney"), "greybox floor is Kenney");
        // And the swapped kit is a VALID kit, not just a different one.
        validate_site_kit(&swapped).expect("a swapped kit must satisfy every rule the shipped one does");
    }

    #[test]
    fn a_non_glb_path_is_refused() {
        let mut kit = load_site_kit(SITE_KIT_PATH).expect("shipped kit loads");
        kit.pipe = "site/ozea/pipe.fbx".into();
        assert!(validate_site_kit(&kit).is_err(), "artist_guide.md §3 is glTF-binary only");
    }
}
