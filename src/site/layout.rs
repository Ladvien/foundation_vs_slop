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

/// The six areas from the design doc §2.4. An enum rather than a string so a typo is a compile error
/// and so [`SiteLayout::validate`] can prove all six are present.
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
    /// The spine that joins them. Not one of the six, but it is floor and it must be walkable.
    Corridor,
}

impl AreaId {
    /// The six the design doc requires. `Corridor` is deliberately absent — it is connective tissue,
    /// not a destination.
    pub const REQUIRED: &'static [AreaId] = &[
        AreaId::AsyncDoor,
        AreaId::Containment,
        AreaId::Research,
        AreaId::Records,
        AreaId::Requisition,
        AreaId::Briefing,
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
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.z < o.z + o.h && o.z < self.z + self.h
    }
    pub fn cells(&self) -> impl Iterator<Item = IVec2> + '_ {
        (self.z..self.z + self.h).flat_map(move |z| (self.x..self.x + self.w).map(move |x| IVec2::new(x, z)))
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
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropPlacement {
    pub piece: SitePiece,
    pub pos: (f32, f32),
    pub yaw: f32,
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
    pub pos: (f32, f32),
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
                return Err(format!("site layout: area {:?} declared more than once", a.id));
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
                    return Err(format!("site layout: areas {:?} and {:?} overlap", a.id, b.id));
                }
            }
        }
        // Every area must actually have floor under it.
        for a in &self.areas {
            if !a.rect.cells().all(|c| self.is_floor(c)) {
                return Err(format!("site layout: area {:?} has cells with no floor", a.id));
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
        let dc = IVec2::new(self.door.pos.0.floor() as i32, self.door.pos.1.floor() as i32);
        if !self.is_walkable(dc) {
            return Err(format!("site layout: the ASYNC door at {dc:?} is not on walkable floor"));
        }
        let (hx, hy, hz) = self.door.trigger_half_extents;
        if hx <= 0.0 || hy <= 0.0 || hz <= 0.0 {
            return Err("site layout: the door trigger has a non-positive extent".into());
        }
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
            return Err("site layout: the ASYNC door is unreachable from the operative spawn".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> SiteLayout {
        SiteLayout::load().expect("the shipped site67.ron must parse and validate")
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
            assert!(a.rect.cells().any(|c| seen.contains(&c)), "area {:?} unreachable", a.id);
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
            l.walls.push(WallPlacement { piece: SitePiece::Wall, cell: (x, 12), yaw: 0.0 });
            l.walls.push(WallPlacement { piece: SitePiece::Wall, cell: (x, 13), yaw: 0.0 });
        }
        assert!(l.validate().is_err(), "a wall sealing the spine must be rejected");
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
        assert!(l.validate().is_err(), "two areas on the same cells must be rejected");
    }

    #[test]
    fn validation_rejects_a_spawn_in_the_void() {
        let mut l = shipped();
        l.spawns[0] = (-9999.0, -9999.0);
        assert!(l.validate().is_err(), "a spawn off the floor must be rejected");
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
