//! **The hub's smart locations, derived from what is already authored there.**
//!
//! `emerge_core::smart` can run an interaction once something says *which props belong together*.
//! FINAL FANTASY XV calls that a smart location — *"a single smart location may refer to two chairs
//! and a table… capture relationships between them, such as furniture grouping"* (Game AI Pro 3
//! ch.35) — and the obvious move is to author them.
//!
//! The Site does not need to. It already has the relation, and it already enforces it:
//! `layout::check_prop_placements` refuses a chair within [`SEAT_REACH`] of a table that does not
//! **face** it, on the grounds that *"a seat at a surface must face it."* That rule only means
//! anything because a seat near a surface is understood to be pulled up to it. Reading the same
//! relation a second way turns the hub's existing furniture into smart locations with nothing new to
//! author and nothing new to keep in sync.
//!
//! This is the derive-don't-author discipline the rest of `site/` follows —
//! `visuals::wall_panels` derives faces from floor edges, `light_the_site` derives a wing's fixtures
//! from its rect, `people::post_positions` derives posts from an area. A hand-authored list of
//! locations would be a fourth place the hub's furniture is described, and the first one to go stale
//! the next time somebody moves a table.
//!
//! # What comes out
//!
//! One location per surface that has seats pulled up to it, offering one interaction: sitting down at
//! it. The roles carry no `socket_role`, because the Site's descriptors mark no seats — a chair *is*
//! the seat, and where an occupant stands is the chair's own position. Sockets are what a mesh with
//! several places to sit needs, and nothing in this kit has one.

use emerge_core::descriptor::Descriptor;
use emerge_core::map::{Effect, Interaction, Location, RoleKind, RoleSlot};

use super::kit::SiteKit;
use super::layout::SiteLayout;
use super::pieces::SitePiece;

/// How close a seat must be to a surface to be pulled up to it, in metres.
///
/// The same 2.0 m `layout::check_prop_placements` uses for the facing rule, and deliberately the same
/// number rather than a second one: two different reaches would mean a chair that must face a table
/// it is not considered to be at, which is a contradiction nobody would ever see reported.
pub const SEAT_REACH: f32 = 2.0;

/// The `kind` token a piece carries when somebody can sit on it.
const SEATING: &str = "seating";

/// The verb the hub's tables afford.
pub const SIT_AT: &str = "sit_at";

/// Derive the hub's smart locations from its authored props.
///
/// A location per surface with at least one seat pulled up to it. Ids are `at_<piece>_<index>` where
/// the index is the surface's position in `layout.props` — stable across a run and stable across
/// edits *above* it in the file, which is the best a derived id can do and is why nothing stores one.
pub fn locations(layout: &SiteLayout, kit: &SiteKit) -> Vec<Location> {
    let is_seat = |piece: SitePiece| {
        kit.piece(piece)
            .kind
            .iter()
            .any(|k| k == SEATING)
    };

    let mut out = Vec::new();
    for (i, surface) in layout.props.iter().enumerate() {
        if !kit.is_surface(surface.piece) {
            continue;
        }
        // Every seat pulled up to this surface. **Nearest wins**, exactly as the facing rule decides
        // it: a chair between two tables belongs to one of them, and putting it in both locations
        // would let two scenes seat the same chair.
        let seats: Vec<usize> = layout
            .props
            .iter()
            .enumerate()
            .filter(|(j, p)| {
                *j != i && is_seat(p.piece) && nearest_surface(layout, kit, *j) == Some(i)
            })
            .map(|(j, _)| j)
            .collect();
        if seats.is_empty() {
            continue;
        }

        let capacity = seats.len().min(u8::MAX as usize) as u8;
        let mut props = vec![prop_id(i, surface.piece)];
        props.extend(seats.iter().map(|j| prop_id(*j, layout.props[*j].piece)));

        out.push(Location {
            id: format!("at_{}", prop_id(i, surface.piece)),
            props,
            interactions: vec![Interaction {
                verb: SIT_AT.to_owned(),
                roles: vec![RoleSlot {
                    name: "sitter".to_owned(),
                    kind: RoleKind::Main,
                    min: 1,
                    // One per chair. A table does not seat more people than it has seats pulled up to
                    // it, and the alternative — a cap somebody picked — would be a number the hub
                    // could contradict by having five chairs.
                    max: capacity,
                    socket_role: None,
                    requires: vec!["rest".to_owned()],
                }],
                guard: None,
                effects: vec![Effect::Restore {
                    drive: "stamina".to_owned(),
                    rate: 0.2,
                }],
                note: Some(format!(
                    "{:?} with {capacity} seat(s) pulled up to it",
                    surface.piece
                )),
            }],
            note: Some(format!(
                "derived from the hub's props: a {:?} and the seats within {SEAT_REACH} m of it",
                surface.piece
            )),
        });
    }
    out
}

/// The surface a seat is pulled up to, as an index into `layout.props`.
///
/// Lifted from `check_prop_placements` so the two cannot disagree about which table a chair belongs
/// to. Nearest within [`SEAT_REACH`]; ties broken by index, which is a total order over a list that
/// does not reorder itself at runtime.
fn nearest_surface(layout: &SiteLayout, kit: &SiteKit, seat: usize) -> Option<usize> {
    let p = layout.props.get(seat)?;
    let mut best: Option<(usize, f32)> = None;
    for (j, q) in layout.props.iter().enumerate() {
        if !kit.is_surface(q.piece) {
            continue;
        }
        let d = ((q.pos.0 - p.pos.0).powi(2) + (q.pos.1 - p.pos.1).powi(2)).sqrt();
        if d <= SEAT_REACH && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((j, d));
        }
    }
    best.map(|(j, _)| j)
}

/// A stable name for one authored prop.
fn prop_id(index: usize, piece: SitePiece) -> String {
    format!(
        "{}_{index}",
        emerge_core::naming::to_snake_case(&format!("{piece:?}"))
    )
}

/// The descriptor a derived location's prop refers to.
///
/// The locations above name props by their layout index; this is how a caller gets from one back to
/// the piece it is.
pub fn piece_of(layout: &SiteLayout, prop: &str) -> Option<SitePiece> {
    let index: usize = prop.rsplit('_').next()?.parse().ok()?;
    layout.props.get(index).map(|p| p.piece)
}

/// Where a derived location's props stand, so a caller can walk to one.
pub fn centre_of(layout: &SiteLayout, location: &Location) -> Option<(f32, f32)> {
    let first = location.props.first()?;
    let index: usize = first.rsplit('_').next()?.parse().ok()?;
    layout.props.get(index).map(|p| p.pos)
}

/// The descriptors a derived location needs, keyed the way its props are named.
///
/// `emerge_core::smart::seats_of` wants a `Map` and a `Library`; the hub has a `SiteLayout` and a
/// `SiteKit`. Nothing here needs seats — the roles carry no `socket_role` — so this exists only for
/// the caller that wants to know what a prop *is*.
pub fn descriptor_of<'a>(layout: &SiteLayout, kit: &'a SiteKit, prop: &str) -> Option<&'a Descriptor> {
    piece_of(layout, prop).map(|p| kit.piece(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::kit::{load_site_kit, SITE_KIT_PATH, SITE_PROJECT_DIR};

    fn kit() -> SiteKit {
        load_site_kit(SITE_KIT_PATH, SITE_PROJECT_DIR).unwrap_or_else(|e| panic!("{e}"))
    }

    fn hub() -> SiteLayout {
        let text = std::fs::read_to_string("assets/site/site67.ron").unwrap_or_else(|e| panic!("{e}"));
        ron::from_str(&text).unwrap_or_else(|e| panic!("{e}"))
    }

    /// **The hub already contains smart locations; nobody had read them out.** It places four mess
    /// tables, four stools, six chairs and three benches, and the placement rules already insist the
    /// seats face the tables they are pulled up to.
    #[test]
    fn the_shipped_hub_derives_places_to_sit() {
        let locations = locations(&hub(), &kit());
        assert!(
            !locations.is_empty(),
            "the hub has tables with chairs at them; none were derived"
        );
        for loc in &locations {
            assert!(
                loc.props.len() >= 2,
                "{}: a location with no seats should not have been derived",
                loc.id
            );
            let interaction = loc
                .interactions
                .first()
                .unwrap_or_else(|| panic!("{}: no interaction", loc.id));
            assert_eq!(interaction.verb, SIT_AT);
            let role = &interaction.roles[0];
            assert_eq!(
                usize::from(role.max),
                loc.props.len() - 1,
                "{}: one seat per chair pulled up to it",
                loc.id
            );
        }
    }

    /// **No chair belongs to two tables.** The nearest-wins rule is what stops two scenes seating the
    /// same chair, and it is the same rule the facing check uses — so a chair can never be required
    /// to face a table it is not considered to be at.
    #[test]
    fn a_seat_belongs_to_exactly_one_location() {
        let locations = locations(&hub(), &kit());
        let mut seen: Vec<&str> = Vec::new();
        for loc in &locations {
            for prop in loc.props.iter().skip(1) {
                assert!(
                    !seen.contains(&prop.as_str()),
                    "{prop} is seated at two locations"
                );
                seen.push(prop);
            }
        }
    }

    /// Every derived location passes the schema's own validation once it is put in a map — the same
    /// check an authored map gets, so a derived one cannot be quietly less well formed.
    #[test]
    fn the_derived_locations_are_valid_map_locations() {
        use emerge_core::map::{Map, Placed};
        let layout = hub();
        let locations = locations(&layout, &kit());

        // The props they refer to, as placements, so `Map::validate` can check the references.
        let mut placements = Vec::new();
        for loc in &locations {
            for prop in &loc.props {
                if placements.iter().any(|p: &Placed| &p.id == prop) {
                    continue;
                }
                placements.push(Placed {
                    id: prop.clone(),
                    descriptor: "site/stool".into(),
                    ..Placed::default()
                });
            }
        }
        let map = Map {
            name: "hub".into(),
            placements,
            locations,
            ..Map::default()
        };
        map.validate().unwrap_or_else(|e| panic!("{e}"));
    }

    /// A prop name round-trips to the piece it came from, which is how a caller gets from a location
    /// back to something it can walk to.
    #[test]
    fn a_prop_name_names_a_real_prop() {
        let layout = hub();
        let locations = locations(&layout, &kit());
        let loc = locations.first().unwrap_or_else(|| panic!("no locations"));
        assert!(piece_of(&layout, &loc.props[0]).is_some());
        assert!(centre_of(&layout, loc).is_some());
    }
}
