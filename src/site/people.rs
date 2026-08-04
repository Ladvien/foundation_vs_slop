//! **The people of Site-67** — who has a body in the hub, and which of them is which.
//!
//! Fiction and the authored roster: `docs/lore/2026-08-02-site-67-recommissioned.md` §3. The axes
//! (`StaffTitle`, `Clearance`) are `crate::personnel`.
//!
//! # Why `SiteAvatar` was split
//!
//! It used to be `SiteAvatar(pub usize)` — one component carrying two unrelated facts: *this is a body
//! that walks the Site*, and *this is squad member N*. That was fine while the only bodies were five
//! operatives. It stops being fine the moment a cook is standing in the galley, because "avatar #7"
//! then means nothing and every query has to know whether the payload is a squad index or a staff
//! index.
//!
//! So the marker and the key are separate components now, and the key is typed:
//!
//! * [`SiteAvatar`] — a body that walks the Site. Movement, animation and the aperture's proximity
//!   glow all want *every* body, and they take this.
//! * [`Operative`] / [`Staff`] — the narrow keys. A system that means "the squad" cannot accidentally
//!   pick up the archivist, because it does not typecheck.
//! * [`CastId`] — the one stable identity, across both.
//!
//! Dropping the tuple payload was a compile error at every use site, which is the point: nothing
//! silently kept reading a squad index off a staff member.
//!
//! # Why `CastId` is offset
//!
//! `knowledge::roster::ROSTER_SLOTS` is 8 — five operatives plus deliberate headroom — and
//! `SquadKnowledge` is an array indexed by `SquadMember`. Staff start at 8 so that growing the squad
//! into its headroom never renumbers a single staff member. Anything that later keys per-person state
//! (Stage C's routines, Stage D's relationships) keys on this, and a renumbering would silently
//! reassign it.
//!
//! # Determinism
//!
//! Windowed-only, and structurally so: this module is reached from `SiteVisualsPlugin`, which the
//! headless harness never registers. Every body it spawns carries a `Transform` and **no `Health`**, so
//! it contributes no row to `sim_harness::snapshot_hash` and no actor to `liveness_violations` — the
//! entity-shape argument `site::mod` makes, not a claim about where the plugin happens to sit.
//!
//! **This claims the same deliberate exception `docs/animation.md` states for the animation layer**,
//! for the same stated reason: a genome gene pointed at cosmetic hub behaviour could never move the
//! fitness, so wiring it into RL/QD would widen the offline search space forever and buy nothing.
//! Recorded here as a decision rather than left as an omission.
//!
//! Note that `tests/determinism_lint.rs` is **textual** and recurses all of `src/`, so the ordering
//! discipline still applies to anything here that picks — see `Stage C`'s slot claiming.

use bevy::prelude::*;
use serde::Deserialize;

use crate::personnel::{Clearance, StaffTitle};
use super::layout::{AreaId, SiteLayout};
use super::nav::SiteNav;

/// Where `assets/` keeps the authored staff roster.
pub const STAFF_PATH: &str = "assets/site/staff.ron";

/// A body that walks the Site. **Never `squad::Unit`** — see `site::mod`'s note on why.
///
/// A marker only. The identity is [`CastId`] and the kind is [`Operative`] / [`Staff`].
#[derive(Component, Debug, Clone, Copy)]
pub struct SiteAvatar;

/// This body is squad member `.0` (0..5), index-matched to `squad::SquadMember` and
/// `squad_ai::persona::PersonaRoster` so FVS-G-3 can map an avatar onto a persistent operative without
/// re-keying anything.
#[derive(Component, Debug, Clone, Copy)]
pub struct Operative(pub usize);

/// This body is entry `.0` of the authored staff roster.
#[derive(Component, Debug, Clone, Copy)]
pub struct Staff(pub usize);

/// The one stable per-person key, across operatives and staff alike.
///
/// Operative *i* is `CastId(i)`; staff *j* is `CastId(ROSTER_SLOTS + j)`. See the module header for why
/// the offset exists rather than a bare running counter.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CastId(pub usize);

impl CastId {
    /// The first `CastId` a staff member can hold. Anything below this is a squad slot, staffed or not.
    pub const STAFF_BASE: usize = crate::knowledge::roster::ROSTER_SLOTS;

    pub fn of_operative(index: usize) -> Self {
        CastId(index)
    }

    pub fn of_staff(index: usize) -> Self {
        CastId(Self::STAFF_BASE + index)
    }
}

/// Which character rig a staff member wears.
///
/// An enum rather than a path string, for the reason `AreaId` gives: a typo should be a compile error,
/// and the set of rigs that actually ship is a fact worth stating once. Every one of these is pinned by
/// `tests/staff_asset.rs` — 20 clips in one shared order, one 55-joint skin, downscaled textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StaffRig {
    /// Female researcher archetype, lab coat.
    Researcher,
    /// Male senior scientist archetype.
    Scientist,
    /// Male field operative — jumpsuit and boots.
    FieldOp,
    /// A kitted tactical agent. The heaviest-armed body in the Site, and deliberately so: it is the
    /// Tactical Response tier, which canon puts between a guard and an MTF operative.
    Makarov,
    /// CIPHER in her white lab coat.
    CipherStandard,
    /// CIPHER in a charcoal formal coat.
    CipherSenior,
    /// CIPHER in a patrol rig with a sidearm.
    CipherField,
    /// CIPHER in a containment suit and respirator. Fully covered, and the most visually distinct body
    /// in the cast.
    CipherHazmat,
}

impl StaffRig {
    /// The asset path, as `AssetServer` wants it.
    pub fn glb(self) -> &'static str {
        match self {
            Self::Researcher => "characters/researcher.glb",
            Self::Scientist => "characters/scientist.glb",
            Self::FieldOp => "characters/fieldop.glb",
            Self::Makarov => "characters/makarov.glb",
            Self::CipherStandard => "characters/cipher_standard.glb",
            Self::CipherSenior => "characters/cipher_senior.glb",
            Self::CipherField => "characters/cipher_field.glb",
            Self::CipherHazmat => "characters/cipher_hazmat.glb",
        }
    }

    /// Every rig, in declaration order. Indexes `StaffAnim`'s per-rig graphs, exactly the way
    /// `Scp1048Variant::index()` indexes that module's `TABLES`.
    pub const ALL: [StaffRig; 8] = [
        Self::Researcher,
        Self::Scientist,
        Self::FieldOp,
        Self::Makarov,
        Self::CipherStandard,
        Self::CipherSenior,
        Self::CipherField,
        Self::CipherHazmat,
    ];

    /// This rig's slot in [`Self::ALL`], and therefore in `StaffAnim`'s per-rig graphs.
    ///
    /// An exhaustive `match` rather than a `position()` over [`Self::ALL`], deliberately. The search
    /// form has to answer "what if it is not in the list?", and the only honest answers are a panic or
    /// a wrong index — while this form cannot compile at all if a variant is added and not given a
    /// slot. `the_index_agrees_with_all` keeps the two in step.
    pub fn index(self) -> usize {
        match self {
            Self::Researcher => 0,
            Self::Scientist => 1,
            Self::FieldOp => 2,
            Self::Makarov => 3,
            Self::CipherStandard => 4,
            Self::CipherSenior => 5,
            Self::CipherField => 6,
            Self::CipherHazmat => 7,
        }
    }
}

/// One authored member of Site-67's staff.
///
/// ⚠️ **`assets/site/staff.ron` is append-only.** [`CastId`] is this entry's index plus an offset, and
/// anything that later keys per-person state keys on that — so reordering the file silently reassigns
/// who is who. Adding to the end is always safe.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaffMember {
    /// Surname as it would appear on a placard. Not a callsign — these are employees, not operatives.
    pub name: String,
    /// The Foundation's own word for this job. See `personnel::StaffTitle`.
    pub title: StaffTitle,
    /// What they are permitted to know. A ceiling on information, never a rank.
    pub clearance: Clearance,
    pub rig: StaffRig,
    /// Which area they are posted to.
    ///
    /// **A post, not a position.** Metre coordinates are deliberately not authorable here: the spawn is
    /// derived from the posted area's own rect by [`post_positions`], the same derived-not-authored
    /// discipline `wall_panels`, `corner_vertices` and `light_the_site` all follow. Move the galley and
    /// the cook moves with it.
    pub post: AreaId,
}

/// Parse a `staff.ron` roster.
pub fn parse_staff_ron(src: &str) -> Result<Vec<StaffMember>, ron::error::SpannedError> {
    ron::from_str(src)
}

/// Load and validate the authored staff.
///
/// Same stance as `squad_ai::persona::load_personas`, and for the same reason: a **missing** file is
/// the normal "this Site has no staff yet" case and yields an empty roster, but a file that is present
/// and malformed is a **loud error, never a silent fall back to nobody**. An author who mistyped a
/// title must see that, not walk into an empty hub and wonder.
///
/// Unlike `load_personas` this is not fixed-width: staff are not index-matched to anything, and there
/// is no `RoleId::ALL` equivalent to validate against. What it does check is that every post is a real
/// area — a typo'd post would otherwise put someone nowhere.
pub fn load_site_staff(path: &str) -> Result<Vec<StaffMember>, String> {
    let src = match std::fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{path}: unreadable: {e}")),
    };
    let list = parse_staff_ron(&src).map_err(|e| format!("{path}: malformed: {e}"))?;
    validate_staff(list).map_err(|e| format!("{path}: {e}"))
}

/// Pure validation, so it is testable without touching the filesystem.
fn validate_staff(list: Vec<StaffMember>) -> Result<Vec<StaffMember>, String> {
    for (i, s) in list.iter().enumerate() {
        if s.name.trim().is_empty() {
            return Err(format!("staff #{i} has no name"));
        }
        // `Corridor` is connective tissue — `AreaId::REQUIRED` deliberately excludes it, and posting
        // someone to "the spine" means posting them to a 34x2 walkway with no verb in it.
        if s.post == AreaId::Corridor {
            return Err(format!(
                "staff #{i} '{}' is posted to the Corridor, which is not a destination — post them \
                 to the room they work in",
                s.name
            ));
        }
        // A D-Class is not staff and must not be authored here: they hold no clearance above 0 and
        // their block is populated separately. Caught at the door rather than at the placard.
        if s.title == StaffTitle::ClassD {
            return Err(format!(
                "staff #{i} '{}' is titled CLASS-D — D-Class are not staff and are not authored in \
                 this file",
                s.name
            ));
        }
    }
    Ok(list)
}

/// A person's plan footprint, in metres. Matches `visuals::AVATAR_HALF` (0.25) with a small margin, so
/// "does this cell clear the furniture" asks the same question the body's own collision does.
const BODY_HALF: f32 = 0.30;
/// How close two people may be posted. Below this they read as one clump rather than two colleagues.
const PEER_GAP: f32 = 1.6;
/// Past this, being nearer a workstation stops meaning anything — it is only used to keep a room with
/// no props at all from scoring every cell identically at infinity.
const WORKSTATION_REACH: f32 = 4.0;

/// Where the `n` people posted to `area` should stand.
///
/// **Derived, never authored** (see [`StaffMember::post`]).
///
/// # The rule, and the one it replaced
///
/// The first version maximised distance from the furniture, on the theory that the thing to avoid was
/// standing inside a bunk. It avoided that and produced something worse: the farthest cell from the
/// furniture is always a **corner**, so all nine staff were posted to rect boundaries facing walls.
/// Measured, then seen — `(14.5, 2.5)`, `(0.5, 14.5)`, `(10.5, 33.5)`, every one on an edge. A room
/// full of people standing in corners reads as broken pathfinding, which is a worse bug than the one
/// being fixed and is legible from any angle.
///
/// So the objective is inverted. A person works **at** their workstation: stand as close to a prop as
/// possible *without overlapping it*. The cook ends up at the counter, the archivist at the desk, the
/// guard at the console — which is both what the fiction wants and what Stage C's use-slots will
/// refine into an actual seat.
///
/// Three terms, one formula, no branches:
///
/// * **Hard** — the body's footprint must not overlap any prop's. Reuses `placement::ir::overlap_area`,
///   the same primitive `check_prop_placements` uses on the authored props themselves, so "clear of the
///   furniture" means exactly one thing in this codebase. This matters because `SiteNav::bake` reads
///   only floor and wall: props are deliberately *not* in the walkable mask, so "walkable" alone would
///   happily stand the cook inside the coffee machine.
/// * **Primary** — minimise distance to the nearest prop, clamped at [`WORKSTATION_REACH`] so a room
///   with no props at all degenerates gracefully to the remaining terms instead of scoring every cell
///   at infinity.
/// * **Secondary** — a penalty for crowding an already-placed peer, and a small pull toward the room's
///   interior so nobody stands with their back inside a wall.
///
/// # Determinism
///
/// No ordering lint is needed and none is being skipped: every input is authored data read in a fixed
/// order (`Rect::cells()` is row-major), there is no ECS query and no RNG, and the pick is broken to a
/// **total** order by `(score, x, z)` — so two equally-good cells cannot tie, and the answer is the
/// same on every boot.
pub fn post_positions(
    layout: &SiteLayout,
    kit: &super::kit::SiteKit,
    nav: &SiteNav,
    area: AreaId,
    n: usize,
) -> Vec<Vec2> {
    use crate::placement::ir::{overlap_area, Footprint};

    let Some(a) = layout.areas.iter().find(|a| a.id == area) else {
        return Vec::new();
    };

    // Every prop standing in this room, as the footprint the placement solver would see. Floor
    // markings are excluded for the reason `check_prop_placements` states: a body standing on a decal
    // is correct, and the 2D model cannot see that one of the two is 5 cm thick and lying down. A
    // prop RESTING on another is excluded for the mirror-image reason — it is 75 cm up in the air,
    // and the table holding it already excludes the floor underneath. Counting the mug as well would
    // shrink where a person may stand on the strength of a mug.
    let mut props: Vec<(Vec2, Footprint)> = layout
        .props
        .iter()
        .filter(|p| a.rect.contains_metres(p.pos))
        .filter(|p| super::layout::occupies_floor(kit, p.piece))
        .map(|p| {
            let (fw, fd) = kit.footprint(p.piece);
            (
                Vec2::new(p.pos.0, p.pos.1),
                Footprint {
                    x: p.pos.0,
                    z: p.pos.1,
                    yaw: p.yaw.to_radians(),
                    hw: fw * 0.5,
                    hd: fd * 0.5,
                },
            )
        })
        .collect();

    // ...and the containment booths, which are **not** in `props`.
    //
    // Found by looking, not by testing: Ito was posted to (16.5, 4.5) and cell 0 stands at (15.5, 4.0),
    // so she spawned *inside the booth* and was invisible behind its walls — a body that exists, is
    // animating, and cannot be seen. `site67.ron` keeps `cells:` as its own list because a cell is
    // derived geometry rather than a kit prop (the cell rooms are walled by the perimeter pass from the
    // glass's placement), so a rule that only reads `props` silently does not apply to a sixth of the
    // containment wing.
    //
    // The booth is a `CELL_DEPTH` square whose near face is the glass and which runs inward from it,
    // so its centre is half a depth in. Square, so its yaw does not matter. `CELL_DEPTH` and
    // `cell_interior_dir` come from `visuals` rather than being re-derived here — one source of truth
    // for where a booth actually is.
    props.extend(layout.cells.iter().filter(|c| a.rect.contains_metres(c.pos)).map(|c| {
        let inward = super::visuals::cell_interior_dir(c.yaw);
        let half = super::visuals::CELL_DEPTH * 0.5;
        let centre = Vec2::new(c.pos.0 + inward.x * half, c.pos.1 + inward.z * half);
        (
            centre,
            Footprint { x: centre.x, z: centre.y, yaw: 0.0, hw: half, hd: half },
        )
    }));

    let (x0, x1, z0, z1) = a.rect.bounds_metres();
    let candidates: Vec<Vec2> = a
        .rect
        .cells()
        .filter(|c| nav.is_walkable(*c))
        // Cell centre, matching `SiteNav::cell_center`'s convention.
        .map(|c| Vec2::new(c.x as f32 + 0.5, c.y as f32 + 0.5))
        .filter(|c| {
            // Hard constraint: the body clears every piece of furniture in the room.
            let body = Footprint { x: c.x, z: c.y, yaw: 0.0, hw: BODY_HALF, hd: BODY_HALF };
            !props.iter().any(|(_, f)| overlap_area(&body, f) > 0.0)
        })
        .collect();

    let mut chosen: Vec<Vec2> = Vec::with_capacity(n);
    for _ in 0..n {
        let mut best: Option<(f32, f32, f32)> = None; // (score, x, z) — a total key
        for c in &candidates {
            if chosen.iter().any(|p| p.distance_squared(*c) < 1.0e-6) {
                continue;
            }
            let to_prop = props
                .iter()
                .map(|(centre, _)| centre.distance(*c))
                .fold(f32::INFINITY, f32::min)
                .min(WORKSTATION_REACH);
            let to_peer = chosen
                .iter()
                .map(|p| p.distance(*c))
                .fold(f32::INFINITY, f32::min);
            let crowding = (PEER_GAP - to_peer).max(0.0);
            // Distance from the nearest wall of the room, so a body is pulled off the boundary when
            // nothing else distinguishes two cells.
            let interior = (c.x - x0).min(x1 - c.x).min(c.y - z0).min(z1 - c.y).min(2.0);

            // Higher is better. Near a workstation dominates; crowding is weighted hard enough that a
            // peer's personal space beats a marginally better desk; interior only breaks ties.
            let score = -to_prop - crowding * 4.0 + interior * 0.25;
            let key = (score, c.x, c.y);
            // Strictly greater on the score, then the positional tiebreak — a total order over
            // authored data, so the result is the same on every boot.
            let better = match best {
                None => true,
                Some(b) => (key.0, -key.1, -key.2) > (b.0, -b.1, -b.2),
            };
            if better {
                best = Some(key);
            }
        }
        match best {
            Some((_, x, z)) => chosen.push(Vec2::new(x, z)),
            // Every walkable cell in the room is inside a piece of furniture, or the room has fewer
            // free cells than people posted to it. Loud, because it means the roster and the layout
            // disagree and someone would otherwise be silently missing from the Site.
            None => {
                warn!(
                    "site: area {area:?} has no cell clear of its furniture for person {} of {n} — \
                     they will not be spawned",
                    chosen.len() + 1
                );
                break;
            }
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cast_ids_never_collide_between_operatives_and_staff() {
        // The whole reason for the offset. Five operatives today, eight slots reserved, so staff must
        // start at 8 — growing the squad into its own headroom must not renumber the archivist.
        let ops: Vec<CastId> = (0..5).map(CastId::of_operative).collect();
        let staff: Vec<CastId> = (0..9).map(CastId::of_staff).collect();
        for o in &ops {
            assert!(!staff.contains(o), "{o:?} is both an operative and a staff member");
        }
        assert_eq!(CastId::of_staff(0).0, crate::knowledge::roster::ROSTER_SLOTS);
        // And the headroom is real: slots 5..8 are unstaffed squad slots, not staff.
        assert!(CastId::of_staff(0).0 > CastId::of_operative(7).0 - 1);
    }

    #[test]
    fn every_rig_resolves_to_a_file_that_ships() {
        // `tests/staff_asset.rs` pins what is *inside* each rig; this pins that the enum still names
        // files that exist. A renamed asset would otherwise be an async load failure at runtime — an
        // invisible body, and one that logs nothing useful about which staff member vanished.
        for rig in StaffRig::ALL {
            let path = format!("assets/{}", rig.glb());
            assert!(
                std::path::Path::new(&path).exists(),
                "{rig:?} points at {path}, which does not exist"
            );
        }
        // The index must round-trip, since it selects the animation graph.
        for (i, rig) in StaffRig::ALL.iter().enumerate() {
            assert_eq!(rig.index(), i);
        }
    }

    #[test]
    fn the_shipped_roster_parses_and_validates() {
        // The anti-"pure library" check for the authored half: the file the game actually loads.
        let staff = load_site_staff(STAFF_PATH).expect("the shipped staff.ron must load");
        assert!(!staff.is_empty(), "Site-67 ships with staff; an empty roster is a regression");
        for s in &staff {
            assert_ne!(s.post, AreaId::Corridor);
            assert_ne!(s.title, StaffTitle::ClassD);
        }
    }

    #[test]
    fn a_missing_roster_is_normal_but_a_malformed_one_is_loud() {
        // Same asymmetry `load_personas` documents, and the same reason: absent means "no staff yet",
        // but present-and-broken means an author is waiting to be told.
        assert!(load_site_staff("assets/site/does_not_exist.ron").expect("absent is fine").is_empty());
        assert!(parse_staff_ron("[ (name: \"x\") ]").is_err(), "a half-authored entry must not parse");
    }

    #[test]
    fn a_corridor_post_and_a_d_class_title_are_both_refused() {
        let corridor = vec![StaffMember {
            name: "Nobody".into(),
            title: StaffTitle::Logistics,
            clearance: Clearance::Level0,
            rig: StaffRig::FieldOp,
            post: AreaId::Corridor,
        }];
        let err = validate_staff(corridor).expect_err("a corridor post must be refused");
        assert!(err.contains("Corridor"), "unhelpful error: {err}");

        let d_class = vec![StaffMember {
            name: "D-9341".into(),
            title: StaffTitle::ClassD,
            clearance: Clearance::Level0,
            rig: StaffRig::FieldOp,
            post: AreaId::Quarters,
        }];
        let err = validate_staff(d_class).expect_err("a D-Class in the staff file must be refused");
        assert!(err.contains("CLASS-D"), "unhelpful error: {err}");
    }

    #[test]
    fn everyone_posted_to_a_room_gets_a_distinct_walkable_cell_clear_of_the_furniture() {
        // The acceptance the plan names, as a pure function so it runs in the hard gate with no `App`.
        let layout = SiteLayout::load().expect("the shipped layout must load");
        let kit = super::super::kit::load_site_kit(super::super::kit::SITE_KIT_PATH, super::super::kit::SITE_PROJECT_DIR)
            .expect("the shipped kit must load");
        let nav = SiteNav::bake(&layout);

        for area in AreaId::REQUIRED {
            let spots = post_positions(&layout, &kit, &nav, *area, 3);
            assert_eq!(spots.len(), 3, "{area:?} could not seat three people");
            for (i, a) in spots.iter().enumerate() {
                // On walkable floor, inside the room.
                let cell = IVec2::new(a.x.floor() as i32, a.y.floor() as i32);
                assert!(nav.is_walkable(cell), "{area:?} spot {i} at {a:?} is not walkable");
                let rect = layout
                    .areas
                    .iter()
                    .find(|x| x.id == *area)
                    .map(|x| x.rect)
                    .expect("area exists");
                assert!(rect.contains_metres((a.x, a.y)), "{area:?} spot {i} is outside its room");
                // ...and distinct, which is the bug this function exists to prevent: three people
                // standing in one another.
                for b in spots.iter().skip(i + 1) {
                    assert!(a.distance(*b) > 0.5, "{area:?} seated two people at {a:?} and {b:?}");
                }
            }
        }
    }

    #[test]
    fn nobody_is_posted_inside_a_containment_booth() {
        // **The regression, and it was invisible in the literal sense.** Ito spawned at (16.5, 4.5)
        // with cell 0's glass at (15.5, 4.0) — inside the booth, behind its walls, animating happily
        // where no camera angle could see her. The body existed, the logs said nine staff, and every
        // test passed.
        //
        // The cause is worth keeping in the failure message: `site67.ron` keeps `cells:` in its own
        // list because a booth is derived geometry rather than a kit prop, so a clearance rule written
        // against `props` alone silently does not cover the containment wing.
        let layout = SiteLayout::load().expect("layout");
        let kit = super::super::kit::load_site_kit(super::super::kit::SITE_KIT_PATH, super::super::kit::SITE_PROJECT_DIR).expect("kit");
        let nav = SiteNav::bake(&layout);
        use crate::placement::ir::{overlap_area, Footprint};

        // Ask for more people than the wing has staff, so the check covers the cells the greedy pick
        // would only reach under crowding.
        let spots = post_positions(&layout, &kit, &nav, AreaId::Containment, 6);
        assert_eq!(spots.len(), 6, "the containment wing could not seat six");
        for (i, s) in spots.iter().enumerate() {
            let body = Footprint { x: s.x, z: s.y, yaw: 0.0, hw: BODY_HALF, hd: BODY_HALF };
            for c in &layout.cells {
                let inward = super::super::visuals::cell_interior_dir(c.yaw);
                let half = super::super::visuals::CELL_DEPTH * 0.5;
                let booth = Footprint {
                    x: c.pos.0 + inward.x * half,
                    z: c.pos.1 + inward.z * half,
                    yaw: 0.0,
                    hw: half,
                    hd: half,
                };
                assert_eq!(
                    overlap_area(&body, &booth),
                    0.0,
                    "person {i} at {s:?} stands inside containment cell {} (glass at {:?}) — they \
                     would be hidden behind the booth walls. `cells:` is a separate list from \
                     `props:` and must be excluded explicitly.",
                    c.index,
                    c.pos
                );
            }
        }
    }

    #[test]
    fn post_positions_are_the_same_on_every_call() {
        // Derived from authored data with no RNG and no query, so this is a property of the code rather
        // than of the data — but it is the property the whole "derive, do not author" stance rests on.
        let layout = SiteLayout::load().expect("layout");
        let kit = super::super::kit::load_site_kit(super::super::kit::SITE_KIT_PATH, super::super::kit::SITE_PROJECT_DIR).expect("kit");
        let nav = SiteNav::bake(&layout);
        let a = post_positions(&layout, &kit, &nav, AreaId::Kitchen, 4);
        let b = post_positions(&layout, &kit, &nav, AreaId::Kitchen, 4);
        assert_eq!(a, b);
    }
}
