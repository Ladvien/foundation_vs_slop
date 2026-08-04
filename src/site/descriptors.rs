//! **The Site kit, as descriptors** — one kit mechanism in this game instead of two.
//!
//! `kit.rs` says why the Site's art became a file rather than a `match`: *"which mesh a piece wears is
//! ART, and art belongs in an authored file, not a `match`."* It stopped short of the last step. The
//! rest of the game describes a piece of furniture with [`emerge_core::descriptor::Descriptor`]; the
//! Site describes one with [`KitPiece`], and the two say almost exactly the same things in almost
//! exactly the same words.
//!
//! This is the translation, and it is nearly field for field:
//!
//! | `KitPiece` | `Descriptor` |
//! |---|---|
//! | `glb` | `mesh` |
//! | `height`, `footprint` | `extent` |
//! | `front`, `scale`, `y_offset` | `align` |
//! | `surfaces` | `offers.surfaces` |
//! | `rests_on` | `mount: OnSurface { class }` |
//! | `DoorPiece::opening` | `mount: InOpening { clear }` |
//!
//! # The one field that does not cross, and why that is the point
//!
//! `SiteKit::y_scale` is `target_height(piece) / piece.height` — *"a scale is `target / authored` and
//! only ONE of those is a game fact."* The authored height is art and belongs in the descriptor. The
//! target is **this facility's architecture**: 2.4 m walls, 2.0 m doorways, a 0.9 m counter. Baking
//! that into a shared library would put SCP-9191's ceiling in every other game that loaded the mesh.
//!
//! So it becomes what it always was — a [`Policy`] patch — and this module produces the pair:
//! measurements in the library, opinions in the project. That is the layer
//! `emerge_core::policy` exists for, and this is the content it was built to hold.
//!
//! # What this is not, yet
//!
//! Producing the descriptors is not the same as the game reading them. `SiteKit` is still what
//! `site_editor`, `visuals`, `layout` and `people` consume, and it is deliberately still there: its
//! named fields plus `deny_unknown_fields` refuse an incomplete kit at **parse** time, which a
//! `required: [id, …]` list over a descriptor library can only do at load. Swapping the consumers is a
//! real change with a real trade-off, and it is a separate one from proving the two descriptions agree.
//!
//! That agreement is what `tests/site_descriptors.rs` pins: every field, for every piece, plus
//! `y_scale` reproduced exactly by the policy layer.

use emerge_core::descriptor::{Align, Descriptor, Extent, Mount, Offers};
use emerge_core::library::{Library, LIBRARY_VERSION};
use emerge_core::policy::{Match, Patch, Policy, POLICY_VERSION};

use super::kit::{KitPiece, SiteKit};
use super::pieces::{target_height, SitePiece};

/// Where the converted pair lives, as a project directory `emerge_core::policy::layered_library`
/// can open.
pub const SITE_PROJECT_DIR: &str = "assets/emerge/site";

/// The descriptor id for a piece — `site/wall`, `site/mess_table`.
///
/// Namespaced, because a library is a flat id space and `chair` is a word two kits will both want.
/// `Library::parse` refuses duplicates, so the collision would be a load error rather than a silent
/// pick — but a load error nobody can act on is barely better, and the prefix makes the answer obvious.
pub fn id_of(piece: SitePiece) -> String {
    format!("site/{}", emerge_core::naming::to_snake_case(&format!("{piece:?}")))
}

/// One kit piece as a descriptor: everything about the mesh, and nothing about the game.
fn descriptor_of(piece: SitePiece, kit: &SiteKit) -> Descriptor {
    let p: &KitPiece = kit.piece(piece);
    Descriptor {
        id: id_of(piece),
        mesh: Some(p.glb.clone()),
        align: Align {
            // `1.0` is "no correction", and the descriptor spells that as absence — carrying
            // `Some(1.0)` on forty pieces would make the interesting ones invisible in a diff.
            scale: (p.scale != 1.0).then_some(p.scale),
            // Deliberately absent. This is `y_scale`, and `y_scale` is policy — see the module docs.
            stretch_y: None,
            y_offset: (p.y_offset != 0.0).then_some(p.y_offset),
            // Every Ozea mesh is XZ-centred by conversion (`--reorigin-base`), which
            // `tests/ozea_asset.rs` pins, so no piece needs a pivot.
            pivot: None,
            front: p.front,
        },
        extent: Extent {
            footprint: Some(p.footprint),
            height: Some(p.height),
        },
        mount: Some(mount_of(piece, kit)),
        clearance: Vec::new(),
        offers: Offers {
            surfaces: p.surfaces.clone(),
            sockets: Vec::new(),
        },
        placement: emerge_core::descriptor::Placement::default(),
        kind: Vec::new(),
        effects: Vec::new(),
        look: Vec::new(),
        note: None,
    }
}

/// Which layer a piece goes on.
///
/// `rests_on` is the Site's word for what the descriptor calls `OnSurface`, and the two use the same
/// class vocabulary already — `kit.rs` says so: *"The Site's vocabulary is
/// `emerge_core::placement::surfaces::SURFACE_CLASSES` verbatim."* The doorways are the one shape the
/// Site could say and the old manifest could not, and they cross exactly.
fn mount_of(piece: SitePiece, kit: &SiteKit) -> Mount {
    if let Some(class) = kit.rests_on(piece) {
        return Mount::OnSurface {
            class: class.to_owned(),
        };
    }
    match piece {
        SitePiece::WallDoorway => Mount::InOpening {
            clear: Some(kit.wall_doorway.opening),
        },
        SitePiece::WallDoorwayWide => Mount::InOpening {
            clear: Some(kit.wall_doorway_wide.opening),
        },
        _ => Mount::OnFloor,
    }
}

/// The whole kit as a library of measurements.
pub fn library(kit: &SiteKit) -> Library {
    Library {
        version: LIBRARY_VERSION,
        note: Some(
            "GENERATED from the Site kit by tests/site_descriptors.rs. Measurements only — this \
             facility's architecture is in project.ron beside it. Do not hand-edit; regenerate with \
             EMERGE_WRITE_SITE=1 cargo test --test site_descriptors."
                .to_owned(),
        ),
        descriptors: SitePiece::ALL
            .iter()
            .map(|p| descriptor_of(*p, kit))
            .collect(),
    }
}

/// This facility's architecture, as patches over those measurements.
///
/// One rule per piece that has a target height, and the rule says the target rather than the ratio —
/// `stretch_y` is `target / authored`, so writing the ratio would be writing a number that silently
/// goes wrong the moment an artist re-exports the mesh at a different size. The ratio is computed here
/// from both, exactly as `SiteKit::y_scale` does, and the `because` line carries the target so a
/// reader can see what the number is *for*.
pub fn policy(kit: &SiteKit) -> Policy {
    let mut patches = Vec::new();
    for piece in SitePiece::ALL {
        let Some(target) = target_height(*piece) else {
            continue;
        };
        let scale = kit.y_scale(*piece);
        patches.push(Patch {
            matches: Match::Id(id_of(*piece)),
            because: format!(
                "this facility builds {piece:?} to {target:.2} m; the mesh is authored at {:.2} m",
                kit.piece(*piece).height
            ),
            patch: Descriptor {
                align: Align {
                    stretch_y: Some(scale),
                    ..Align::default()
                },
                ..Descriptor::default()
            },
        });
    }
    Policy {
        version: POLICY_VERSION,
        note: Some(
            "SCP-9191's architecture over the Site kit's measurements. GENERATED — regenerate with \
             EMERGE_WRITE_SITE=1 cargo test --test site_descriptors."
                .to_owned(),
        ),
        patches,
    }
}
