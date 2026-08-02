//! **The authored Site-67 layout** — hand-placed geometry, loaded from `assets/site/site67.ron`.
//!
//! ## Why RON and not the placement grammar
//!
//! `crate::placement` exists to *search* for arrangements (`WfcSolver`, `MetropolisSolver`). Routing the
//! Site through it would make the hub's layout a function of a seed, and the design doc is explicit that
//! it must not be: a hub you return to after every expedition has to be **learnable**, which procedural
//! generation actively fights. Every roguelite hub that works is hand-built.
//!
//! ## Why RON and not a `const` table
//!
//! Every layout tweak would be a rebuild, and this file will be tweaked dozens of times before it feels
//! right. RON is also the repo's established shape for authored content (`config.ron`, `personas.ron`,
//! `furniture_kenney.ron`).
//!
//! **Standalone, not a `config.ron` slice**, for the same reason `session::SessionConfig` is excluded
//! from `WorldConfig`: this is level data the offline search must never touch. A search free to evolve
//! the hub's layout would destroy the one property the hub exists to have.
//!
//! ## The schema is a placement LIST, not a grammar
//!
//! `floor` is authored as **rect runs** rather than per-cell, so a 16×10 wing is one row instead of 160
//! — and the walkable mask for [`super::nav`] falls out of it for free. `walls` are on integer cells
//! (greybox readability); `props` are in metres (freedom where it matters).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::pieces::SitePiece;

/// Path to the authored layout, cwd-relative like `config::GAME_CONFIG_PATH`.
pub const SITE_LAYOUT_PATH: &str = "assets/site/site67.ron";

/// Cells a doorway piece spans.
///
/// Both shipped doorway meshes are authored **two tiles** wide —
/// `tests/ozea_asset.rs::the_prop_is_authored_in_metres_on_the_games_grid` pins that against the
/// bytes — so this is a fact about `TILE_SIZE` and the art contract, not about which kit is worn.
const DOORWAY_CELLS: i32 = 2;

/// The areas of Site-67. An enum rather than a string so a typo is a compile error and so
/// [`SiteLayout::validate`] can prove every one of them is present.
///
/// # Two generations, and the difference is deliberate
///
/// The **first six** are the design doc's §2.4 table, where every row named the system that needed a
/// location: the ASYNC door needed FVS-A-5, containment needed FVS-D-4, research needed the Thaumiel
/// tree. The area existed because a mechanic had nowhere to happen.
///
/// The **second five** (2026-08-02) invert that: the space comes first and the mechanics follow. They
/// exist because a Foundation site that contains anomalies is also somewhere people *live* — they
/// sleep, eat, train, plan and watch — and a hub with none of that reads as a facility diagram. That
/// is a Director's call and it is worth stating plainly here, because it is the opposite of the rule
/// the first six were chosen by, and the repo's named top process risk is shipping a room with no
/// verb in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum AreaId {
    /// Leave on an expedition. The only exit.
    AsyncDoor,
    /// Captured specimens, visibly held, one cell each (FVS-D-4).
    Containment,
    /// Run experiments on specimens; the Thaumiel tree (Push 4).
    Research,
    /// Read and write reports — where knowledge propagates between runs (Push 10).
    Records,
    /// Spend the O5 budget on consumables (FVS-P-2).
    Requisition,
    /// The O5 performance review; the Director's standing (FVS-P-1).
    Briefing,
    /// Where the operatives and staff sleep. Bunks, lockers, personal effects.
    Quarters,
    /// The galley. Where a shift starts and where people are found between tasks.
    Kitchen,
    /// Training and recreation — the room that offsets what the field does to people.
    Activities,
    /// Plan the next expedition. **`RunState::Idle` only** — see `docs/2026-08-01-two-live-layers.md`
    /// §5: you may not supervise the squad you left unattended, so this room is dark during a visit.
    WarRoom,
    /// Watches the containment wing it stands beside. Same `RunState::Idle` rule as [`Self::WarRoom`].
    Monitoring,
    /// The spines that join them. Not a destination, but it is floor and it must be walkable.
    Corridor,
}

impl AreaId {
    /// Every area the layout must contain. `Corridor` is deliberately absent — it is connective
    /// tissue, not a destination.
    pub const REQUIRED: &'static [AreaId] = &[
        AreaId::AsyncDoor,
        AreaId::Containment,
        AreaId::Research,
        AreaId::Records,
        AreaId::Requisition,
        AreaId::Briefing,
        AreaId::Quarters,
        AreaId::Kitchen,
        AreaId::Activities,
        AreaId::WarRoom,
        AreaId::Monitoring,
    ];
}

/// An axis-aligned rect of cells, `[x, x+w)` × `[z, z+h)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: i32,
    pub z: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, c: IVec2) -> bool {
        c.x >= self.x && c.x < self.x + self.w && c.y >= self.z && c.y < self.z + self.h
    }
    /// Cell-space `contains` in METRES — props are authored off-grid, so "which room is this in"
    /// is a continuous question. Same half-open convention as [`Self::contains`].
    pub fn contains_metres(&self, p: (f32, f32)) -> bool {
        p.0 >= self.x as f32
            && p.0 < (self.x + self.w) as f32
            && p.1 >= self.z as f32
            && p.1 < (self.z + self.h) as f32
    }
    /// Interior extents in metres, as `ir::escapes_bounds` wants them.
    pub fn bounds_metres(&self) -> (f32, f32, f32, f32) {
        (
            self.x as f32,
            (self.x + self.w) as f32,
            self.z as f32,
            (self.z + self.h) as f32,
        )
    }
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.z < o.z + o.h && o.z < self.z + self.h
    }
    pub fn cells(&self) -> impl Iterator<Item = IVec2> + '_ {
        (self.z..self.z + self.h)
            .flat_map(move |z| (self.x..self.x + self.w).map(move |x| IVec2::new(x, z)))
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
}

/// One named area and the floor it claims.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Area {
    pub id: AreaId,
    /// Shown on the floor decal / signage. Player-facing copy.
    pub label: String,
    pub rect: Rect,
}

/// A wall-kit piece on an integer cell.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WallPlacement {
    pub piece: SitePiece,
    pub cell: (i32, i32),
    /// Yaw in degrees about +Y.
    pub yaw: f32,
}

/// A dressing prop, positioned in metres so it need not sit on the grid.
///
/// **Not `Copy`** since 2026-08-02: it owns a waiver reason. Callers iterate by reference, which they
/// already did.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropPlacement {
    pub piece: SitePiece,
    pub pos: (f32, f32),
    pub yaw: f32,
    /// **Why this prop is exempt from the placement rules** — see [`check_prop_placements`].
    ///
    /// A *reason*, never a boolean. `Some("...")` waives this one prop and prints the reason in the
    /// startup log, so an exemption is greppable, self-documenting, and visibly someone's decision.
    /// A `bool` would let "I could not be bothered" and "this deliberately overhangs the counter"
    /// look identical in the diff, which is exactly the ambiguity the rules exist to remove.
    ///
    /// Defaults to `None`, so the rules apply unless somebody writes down why they should not.
    #[serde(default)]
    pub waive: Option<String>,
}

/// A containment cell that can display one specimen (FVS-D-4).
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CellPlacement {
    /// Display order. Specimens fill cells in `Specimen::captured_tick` order.
    pub index: u32,
    pub pos: (f32, f32),
    pub yaw: f32,
}

/// The ASYNC door: where it stands, and the volume that fires the transition.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DoorPlacement {
    /// The TRIGGER centre, in metres. Must sit on reachable floor — [`SiteLayout::validate`]
    /// flood-fills to it, because a hub whose exit cannot be walked to is a prison.
    pub pos: (f32, f32),
    /// Where the FRAME stands, in metres.
    ///
    /// **Separate from [`Self::pos`], and that separation is the whole point.** The frame belongs in
    /// the perimeter gap — which is deliberately *not* floor, so it can never be the trigger's home —
    /// while the trigger belongs on the floor in front of it. Sharing one field placed the frame a
    /// metre out in the hall, standing free of the wall it is supposed to fill.
    pub frame_pos: (f32, f32),
    pub yaw: f32,
    /// Trigger half-extents in metres (x, y, z), before yaw.
    pub trigger_half_extents: (f32, f32, f32),
}

/// The whole authored hub.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SiteLayout {
    /// World-space offset of cell (0,0).
    ///
    /// The Site lives far from the dungeon so that fog, the knee-wall cutaway, the light field, mould
    /// and almond water — all of which are dungeon-grid indexed — simply never touch it. One mechanism
    /// (distance), rather than a `Visibility` toggle on every Site entity at every state change.
    pub origin: (f32, f32, f32),
    pub areas: Vec<Area>,
    pub floor: Vec<Rect>,
    pub walls: Vec<WallPlacement>,
    pub props: Vec<PropPlacement>,
    pub cells: Vec<CellPlacement>,
    pub door: DoorPlacement,
    /// Where operative avatars stand at the start of an idle period.
    pub spawns: Vec<(f32, f32)>,
}

impl SiteLayout {
    /// Load and validate. **One path, no fallback** — a malformed hub is a content bug that must fail
    /// at the door, exactly like `config::load_game_config`.
    pub fn load() -> Result<Self, String> {
        let text = std::fs::read_to_string(SITE_LAYOUT_PATH)
            .map_err(|e| format!("{SITE_LAYOUT_PATH}: {e}"))?;
        let layout: SiteLayout =
            ron::from_str(&text).map_err(|e| format!("{SITE_LAYOUT_PATH}: {e}"))?;
        layout.validate()?;
        Ok(layout)
    }

    /// Is this cell floored at all?
    ///
    /// Floor is the *architecture*; it is not the same question as "can an operative stand here" — see
    /// [`Self::is_walkable`].
    pub fn is_floor(&self, c: IVec2) -> bool {
        self.floor.iter().any(|r| r.contains(c))
    }

    /// Can an operative stand here? **Floor minus walls.**
    ///
    /// The two are deliberately separate. The first draft of this file made "a wall on floor" a
    /// validation *error*, on the reasoning that nav derives walkability from floor rects and would
    /// report a column cell as walkable, so operatives would clip through it. The validator promptly
    /// caught two columns authored down the spine corridor — which is exactly the greybox detail that
    /// makes a corridor read as architecture rather than a tube.
    ///
    /// The rule was guarding the right hazard with the wrong invariant. Subtracting walls here removes
    /// the hazard at its source, and columns in a corridor become a thing you can author instead of a
    /// thing the validator forbids. What replaces the rule is stronger: [`Self::validate`] flood-fills
    /// the **walkable** mask, so a wall that seals a wing off is still rejected — and now for the reason
    /// that actually matters.
    pub fn is_walkable(&self, c: IVec2) -> bool {
        self.is_floor(c) && !self.walls.iter().any(|w| w.cell == (c.x, c.y))
    }

    /// Flood-fill the walkable mask from `start`.
    pub fn reachable_from(&self, start: IVec2) -> std::collections::HashSet<IVec2> {
        let mut seen = std::collections::HashSet::new();
        if !self.is_walkable(start) {
            return seen;
        }
        let mut stack = vec![start];
        seen.insert(start);
        while let Some(c) = stack.pop() {
            for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let n = c + d;
                if self.is_walkable(n) && seen.insert(n) {
                    stack.push(n);
                }
            }
        }
        seen
    }

    /// World position of a cell centre.
    pub fn cell_center(&self, c: IVec2) -> Vec3 {
        Vec3::new(
            self.origin.0 + c.x as f32 + 0.5,
            self.origin.1,
            self.origin.2 + c.y as f32 + 0.5,
        )
    }

    /// World position of a metre-space point.
    pub fn point(&self, p: (f32, f32)) -> Vec3 {
        Vec3::new(self.origin.0 + p.0, self.origin.1, self.origin.2 + p.1)
    }

    /// Reject a hub that cannot work. Each check is here because the failure it catches would otherwise
    /// surface as "the Site looks wrong" — a bug report with no cause attached.
    pub fn validate(&self) -> Result<(), String> {
        // All six destinations exist. The design doc lists them as the hub's purpose; a missing wing is
        // a silently half-built Site.
        for want in AreaId::REQUIRED {
            if !self.areas.iter().any(|a| a.id == *want) {
                return Err(format!("site layout: missing required area {want:?}"));
            }
        }
        // No area declared twice — two rects claiming one id makes "where is Records?" ambiguous.
        for (i, a) in self.areas.iter().enumerate() {
            if self.areas[..i].iter().any(|b| b.id == a.id) {
                return Err(format!(
                    "site layout: area {:?} declared more than once",
                    a.id
                ));
            }
            if a.rect.is_empty() {
                return Err(format!("site layout: area {:?} has an empty rect", a.id));
            }
        }
        // Areas must not overlap: a cell belonging to two wings breaks the floor decal that makes the
        // hub learnable without signage.
        for (i, a) in self.areas.iter().enumerate() {
            for b in &self.areas[i + 1..] {
                if a.rect.overlaps(&b.rect) {
                    return Err(format!(
                        "site layout: areas {:?} and {:?} overlap",
                        a.id, b.id
                    ));
                }
            }
        }
        // Every area must actually have floor under it.
        for a in &self.areas {
            if !a.rect.cells().all(|c| self.is_floor(c)) {
                return Err(format!(
                    "site layout: area {:?} has cells with no floor",
                    a.id
                ));
            }
        }
        // Operatives must start somewhere they can stand.
        if self.spawns.is_empty() {
            return Err("site layout: no operative spawns".into());
        }
        for s in &self.spawns {
            let c = IVec2::new(s.0.floor() as i32, s.1.floor() as i32);
            if !self.is_walkable(c) {
                return Err(format!("site layout: spawn {s:?} is not on walkable floor"));
            }
        }
        // The door trigger must be reachable, or the hub is a prison.
        let dc = IVec2::new(
            self.door.pos.0.floor() as i32,
            self.door.pos.1.floor() as i32,
        );
        if !self.is_walkable(dc) {
            return Err(format!(
                "site layout: the ASYNC door at {dc:?} is not on walkable floor"
            ));
        }
        let (hx, hy, hz) = self.door.trigger_half_extents;
        if hx <= 0.0 || hy <= 0.0 || hz <= 0.0 {
            return Err("site layout: the door trigger has a non-positive extent".into());
        }
        self.validate_doorway_gap()?;
        // Cell display indices must be unique and dense, so "specimen N goes in cell N" is unambiguous.
        let mut idx: Vec<u32> = self.cells.iter().map(|c| c.index).collect();
        // SORT-OK: the input is an authored RON list, never an ECS query — `site67.ron` yields its
        // cells in file order, which is stable across `App` instances by construction. The sort exists
        // only to check density, and its result is thrown away.
        idx.sort_unstable();
        for (want, got) in idx.iter().enumerate() {
            if *got != want as u32 {
                return Err(format!(
                    "site layout: containment cell indices must be dense from 0; found {idx:?}"
                ));
            }
        }
        // CONNECTIVITY — the check that replaced "no wall on floor", and the one that actually matters.
        // Every area and the door must be reachable from the first spawn across the WALKABLE mask, so a
        // wall or column that seals a wing off is rejected at load rather than discovered by walking at
        // it. This is FVS-G-4's "navigable space" acceptance, proven with no engine involved.
        let start = self.spawns[0];
        let seen = self.reachable_from(IVec2::new(start.0.floor() as i32, start.1.floor() as i32));
        for a in &self.areas {
            if !a.rect.cells().any(|c| seen.contains(&c)) {
                return Err(format!(
                    "site layout: area {:?} is unreachable from the operative spawn",
                    a.id
                ));
            }
        }
        if !seen.contains(&dc) {
            return Err(
                "site layout: the ASYNC door is unreachable from the operative spawn".into(),
            );
        }
        Ok(())
    }

    /// The perimeter gap the ASYNC frame stands in must be exactly as wide as the frame.
    ///
    /// **This is the check whose absence let the signature image break.** The perimeter holds cells
    /// clear so the spawner's doorway is not bricked up, and the spawner stands a frame in them —
    /// but nothing made the two agree. They did not: four cells (4.0 m) were held for a 2.003 m
    /// frame, so a metre of perimeter either side of the aperture was simply open, and the frame
    /// itself was authored at the trigger's position, a metre out on the hall floor.
    ///
    /// The doorway pieces are authored **two tiles** wide — `tests/ozea_asset.rs`'s
    /// `the_prop_is_authored_in_metres_on_the_games_grid` pins that against the bytes — so the gap is
    /// two cells and the frame sits centred on the edge they share. Kit-independent by construction:
    /// it is a fact about `TILE_SIZE` and the layout, not about which mesh is worn.
    fn validate_doorway_gap(&self) -> Result<(), String> {
        let f = self.door.frame_pos;
        let cells = self.doorway_gap_cells();
        if cells.is_empty() {
            return Err(format!(
                "site layout: the ASYNC frame at {f:?} is not standing in a held-clear perimeter \
                 cell — it is floor, or already carries a wall"
            ));
        }
        let span = cells.len() as i32;
        if span != DOORWAY_CELLS {
            return Err(format!(
                "site layout: the ASYNC doorway gap is {span} cells but the frame spans \
                 {DOORWAY_CELLS} — hold exactly {DOORWAY_CELLS} clear in \
                 scripts/gen_site_perimeter.py's DOOR_KEEP_OUT, or the perimeter stands open beside \
                 the aperture"
            ));
        }
        // ...and the frame is centred on the gap. `cells[0]` is the low cell's INDEX, so the run's
        // low edge is there and its centre is half a span further along the run.
        let step = self.doorway_run_step().as_vec2();
        let centre = cells[0].as_vec2() + step * (span as f32 * 0.5) + (Vec2::ONE - step) * 0.5;
        let want = Vec2::new(f.0, f.1);
        if (centre - want).length() > 1e-3 {
            return Err(format!(
                "site layout: the ASYNC frame is authored at {f:?} but its gap is centred on \
                 ({}, {}) — an off-centre frame leaves an uneven reveal",
                centre.x, centre.y
            ));
        }
        Ok(())
    }

    /// Which way the wall run the ASYNC door sits in travels, as a unit cell step.
    ///
    /// The same half-turn convention the walls are authored on (see `site67.ron`'s walls header): a
    /// wall at yaw 90 separates along Z, so its RUN goes along X.
    fn doorway_run_step(&self) -> IVec2 {
        if (45.0..135.0).contains(&self.door.yaw.rem_euclid(180.0)) {
            IVec2::X
        } else {
            IVec2::Y
        }
    }

    /// The contiguous held-clear perimeter cells the ASYNC frame stands in, low end first.
    ///
    /// Empty when the frame is not in a gap at all. **Derived rather than authored**, so the
    /// validator and the spawner cannot disagree about which cells the doorway occupies: the header
    /// course ([`SitePiece::WallHeader`]) is placed on exactly these, and a gap that stops matching
    /// the frame is rejected at load by [`Self::validate`].
    pub fn doorway_gap_cells(&self) -> Vec<IVec2> {
        let f = self.door.frame_pos;
        let start = IVec2::new(f.0.floor() as i32, f.1.floor() as i32);
        // A gap cell: outside the floor (so it can never affect walkability) and carrying no wall.
        let is_gap =
            |c: IVec2| !self.is_floor(c) && !self.walls.iter().any(|w| w.cell == (c.x, c.y));
        if !is_gap(start) {
            return Vec::new();
        }
        let step = self.doorway_run_step();
        // Walk the contiguous gap both ways. The width is checked by `validate_doorway_gap`; the cap
        // here only stops a layout that opens a whole side from walking forever.
        let (mut lo, mut hi) = (start, start);
        for _ in 0..DOORWAY_CELLS * 4 {
            if is_gap(lo - step) {
                lo -= step;
            }
            if is_gap(hi + step) {
                hi += step;
            }
        }
        let span = (hi - lo).dot(step) + 1;
        (0..span).map(|i| lo + step * i).collect()
    }
}

/// **Run Site-67's hand-authored props through the same placement rules the dungeon's solved
/// furniture obeys** — and report every violation at once, with the distance it is out by.
///
/// # Why this exists
///
/// The dungeon's furniture is *solved*: `placement::solvers::metropolis` anneals against energy terms
/// that include footprint overlap and staying inside the room, so a piece through a wall is a state
/// the solver climbs out of. Site-67 is **hand-authored on purpose** (design doc §2.1 — a hub has to
/// be learnable), so its props never enter a solver, and until 2026-08-02 nothing checked them at all.
///
/// The cost of that gap, measured: the first pass at furnishing the living half put three bunks
/// 0.15 m through the west wall (a bunk is 2.29 m long and was laid across a 5 m room), ran the war
/// room's 3.68 m control desk a full metre out into the corridor, pushed three surveillance consoles
/// 0.17 m through the wall they share with containment, and sat three bedside tables inside their own
/// bunks. **Seventeen faults, none of them visible in a screenshot at play zoom.**
///
/// # It reuses the solver's geometry rather than restating it
///
/// [`ir::overlap_area`] and [`ir::escapes_bounds`] are the *same functions* `metropolis` scores with.
/// A private copy here would be a second answer to "do these two overlap", and the two would drift the
/// first time either was tuned.
///
/// # The escape hatch is a sentence, not a flag
///
/// [`PropPlacement::waive`] takes a reason string. A waived prop is skipped and its reason is logged,
/// so an exemption stays greppable and stays somebody's stated decision.
///
/// Props whose piece has no footprint in the kit cannot be checked and are not silently passed —
/// there is no such piece, because `footprint` is a required field on every `KitPiece`.
/// Rendered height (metres) at or below which a piece is a **floor marking** — a decal, a threshold
/// pad, a floor plate — rather than an object standing in the room.
///
/// The shipped kit separates cleanly at this value: markings are 0.05–0.06 m and the next thinnest
/// piece of real furniture is the 0.30 m pipe. It is a fact about what a mesh IS, so it is derived
/// from the mesh's own height rather than from a hand-kept list of piece names that would need a new
/// entry every time the kit grew.
const FLOOR_MARKING_HEIGHT: f32 = 0.15;

/// Does this piece lie flat on the floor, such that things may legitimately stand on it?
fn is_floor_marking(kit: &super::kit::SiteKit, piece: SitePiece) -> bool {
    kit.piece(piece).height * kit.y_scale(piece) <= FLOOR_MARKING_HEIGHT
}

pub fn check_prop_placements(
    layout: &SiteLayout,
    kit: &super::kit::SiteKit,
) -> Result<Vec<String>, String> {
    use crate::placement::ir::{escapes_bounds, overlap_area, Footprint};

    let mut waived = Vec::new();
    // (index, area label, footprint) for every prop the OVERLAP rule applies to.
    let mut solid: Vec<(usize, Footprint)> = Vec::new();
    let mut faults: Vec<String> = Vec::new();

    for (i, p) in layout.props.iter().enumerate() {
        if let Some(reason) = &p.waive {
            waived.push(format!("{:?} at {:?} — waived: {reason}", p.piece, p.pos));
            continue;
        }
        let (fw, fd) = kit.piece(p.piece).footprint;
        let f = Footprint {
            x: p.pos.0,
            z: p.pos.1,
            yaw: p.yaw.to_radians(),
            hw: fw * 0.5,
            hd: fd * 0.5,
        };
        // Which area is it in? Props outside every area are dressing in a corridor — legal, and the
        // bounds rule simply has nothing to measure against, so only the overlap rule applies.
        let area = layout.areas.iter().find(|a| a.rect.contains_metres(p.pos));
        if let Some(area) = area {
            let label = area.label.as_str();
            let out = escapes_bounds(&f, area.rect.bounds_metres());
            if out > 0.02 {
                faults.push(format!(
                    "{:?} at {:?} yaw {} sticks {out:.2} m out of {label} — its footprint is {fw:.2} \
                     x {fd:.2} m, so at this yaw it does not fit where it was put",
                    p.piece, p.pos, p.yaw
                ));
            }
        }
        // ...but the OVERLAP rule only applies to things that occupy space. A floor marking does not:
        // furniture standing on top of a decal is correct, and the first run of this check called six
        // such pairs faults. That is the 2D footprint model's known blind spot — it compares plan
        // outlines and cannot see that one of the two is 5 cm thick and lying on the ground — so the
        // exclusion is stated here rather than waived away six times at the call site.
        if !is_floor_marking(kit, p.piece) {
            solid.push((i, f));
        }
    }

    for (n, (i, a)) in solid.iter().enumerate() {
        for (j, b) in solid.iter().skip(n + 1) {
            let ov = overlap_area(a, b);
            if ov > 0.02 {
                faults.push(format!(
                    "{:?} at {:?} overlaps {:?} at {:?} by {ov:.2} m²",
                    layout.props[*i].piece,
                    layout.props[*i].pos,
                    layout.props[*j].piece,
                    layout.props[*j].pos
                ));
            }
        }
    }

    if faults.is_empty() {
        Ok(waived)
    } else {
        // Every fault at once. Reporting the first would mean N build-run cycles to place N props.
        Err(format!(
            "site layout: {} prop placement(s) break the placement rules —\n  {}\n\
             Fix the position/yaw, or give that prop a `waive: Some(\"reason\")` in site67.ron \
             stating why it is allowed to.",
            faults.len(),
            faults.join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> SiteLayout {
        SiteLayout::load().expect("the shipped site67.ron must parse and validate")
    }

    /// **The floor plan has two sources of truth, and this is the only thing keeping them equal.**
    ///
    /// `scripts/gen_site_perimeter.py` carries a hand-duplicated copy of `site67.ron`'s `floor:` list
    /// in its `FLOOR` literal, because it is a standalone script with no RON parser. Nothing made the
    /// two agree until 2026-08-02.
    ///
    /// Drift here is close to undetectable by eye and catastrophic in a specific way: the generator
    /// emits the perimeter of a building that does not exist, so the new floor keeps the OLD layout's
    /// wall cells sitting **on top of it** — and `is_walkable = is_floor && !wall`, so those become
    /// unwalkable holes in the middle of a room, while the walls the new floor actually needs are
    /// never emitted at all. A room you cannot cross, ringed by nothing.
    ///
    /// Parsed with a regex rather than by importing Python: the point is to compare the two authored
    /// lists, and a parser that shares code with either one would not.
    #[test]
    fn the_perimeter_generator_agrees_with_the_layout_about_where_the_floor_is() {
        let script = std::fs::read_to_string("scripts/gen_site_perimeter.py")
            .expect("the perimeter generator must be readable from the crate root");
        let body = script
            .split_once("FLOOR = [")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(inner, _)| inner)
            .expect("gen_site_perimeter.py must define a FLOOR = [ ... ] literal");

        let mut from_script: Vec<(i32, i32, i32, i32)> = Vec::new();
        for line in body.lines() {
            let Some(open) = line.find('(') else { continue };
            let Some(close) = line[open..].find(')') else {
                continue;
            };
            let nums: Vec<i32> = line[open + 1..open + close]
                .split(',')
                .filter_map(|t| t.trim().parse().ok())
                .collect();
            if let [x, z, w, h] = nums[..] {
                from_script.push((x, z, w, h));
            }
        }

        let from_layout: Vec<(i32, i32, i32, i32)> = shipped()
            .floor
            .iter()
            .map(|r| (r.x, r.z, r.w, r.h))
            .collect();

        assert!(
            !from_script.is_empty(),
            "parsed no rects out of the script's FLOOR list — has its formatting changed?"
        );
        assert_eq!(
            from_script, from_layout,
            "scripts/gen_site_perimeter.py's FLOOR list has drifted from site67.ron's floor: list. \
             Make them identical and RE-RUN the script over the walls: block — a stale perimeter \
             leaves wall cells standing on floor, which `is_walkable` reads as holes you cannot \
             walk through."
        );
    }

    /// The shipped Site passes its own placement rules.
    ///
    /// This is the acceptance test for the whole check: it runs the real `site67.ron` against the real
    /// kit, using the same `ir` geometry the dungeon solver scores with. When it fails it names every
    /// offending prop and the distance it is out by, so fixing a furnishing pass is one read of the
    /// message rather than N build-run cycles.
    #[test]
    fn every_authored_prop_obeys_the_placement_rules() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");
        let waived = check_prop_placements(&shipped(), &kit).expect("the shipped Site must be legal");
        // Waivers are legal, but a silent drift toward "everything is waived" is not. If this ever
        // trips, read the reasons before raising it.
        assert!(
            waived.len() <= 3,
            "{} props are waived out of the placement rules — that is a lot of exceptions: {waived:#?}",
            waived.len()
        );
    }

    /// ...and the rules actually BITE. A check that cannot fail is worse than no check, because it
    /// reads like coverage.
    ///
    /// Both faults below are real ones from the first furnishing pass on 2026-08-02, reproduced: a
    /// bunk laid across a 5 m room so it pushes through the wall, and two props in the same place.
    #[test]
    fn a_prop_through_a_wall_or_inside_another_prop_is_refused() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");

        // A 2.29 m bunk at yaw 0 centred 1.0 m into a room whose west wall is at x = 0.
        let mut l = shipped();
        l.props.push(PropPlacement {
            piece: SitePiece::Bunk,
            pos: (1.0, 29.0),
            yaw: 0.0,
            waive: None,
        });
        let err = check_prop_placements(&l, &kit).expect_err("a bunk through a wall must be refused");
        assert!(
            err.contains("Bunk") && err.contains("sticks"),
            "the message must name the piece and how far out it is: {err}"
        );

        // Two chairs in exactly the same place.
        let mut l = shipped();
        for _ in 0..2 {
            l.props.push(PropPlacement {
                piece: SitePiece::Chair,
                pos: (20.5, 31.0),
                yaw: 0.0,
                waive: None,
            });
        }
        let err = check_prop_placements(&l, &kit).expect_err("two props in one spot must be refused");
        assert!(err.contains("overlaps"), "the message must say what overlapped: {err}");
    }

    /// A waiver is a sentence, and it exempts exactly the prop that carries it.
    #[test]
    fn a_waived_prop_is_skipped_and_says_why() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");
        let mut l = shipped();
        l.props.push(PropPlacement {
            piece: SitePiece::Bunk,
            pos: (1.0, 29.0),
            yaw: 0.0,
            waive: Some("deliberately recessed into the alcove".into()),
        });
        let waived = check_prop_placements(&l, &kit).expect("a waived prop must not fail the check");
        assert!(
            waived.iter().any(|w| w.contains("deliberately recessed")),
            "the reason must be reported so an exemption stays visible: {waived:#?}"
        );
    }

    #[test]
    fn the_shipped_layout_parses_and_validates() {
        let l = shipped();
        assert!(!l.floor.is_empty());
        for want in AreaId::REQUIRED {
            assert!(l.areas.iter().any(|a| a.id == *want), "missing {want:?}");
        }
    }

    #[test]
    fn every_area_and_the_door_are_reachable_from_the_spawn() {
        // G-4's "navigable space" acceptance. `validate` enforces it at load, so this asserts the
        // SHIPPED layout satisfies it — an unreachable wing is the most likely authoring mistake and is
        // invisible until someone walks at it.
        let l = shipped();
        let s0 = l.spawns[0];
        let seen = l.reachable_from(IVec2::new(s0.0.floor() as i32, s0.1.floor() as i32));
        for a in &l.areas {
            assert!(
                a.rect.cells().any(|c| seen.contains(&c)),
                "area {:?} unreachable",
                a.id
            );
        }
        let dc = IVec2::new(l.door.pos.0.floor() as i32, l.door.pos.1.floor() as i32);
        assert!(seen.contains(&dc), "the ASYNC door is unreachable");
    }

    #[test]
    fn a_wall_that_seals_a_wing_off_is_rejected() {
        // The invariant that replaced "no wall may stand on floor". Walling the whole spine splits the
        // hub, and that must fail at load rather than at the player.
        let mut l = shipped();
        for x in 0..34 {
            l.walls.push(WallPlacement {
                piece: SitePiece::Wall,
                cell: (x, 12),
                yaw: 0.0,
            });
            l.walls.push(WallPlacement {
                piece: SitePiece::Wall,
                cell: (x, 13),
                yaw: 0.0,
            });
        }
        assert!(
            l.validate().is_err(),
            "a wall sealing the spine must be rejected"
        );
    }

    #[test]
    fn the_site_is_far_enough_from_any_dungeon() {
        // The Site's isolation from fog, the cutaway, the light field, mould and almond water is
        // achieved by DISTANCE, so that distance is load-bearing rather than decorative. The dungeon's
        // configured maximum is coarse × block cells; 512 m of clearance is many times that and leaves
        // room for the level genome to grow the world.
        let l = shipped();
        assert!(
            l.origin.0 >= 512.0 && l.origin.2 >= 512.0,
            "the Site must sit far outside any dungeon extent, got {:?}",
            l.origin
        );
    }

    #[test]
    fn validation_rejects_overlapping_areas() {
        let mut l = shipped();
        let first = l.areas[0].rect;
        l.areas[1].rect = first;
        assert!(
            l.validate().is_err(),
            "two areas on the same cells must be rejected"
        );
    }

    #[test]
    fn validation_rejects_a_spawn_in_the_void() {
        let mut l = shipped();
        l.spawns[0] = (-9999.0, -9999.0);
        assert!(
            l.validate().is_err(),
            "a spawn off the floor must be rejected"
        );
    }

    #[test]
    fn validation_rejects_sparse_cell_indices() {
        let mut l = shipped();
        if let Some(c) = l.cells.first_mut() {
            c.index = 999;
            assert!(l.validate().is_err(), "cell indices must be dense from 0");
        }
    }
}
