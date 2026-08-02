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
    /// A doorway between areas. The frame carries its own header, so nothing pairs with it.
    ///
    /// (This said "pair with `SitePiece::Lintel` to fill the header course" — a variant that has never
    /// existed, so the link did not even resolve.)
    WallDoorway,
    /// The wide opening the ASYNC aperture sits in.
    WallDoorwayWide,
    /// The header course over a doorway — the band of wall between `DOORWAY_HEIGHT` and
    /// `WALL_HEIGHT`, without which the perimeter has a slot straight through it above the door.
    WallHeader,
    /// Half a wall panel: the leg of a junction, running from the corner point to the cell edge.
    ///
    /// A junction cannot be built from full panels. The corner point is a cell CENTRE, so a 1 m panel
    /// centred there runs 0.5 m PAST it — two of them cross in a plus and leave a half-panel stub
    /// jutting into the open on each axis, which the player read as the walls overlapping.
    WallLeg,
    /// Glazing for a containment cell front.
    WallWindow,
    /// Waist-high run: counters, records desks, the requisition bar.
    WallLow,
    /// The examination slab in the research wing — where `research::lab::StudySubject` lies.
    Slab,
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
    // `glb()` lived here until 2026-08-01 and now lives in `site::kit`: which mesh a piece wears is
    // ART, and art belongs in an authored file, not a `match`. See `kit.rs` for why the Site stopped
    // being the one part of the game whose kit was a code property.

    // `y_scale` moved to `kit::SiteKit::y_scale` on 2026-08-01, because a scale is
    // `target / authored` and only ONE of those is a game fact. What a wall must REACH is policy and
    // stays here; how tall the artist made the mesh is art and lives in the kit.
}

/// The height, in metres, this piece must reach — or `None` when the authored size IS the intent.
///
/// Game policy, deliberately kit-independent. The old `y_scale` folded this together with a
/// `KIT_MODULE_HEIGHT` constant that assumed a uniform 1 m kit; that held for the Kenney prototype
/// blocks and breaks the moment a second kit exists. Ozea is mixed — `SM_Wall_1x1` is 1.00 m but
/// `SM_DoorFrame_Single` is already 2.01 m — so scaling by a fixed module would have produced a 4 m
/// doorway.
pub fn target_height(piece: SitePiece) -> Option<f32> {
    use SitePiece::*;
    match piece {
        // Full-height architecture.
        Wall | WallCorner | WallLeg | WallWindow | Column => Some(crate::dungeon::WALL_HEIGHT),
        // The frame's OUTER height. What the player walks through is smaller and is a separate art
        // fact — `kit::DoorPiece::opening`, which the ASYNC aperture quad is built from. (This said a
        // separate `WallLow` header course was placed above it at y = 2.0. No spawner ever did that.)
        WallDoorway | WallDoorwayWide => Some(crate::dungeon::DOORWAY_HEIGHT),
        // What is LEFT of the wall above a doorway — derived, never a literal, so the two heights
        // cannot drift apart and reopen the slot. `DOORWAY_HEIGHT`'s own doc says "the wall runs
        // continuous above it"; the dungeon honoured that and the Site did not until 2026-08-01.
        WallHeader => Some(crate::dungeon::WALL_HEIGHT - crate::dungeon::DOORWAY_HEIGHT),
        // Waist-high: a counter you can stand at. Authored in `site67.ron`'s props as the Records desk
        // and the Requisition counter.
        WallLow => Some(0.9),
        // Native height is the intent — dressing, decals, and the specimen stand-in.
        Floor | Crate | Pipe | PipeCorner | FloorButton | AreaDecal | ArrowDecal | Slab
        | SpecimenStandin => None,
    }
}

impl SitePiece {
    /// Every piece the kit defines — so a test can walk the whole table.
    pub const ALL: &'static [SitePiece] = &[
        SitePiece::Floor,
        SitePiece::Wall,
        SitePiece::WallCorner,
        SitePiece::WallDoorway,
        SitePiece::WallDoorwayWide,
        SitePiece::WallHeader,
        SitePiece::WallLeg,
        SitePiece::WallWindow,
        SitePiece::WallLow,
        SitePiece::Column,
        SitePiece::Slab,
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
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped greybox kit loads");
        // No precedent in the repo for this check, and it is the single most likely greybox failure:
        // a mistyped path is a silently missing prop at runtime, discovered by squinting at a screenshot.
        // Costs nothing to catch at `cargo test` instead. Paths are cwd-relative like `GAME_CONFIG_PATH`.
        let missing: Vec<&str> = kit
            .entries()
            .into_iter()
            .map(|(_, glb)| glb)
            .filter(|rel| !std::path::Path::new("assets").join(rel).exists())
            .collect();
        assert!(
            missing.is_empty(),
            "Site pieces name GLBs that are not in assets/: {missing:?}"
        );
    }

    #[test]
    fn architecture_reaches_its_target_and_dressing_keeps_its_authored_size() {
        // Policy only — heights the GAME requires, independent of any kit. The old version of this
        // test asserted a SCALE, which silently baked in the Kenney 1 m module and would have passed
        // just as happily against an Ozea kit it was sizing wrong.
        assert_eq!(
            target_height(SitePiece::Wall),
            Some(crate::dungeon::WALL_HEIGHT)
        );
        assert_eq!(
            target_height(SitePiece::Column),
            Some(crate::dungeon::WALL_HEIGHT)
        );
        assert_eq!(
            target_height(SitePiece::WallDoorway),
            Some(crate::dungeon::DOORWAY_HEIGHT)
        );
        // Dressing and decals are authored at the size they want; a decal scaled in Y would z-fight
        // the floor it sits on.
        assert_eq!(target_height(SitePiece::Crate), None);
        assert_eq!(target_height(SitePiece::Floor), None);
        assert_eq!(target_height(SitePiece::AreaDecal), None);
    }

    #[test]
    fn every_shipped_kit_opens_its_doorways_at_the_floor_and_measures_the_hole() {
        // ⚠️ This was `..._clears_an_operative_through_its_doorways`, and it asserted
        // `height * y_scale > 1.82` — the Valkyrie's render height. That number collapses to
        // `target_height` for any piece that has one, so it was very nearly tautological; worse, it
        // measured the frame's OUTLINE, and what an operative would walk through is the hole. The two
        // differ by a third: `doorframe_double` is 2.00 m tall rendered and its opening is 1.64 m.
        //
        // Measured, the Ozea doorways do NOT clear a 1.82 m operative, and that is accepted rather
        // than overlooked: the wide one is the ASYNC aperture, a portal whose trigger fires before an
        // avatar reaches the frame, and the single one is not placed in the shipped layout at all.
        // The alternative was scaling the frame 23% in Y alone to force a 2 m hole, which stretches
        // its trim. So this pins what is actually true — the opening starts at the floor, it is a hole
        // inside a real frame, and it is big enough to read as a door.
        use crate::site::kit::load_site_kit;
        for path in [
            crate::site::kit::SITE_KIT_PATH,
            crate::site::kit::GREYBOX_KIT_PATH,
        ] {
            let kit = load_site_kit(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            for piece in [SitePiece::WallDoorway, SitePiece::WallDoorwayWide] {
                let door = match piece {
                    SitePiece::WallDoorway => &kit.wall_doorway,
                    _ => &kit.wall_doorway_wide,
                };
                // Every kit mesh is base-at-0 (`artist_guide` §3 rule 7) and `place` scales about the
                // entity origin, so the rendered span is `[y_offset, y_offset + final_h]`. A doorway
                // must start at the floor: an opening whose sill is 30 cm up is a window. THIS is the
                // assertion with teeth — it pins where the opening sits, not just how tall it is. See
                // `tests/ozea_asset.rs::every_ozea_mesh_is_base_origined_and_xz_centred`.
                let base = kit.y_offset(piece);
                assert!(
                    base.abs() < 0.01,
                    "{path}: {piece:?} sits at y={base:.3}, so its opening starts {base:.3} m off the \
                     floor rather than at it"
                );
                // The hole is a hole: strictly inside the frame, and tall enough to read as a door
                // rather than a hatch. `opening` is authored scale, so the rendered height rides
                // `y_scale` exactly as the frame does.
                let opening_h = door.opening.1 * kit.y_scale(piece);
                let frame_h = door.mesh.height * kit.y_scale(piece);
                assert!(
                    door.opening.1 < door.mesh.height,
                    "{path}: {piece:?} opening {:?} is not inside its {} m frame — measured from the \
                     bounding box instead of between the jambs?",
                    door.opening,
                    door.mesh.height
                );
                assert!(
                    opening_h > 1.5 && opening_h < frame_h,
                    "{path}: {piece:?} renders a {opening_h:.2} m opening in a {frame_h:.2} m frame"
                );
            }
        }
    }

    // `no_two_pieces_share_a_glb` moved to `kit::validate_site_kit` — it is a property of a KIT, and
    // every kit must satisfy it, not just the one that used to be hardcoded here.
}
