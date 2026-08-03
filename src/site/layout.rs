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
    /// The containment block's corridor — the row the cells open off.
    ///
    /// Until 2026-08-02 this WAS the containment wing: a 16x10 room with six glazed booths standing in
    /// it. The Director's call was that a cell should be a room you walk into, so the wing became a
    /// corridor and the booths became [`Self::ContainmentCell`].
    Containment,
    /// **One cell — a room, not a booth** (FVS-D-4). Declared TWELVE times.
    ///
    /// Repeatable, like [`Self::Corridor`] and for a related reason: "where is Records?" is a question
    /// with one answer, and "where is a containment cell?" is not. Each carries its own `label`
    /// (`CELL 01`..`CELL 12`), which is what the signage and the room tone read, so they are
    /// distinguishable to the player without being distinguishable to the type system.
    ///
    /// A booth was 2 m deep behind a pane and you could only ever look at it. A cell is 3x3 with a
    /// door, which means the specimen inside is something you can stand next to — and, once the door
    /// carries a `Clearance`, something you can be refused.
    ContainmentCell,
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
        AreaId::ContainmentCell,
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

    /// **May this id be declared more than once?**
    ///
    /// Only for ids where "where is the X?" is not a question with a single answer. A corridor is
    /// connective tissue — the Site has a north spine, a south spine, a service ring and two
    /// connectors — and a containment cell is one of twelve. Everything else is a destination, and a
    /// second rect claiming a destination's id makes the hub ambiguous to every system that looks one
    /// up by id.
    pub fn repeatable(self) -> bool {
        matches!(self, AreaId::Corridor | AreaId::ContainmentCell)
    }
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

/// **An interior doorway** — one cell of floor punched through a wall row, and the frame in it.
///
/// # Site-67 has doors now, and the rule it broke was never its own
///
/// Until 2026-08-02 an opening here was the *absence* of wall, so a room could stand open along a
/// whole side. That was inherited from `placement::furnish`'s Backrooms art direction — *"No doors —
/// the Backrooms look leaves every opening as a bare doorway"* — which is a decision about the
/// **dungeon**, where the liminal emptiness is the point. Site-67 is a Foundation facility that
/// contains anomalies. It has doors.
///
/// # Why the doorway cell is FLOOR
///
/// Walls are cells here, so a wall row between two rooms is solid. Making one cell of it floor is
/// what turns a solid row into a door: the flood-fill in [`SiteLayout::validate`] passes straight
/// through, so the hub stays provably connected, while the perimeter pass walls the cells either
/// side because they are non-floor adjacent to floor. A one-cell gap is a door; a whole shared edge
/// is an open side.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Doorway {
    /// The floor cell the opening occupies. The frame stands at its centre.
    pub cell: (i32, i32),
    /// Yaw of the frame, degrees. Same convention as every wall: a frame separating along X is 90.
    pub yaw: f32,
    /// **The clearance a person needs to pass**, or `None` for an unrestricted door.
    ///
    /// `personnel::Clearance` is documented as *"a CEILING ON INFORMATION, not a rank and not an XP
    /// ladder"*, and both confusions are named amateur tells. A door reading it is the first place
    /// that ceiling becomes something the player meets rather than something the roster asserts.
    pub clearance: Option<crate::personnel::Clearance>,
    /// What is on the other side, for the refusal message. Player-facing copy, like `Area::label`.
    pub label: String,
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
    /// Every interior doorway. The ASYNC aperture is [`Self::door`] and is a different thing — it is
    /// the way OUT of the hub, with a state transition behind it rather than a leaf that slides.
    pub doorways: Vec<Doorway>,
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

    /// The area declared with this id.
    pub fn area(&self, id: AreaId) -> Option<&Area> {
        self.areas.iter().find(|a| a.id == id)
    }

    /// **Which area is this cell in?** `None` in a corridor gap or off the floor entirely.
    ///
    /// The hub's missing keystone: until 2026-08-02 `AreaId` was used only to *look up* a rect —
    /// per-wing lighting, staff spawn grouping — and nothing ever asked the question the other way
    /// round. So no system could know which room the player was standing in, which is why every hub
    /// verb was a screen that worked anywhere rather than something that happened somewhere.
    ///
    /// A linear scan is not a shortcut to be optimised later: [`Self::validate`] proves the areas do
    /// not overlap, so the answer is unique and the list is twelve long. Returning the first match is
    /// therefore not a *pick* — there is nothing to tie-break, and no ordering key is owed.
    pub fn area_at(&self, c: IVec2) -> Option<AreaId> {
        self.areas.iter().find(|a| a.rect.contains(c)).map(|a| a.id)
    }

    /// [`Self::area_at`] in metre space — props are authored off-grid, so "which room is this in" is
    /// a continuous question for everything except the player's own footprint.
    pub fn area_at_metres(&self, p: (f32, f32)) -> Option<&Area> {
        self.areas.iter().find(|a| a.rect.contains_metres(p))
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
        // No DESTINATION declared twice — two rects claiming one id makes "where is Records?"
        // ambiguous. `Corridor` is exempt and that is the enum's own definition of it: *"the spines
        // that join them. Not a destination"* — the Site has a north spine, a south spine and two
        // short connectors, and "where is the corridor?" is not a question anybody asks.
        //
        // The exemption exists because the alternative was worse. Until 2026-08-02 those three extra
        // runs were simply absent from `areas` — floor belonging to no area at all — which was
        // harmless while `AreaId` was only ever used to look a rect UP. `area_at` asks the reverse,
        // and a player standing on unclaimed floor is *nowhere*: every presence-driven verb goes
        // quiet and no room-tone emitter owns the air. They were also unlit, `light_the_site` being
        // another per-area pass.
        for (i, a) in self.areas.iter().enumerate() {
            if !a.id.repeatable() && self.areas[..i].iter().any(|b| b.id == a.id) {
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
        self.validate_doorway_plaques()?;
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
    /// **Every doorway has solid wall beside it to hang its plaque on.**
    ///
    /// `site::visuals` derives one plaque per clearance level and hangs it `PLAQUE_BESIDE_DOOR` along
    /// the wall from the opening's centre. That cell has to actually be wall: floor there means the
    /// sign floats in a gap, and a second doorway there means two doors sharing one plate, which reads
    /// as the wrong door being restricted. Neither shows up in a test that only looks at the floor,
    /// and neither is visible from most camera angles — a sign edge-on is a sliver.
    ///
    /// Checked for **both** sides, because the plaque hangs on one and the frame needs the other to
    /// stand against: a doorway with open floor on both sides is not a doorway, it is a gap.
    fn validate_doorway_plaques(&self) -> Result<(), String> {
        for d in &self.doorways {
            let cell = IVec2::new(d.cell.0, d.cell.1);
            if !self.is_floor(cell) {
                return Err(format!(
                    "site layout: the {} doorway at {:?} is not floor — an opening has to be walkable \
                     or the flood-fill cannot pass through it and the room is sealed",
                    d.label, d.cell
                ));
            }
            // The wall the door sits in runs across the direction it separates.
            let step = if (45.0..135.0).contains(&d.yaw.rem_euclid(180.0)) {
                IVec2::X
            } else {
                IVec2::Y
            };
            for side in [step, -step] {
                let beside = cell + side;
                if self.is_floor(beside) {
                    return Err(format!(
                        "site layout: the {} doorway at {:?} has FLOOR at {:?}, along the wall it is \
                         supposed to sit in — so it is a gap rather than a door, and its plaque would \
                         hang in mid-air. Move the opening, or narrow the floor either side of it.",
                        d.label, d.cell, (beside.x, beside.y)
                    ));
                }
            }
        }
        Ok(())
    }

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
                 scripts/gen_site67.py's DOOR_KEEP_OUT, or the perimeter stands open beside \
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
/// How close a seat must be to a surface before it is considered "pulled up to" it, in metres.
///
/// 2.0 m covers a chair tucked at a table and a console operator's seat, and excludes a bench against
/// the far wall of the same room — which should NOT be required to face the table it happens to share
/// a room with.
const SEAT_ADDRESSES_SURFACE_WITHIN: f32 = 2.0;

/// Minimum cosine between a seat's front and the direction to its surface. `0.7` is a 45° cone, so a
/// chair at the corner of a table still passes while a side-on or back-turned one does not.
const SEAT_FACING_MIN_COS: f32 = 0.7;

/// Rendered height (metres) at or below which a piece is a **floor marking** — a decal, a threshold
/// pad, a floor plate — rather than an object standing in the room.
///
/// The shipped kit separates cleanly at this value: markings are 0.05–0.06 m and the next thinnest
/// piece of real furniture is the 0.30 m pipe. It is a fact about what a mesh IS, so it is derived
/// from the mesh's own height rather than from a hand-kept list of piece names that would need a new
/// entry every time the kit grew.
const FLOOR_MARKING_HEIGHT: f32 = 0.15;

/// Does this piece lie flat on the floor, such that things may legitimately stand on it?
/// How far inside a room's opening must stay clear of furniture.
///
/// A body is 0.5 m across (`visuals::AVATAR_HALF`), so this is a person's width plus enough that
/// walking in does not mean squeezing. Deliberately modest: the rooms are 5 m wide and a larger band
/// would forbid the wall runs the rooms are actually built from.
const THRESHOLD_CLEAR: f32 = 1.0;

/// Clear floor a fronted prop must have in front of it, beyond its own footprint.
const FRONT_CLEAR: f32 = 0.6;

/// The ways into a room, and the band inside each that must stay empty.
///
/// A **threshold** is a walkable cell on the room's boundary whose neighbour immediately outside the
/// rect is *also* walkable — i.e. the floor continues out of the room there, so that is a way in.
/// Site-67 has no door leaves: `site67.ron`'s perimeter is generated by walling every non-floor cell
/// beside floor, so two adjacent floor regions simply meet with nothing between them and a room's
/// whole edge can be its entrance.
///
/// Yields `(band, cell, direction)` — the box that must stay clear, and the cell and compass word the
/// error message names so the author can find it on the map.
fn thresholds(rect: &Rect, nav: &super::nav::SiteNav) -> Vec<(crate::placement::ir::Footprint, IVec2, &'static str)> {
    use crate::placement::ir::Footprint;
    let mut out = Vec::new();
    // (outward direction, the word for it, whether the edge runs along X)
    const SIDES: [(IVec2, &str, bool); 4] = [
        (IVec2::new(0, -1), "north", true),
        (IVec2::new(0, 1), "south", true),
        (IVec2::new(-1, 0), "west", false),
        (IVec2::new(1, 0), "east", false),
    ];
    for (dir, word, along_x) in SIDES {
        for c in rect.cells() {
            // Only cells on THIS edge of the rect.
            let on_edge = match (dir.x, dir.y) {
                (0, -1) => c.y == rect.z,
                (0, 1) => c.y == rect.z + rect.h - 1,
                (-1, 0) => c.x == rect.x,
                _ => c.x == rect.x + rect.w - 1,
            };
            if !on_edge || !nav.is_walkable(c) || !nav.is_walkable(c + dir) {
                continue;
            }
            // The band: 1 m along the edge, `THRESHOLD_CLEAR` into the room, with its outer face on the
            // room boundary. The cell centre sits half a metre inside that boundary.
            let inward = -Vec2::new(dir.x as f32, dir.y as f32);
            let centre = Vec2::new(c.x as f32 + 0.5, c.y as f32 + 0.5)
                + inward * (THRESHOLD_CLEAR * 0.5 - 0.5);
            let (hw, hd) = if along_x {
                (0.5, THRESHOLD_CLEAR * 0.5)
            } else {
                (THRESHOLD_CLEAR * 0.5, 0.5)
            };
            out.push((
                Footprint { x: centre.x, z: centre.y, yaw: 0.0, hw, hd },
                c,
                word,
            ));
        }
    }
    out
}

/// How high `p` sits, and on what — `None` when it is a floor-standing piece.
///
/// A `rests_on` piece is authored at the XZ position it should occupy **on** its host; the height is
/// then read off the host rather than authored, so moving the table moves the mug and re-skinning the
/// kit re-seats it. See `kit::KitPiece::rests_on` for why this is derived rather than a per-placement
/// number.
///
/// Returns `Ok(None)` for a floor piece and `Err` for a resting piece with no host in reach — that is
/// a **fault**, not a fallback: seating it at y = 0 would bury a mug in the deck, and nothing about
/// that errors at spawn.
///
/// # Three things a host must be
///
/// **It must offer the class asked for.** `kit.surface_bits(host) & want != 0` — the two-sided match
/// `placement::manifest::validate_manifest` already enforces for the dungeon. This read a `bool` when
/// `rests_on` landed, which made the class decorative: a mug asking for `"worktop"` seated on the
/// specimen slab as happily as on a table.
///
/// **It must be in the same area.** The reach test alone is a 2.5 m radius with no notion of walls,
/// and the Site's rooms are separated by exactly one cell of wall — so a mug authored near a party
/// wall could take its height from a table *in the next room*, which spawns cleanly and looks like a
/// float. [`SiteLayout::area_at_metres`] is what makes this expressible; before it existed the
/// question could not be asked. A prop in a corridor (in no area at all) may only host from the
/// corridor, by the same rule.
///
/// **Its top must be measured the way the host is actually drawn** — see `SiteKit::top_height`.
///
/// Deterministic without an ordering lint: `layout.props` is an authored `Vec` read in file order, and
/// the pick is broken to a **total** key by `(distance, x, z)`, so two equally close hosts cannot tie.
/// ⚠️ That claim is unenforced — `tests/determinism_lint.rs` is textual and cannot see a hand-rolled
/// loop — so it is asserted by `two_hosts_at_equal_distance_are_broken_by_position` below.
pub(crate) fn resting_on(
    layout: &SiteLayout,
    kit: &super::kit::SiteKit,
    p: &PropPlacement,
) -> Option<Result<(f32, usize), String>> {
    let class = kit.rests_on(p.piece)?;
    let want = kit.rests_on_bits(p.piece)?;
    // Compared by RECT, not by id. `AreaId::ContainmentCell` is declared twelve times — repeatable,
    // like `Corridor` — so two different cell rooms share an id, and scoping by id would let a mug in
    // CELL 02 take its height from a table in CELL 01 through the wall between them. The rect is what
    // makes an area a *place*.
    let area = layout.area_at_metres(p.pos).map(|a| a.rect);
    let mut best: Option<(f32, f32, f32, usize)> = None;
    for (ix, q) in layout.props.iter().enumerate() {
        if kit.surface_bits(q.piece) & want == 0 || std::ptr::eq(q, p) {
            continue;
        }
        if layout.area_at_metres(q.pos).map(|a| a.rect) != area {
            continue;
        }
        let d = ((q.pos.0 - p.pos.0).powi(2) + (q.pos.1 - p.pos.1).powi(2)).sqrt();
        if d > super::kit::RESTS_ON_REACH {
            continue;
        }
        // Explicit loop rather than `min_by`: a tied comparator would hand the choice to iteration
        // order, which is the shape `tests/determinism_lint.rs` exists to catch. `(d, x, z)` is total.
        let key = (d, q.pos.0, q.pos.1);
        let better = match best {
            None => true,
            Some(b) => key < (b.0, b.1, b.2),
        };
        if better {
            best = Some((key.0, key.1, key.2, ix));
        }
    }
    Some(match best {
        Some((_, _, _, host)) => Ok((kit.top_height(layout.props[host].piece), host)),
        None => Err(format!(
            "{:?} at {:?} rests on {class:?} but no piece offering that class stands within {:.1} m \
             of it in {} — it would be seated at floor level, buried in the deck, with nothing \
             logged. Move it onto a surface that offers {class:?}, or change its kit entry.",
            p.piece,
            p.pos,
            super::kit::RESTS_ON_REACH,
            layout
                .area_at_metres(p.pos)
                .map_or_else(|| "the corridor".to_string(), |a| a.label.clone()),
        )),
    })
}

/// A flat thing **lying on the deck** — a decal, a line marking, a threshold pad.
///
/// ⚠️ **A resting prop is never one, whatever it measures.** This was a bare height threshold until
/// 2026-08-02, which was fine while everything short was a decal; the dressing pass then added a
/// 0.109 m mug, a 0.04 m data folder and a 0.107 m stack of books, and *every one of them* was
/// silently reclassified as a floor marking. That exempted them from the overlap rule and from the
/// staff-exclusion set — so two mugs in the same spot on the same table were not a fault, and the
/// evidence was a test that passed. The height was never the definition; "is it lying on the floor"
/// was, and `rests_on` is the part of that question the kit can now answer.
pub(crate) fn is_floor_marking(kit: &super::kit::SiteKit, piece: SitePiece) -> bool {
    kit.rests_on(piece).is_none()
        && kit.piece(piece).height * kit.y_scale(piece) <= FLOOR_MARKING_HEIGHT
}

/// Does this piece take up floor space — the question "can a person stand here, and is it in the
/// doorway" actually asks?
///
/// Distinct from [`is_floor_marking`] by exactly one term. A decal does not occupy the floor because
/// you walk over it; a mug does not occupy the floor because it is 75 cm above it, standing on a
/// table that occupies the floor on its own account. The overlap rule wants the *first* exclusion
/// only, because two mugs on one table genuinely do collide.
pub(crate) fn occupies_floor(kit: &super::kit::SiteKit, piece: SitePiece) -> bool {
    !is_floor_marking(kit, piece) && kit.rests_on(piece).is_none()
}

/// What the placement rules found: every fault, and every prop that waived them.
///
/// Split out of [`check_prop_placements`] so the faults can be read as a **list** rather than as one
/// joined string. The dev-only Site editor re-runs the rules after every drag and draws a marker on
/// each offending prop, which means it needs them one by one; recovering them by splitting the error
/// message would be a parser for a human-readable string, and it would break silently the first time
/// that wording changed.
#[derive(Debug, Default, Clone)]
pub struct PlacementReport {
    /// One entry per broken rule.
    pub faults: Vec<PlacementFault>,
    /// One message per prop carrying a `waive`, naming the reason.
    pub waived: Vec<String>,
}

/// One broken placement rule, and which record(s) it is about.
///
/// The message alone was enough while these were only ever printed at startup. The Site editor draws
/// each fault on the offending prop, which needs the record — and recovering it by parsing the piece
/// name and position back out of the message would be a parser for prose.
#[derive(Debug, Clone)]
pub struct PlacementFault {
    /// Index into [`SiteLayout::props`] — the prop to move.
    pub prop: usize,
    /// The other record, for a rule about a *pair*. Only the overlap rule sets this; it names both
    /// props because either one moving would resolve it.
    pub other: Option<usize>,
    /// The human-readable message, unchanged from when these were strings.
    pub message: String,
}

impl PlacementFault {
    fn of(prop: usize, message: String) -> Self {
        PlacementFault {
            prop,
            other: None,
            message,
        }
    }

    fn pair(prop: usize, other: usize, message: String) -> Self {
        PlacementFault {
            prop,
            other: Some(other),
            message,
        }
    }
}

/// Run every placement rule and collect the results. [`check_prop_placements`] is this plus the
/// load-time string formatting — one implementation, two presentations.
pub fn prop_placement_report(layout: &SiteLayout, kit: &super::kit::SiteKit) -> PlacementReport {
    use crate::placement::ir::{escapes_bounds, facing_cosine, overlap_area, Footprint};

    let mut waived = Vec::new();
    // (index, footprint, host index) for every prop the OVERLAP rule applies to. `host` is the prop
    // this one RESTS ON, and it is the only pair the rule is allowed to forgive — see below.
    let mut solid: Vec<(usize, Footprint, Option<usize>)> = Vec::new();
    let mut faults: Vec<PlacementFault> = Vec::new();

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
        let area = layout.area_at_metres(p.pos);
        if let Some(area) = area {
            let label = area.label.as_str();
            let out = escapes_bounds(&f, area.rect.bounds_metres());
            if out > 0.02 {
                faults.push(PlacementFault::of(
                    i,
                    format!(
                        "{:?} at {:?} yaw {} sticks {out:.2} m out of {label} — its footprint is \
                         {fw:.2} x {fd:.2} m, so at this yaw it does not fit where it was put",
                        p.piece, p.pos, p.yaw
                    ),
                ));
            }
        }
        // ...but the OVERLAP rule only applies to things that occupy space. A floor marking does not:
        // furniture standing on top of a decal is correct, and the first run of this check called six
        // such pairs faults. That is the 2D footprint model's known blind spot — it compares plan
        // outlines and cannot see that one of the two is 5 cm thick and lying on the ground — so the
        // exclusion is stated here rather than waived away six times at the call site.
        // ...and a piece that RESTS ON another overlaps that ONE prop completely in plan —
        // `overlap_area` is a 2D outline test and cannot see that one of the two is 75 cm higher.
        //
        // ⚠️ The first cut of this dropped every resting prop from the rule entirely, which is a
        // bigger hole than the one it was patching: two mugs authored at the same spot on the same
        // table were not a fault, and neither was a mug standing in a chair. The exemption is
        // one-pair-wide — this prop against **its own host** — so everything else is still checked,
        // and a resting prop that overlaps another resting prop is caught like anything else.
        if !is_floor_marking(kit, p.piece) {
            let host = match resting_on(layout, kit, p) {
                Some(Ok((_, host))) => Some(host),
                // No host: `resting_on` has already recorded that as its own fault below, and
                // forgiving nothing is the right behaviour for a prop that should not be there.
                Some(Err(_)) => None,
                None => None,
            };
            solid.push((i, f, host));
        }
    }

    // ── Anything that rests on a surface has one to rest on ──
    //
    // The height of a `rests_on` piece is derived from its host (`resting_on`), so a piece authored
    // away from every surface has no height to derive and would be seated at y = 0 — a mug sunk into
    // the deck, which spawns cleanly and logs nothing.
    for (i, p) in layout.props.iter().enumerate() {
        if p.waive.is_some() {
            continue;
        }
        if let Some(Err(e)) = resting_on(layout, kit, p) {
            faults.push(PlacementFault::of(i, e));
        }
    }

    // ── Seats address the surface they are pulled up to ──
    //
    // A chair at a table that faces away from it is the single most legible piece of wrongness a room
    // can have: it reads as a mistake instantly, from any angle, even to someone who has never seen the
    // room before. Every chair in the Site was authored exactly that way on 2026-08-02 — the yaws were
    // written against the ENGINE's facing convention while the mesh fronts a quarter-turn off it — and
    // the fault survived a four-angle visual pass because a sideways chair still looks like a chair.
    //
    // Only pieces that HAVE a front are checked. A stool and a bench measure symmetric, so they get no
    // `front` in the kit and nothing is asserted about them: demanding a facing from a backless seat
    // would be inventing a fact about the art.
    for (i, p) in layout.props.iter().enumerate() {
        if p.waive.is_some() {
            continue;
        }
        let Some(front) = kit.piece(p.piece).front else {
            continue;
        };
        // The nearest surface within reach, if any. Nearest rather than "every surface": a chair
        // between two tables belongs to one of them, and requiring it to face both is unsatisfiable.
        let mut nearest: Option<(f32, &PropPlacement)> = None;
        for q in layout.props.iter().filter(|q| kit.is_surface(q.piece)) {
            let d = ((q.pos.0 - p.pos.0).powi(2) + (q.pos.1 - p.pos.1).powi(2)).sqrt();
            if d <= SEAT_ADDRESSES_SURFACE_WITHIN && nearest.is_none_or(|(bd, _)| d < bd) {
                nearest = Some((d, q));
            }
        }
        let Some((d, target)) = nearest else { continue };
        // `front` is the mesh's own quarter-turn offset from the engine convention — see
        // `kit::KitPiece::front`. Without it this test is exactly 90° wrong for every chair.
        let yaw = (p.yaw + front).to_radians();
        let cos = facing_cosine(p.pos, yaw, target.pos);
        if cos < SEAT_FACING_MIN_COS {
            let want = (target.pos.1 - p.pos.1)
                .atan2(target.pos.0 - p.pos.0)
                .to_degrees();
            faults.push(PlacementFault::of(
                i,
                format!(
                    "{:?} at {:?} yaw {} is {:.0}° off the {:?} it is pulled up to {d:.2} m away — \
                     a seat at a surface must face it. Try yaw {:.0}.",
                    p.piece,
                    p.pos,
                    p.yaw,
                    cos.clamp(-1.0, 1.0).acos().to_degrees(),
                    target.piece,
                    (90.0 - want - front).rem_euclid(360.0),
                ),
            ));
        }
    }

    for (n, (i, a, a_host)) in solid.iter().enumerate() {
        for (j, b, b_host) in solid.iter().skip(n + 1) {
            // The one forgiven pair: a prop and the surface it stands on.
            if *a_host == Some(*j) || *b_host == Some(*i) {
                continue;
            }
            let ov = overlap_area(a, b);
            // ⚠️ The tolerance is RELATIVE for anything small, and it has to be. A flat 0.02 m² is a
            // sane "they are touching, not intersecting" slack for furniture, but the dressing pass
            // added props whose ENTIRE footprint is under it — a mug is 0.136 × 0.105 = 0.014 m² —
            // so two of them could occupy exactly the same point and never reach the threshold. An
            // absolute slack silently stops being a rule once the props get smaller than the slack.
            // A quarter of the smaller of the two is the same judgement expressed proportionally.
            let smallest = (a.hw * a.hd).min(b.hw * b.hd) * 4.0;
            if ov > 0.02_f32.min(smallest * 0.25) {
                faults.push(PlacementFault::pair(
                    *i,
                    *j,
                    format!(
                        "{:?} at {:?} overlaps {:?} at {:?} by {ov:.2} m²",
                        layout.props[*i].piece,
                        layout.props[*i].pos,
                        layout.props[*j].piece,
                        layout.props[*j].pos
                    ),
                ));
            }
        }
    }

    // ── Nothing stands in the way in ──
    //
    // Director's call, 2026-08-02, on seeing the galley: *"ensure there is a rule ... to not have
    // something where the door is."*
    //
    // **Site-67's rooms have no doorways — they have open edges.** Openings here are the *absence* of
    // wall (`site67.ron`'s perimeter header says so), so the galley's entire north edge is its way in,
    // and the author who wrote "work surface along the north wall" was placing counters across a
    // threshold that has no wall in it at all. Measured: x6..10 at z=26 are all walkable and open
    // straight onto the south spine.
    //
    // So the threshold is derived rather than authored: a boundary cell of a room whose neighbour just
    // outside is *also* walkable is a way in, and [`THRESHOLD_CLEAR`] metres inside it must stay empty.
    // That is a fact about the floor, not about the art, so it needs no new measurement.
    let nav = super::nav::SiteNav::bake(layout);
    for area in &layout.areas {
        // **Corridors are exempt, and not as a let-off.** A corridor is connective tissue rather than
        // a destination — `AreaId::REQUIRED` already excludes it for that reason — so *every* cell of
        // the spine borders something walkable and the rule would forbid dressing it at all. The first
        // run flagged the three floor pipes that run its length, which are 0.30 m tall and exactly the
        // kind of thing a corridor is decorated with.
        if area.id == AreaId::Corridor {
            continue;
        }
        for (band, cell, dir) in thresholds(&area.rect, &nav) {
            for (i, f, _) in &solid {
                // A prop standing ON another prop is not in anybody's way — its host is the thing
                // occupying the doorway, and the host is checked on its own account. Flagging the mug
                // as well as the table it sits on names the same defect twice and points at the wrong
                // prop to move.
                if !occupies_floor(kit, layout.props[*i].piece) {
                    continue;
                }
                let ov = overlap_area(f, &band);
                if ov > 0.02 {
                    faults.push(PlacementFault::of(
                        *i,
                        format!(
                            "{:?} at {:?} stands in the way into {} — cell {cell:?} is an OPENING \
                             (the floor continues {dir} out of the room), and \
                             {THRESHOLD_CLEAR:.1} m inside it must stay clear so a person can walk \
                             in. Overlap {ov:.2} m². Move it against a real wall, or waive it with \
                             a reason.",
                            layout.props[*i].piece, layout.props[*i].pos, area.label
                        ),
                    ));
                }
            }
        }
    }

    // ── Nothing faces a wall ──
    //
    // Same call: *"it also can't face the wall. That's dumb."* A fronted prop — a seat, a console, an
    // appliance with a service side — must have open floor in front of it. Turned into a wall it reads
    // as a mistake instantly, from any angle, which is exactly the argument the seat-faces-surface rule
    // above already makes.
    //
    // Only pieces that declare a `front` are checked, for the reason that rule gives: demanding a
    // facing from a symmetric mesh would be inventing a fact about the art.
    for (i, p) in layout.props.iter().enumerate() {
        if p.waive.is_some() {
            continue;
        }
        let piece = kit.piece(p.piece);
        let Some(front) = piece.front else { continue };
        let yaw = (p.yaw + front).to_radians();
        // Far enough ahead to clear the prop's own footprint at any rotation, plus room for a person.
        let (fw, fd) = piece.footprint;
        let reach = 0.5 * fw.max(fd) + FRONT_CLEAR;
        let ahead = (p.pos.0 + yaw.sin() * reach, p.pos.1 + yaw.cos() * reach);
        let cell = IVec2::new(ahead.0.floor() as i32, ahead.1.floor() as i32);
        if !nav.is_walkable(cell) {
            faults.push(PlacementFault::of(
                i,
                format!(
                    "{:?} at {:?} yaw {} faces a wall — {reach:.2} m in front of it is cell \
                     {cell:?}, which is not walkable floor. Turn it to face the room.",
                    p.piece, p.pos, p.yaw
                ),
            ));
        }
    }

    PlacementReport { faults, waived }
}

/// The load-time check: `Ok` lists the waived props, `Err` reports every fault at once.
///
/// Reporting only the first fault would mean N build-run cycles to place N props, which is the whole
/// reason this returns them together.
pub fn check_prop_placements(
    layout: &SiteLayout,
    kit: &super::kit::SiteKit,
) -> Result<Vec<String>, String> {
    let report = prop_placement_report(layout, kit);
    if report.faults.is_empty() {
        Ok(report.waived)
    } else {
        Err(format!(
            "site layout: {} prop placement(s) break the placement rules —\n  {}\n\
             Fix the position/yaw, or give that prop a `waive: Some(\"reason\")` in site67.ron \
             stating why it is allowed to.",
            report.faults.len(),
            report
                .faults
                .iter()
                .map(|f| f.message.as_str())
                .collect::<Vec<_>>()
                .join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rect of a room, by id. Tests that need "somewhere in the galley" ask for it rather than
    /// writing a literal — the Site's coordinates have moved wholesale twice in one day, and every
    /// literal in this module broke both times. Derive, don't author, applies to tests too.
    fn room(l: &SiteLayout, id: AreaId) -> Rect {
        l.area(id).unwrap_or_else(|| panic!("{id:?} must exist")).rect
    }

    /// A point `inset` metres inside a room's east wall, centred on its depth.
    fn by_east_wall(r: Rect, inset: f32) -> (f32, f32) {
        ((r.x + r.w) as f32 - inset, r.z as f32 + r.h as f32 * 0.5)
    }

    /// Where the briefing room's mess table actually is.
    fn briefing_table() -> (f32, f32) {
        let l = shipped();
        let b = room(&l, AreaId::Briefing);
        l.props
            .iter()
            .find(|p| p.piece == SitePiece::MessTable && b.contains_metres(p.pos))
            .map(|p| p.pos)
            .expect("the briefing room ships a mess table")
    }

    /// The middle of the briefing room, clear of anything it ships.
    fn briefing_centre() -> (f32, f32) {
        let b = room(&shipped(), AreaId::Briefing);
        (b.x as f32 + b.w as f32 * 0.5, b.z as f32 + b.h as f32 * 0.5)
    }

    fn shipped() -> SiteLayout {
        SiteLayout::load().expect("the shipped site67.ron must parse and validate")
    }

    /// **The floor plan has two sources of truth, and this is the only thing keeping them equal.**
    ///
    /// **A door plaque needs a wall to hang on, and the rule bites.**
    ///
    /// A check that cannot fail reads like coverage. The failure it guards is invisible from most
    /// camera angles — a sign seen edge-on is a sliver — and invisible to every floor-only test.
    #[test]
    fn a_doorway_with_open_floor_beside_it_is_refused() {
        let mut l = shipped();
        // Widen one doorway into a two-cell gap by flooring the cell next to it along its own wall.
        let d = l.doorways[0].clone();
        let step = if (45.0..135.0).contains(&d.yaw.rem_euclid(180.0)) { (1, 0) } else { (0, 1) };
        l.floor.push(Rect { x: d.cell.0 + step.0, z: d.cell.1 + step.1, w: 1, h: 1 });
        let err = l
            .validate_doorway_plaques()
            .expect_err("a doorway with floor beside it is a gap, not a door");
        assert!(
            err.contains(&d.label) && err.contains("mid-air"),
            "the message must name the door and say what breaks: {err}"
        );

        // ...and the shipped Site has solid wall beside every one of its doors.
        shipped()
            .validate_doorway_plaques()
            .expect("every shipped doorway must have a wall to hang its plaque on");
    }

    /// Every clearance-gated door wears its level, and an open one wears nothing.
    #[test]
    fn a_restricted_door_is_countable_from_across_the_corridor() {
        use crate::personnel::Clearance;
        let l = shipped();
        let gated: Vec<_> = l.doorways.iter().filter(|d| d.clearance.is_some()).collect();
        assert!(
            gated.len() >= 13,
            "the twelve cells and the block itself are Level 2; found {}",
            gated.len()
        );
        for d in &gated {
            let n = d.clearance.expect("filtered").rank();
            assert!(
                (1..=5).contains(&n),
                "{} would hang {n} plaques — a Level 0 door is not restricted and must be authored \
                 as `None`, or it wears no sign and reads as open anyway",
                d.label
            );
        }
        // The living half is deliberately open: a site where people cannot reach their own bunks is
        // a prison, and a plaque on the galley door would say the opposite.
        assert!(
            l.doorways
                .iter()
                .any(|d| d.label == "GALLEY" && d.clearance.is_none()),
            "the galley must be unrestricted"
        );
        assert_eq!(Clearance::Level2.rank(), 2);
    }

    /// **The authored walls must BE the boundary of the authored floor.**
    ///
    /// This replaces a test that compared `site67.ron`'s `floor:` against a hand-duplicated copy of
    /// it inside `gen_site_perimeter.py`. That script is gone: `scripts/gen_site67.py` now derives the
    /// floor AND the perimeter in one pass, so the two cannot disagree at generation time. The drift
    /// that remains is somebody hand-editing the RON afterwards, and comparing against a *script* can
    /// never catch that — so this asserts the invariant itself instead.
    ///
    /// The failure it guards is unchanged and still nasty: `is_walkable = is_floor && !wall`, so a
    /// stale wall cell standing on floor is an unwalkable hole in the middle of a room, and a missing
    /// one is a gap you can see straight through. Neither shows up anywhere else.
    #[test]
    fn every_wall_cell_is_exactly_the_boundary_of_the_floor() {
        let l = shipped();
        let floor: std::collections::HashSet<(i32, i32)> = l
            .floor
            .iter()
            .flat_map(|r| r.cells().map(|c| (c.x, c.y)).collect::<Vec<_>>())
            .collect();

        // Orthogonally adjacent to floor, or touching it only diagonally (a room's convex corner —
        // the notch you could see through at all eighteen of them until 2026-08-01).
        let mut want: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
        for &(x, z) in &floor {
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = (x + dx, z + dz);
                if !floor.contains(&n) {
                    want.insert(n);
                }
            }
        }
        let ortho = want.clone();
        for &(x, z) in &floor {
            for (dx, dz) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let c = (x + dx, z + dz);
                if floor.contains(&c) || ortho.contains(&c) {
                    continue;
                }
                if [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(ox, oz)| floor.contains(&(c.0 + ox, c.1 + oz)))
                {
                    continue;
                }
                want.insert(c);
            }
        }
        // ...minus the ASYNC aperture's own gap, which its frame fills.
        for c in l.doorway_gap_cells() {
            want.remove(&(c.x, c.y));
        }

        let got: std::collections::HashSet<(i32, i32)> = l
            .walls
            .iter()
            .filter(|w| w.piece == SitePiece::Wall)
            .map(|w| w.cell)
            .collect();

        let missing: Vec<_> = want.difference(&got).copied().collect();
        let extra: Vec<_> = got.difference(&want).copied().collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "the walls: block is not the boundary of the floor: list. Re-run \
             `python3 scripts/gen_site67.py` and splice its WALLS block.\n  \
             {} cells missing (a gap you can see through): {:?}\n  \
             {} cells extra (an unwalkable hole standing on floor): {:?}",
            missing.len(),
            &missing[..missing.len().min(12)],
            extra.len(),
            &extra[..extra.len().min(12)],
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

        // A 2.29 m bunk at yaw 0 centred half a metre inside the quarters' west wall, so it pushes
        // through. Derived from the room, not written down: these coordinates have moved twice.
        let mut l = shipped();
        let q = room(&l, AreaId::Quarters);
        l.props.push(PropPlacement {
            piece: SitePiece::Bunk,
            pos: (q.x as f32 + 0.5, q.z as f32 + q.h as f32 * 0.5),
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

    /// A seat pulled up to a surface must address it — and a backless one must not be asked to.
    ///
    /// Both halves matter. The first caught all eight seats in the Site on 2026-08-02: their yaws were
    /// authored against the ENGINE's facing convention (`forward = local +Z`) while `chair.glb` and
    /// `command_chair.glb` front local **+X**, measured from where the backrest mass sits. Every chair
    /// was a quarter-turn sideways to its table, and a four-angle visual pass had already missed it,
    /// because a sideways chair still looks like a chair.
    ///
    /// The second half is why `stool` and `bench` carry no `front` in the kit: they measure symmetric
    /// to within a centimetre because they genuinely have no back, so asserting a facing on them would
    /// be asserting something about the art that is not true.
    #[test]
    fn a_seat_at_a_surface_must_face_it_and_a_backless_one_is_exempt() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");

        // A chair beside the galley's mess table, turned side-on to it.
        let galley = room(&shipped(), AreaId::Kitchen);
        let table = shipped()
            .props
            .iter()
            .find(|p| p.piece == SitePiece::MessTable && galley.contains_metres(p.pos))
            .map(|p| p.pos)
            .expect("the galley ships a mess table");
        let mut l = shipped();
        l.props.push(PropPlacement {
            piece: SitePiece::Chair,
            // Beside whichever mess table the galley actually ships, offset along +X so the chair
            // is side-on to it. Derived, so a re-laid galley cannot silently move this into a wall
            // and let the WALL rule fire first, masking the facing fault this test is about.
            pos: (table.0 + 1.1, table.1),
            yaw: 0.0,
            waive: None,
        });
        let err = check_prop_placements(&l, &kit).expect_err("a side-on chair must be refused");
        assert!(
            err.contains("must face it") && err.contains("Try yaw"),
            "the message must say what is wrong AND what to do: {err}"
        );

        // The same spot, same yaw, with a STOOL — no back, so no facing to assert.
        let mut l = shipped();
        l.props.push(PropPlacement {
            piece: SitePiece::Stool,
            pos: (8.5, 29.7),
            yaw: 0.0,
            waive: None,
        });
        check_prop_placements(&l, &kit)
            .expect("a backless stool has no front, so no facing may be demanded of it");
    }

    /// **Nothing stands in the way into a room, and a corridor is exempt.**
    ///
    /// Director's call on seeing the galley, 2026-08-02: *"ensure there is a rule ... to not have
    /// something where the door is."* The thing that made this a real bug rather than a near-miss is
    /// that Site-67's rooms have **no doors** — an opening here is the absence of wall, so the galley's
    /// entire north edge is its entrance, and three counters were authored across it against a "north
    /// wall" that does not exist. Measured before the fix: 31 faults across ten props in five rooms.
    #[test]
    fn nothing_may_stand_in_the_way_into_a_room_but_a_corridor_is_all_threshold() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");

        // A locker planted in the galley's own doorway. Found by asking the layout which cell that
        // is rather than writing it down, then standing just inside it.
        let galley = room(&shipped(), AreaId::Kitchen);
        let l0 = shipped();
        let d = l0
            .doorways
            .iter()
            .find(|d| d.label == "GALLEY" && d.cell.1 < galley.z)
            .expect("the galley has a door off the south spine");
        let in_the_way = (d.cell.0 as f32 + 0.5, galley.z as f32 + 0.4);
        let mut l = shipped();
        l.props.push(PropPlacement {
            piece: SitePiece::Locker,
            pos: in_the_way,
            yaw: 0.0,
            waive: None,
        });
        let err = check_prop_placements(&l, &kit).expect_err("a prop in the doorway must be refused");
        assert!(
            err.contains("stands in the way into GALLEY") && err.contains("OPENING"),
            "the message must name the room and say the edge is an opening: {err}"
        );

        // The SAME piece a couple of metres further in is fine — the rule is about the threshold, not
        // about the room's edge in general.
        let mut l = shipped();
        l.props.push(PropPlacement {
            piece: SitePiece::Locker,
            pos: (8.5, 29.9),
            yaw: 0.0,
            waive: None,
        });
        check_prop_placements(&l, &kit).expect("a prop clear of the threshold is fine");

        // And a corridor is exempt, because every one of its cells borders something walkable. The
        // shipped Site dresses the spine with three floor pipes; without the exemption the rule
        // forbids decorating a corridor at all.
        let mut l = shipped();
        l.props.push(PropPlacement {
            piece: SitePiece::Pipe,
            pos: (20.0, 12.5),
            yaw: 0.0,
            waive: None,
        });
        check_prop_placements(&l, &kit).expect("a corridor is connective tissue, not a destination");
    }

    /// **Nothing faces a wall.** Same call: *"it also can't face the wall. That's dumb."*
    ///
    /// Only pieces that declare a `front` are checked — the same restraint the seat rule takes, and for
    /// the same reason: demanding a facing from a symmetric mesh would be inventing a fact about the
    /// art. A chair turned into the wall passes the seat rule whenever there is no surface within
    /// reach, which is exactly the gap this closes.
    #[test]
    fn a_fronted_prop_must_have_open_floor_in_front_of_it() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");

        // A chair in the middle of the quarters' east wall, turned to face into it. No surface is
        // within `SEAT_ADDRESSES_SURFACE_WITHIN`, so only the wall rule can catch this.
        let against_the_wall = by_east_wall(room(&shipped(), AreaId::Quarters), 0.5);
        let mut l = shipped();
        l.props.push(PropPlacement {
            piece: SitePiece::Chair,
            // Forward is `(sin yaw, cos yaw)` and `front` is +90°, so an authored yaw of 0 faces
            // +X — straight into the quarters' east wall.
            pos: against_the_wall,
            yaw: 0.0,
            waive: None,
        });
        let err = check_prop_placements(&l, &kit).expect_err("a chair facing a wall must be refused");
        assert!(
            err.contains("faces a wall"),
            "the message must say what is wrong: {err}"
        );

        // Turned around, into the room, the same chair in the same spot is fine.
        let mut l = shipped();
        l.props.push(PropPlacement {
            piece: SitePiece::Chair,
            pos: against_the_wall,
            yaw: 180.0,
            waive: None,
        });
        check_prop_placements(&l, &kit).expect("a chair facing the room is fine");
    }

    /// **A prop that rests on a surface finds one, and takes its height from it.**
    ///
    /// The height of a `rests_on` piece is derived from its host, never authored — `PropPlacement` has
    /// no height field at all. The failure this guards is silent by construction: a mug authored away
    /// from every surface would seat at y = 0, spawn cleanly, log nothing, and be buried in the deck.
    #[test]
    fn a_resting_prop_finds_its_host_and_a_stranded_one_is_refused() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");
        let l = shipped();

        // Every resting prop the Site actually ships resolves to a host, and to a host that is
        // genuinely a surface rather than whatever happened to be nearest.
        let mut resting = 0;
        for p in &l.props {
            let Some(rest) = resting_on(&l, &kit, p) else { continue };
            resting += 1;
            let (top, host_ix) = rest.unwrap_or_else(|e| panic!("{e}"));
            let host = &l.props[host_ix];
            let want = kit.rests_on_bits(p.piece).expect("a resting piece asks for a class");
            assert!(
                kit.surface_bits(host.piece) & want != 0,
                "{:?} rests on {:?}, which does not offer {:?}",
                p.piece,
                host.piece,
                kit.rests_on(p.piece),
            );
            assert_eq!(
                l.area_at_metres(host.pos).map(|a| a.rect),
                l.area_at_metres(p.pos).map(|a| a.rect),
                "{:?} took its height from a host in another room",
                p.piece,
            );
            assert!(top > 0.0, "{:?} would be seated at floor level", p.piece);
        }
        assert!(resting >= 4, "the Site ships resting dressing; found {resting}");

        // ...and one authored in open floor is a loud fault, naming the piece.
        let mut l = shipped();
        l.props.push(PropPlacement {
            // The middle of the ASYNC hall — deliberately the emptiest floor in the Site.
            piece: SitePiece::Mug,
            pos: (5.5, 6.5),
            yaw: 0.0,
            waive: None,
        });
        let err = check_prop_placements(&l, &kit).expect_err("a stranded mug must be refused");
        assert!(
            err.contains("rests on") && err.contains("Mug"),
            "the message must name the piece and the relation: {err}"
        );
    }

    /// A resting prop is exempt from the FLOOR rules, and that is not a loophole.
    ///
    /// `overlap_area` is a plan-view test: a mug on a table overlaps that table completely and cannot
    /// see that one of the two is three-quarters of a metre higher. The same exemption
    /// `is_floor_marking` takes, for the same reason.
    #[test]
    fn a_mug_on_a_table_does_not_count_as_overlapping_it() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");
        let mut l = shipped();
        // Wholly inside the briefing room's mess table — maximum plan overlap with the host, offset
        // just enough to clear the folder and the mug the room already ships, which the rule DOES
        // judge. Derived from wherever that table actually is.
        let table = briefing_table();
        l.props.push(PropPlacement {
            piece: SitePiece::Mug,
            pos: (table.0 + 0.2, table.1 - 0.2),
            yaw: 0.0,
            waive: None,
        });
        check_prop_placements(&l, &kit)
            .expect("a mug standing on a table is the point, not an overlap");
    }

    /// ...but the exemption is exactly ONE pair wide.
    ///
    /// The first cut of the rests-on work dropped every resting prop out of the overlap rule
    /// altogether, which forgave far more than the one pair it meant to: two mugs authored at the
    /// same spot on the same table were not a fault, and neither was a mug standing in a chair. The
    /// prop-vs-**host** pair is the only thing a plan-view test genuinely cannot judge.
    #[test]
    fn two_mugs_in_the_same_spot_on_the_same_table_are_still_a_fault() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");
        let mut l = shipped();
        for _ in 0..2 {
            l.props.push(PropPlacement {
                piece: SitePiece::Mug,
                pos: briefing_table(),
                yaw: 0.0,
                waive: None,
            });
        }
        let err = check_prop_placements(&l, &kit)
            .expect_err("two mugs occupying one spot is a fault, host or no host");
        assert!(
            err.contains("overlaps"),
            "the message must name the overlap: {err}"
        );
    }

    /// **The surface class has to mean something.** A mug asks for a `worktop`; the specimen slab
    /// offers only `support`.
    ///
    /// When `rests_on` first landed, `resting_on` bound the requested class purely to interpolate it
    /// into an error string and then tested the host with a `bool` — so this exact arrangement was
    /// accepted and a mug sat on the slab beside whatever was laid out on it. This is the test that
    /// makes the two-sided match load-bearing rather than decorative.
    #[test]
    fn a_mug_may_not_rest_on_the_specimen_slab() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");
        // The premise, asserted rather than assumed: the slab is a surface, but not that class.
        let want = kit
            .rests_on_bits(SitePiece::Mug)
            .expect("a mug rests on something");
        assert!(kit.is_surface(SitePiece::Slab), "the slab is a surface");
        assert_eq!(
            kit.surface_bits(SitePiece::Slab) & want,
            0,
            "the slab must not offer what a mug asks for"
        );

        let l = shipped();
        let slab = l
            .props
            .iter()
            .find(|p| p.piece == SitePiece::Slab)
            .expect("the research wing ships a slab");
        let mut l2 = shipped();
        l2.props.push(PropPlacement {
            piece: SitePiece::Mug,
            pos: slab.pos,
            yaw: 0.0,
            waive: None,
        });
        let err = check_prop_placements(&l2, &kit)
            .expect_err("a mug on the specimen slab must be refused");
        assert!(
            err.contains("rests on") && err.contains("Mug"),
            "the message must name the piece and the relation: {err}"
        );
    }

    /// **A host across a wall is not a host.** The reach test is a radius and knows nothing of walls.
    ///
    /// Site rooms are separated by exactly one cell, so a 2.5 m radius reaches comfortably into the
    /// next room; a prop authored near a party wall could take its height from a table it has no
    /// physical relationship with, spawn cleanly, and read as a float. `area_at_metres` is what makes
    /// the question expressible at all.
    #[test]
    fn a_prop_may_not_take_its_height_from_a_table_in_the_next_room() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");
        // Stripped to the two props under test, so the answer is about the wall and not about which
        // of the Site's fourteen surfaces happened to be nearest.
        let mut l = shipped();
        l.props.retain(|p| !kit.is_surface(p.piece) && kit.rests_on(p.piece).is_none());

        // Two neighbouring cell rooms, separated by a single wall column. A table just inside one
        // and a mug just inside the other are within reach of each other, and on opposite sides of a
        // wall. Taken from the authored rects so doubling the cells cannot silently un-test this.
        let cells: Vec<Rect> = l
            .areas
            .iter()
            .filter(|a| a.id == AreaId::ContainmentCell)
            .map(|a| a.rect)
            .collect();
        let (a, b) = (cells[0], cells[1]);
        let table: (f32, f32) = ((a.x + a.w) as f32 - 0.5, a.z as f32 + 0.5);
        let mug: (f32, f32) = (b.x as f32 + 0.5, b.z as f32 + 0.5);
        assert!(
            ((mug.0 - table.0).powi(2) + (mug.1 - table.1).powi(2)).sqrt()
                < super::super::kit::RESTS_ON_REACH,
            "the point of this test is that it IS within reach"
        );
        assert_ne!(
            l.area_at_metres(mug).map(|a| a.rect),
            l.area_at_metres(table).map(|a| a.rect),
            "the two points must be in different ROOMS — and by rect, not by id: both of these are \
             `ContainmentCell`, which is exactly the case that made scoping by id wrong"
        );
        for (piece, pos) in [(SitePiece::MessTable, table), (SitePiece::Mug, mug)] {
            l.props.push(PropPlacement { piece, pos, yaw: 0.0, waive: None });
        }
        let mug_prop = l.props.last().expect("just pushed").clone();
        let err = resting_on(&l, &kit, &mug_prop)
            .expect("a mug rests on something")
            .expect_err("a host on the other side of a wall must not count");
        assert!(
            err.contains("CELL 02"),
            "the message must name the room it looked in, in the room's own player-facing words \
             rather than as an enum variant — `Kitchen` is the variant and `GALLEY` is the room: {err}"
        );
    }

    /// The host pick is total, and the lint cannot prove it.
    ///
    /// `tests/determinism_lint.rs` scans for `min_by`/`sort_by`, so a hand-rolled loop like
    /// `resting_on`'s passes it silently — the totality claim in that function's doc comment is
    /// exactly the kind of comment this repo has learned not to trust. Two hosts at *identical*
    /// distance must still be separated, by position, in file order or reversed.
    #[test]
    fn two_hosts_at_equal_distance_are_broken_by_position() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");
        let build = |reversed: bool| {
            let mut l = shipped();
            // Every shipped surface removed: otherwise the briefing room's own mess table sits
            // nearer than either of the two this test is trying to make tie.
            l.props
                .retain(|p| !kit.is_surface(p.piece) && kit.rests_on(p.piece).is_none());
            // Exact halves: 0.4 apart is NOT equidistant in f32, and the rounding decided the tie
            // before the tiebreak could — which made the test pass for the wrong reason.
            // Exact halves either side of the room's centre: 0.4 apart is NOT equidistant in f32,
            // and the rounding decided the tie before the tiebreak could — which made this pass for
            // the wrong reason.
            let c = briefing_centre();
            let mut tables = vec![(c.0 - 0.5, c.1), (c.0 + 0.5, c.1)];
            if reversed {
                tables.reverse();
            }
            for pos in tables {
                l.props.push(PropPlacement {
                    piece: SitePiece::MessTable,
                    pos,
                    yaw: 0.0,
                    waive: None,
                });
            }
            l.props.push(PropPlacement {
                piece: SitePiece::Mug,
                // Exactly equidistant from both.
                pos: briefing_centre(),
                yaw: 0.0,
                waive: None,
            });
            let mug = l.props.last().expect("just pushed").clone();
            let (_, host_ix) = resting_on(&l, &kit, &mug)
                .expect("a mug rests on something")
                .unwrap_or_else(|e| panic!("{e}"));
            l.props[host_ix].pos
        };
        assert_eq!(
            build(false),
            build(true),
            "a tie broken by authoring order would make the Site's dressing depend on file order"
        );
        let c = briefing_centre();
        assert_eq!(build(false), (c.0 - 0.5, c.1), "the lower x wins the tie");
    }

    /// **Which room is this?** — the question nothing could ask before 2026-08-02.
    #[test]
    fn every_authored_area_answers_for_its_own_cells_and_nothing_else() {
        let l = shipped();
        for a in &l.areas {
            for c in a.rect.cells() {
                assert_eq!(
                    l.area_at(c),
                    Some(a.id),
                    "{:?} does not claim its own cell {c:?}",
                    a.id
                );
            }
        }
        // Off the map entirely is `None`, not a nearest-guess. "Nowhere" is a real answer — the
        // corridor gaps between wings are floorless, and a hub verb must not fire in one.
        assert_eq!(l.area_at(IVec2::new(-5, -5)), None);
        assert_eq!(l.area_at(IVec2::new(9_999, 9_999)), None);
    }

    /// The kit records the measured facing, and it is the quarter turn that caused the bug.
    #[test]
    fn the_seat_meshes_declare_the_front_that_was_measured_off_them() {
        let kit = crate::site::kit::load_site_kit(crate::site::kit::SITE_KIT_PATH)
            .expect("the shipped kit must load");
        for seat in [SitePiece::Chair, SitePiece::CommandChair] {
            assert_eq!(
                kit.piece(seat).front,
                Some(90.0),
                "{seat:?} fronts local +X (backrest mass at -X), a quarter turn off the engine's +Z"
            );
        }
        for backless in [SitePiece::Stool, SitePiece::Bench] {
            assert_eq!(
                kit.piece(backless).front,
                None,
                "{backless:?} measured symmetric — it has no front and none may be asserted"
            );
        }
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
