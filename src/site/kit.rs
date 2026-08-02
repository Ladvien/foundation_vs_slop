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

/// The shipped kit — Ozea's low-poly sci-fi facility set, promoted into `assets/ozea/`.
///
/// This pointed at `kit_greybox.ron` until 2026-08-01, when enough of the library was converted to
/// dress the hub. The greybox kit did not go away; it became [`GREYBOX_KIT_PATH`], the fixture that
/// proves the swap.
pub const SITE_KIT_PATH: &str = "assets/site/kit_ozea.ron";

/// The greybox kit, kept as the **swap fixture**: the pair proves re-skinning the Site is authoring
/// one file. Not dead weight — `pieces::every_piece_maps_to_a_glb_that_exists_on_disk` runs against
/// every shipped kit, so this one stays loadable and complete rather than rotting into a file that
/// names meshes nobody has checked in years.
pub const GREYBOX_KIT_PATH: &str = "assets/site/kit_greybox.ron";

/// One mesh in a kit: where it lives, and how tall the artist made it.
///
/// **`height` is why this is a struct and not a bare path.** The Site's pieces are scaled to game
/// heights (`WALL_HEIGHT` 2.4 m, `DOORWAY_HEIGHT` 2.0 m), and the scale factor is
/// `target / authored`. The authored height is an **art** fact that differs per kit — the Kenney
/// prototype kit is a uniform 1 m module, while Ozea is mixed: `SM_Wall_1x1` is 1.00 m but
/// `SM_DoorFrame_Single` is already 2.01 m and `SM_Cryogenic_Stasis_Chamber` 2.41 m. A single
/// `KIT_MODULE_HEIGHT` constant in code cannot describe both, and using Kenney's would have scaled
/// the Ozea doorframe to 4 m. Measured at conversion time by `scripts/fbx_to_glb.py`, which writes an
/// `INVENTORY.md` beside the staged GLBs recording every mesh's W/H/D for exactly this reason. (This
/// named `scripts/ozea_to_glb.py` for months — a script that has never existed.)
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KitPiece {
    pub glb: String,
    /// The mesh's authored height in metres, as it sits in the file.
    pub height: f32,
    /// The mesh's authored XZ footprint `(width, depth)` in metres, before yaw.
    ///
    /// **Required, and measured — never guessed**, on the same terms as [`Self::height`] and
    /// [`DoorPiece::opening`]. `scripts/fbx_to_glb.py` writes every mesh's W/H/D to an `INVENTORY.md`
    /// beside its output; these are those numbers.
    ///
    /// # What it is for
    ///
    /// `site::layout::check_prop_placements` runs every authored prop in `site67.ron` through the
    /// **same** overlap and in-bounds rules the dungeon's solver anneals against
    /// (`placement::ir::overlap_area` / `escapes_bounds`). It cannot do that without knowing how much
    /// floor a piece actually covers, and before 2026-08-02 the kit did not say — so the Site's props
    /// were the one placement path in the game that nothing checked. The first furnishing pass put
    /// three bunks through a wall and a control desk a metre into a corridor.
    ///
    /// Note this is the FOOTPRINT, not the reservation: `placement::manifest::ManifestItem` carries a
    /// deliberately symmetric footprint plus a `pivot` for off-centre meshes. The Site kit needs no
    /// pivot because every Ozea mesh is XZ-centred by conversion (`--reorigin-base`), which
    /// `tests/ozea_asset.rs` pins.
    pub footprint: (f32, f32),

    /// Which way this mesh **fronts**, as degrees to add to its authored yaw to get the direction the
    /// engine's facing convention (`forward = (sin yaw, cos yaw)`, i.e. local +Z) would report.
    /// `None` means the piece has no front and nothing may assert where it points.
    ///
    /// **Measured from the mesh, not assumed.** A seat's front is opposite its backrest, and the
    /// backrest is the vertical mass above the seat: taking the XZ centroid of every vertex in the
    /// upper 45% of the mesh and comparing it to the whole mesh's centroid gives the back direction.
    /// For `chair.glb` and `command_chair.glb` that offset is −X, so both front **local +X** — a
    /// quarter turn off the engine's +Z, hence `Some(90.0)`.
    ///
    /// `stool.glb` and `bench.glb` measure symmetric to within a centimetre, because they genuinely
    /// have no back. They get `None`, and that is not a shrug: asserting a facing on a stool would be
    /// asserting a fact about the art that is not true, and the rule below would then demand people
    /// sit on them a particular way round for no reason.
    ///
    /// This existing 90° gap is why every chair in `site67.ron` was authored sideways to its table on
    /// 2026-08-02 — the yaws were written against the engine convention while the mesh fronted
    /// somewhere else. Same class as `visuals::APERTURE_QUAD_YAW_OFFSET`.
    #[serde(default)]
    pub front: Option<f32>,

    /// A horizontal work or dining surface that seating should address — a table, a counter, a
    /// console. Seats near one are required to face it (`layout::check_prop_placements`).
    ///
    /// The furniture manifest expresses the same idea as `surfaces: ["support", "worktop"]`
    /// (`placement::manifest::ManifestItem`); the Site kit needs only the boolean, because nothing
    /// here scatters objects ONTO surfaces yet.
    #[serde(default)]
    pub surface: bool,
    /// Metres to lift this piece off the ground plane. Defaults to 0 — only floor **inlays** need it.
    ///
    /// **This is a geometric fix, not a depth-buffer one.** The Ozea floor plate is 0.06 m thick and
    /// so are `floor_light` and the line decals, so a decal placed at y = 0 has its top face *exactly*
    /// coplanar with the floor's. Coplanar faces are not a precision problem that a depth bias would
    /// paper over — they are genuinely the same plane, and the winner is undefined. Separating them in
    /// space is the honest fix and it holds at any depth range or camera distance.
    ///
    /// It lives in the kit rather than the layout because it is a fact about the **mesh** (how thick
    /// the floor it sits on is), not about the placement — the same decal wants the same lift at every
    /// one of its positions, and a per-placement offset would be fifteen chances to author it wrong.
    #[serde(default)]
    pub y_offset: f32,
}

/// A doorway: a [`KitPiece`] plus the **clear opening** its mesh leaves.
///
/// The opening is a separate art fact from `height`, and neither is derivable from the other:
/// `doorframe_double.glb` stands 1.980 m tall overall, but its jambs face each other across 1.600 m
/// and its lintel underside is 1.626 m up. Nothing in a bounding box says so.
///
/// **The ASYNC aperture quad is sized from this.** Until 2026-08-01 it was sized from
/// `DoorPlacement::trigger_half_extents` — a *gameplay* volume, deliberately generous because it has
/// to catch a walking avatar — which made the quad 3.2 m against a 1.6 m hole. The material is
/// `AlphaMode::Opaque` by design, so that overhang was not a soft artifact: it punched a hole through
/// the wall either side of the door.
///
/// Measured from the mesh's `POSITION` accessors, never guessed — the same rule `kit_ozea.ron`'s
/// header states for `height`. Both doorway pieces carry it as a **required** field, so a kit that
/// omits it fails at parse time; there is no `Option` for a spawner to branch on.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DoorPiece {
    /// The mesh, on exactly the same terms as every other piece.
    pub mesh: KitPiece,
    /// Clear opening `(width, height)` in metres, in the mesh's own authored scale — so the rendered
    /// height is this times [`SiteKit::y_scale`], and the width is this as-is (`site::visuals::place`
    /// scales Y only).
    pub opening: (f32, f32),
}

/// One [`KitPiece`] per [`SitePiece`]; the two doorways carry an opening as well.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SiteKit {
    pub floor: KitPiece,
    pub wall: KitPiece,
    pub wall_corner: KitPiece,
    pub wall_doorway: DoorPiece,
    pub wall_doorway_wide: DoorPiece,
    pub wall_header: KitPiece,
    pub wall_window: KitPiece,
    pub wall_low: KitPiece,
    pub slab: KitPiece,
    pub column: KitPiece,
    pub crate_: KitPiece,
    pub pipe: KitPiece,
    pub pipe_corner: KitPiece,
    pub floor_button: KitPiece,
    pub area_decal: KitPiece,
    pub arrow_decal: KitPiece,
    pub specimen_standin: KitPiece,

    // ── Furniture for the living half (2026-08-02). Every one is authored at its own size;
    // `target_height` returns `None` for all of them, so `y_scale` is a no-op 1.0.
    pub bunk: KitPiece,
    pub locker: KitPiece,
    pub bedside_table: KitPiece,
    pub galley_counter: KitPiece,
    pub mess_table: KitPiece,
    pub stool: KitPiece,
    pub coffee_machine: KitPiece,
    pub water_dispenser: KitPiece,
    pub bench: KitPiece,
    pub vending_machine: KitPiece,
    pub chair: KitPiece,
    pub map_table: KitPiece,
    pub control_desk: KitPiece,
    pub surveillance_console: KitPiece,
    pub server_rack: KitPiece,
    pub command_chair: KitPiece,
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
        super::pieces::target_height(piece).map_or(1.0, |target| target / self.piece(piece).height)
    }

    /// How far off the ground plane `piece` sits in THIS kit — see [`KitPiece::y_offset`].
    pub fn y_offset(&self, piece: SitePiece) -> f32 {
        self.piece(piece).y_offset
    }

    /// The kit entry for `piece`.
    pub fn piece(&self, piece: SitePiece) -> &KitPiece {
        use SitePiece::*;
        match piece {
            Floor => &self.floor,
            Wall => &self.wall,
            WallCorner => &self.wall_corner,
            WallDoorway => &self.wall_doorway.mesh,
            WallDoorwayWide => &self.wall_doorway_wide.mesh,
            WallHeader => &self.wall_header,
            WallWindow => &self.wall_window,
            WallLow => &self.wall_low,
            Slab => &self.slab,
            Column => &self.column,
            Crate => &self.crate_,
            Pipe => &self.pipe,
            PipeCorner => &self.pipe_corner,
            FloorButton => &self.floor_button,
            AreaDecal => &self.area_decal,
            ArrowDecal => &self.arrow_decal,
            SpecimenStandin => &self.specimen_standin,
            Bunk => &self.bunk,
            Locker => &self.locker,
            BedsideTable => &self.bedside_table,
            GalleyCounter => &self.galley_counter,
            MessTable => &self.mess_table,
            Stool => &self.stool,
            CoffeeMachine => &self.coffee_machine,
            WaterDispenser => &self.water_dispenser,
            Bench => &self.bench,
            VendingMachine => &self.vending_machine,
            Chair => &self.chair,
            MapTable => &self.map_table,
            ControlDesk => &self.control_desk,
            SurveillanceConsole => &self.surveillance_console,
            ServerRack => &self.server_rack,
            CommandChair => &self.command_chair,
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
            return Err(format!(
                "site kit: {piece:?} -> {glb:?} is not a .glb (artist_guide.md §3)"
            ));
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
    // A doorway's opening must be a real hole inside a real mesh. The aperture quad is built from
    // these two numbers, so a zero leaves an invisible portal and an opening taller than the frame
    // means it was copied from a bounding box instead of measured between the jambs.
    for (name, door) in [
        ("wall_doorway", &kit.wall_doorway),
        ("wall_doorway_wide", &kit.wall_doorway_wide),
    ] {
        let (w, h) = door.opening;
        if !(w.is_finite() && w > 0.0 && h.is_finite() && h > 0.0) {
            return Err(format!(
                "site kit: {name} has clear opening {:?} — the ASYNC aperture quad is sized from \
                 this, and a non-positive opening is an invisible portal",
                door.opening
            ));
        }
        if h > door.mesh.height {
            return Err(format!(
                "site kit: {name}'s clear opening is {h} m tall but the mesh is only {} m — an \
                 opening is the hole BETWEEN the jambs and UNDER the lintel, not the bounding box",
                door.mesh.height
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
        assert_eq!(
            kit.entries().len(),
            SitePiece::ALL.len(),
            "every piece is dressed"
        );
    }

    #[test]
    fn a_kit_missing_a_piece_is_refused_at_parse_time() {
        // The whole reason this is a struct rather than a map: forgetting a piece must not be a
        // runtime hole in a wall.
        let text = r#"( floor: "a.glb", wall: "b.glb" )"#;
        assert!(
            parse_site_kit(text).is_err(),
            "a partial kit must not parse"
        );
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
        // The shipped kit is Ozea now and the greybox is the fixture — the reverse of how this test
        // was first written, and the swap it proves is the same swap either way round.
        let shipped = load_site_kit(SITE_KIT_PATH).expect("the shipped Ozea kit loads");
        let swapped = load_site_kit(GREYBOX_KIT_PATH).expect("the greybox fixture loads");
        assert_ne!(
            shipped, swapped,
            "the fixture must actually differ, or it proves nothing"
        );
        // Named pieces really did change kit — not merely "the structs differ somewhere".
        assert!(
            shipped.glb(SitePiece::Floor).contains("ozea"),
            "the shipped floor is Ozea"
        );
        assert!(
            swapped.glb(SitePiece::Floor).contains("kenney"),
            "the fixture floor is Kenney"
        );
        // The scale really is kit-derived rather than a constant. `Wall` is the honest comparison now
        // that the shipped kit swaps it: Ozea authors the wall at 2.40 m and Kenney at 1.00 m, so the
        // SAME piece must want a different scale in each. (This asserted on `WallDoorwayWide` while
        // the old partial fixture still used the Kenney wall, which made a `Wall` comparison vacuous.)
        assert!(
            (shipped.y_scale(SitePiece::Wall) - swapped.y_scale(SitePiece::Wall)).abs() > 0.5,
            "a 2.00 m wall and a 1.00 m one cannot want the same scale — the kit is not driving it"
        );
        // And the swapped kit is a VALID kit, not just a different one.
        validate_site_kit(&swapped)
            .expect("a swapped kit must satisfy every rule the shipped one does");
    }

    /// The regression that shipped the broken aperture: an opening taken from the frame's OUTLINE
    /// rather than measured between its jambs.
    #[test]
    fn a_doorway_opening_bigger_than_its_mesh_is_refused() {
        let mut kit = load_site_kit(SITE_KIT_PATH).expect("shipped kit loads");
        kit.wall_doorway_wide.opening.1 = kit.wall_doorway_wide.mesh.height + 0.5;
        let err =
            validate_site_kit(&kit).expect_err("an opening taller than the frame is nonsense");
        assert!(
            err.contains("BETWEEN the jambs"),
            "the message must say WHY: {err}"
        );

        let mut kit = load_site_kit(SITE_KIT_PATH).expect("shipped kit loads");
        kit.wall_doorway.opening.0 = 0.0;
        assert!(
            validate_site_kit(&kit).is_err(),
            "a zero-width opening is an invisible portal"
        );
    }

    /// Both kits must carry the openings, or the swap fixture stops proving the swap works.
    #[test]
    fn every_shipped_kit_measures_its_doorway_openings() {
        for path in [SITE_KIT_PATH, GREYBOX_KIT_PATH] {
            let kit = load_site_kit(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            for (name, door) in [
                ("wall_doorway", &kit.wall_doorway),
                ("wall_doorway_wide", &kit.wall_doorway_wide),
            ] {
                // A doorway whose "opening" equals its whole footprint was copied from a bbox.
                assert!(
                    door.opening.1 < door.mesh.height,
                    "{path}: {name} opening {:?} is not strictly inside a {} m mesh",
                    door.opening,
                    door.mesh.height
                );
            }
            // The wide doorway is the one the ASYNC aperture wears, and it must be the wider of the
            // two — otherwise `wall_doorway_wide` is misnamed and the layout's 2-cell gap is wrong.
            assert!(
                kit.wall_doorway_wide.opening.0 > kit.wall_doorway.opening.0,
                "{path}: the wide doorway must open wider than the single one"
            );
        }
    }

    #[test]
    fn a_non_glb_path_is_refused() {
        let mut kit = load_site_kit(SITE_KIT_PATH).expect("shipped kit loads");
        kit.pipe = KitPiece {
            glb: "ozea/pipe.fbx".into(),
            height: 1.0,
            y_offset: 0.0,
            footprint: (0.3, 0.3),
            front: None,
            surface: false,
        };
        assert!(
            validate_site_kit(&kit).is_err(),
            "artist_guide.md §3 is glTF-binary only"
        );
    }
}
