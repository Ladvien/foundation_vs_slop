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

use emerge_core::descriptor::{Descriptor, Mount};
use emerge_core::library::Library;
use serde::{Deserialize, Serialize};

use super::pieces::SitePiece;

/// The shipped kit — Ozea's low-poly sci-fi facility set, promoted into `assets/ozea/`.
///
/// This pointed at `kit_greybox.ron` until 2026-08-01, when enough of the library was converted to
/// dress the hub. The greybox kit did not go away; it became [`GREYBOX_KIT_PATH`], the fixture that
/// proves the swap.
pub const SITE_KIT_PATH: &str = "assets/site/kit_ozea.ron";

/// Where the shipped kit's descriptors live: measurements in `library.ron`, this facility's
/// architecture in `project.ron`. Opened with `emerge_core::policy::layered_library`, the same call
/// the editor and the map loader make.
pub const SITE_PROJECT_DIR: &str = "assets/emerge/site";

/// The greybox kit's descriptors. Two kits, two projects — a greybox wall is a 1 m module stretched
/// to 2.4 m, which is exactly the architecture the Ozea kit mostly does not need, and having it in a
/// separate file is what makes the difference visible instead of baked in.
pub const GREYBOX_PROJECT_DIR: &str = "assets/emerge/site_greybox";

/// The greybox kit, kept as the **swap fixture**: the pair proves re-skinning the Site is authoring
/// one file. Not dead weight — `pieces::every_piece_maps_to_a_glb_that_exists_on_disk` runs against
/// every shipped kit, so this one stays loadable and complete rather than rotting into a file that
/// names meshes nobody has checked in years.
pub const GREYBOX_KIT_PATH: &str = "assets/site/kit_greybox.ron";

/// How far a `rests_on` piece may look for the surface it stands on.
///
/// A dressing prop is authored at the position it should occupy *on* its host, so the host's own
/// centre is at most half its diagonal away — a 3.68 m control desk is the longest in the kit. This
/// is comfortably past that and comfortably short of the next room.
pub const RESTS_ON_REACH: f32 = 2.5;

/// `serde` default for [`KitPiece::scale`] — an unscaled piece is the overwhelming case, and the kit
/// should not have to say `scale: 1.0` thirty-three times.
fn one() -> f32 {
    1.0
}

/// **The authored kit: one descriptor id per piece.**
///
/// This held the art itself until the descriptor library landed — a `glb` path, a height, a
/// footprint, a `front`, a `rests_on`, all inline. Every one of those is now something the rest of the
/// game already knows how to say, and saying it twice is how two descriptions of one mesh drift apart.
///
/// So a kit names pieces and the library describes them. What survives unchanged is the reason this
/// was a struct in the first place: **every piece is a named field with `deny_unknown_fields`, so a
/// kit that forgets one fails at parse time naming the field.** A `HashMap<SitePiece, String>` would
/// push that to runtime, and the failure mode of a missing structural piece is an invisible wall.
///
/// The ids resolve against a project directory — `assets/emerge/site/` for the shipped kit,
/// `assets/emerge/site_greybox/` for the fixture — which is measurements plus this facility's
/// architecture. See [`SiteKit::resolve`].
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KitIds {
    pub trash_bin: String,
    pub pallet: String,
    pub step_ladder: String,
    pub storage_box: String,
    pub storage_crate: String,
    pub utility_cart: String,
    pub tube_rack: String,
    pub data_folder: String,
    pub medical_vial: String,
    pub mug: String,
    pub books: String,
    pub floor: String,
    pub wall: String,
    pub wall_corner: String,
    pub wall_doorway: String,
    pub wall_doorway_wide: String,
    pub wall_header: String,
    pub wall_window: String,
    pub door_plaque: String,
    pub wall_low: String,
    pub slab: String,
    pub column: String,
    pub crate_: String,
    pub pipe: String,
    pub pipe_corner: String,
    pub floor_button: String,
    pub area_decal: String,
    pub arrow_decal: String,
    pub specimen_standin: String,

    // ── Furniture for the living half (2026-08-02). Every one is authored at its own size;
    // `target_height` returns `None` for all of them, so `y_scale` is a no-op 1.0.
    pub bunk: String,
    pub locker: String,
    pub bedside_table: String,
    pub galley_counter: String,
    pub mess_table: String,
    pub stool: String,
    pub coffee_machine: String,
    pub water_dispenser: String,
    pub bench: String,
    pub vending_machine: String,
    pub chair: String,
    pub map_table: String,
    pub control_desk: String,
    pub surveillance_console: String,
    pub server_rack: String,
    pub command_chair: String,
}

impl KitIds {
    /// The descriptor id this kit dresses `piece` in.
    pub fn id(&self, piece: SitePiece) -> &str {
        use SitePiece::*;
        match piece {
            TrashBin => &self.trash_bin,
            Pallet => &self.pallet,
            StepLadder => &self.step_ladder,
            StorageBox => &self.storage_box,
            StorageCrate => &self.storage_crate,
            UtilityCart => &self.utility_cart,
            TubeRack => &self.tube_rack,
            DataFolder => &self.data_folder,
            MedicalVial => &self.medical_vial,
            Mug => &self.mug,
            Books => &self.books,
            Floor => &self.floor,
            Wall => &self.wall,
            WallCorner => &self.wall_corner,
            WallDoorway => &self.wall_doorway,
            WallDoorwayWide => &self.wall_doorway_wide,
            WallHeader => &self.wall_header,
            WallWindow => &self.wall_window,
            DoorPlaque => &self.door_plaque,
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
}

/// **A kit resolved against a descriptor library** — what the game actually reads.
///
/// Holds one [`Descriptor`] per [`SitePiece`], in `SitePiece::ALL` order, so every accessor is an
/// index rather than a lookup. The descriptors are the *layered* ones: measurements from
/// `library.ron` with this facility's architecture from `project.ron` on top, which is why
/// [`Self::y_scale`] is now a field read rather than a division.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteKit {
    ids: KitIds,
    resolved: Vec<Descriptor>,
}

impl SiteKit {
    /// Resolve every id against a layered library, or say which one is missing.
    ///
    /// A kit naming a descriptor the library does not define is refused here, naming both — the same
    /// contract `EmergeWorld::new` holds for a map, and for the same reason: a piece that silently
    /// fails to resolve is an invisible wall.
    pub fn resolve(ids: KitIds, library: &Library) -> Result<SiteKit, String> {
        let mut resolved = Vec::with_capacity(SitePiece::ALL.len());
        for piece in SitePiece::ALL {
            let id = ids.id(*piece);
            let d = library.get(id).ok_or_else(|| {
                format!(
                    "site kit: {piece:?} names descriptor `{id}`, which this project does not \
                     define. A kit names pieces; the library describes them."
                )
            })?;
            resolved.push(d.clone());
        }
        Ok(SiteKit { ids, resolved })
    }

    /// The ids this kit was authored with.
    pub fn ids(&self) -> &KitIds {
        &self.ids
    }

    /// The descriptor id for `piece`.
    pub fn id(&self, piece: SitePiece) -> &str {
        self.ids.id(piece)
    }

    /// The GLB this kit dresses `piece` in.
    ///
    /// A descriptor may legitimately carry no mesh — one can exist to hold tags before anyone has
    /// given it geometry — but a *kit* piece may not, and [`validate_site_kit`] refuses that at the
    /// door. The empty string here is unreachable rather than a fallback.
    pub fn glb(&self, piece: SitePiece) -> &str {
        self.piece(piece).mesh.as_deref().unwrap_or("")
    }

    /// Vertical scale for `piece` in THIS kit.
    ///
    /// **Read, no longer computed.** This was `target_height(piece) / piece.height`, and the comment
    /// beside it said the honest thing: *"a scale is `target / authored` and only ONE of those is a
    /// game fact."* Both halves now live where they belong — the authored height in the library, the
    /// target in the project's policy — and `stretch_y` is what the layering of the two produces.
    /// `tests/site_descriptors.rs` pins that this returns exactly what the division used to.
    pub fn y_scale(&self, piece: SitePiece) -> f32 {
        self.piece(piece).align.stretch_y.unwrap_or(1.0)
    }

    /// How far off the ground plane `piece` sits in THIS kit.
    pub fn y_offset(&self, piece: SitePiece) -> f32 {
        self.piece(piece).align.y_offset.unwrap_or(0.0)
    }

    /// Uniform placement scale for `piece` in THIS kit.
    pub fn scale(&self, piece: SitePiece) -> f32 {
        self.piece(piece).align.scale.unwrap_or(1.0)
    }

    /// The mesh's authored height, before any scaling.
    pub fn height(&self, piece: SitePiece) -> f32 {
        self.piece(piece).extent.height.unwrap_or(0.0)
    }

    /// The mesh's authored XZ footprint `(width, depth)`, before yaw.
    pub fn footprint(&self, piece: SitePiece) -> (f32, f32) {
        self.piece(piece).extent.footprint.unwrap_or((0.0, 0.0))
    }

    /// Which of its own faces this mesh fronts, if it has a front at all.
    ///
    /// A face rather than an angle — see `emerge_core::descriptor::Face`. Callers composing a world
    /// facing want `placement_yaw + front.yaw_degrees()`.
    pub fn front(&self, piece: SitePiece) -> Option<emerge_core::descriptor::Face> {
        self.piece(piece).align.front
    }

    /// The clear opening a doorway leaves, if this piece is one.
    ///
    /// `Mount::InOpening` is the descriptor's word for what `DoorPiece::opening` was, and it carries
    /// the same two numbers the ASYNC aperture quad is built from.
    pub fn opening(&self, piece: SitePiece) -> Option<(f32, f32)> {
        match &self.piece(piece).mount {
            Some(Mount::InOpening { clear }) => *clear,
            _ => None,
        }
    }

    /// The surface class `piece` must rest on, if it is not a floor-standing piece.
    pub fn rests_on(&self, piece: SitePiece) -> Option<&str> {
        match &self.piece(piece).mount {
            Some(Mount::OnSurface { class }) => Some(class.as_str()),
            _ => None,
        }
    }

    /// The class bit `piece` requires of a host, if it rests on one.
    ///
    /// An unknown token maps to `0`, which matches nothing. That is not a silent drop: it cannot
    /// reach here, because [`validate_site_kit`] rejects an unrecognised token at load.
    pub fn rests_on_bits(&self, piece: SitePiece) -> Option<u32> {
        self.rests_on(piece)
            .map(crate::placement::furnish::surface_bits)
    }

    /// The OR of the classes `piece` OFFERS as a host.
    pub fn surface_bits(&self, piece: SitePiece) -> u32 {
        self.piece(piece)
            .offers
            .surfaces
            .iter()
            .map(|s| crate::placement::furnish::surface_bits(s))
            .fold(0, |acc, b| acc | b)
    }

    /// The classes `piece` offers, as authored.
    pub fn surfaces(&self, piece: SitePiece) -> &[String] {
        &self.piece(piece).offers.surfaces
    }

    /// Does `piece` offer any surface at all? The question the seat-facing rule asks, where *which*
    /// class a top offers is irrelevant — a chair addresses the thing it is pulled up to whatever
    /// that thing is for.
    pub fn is_surface(&self, piece: SitePiece) -> bool {
        self.surface_bits(piece) != 0
    }

    /// **How high the top of `piece` stands** — the number a resting prop is seated at.
    ///
    /// Every transform `visuals::place` applies to a host, in the same order it applies them:
    /// `y_offset` lifts the piece off the deck, `scale` is the uniform art correction and `y_scale`
    /// the architectural stretch. Reading only `height * y_scale` — as this did when `rests_on`
    /// landed — is correct exactly while every surface piece is scale 1.0 and offset 0.0, and
    /// silently floats or sinks the dressing the moment one is not.
    pub fn top_height(&self, piece: SitePiece) -> f32 {
        self.y_offset(piece) + self.height(piece) * self.scale(piece) * self.y_scale(piece)
    }

    /// The resolved descriptor for `piece`.
    pub fn piece(&self, piece: SitePiece) -> &Descriptor {
        let at = SitePiece::ALL
            .iter()
            .position(|p| p == &piece)
            .unwrap_or_default();
        // `resolve` fills one entry per `SitePiece::ALL` and the position above comes from that same
        // list, so this cannot miss. `SitePiece::ALL` is itself pinned complete by
        // `pieces::all_lists_every_variant`.
        &self.resolved[at.min(self.resolved.len().saturating_sub(1))]
    }

    /// Every (piece, glb) pair — for preloading and for the validators.
    pub fn entries(&self) -> Vec<(SitePiece, &str)> {
        SitePiece::ALL.iter().map(|p| (*p, self.glb(*p))).collect()
    }
}

/// The descriptor id convention: `site/wall`, `site/mess_table`.
///
/// Namespaced, because a library is a flat id space and `chair` is a word two kits will both want.
/// `Library::parse` refuses duplicates, so a collision would be a load error rather than a silent
/// pick — but a load error nobody can act on is barely better, and the prefix makes it obvious.
pub fn id_of(piece: SitePiece) -> String {
    format!(
        "site/{}",
        emerge_core::naming::to_snake_case(&format!("{piece:?}"))
    )
}

/// Parse a kit's ids. Loud on a missing or unknown field (`deny_unknown_fields`).
pub fn parse_site_kit(text: &str) -> Result<KitIds, String> {
    ron::from_str::<KitIds>(text).map_err(|e| format!("site kit parse error: {e}"))
}

/// Reject a kit that would produce invisible geometry.
///
/// Every check is about a failure mode that is silent in play rather than loud. They read the
/// *resolved* descriptors now rather than the kit file, which is the point of the change: the numbers
/// being validated are the numbers the game will draw with, layered policy and all.
pub fn validate_site_kit(kit: &SiteKit) -> Result<(), String> {
    for (piece, glb) in kit.entries() {
        if glb.trim().is_empty() {
            return Err(format!(
                "site kit: {piece:?} resolves to `{}`, which carries no mesh. A descriptor may exist \
                 to hold tags before it has geometry; a kit piece may not.",
                kit.id(piece)
            ));
        }
        if !glb.ends_with(".glb") {
            return Err(format!(
                "site kit: {piece:?} -> {glb:?} is not a .glb (artist_guide.md §3)"
            ));
        }
    }
    for piece in SitePiece::ALL {
        let piece = *piece;
        let h = kit.height(piece);
        if !(h.is_finite() && h > 0.0) {
            return Err(format!(
                "site kit: {piece:?} has authored height {h} — the architectural stretch is \
                 target/authored, so a zero or negative height is a divide-by-zero or an inside-out \
                 mesh"
            ));
        }
        // Same reasoning as the height, one step further along: `scale` multiplies the placement
        // transform, so a zero collapses the mesh to a point and a negative one turns it inside out —
        // both of which render as "the prop is missing" with nothing in the log.
        for (what, v) in [("scale", kit.scale(piece)), ("stretch", kit.y_scale(piece))] {
            if !(v.is_finite() && v > 0.0) {
                return Err(format!(
                    "site kit: {piece:?} has {what} {v} — a zero collapses the mesh and a negative \
                     one mirrors it; both look like a missing asset and neither errors at spawn"
                ));
            }
        }
        let (fw, fd) = kit.footprint(piece);
        if !(fw.is_finite() && fw > 0.0 && fd.is_finite() && fd > 0.0) {
            return Err(format!(
                "site kit: {piece:?} has footprint ({fw}, {fd}) — `check_prop_placements` measures \
                 floor with it, and a zero makes every overlap test pass"
            ));
        }
        // A `rests_on` token must name a surface class something in this kit actually offers,
        // otherwise the piece can never be seated and would silently sit on the floor. Checked at the
        // door in the same spirit as `placement::manifest::validate_manifest`'s two-sided contract.
        for class in kit.surfaces(piece) {
            if crate::placement::furnish::surface_bits(class) == 0 {
                return Err(format!(
                    "site kit: {piece:?} offers surface class {class:?}, which is not one. The \
                     vocabulary is `emerge_core::placement::surfaces::SURFACE_CLASSES`."
                ));
            }
        }
        if let Some(class) = kit.rests_on(piece) {
            let want = crate::placement::furnish::surface_bits(class);
            if want == 0 {
                return Err(format!(
                    "site kit: {piece:?} rests on {class:?}, which is not a surface class. The \
                     vocabulary is `emerge_core::placement::surfaces::SURFACE_CLASSES`."
                ));
            }
            // The two-sided half. Asking "does ANY piece have a surface" passes a kit in which
            // nothing offers the class actually requested, and the failure then surfaces as a
            // placement fault per authored prop rather than as one sentence about the kit.
            if !SitePiece::ALL
                .iter()
                .any(|p| kit.surface_bits(*p) & want != 0)
            {
                return Err(format!(
                    "site kit: {piece:?} rests on {class:?} but no piece in this kit OFFERS that \
                     class in its `surfaces` — it could never be seated"
                ));
            }
        }
    }
    // A doorway's opening must be a real hole inside a real mesh. The aperture quad is built from
    // these two numbers, so a zero leaves an invisible portal and an opening taller than the frame
    // means it was copied from a bounding box instead of measured between the jambs.
    for piece in [SitePiece::WallDoorway, SitePiece::WallDoorwayWide] {
        let Some((w, h)) = kit.opening(piece) else {
            return Err(format!(
                "site kit: {piece:?} resolves to `{}`, which records no clear opening. A doorway \
                 mounts `InOpening` and carries the hole it leaves; the ASYNC aperture quad is sized \
                 from it.",
                kit.id(piece)
            ));
        };
        if !(w.is_finite() && w > 0.0 && h.is_finite() && h > 0.0) {
            return Err(format!(
                "site kit: {piece:?} has clear opening ({w}, {h}) — the ASYNC aperture quad is \
                 sized from this, and a non-positive opening is an invisible portal"
            ));
        }
        if h > kit.height(piece) {
            return Err(format!(
                "site kit: {piece:?}'s clear opening is {h} m tall but the mesh is only {} m — an \
                 opening is the hole BETWEEN the jambs and UNDER the lintel, not the bounding box",
                kit.height(piece)
            ));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for (piece, glb) in kit.entries() {
        if !seen.insert(glb) {
            return Err(format!(
                "site kit: {piece:?} reuses {glb:?} — two pieces the layout distinguishes would \
                 render identically, and the hub is supposed to be learnable"
            ));
        }
    }
    Ok(())
}

/// Read, resolve and validate a kit from disk. One path: a bad kit is a loud startup failure, never a
/// default.
///
/// Two files rather than one, and that is the whole change: `kit_path` names pieces, `project_dir`
/// describes them. `emerge_core::policy::layered_library` is the same call the editor and the map
/// loader make, so the Site cannot end up with a differently-layered library than the tool that
/// authors for it.
pub fn load_site_kit(kit_path: &str, project_dir: &str) -> Result<SiteKit, String> {
    let text = std::fs::read_to_string(kit_path)
        .map_err(|e| format!("site kit {kit_path} is unreadable: {e}"))?;
    let ids = parse_site_kit(&text)?;
    let library = emerge_core::policy::layered_library(std::path::Path::new(project_dir))
        .map_err(|e| format!("site kit {kit_path}: {e}"))?
        .library;
    let kit = SiteKit::resolve(ids, &library)?;
    validate_site_kit(&kit)?;
    Ok(kit)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> SiteKit {
        load_site_kit(SITE_KIT_PATH, SITE_PROJECT_DIR).expect("the shipped Ozea kit must load")
    }

    /// The shipped kit's ids and its library, separately — so a test can break one and re-resolve,
    /// which is the only way to test the validator now that the numbers come from the library rather
    /// than from the kit file.
    fn parts() -> (KitIds, Library) {
        let text = std::fs::read_to_string(SITE_KIT_PATH).unwrap_or_else(|e| panic!("{e}"));
        let ids = parse_site_kit(&text).unwrap_or_else(|e| panic!("{e}"));
        let library =
            emerge_core::policy::layered_library(std::path::Path::new(SITE_PROJECT_DIR))
                .unwrap_or_else(|e| panic!("{e}"))
                .library;
        (ids, library)
    }

    /// Rebuild the kit with one descriptor edited.
    fn with(piece: SitePiece, edit: impl FnOnce(&mut Descriptor)) -> Result<SiteKit, String> {
        let (ids, mut library) = parts();
        let id = ids.id(piece).to_owned();
        let d = library
            .descriptors
            .iter_mut()
            .find(|d| d.id == id)
            .unwrap_or_else(|| panic!("{id} is not in the shipped library"));
        edit(d);
        let kit = SiteKit::resolve(ids, &library)?;
        validate_site_kit(&kit)?;
        Ok(kit)
    }

    #[test]
    fn the_shipped_kit_parses_resolves_and_validates() {
        let kit = shipped();
        assert_eq!(
            kit.entries().len(),
            SitePiece::ALL.len(),
            "every piece is dressed"
        );
    }

    #[test]
    fn a_kit_missing_a_piece_is_refused_at_parse_time() {
        // The whole reason this is a struct rather than a map, and the guarantee the move to ids was
        // required to keep: forgetting a piece must not be a runtime hole in a wall.
        let text = r#"( floor: "site/floor", wall: "site/wall" )"#;
        assert!(
            parse_site_kit(text).is_err(),
            "a partial kit must not parse"
        );
    }

    /// **A kit names pieces; the library describes them.** An id nothing defines is refused at load,
    /// naming both — the failure a silent resolve would turn into an invisible wall.
    #[test]
    fn a_kit_naming_an_unknown_descriptor_is_refused() {
        let (mut ids, library) = parts();
        ids.wall = "site/wall_that_does_not_exist".into();
        let err = SiteKit::resolve(ids, &library)
            .err()
            .unwrap_or_default();
        assert!(err.contains("wall_that_does_not_exist"), "{err}");
        assert!(err.contains("does not \\
                     define") || err.contains("does not"), "{err}");
    }

    #[test]
    fn a_kit_that_reuses_one_mesh_for_two_pieces_is_refused() {
        let (ids, mut library) = parts();
        let wall_mesh = library
            .get(ids.id(SitePiece::Wall))
            .and_then(|d| d.mesh.clone())
            .unwrap_or_else(|| panic!("the wall has a mesh"));
        let corner = ids.id(SitePiece::WallCorner).to_owned();
        if let Some(d) = library.descriptors.iter_mut().find(|d| d.id == corner) {
            d.mesh = Some(wall_mesh);
        }
        let kit = SiteKit::resolve(ids, &library).unwrap_or_else(|e| panic!("{e}"));
        let err = validate_site_kit(&kit).expect_err("duplicate GLBs must be refused");
        assert!(err.contains("learnable"), "the message must say WHY: {err}");
    }

    /// **The whole point of the seam**, and the Site's version of the furniture library's
    /// asset-swap contract: re-skinning the Site is authoring, zero code changes.
    ///
    /// Before 2026-08-01 this test could not have been written — `SitePiece::glb()` was a `match`
    /// returning `&'static str`, so the Site's look was a property of the binary.
    #[test]
    fn the_site_kit_is_swappable_by_authoring_one_project() {
        let shipped = shipped();
        let swapped = load_site_kit(GREYBOX_KIT_PATH, GREYBOX_PROJECT_DIR)
            .expect("the greybox fixture loads");
        assert_ne!(
            shipped, swapped,
            "the fixture must actually differ, or it proves nothing"
        );
        assert!(
            shipped.glb(SitePiece::Floor).contains("ozea"),
            "the shipped floor is Ozea"
        );
        assert!(
            swapped.glb(SitePiece::Floor).contains("kenney"),
            "the fixture floor is Kenney"
        );
        // **The stretch really is project-derived rather than a constant.** Ozea authors the wall at
        // 2.40 m and Kenney at 1.00 m, so the SAME piece and the SAME id must want a different number
        // — which now comes from two `project.ron` files rather than from a division in this crate.
        assert!(
            (shipped.y_scale(SitePiece::Wall) - swapped.y_scale(SitePiece::Wall)).abs() > 0.5,
            "a 2.40 m wall and a 1.00 m one cannot want the same stretch — the project is not \
             driving it"
        );
        validate_site_kit(&swapped)
            .expect("a swapped kit must satisfy every rule the shipped one does");
    }

    /// The regression that shipped the broken aperture: an opening taken from the frame's OUTLINE
    /// rather than measured between its jambs.
    #[test]
    fn a_doorway_opening_bigger_than_its_mesh_is_refused() {
        let err = with(SitePiece::WallDoorwayWide, |d| {
            let h = d.extent.height.unwrap_or(0.0) + 0.5;
            d.mount = Some(Mount::InOpening {
                clear: Some((1.0, h)),
            });
        })
        .err()
        .unwrap_or_default();
        assert!(
            err.contains("BETWEEN the jambs"),
            "the message must say WHY: {err}"
        );

        assert!(
            with(SitePiece::WallDoorway, |d| {
                d.mount = Some(Mount::InOpening {
                    clear: Some((0.0, 1.9)),
                });
            })
            .is_err(),
            "a zero-width opening is an invisible portal"
        );

        // And a doorway that forgot its opening entirely, which the descriptor CAN express and the
        // old `DoorPiece` could not.
        assert!(
            with(SitePiece::WallDoorway, |d| {
                d.mount = Some(Mount::InOpening { clear: None });
            })
            .is_err(),
            "a doorway with no recorded opening cannot size an aperture"
        );
    }

    /// Both kits must carry the openings, or the swap fixture stops proving the swap works.
    #[test]
    fn every_shipped_kit_measures_its_doorway_openings() {
        for (kit_path, project) in [
            (SITE_KIT_PATH, SITE_PROJECT_DIR),
            (GREYBOX_KIT_PATH, GREYBOX_PROJECT_DIR),
        ] {
            let kit = load_site_kit(kit_path, project).unwrap_or_else(|e| panic!("{kit_path}: {e}"));
            for piece in [SitePiece::WallDoorway, SitePiece::WallDoorwayWide] {
                let (_, oh) = kit
                    .opening(piece)
                    .unwrap_or_else(|| panic!("{kit_path}: {piece:?} has no opening"));
                // A doorway whose "opening" equals its whole footprint was copied from a bbox.
                assert!(
                    oh < kit.height(piece),
                    "{kit_path}: {piece:?} opening height {oh} is not strictly inside a {} m mesh",
                    kit.height(piece)
                );
            }
            // The wide doorway is the one the ASYNC aperture wears, and it must be the wider of the
            // two — otherwise `wall_doorway_wide` is misnamed and the layout's 2-cell gap is wrong.
            let single = kit.opening(SitePiece::WallDoorway).unwrap_or_default();
            let wide = kit.opening(SitePiece::WallDoorwayWide).unwrap_or_default();
            assert!(
                wide.0 > single.0,
                "{kit_path}: the wide doorway must open wider than the single one"
            );
        }
    }

    #[test]
    fn a_non_glb_path_is_refused() {
        assert!(
            with(SitePiece::Pipe, |d| d.mesh = Some("ozea/pipe.fbx".into())).is_err(),
            "artist_guide.md §3 is glTF-binary only"
        );
        // A descriptor with no mesh at all is legal in a library and illegal in a kit.
        assert!(
            with(SitePiece::Pipe, |d| d.mesh = None).is_err(),
            "a kit piece must have geometry"
        );
    }

    /// A zero or negative stretch collapses or mirrors the mesh, and both look like a missing asset.
    /// It comes from the project now, so it is a project mistake the kit still has to catch.
    #[test]
    fn a_non_positive_stretch_is_refused() {
        assert!(with(SitePiece::Wall, |d| d.align.stretch_y = Some(0.0)).is_err());
        assert!(with(SitePiece::Wall, |d| d.align.stretch_y = Some(-1.0)).is_err());
    }
}
