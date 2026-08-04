//! **The asset descriptor** — one schema for what a mesh is, where it may go, and what it offers.
//!
//! Replaces the two parallel schemas the game grew: `placement::manifest::ManifestItem` (open,
//! string-keyed, with `Role`/`tags`/`affordances`) and `site::kit::KitPiece` (closed enum, with
//! `front`/`scale`/`rests_on`). Neither was a superset; they intersected on `glb`, `height`,
//! `footprint`, `surfaces` and `y_offset` and diverged everywhere else. See
//! `docs/2026-08-03-asset-schema-audit.md` §4 for the field-by-field comparison.
//!
//! # Every field is optional, and that is the design
//!
//! A [`Descriptor`] is a **patch**, not an instance: absence means *inherit*, never *zero*. That
//! shape is deliberate rather than lazy — Bevy 0.19's BSN scenes are patches that layer over
//! defaults, and if a first-party `.bsn` asset loader lands, a descriptor that is already a patch
//! ports mechanically instead of becoming a fourth migration. It also gives per-instance overrides in
//! a map for free: the base says what a crate is, the placement says what *this* crate is.
//!
//! The price is that "is this descriptor usable?" becomes a question with an answer —
//! [`Descriptor::resolve`] — instead of being true by construction. That is the right trade: a
//! missing footprint should be one loud error naming the id, not a silent `(0.0, 0.0)` reservation
//! that lets a prop overlap everything.
//!
//! # `mount` is the layering axis
//!
//! It replaces `Role`, `rests_on`, and — importantly — the height heuristic in
//! `site::layout::is_floor_marking`, which inferred "this is a decal" from a mesh being under 15 cm
//! and had already misclassified a 10.9 cm mug. Layering is now something an author *states*.
//!
//! [`Mount::Overlay`] is the case that could not be expressed at all before: a decal on a wall. The
//! Site's props carried no Y, no host and no normal, so the only wall-mounted object in the game was
//! `DoorPlaque`, hardcoded in Rust and never authored.

use serde::{Deserialize, Serialize};


/// What an asset is. A patch — see the module docs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Descriptor {
    /// Stable identity, opaque to everything here. The only required field.
    pub id: String,
    /// Path to the mesh, relative to the consuming project's asset root.
    pub mesh: Option<String>,
    pub align: Align,
    pub extent: Extent,
    pub mount: Option<Mount>,
    /// Space that must stay free around the piece. See [`Clearance`].
    pub clearance: Vec<Clearance>,
    pub offers: Offers,

    // ── The typed tag axes ────────────────────────────────────────────────────────────────────
    //
    // Three lists rather than one, because the alternative was tried and named a bug. The kit split
    // `surfaces` (what a piece OFFERS) from `affordances` (what it is FOR) after conflating them let
    // a mug seat itself on a bed — "exactly the 'prop rests on a bed' bug", as that comment puts it.
    // Appearance, category and effect are three more axes, and folding them into one `tags` list
    // would make a query for "something that recharges stamina" able to match a brown thing.
    /// What it *is*: `"furniture"`, `"table"`, `"light-source"`, `"food"`.
    pub kind: Vec<String>,
    /// What it *does* to whoever uses it or to the world: `"stamina-recharge"`, `"uses-electricity"`.
    pub effects: Vec<String>,
    /// What it *looks like*: `"brown"`, `"rusted"`, `"metal"`. Never matched by gameplay; present so
    /// an author can search the palette the way they think about it.
    pub look: Vec<String>,

    /// Hints for the generator — where this belongs and what it belongs *with*. See [`Placement`].
    pub placement: Placement,

    /// The tile's internal lattice. See [`Subgrid`].
    pub subgrid: Subgrid,

    /// What this asset is and why it is set up the way it is, as data.
    ///
    /// Same argument as [`crate::map::Map::note`]: prose a serializer can lose is prose that gets
    /// lost. The kit's own entries carry paragraphs of it — why `wall_low` is the one piece still
    /// scaled, how `front` was derived — and today that survives only because nothing re-serializes
    /// the file.
    pub note: Option<String>,
}

/// **A tile's internal lattice** — the thing that lets two pieces agree on where they meet.
///
/// A descriptor's [`Extent::footprint`] says how much floor a piece takes; `grid::cells` rounds that
/// up to whole cells. That is enough to stop two pieces overlapping and not enough for anything else:
/// it cannot say that an L-shaped desk leaves its inner corner free, that a table's four sides each
/// seat someone, or that this wall segment may only abut another wall segment. Those are three
/// questions about *where inside the tile*, and the tile had no inside.
///
/// One lattice answers all three, because they are facets of the same fact — what is at (x, y, z)
/// within this piece:
///
/// * [`SubCell::solid`] — occupancy. Clearance and flood fill can respect the shape rather than the
///   bounding box.
/// * [`SubCell::edge`] — what the cell presents to the neighbour. WFC matches a tile's face against
///   the facing cells of the tile beside it, which is what makes a corridor meet a corridor.
/// * [`SubCell::anchor`] — a role an interacting item may occupy. The regular-grid sibling of
///   [`Offers::sockets`]: a socket is a hand-placed point, an anchor is a lattice cell.
///
/// # Sparse, because most tiles have nothing to say
///
/// 3×3×3 is 27 cells and the shipped library is 43 mostly-rectangular props. Storing 27 entries each
/// would be 1,161 rows of `solid: false` — so only cells that differ from open-and-unlabelled are
/// written, and a piece with no lattice detail costs one defaulted field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Subgrid {
    /// Divisions per axis, `(x, y, z)`.
    ///
    /// **3×3×3.** Three is the smallest division with a *middle* — a centre distinct from its two
    /// sides — which is what "left edge / interior / right edge" needs. Two would give a tile no
    /// inside, and four would double the authoring for a symmetry nothing yet asks for.
    pub div: (u32, u32, u32),
    /// Only the cells that are not plain open space. See the note on sparseness.
    pub cells: Vec<SubCell>,
}

impl Default for Subgrid {
    fn default() -> Self {
        Subgrid {
            div: (3, 3, 3),
            cells: Vec::new(),
        }
    }
}

impl Subgrid {
    /// How many cells the lattice has, if it were written out in full.
    pub fn volume(&self) -> u32 {
        self.div.0 * self.div.1 * self.div.2
    }

    /// What is at `at`, or `None` for a cell nobody has said anything about.
    pub fn at(&self, at: (u32, u32, u32)) -> Option<&SubCell> {
        self.cells.iter().find(|c| c.at == at)
    }

    /// Is `at` inside the lattice?
    pub fn holds(&self, at: (u32, u32, u32)) -> bool {
        at.0 < self.div.0 && at.1 < self.div.1 && at.2 < self.div.2
    }

    /// The cell at `at`, created empty if nobody has written one yet.
    ///
    /// `None` when `at` is outside the lattice — a caller asking about a cell that cannot exist gets
    /// an answer, not a row appended somewhere unreachable.
    fn entry(&mut self, at: (u32, u32, u32)) -> Option<&mut SubCell> {
        if !self.holds(at) {
            return None;
        }
        if let Some(i) = self.cells.iter().position(|c| c.at == at) {
            return self.cells.get_mut(i);
        }
        self.cells.push(SubCell {
            at,
            ..SubCell::default()
        });
        self.cells.last_mut()
    }

    /// **Drop any cell that has gone back to saying nothing.**
    ///
    /// The sparse invariant is not decoration: an author who marks a cell solid and then unmarks it
    /// must leave the file as it was, or every tile ever poked at accretes rows of `solid: false`
    /// that mean exactly what absence means.
    fn prune(&mut self) {
        self.cells
            .retain(|c| c.solid || c.edge.is_some() || c.anchor.is_some());
    }

    /// Toggle a cell's occupancy. Returns what it became, or `None` if `at` is outside.
    pub fn toggle_solid(&mut self, at: (u32, u32, u32)) -> Option<bool> {
        let now = {
            let cell = self.entry(at)?;
            cell.solid = !cell.solid;
            cell.solid
        };
        self.prune();
        Some(now)
    }

    /// Set or clear a cell's edge label. An empty string clears it — the same keystroke that types a
    /// token has to be able to take it back.
    pub fn set_edge(&mut self, at: (u32, u32, u32), token: &str) -> Option<()> {
        let cell = self.entry(at)?;
        cell.edge = (!token.trim().is_empty()).then(|| token.trim().to_owned());
        self.prune();
        Some(())
    }

    /// Set or clear a cell's anchor role.
    pub fn set_anchor(&mut self, at: (u32, u32, u32), token: &str) -> Option<()> {
        let cell = self.entry(at)?;
        cell.anchor = (!token.trim().is_empty()).then(|| token.trim().to_owned());
        self.prune();
        Some(())
    }

    /// Forget everything about a cell.
    pub fn clear(&mut self, at: (u32, u32, u32)) {
        self.cells.retain(|c| c.at != at);
    }

    /// **The lattice as it sits after `quarter` 90° turns about +Y.**
    ///
    /// A placement carries a yaw ([`crate::map::Placed::yaw`]) and the lattice does not know about
    /// it, so anything comparing two placed tiles face to face has to turn one of them first.
    /// Reading a face straight off the authored lattice would be silently wrong for every rotated
    /// piece — which is exactly the piece a face-matching rule exists to check.
    ///
    /// The convention is the project's one forward rule: **a positive yaw turns +X toward −Z**
    /// (`stack::covers`). So local +X becomes −Z, and a cell on the +X face lands on the −Z face:
    ///
    /// ```text
    /// (x, y, z) -> (z, y, dx - 1 - x)      div (dx, dy, dz) -> (dz, dy, dx)
    /// ```
    ///
    /// Four turns are the identity, which `rotating_four_times_is_the_identity` pins.
    ///
    /// Lives here rather than beside the matcher because the schema owns its own transforms for the
    /// same reason it owns its own edits: the sparse invariant is this type's to keep.
    pub fn rotated(&self, quarter: u8) -> Subgrid {
        let mut out = self.clone();
        for _ in 0..(quarter % 4) {
            let (dx, dy, dz) = out.div;
            out = Subgrid {
                div: (dz, dy, dx),
                cells: out
                    .cells
                    .iter()
                    .map(|c| SubCell {
                        at: (c.at.2, c.at.1, dx.saturating_sub(1) - c.at.0.min(dx.saturating_sub(1))),
                        ..c.clone()
                    })
                    .collect(),
            };
        }
        out
    }

    /// Refuse a lattice that cannot be true.
    ///
    /// Every rule here is one whose violation is silent: a zero division makes the lattice empty
    /// while still claiming to have one, an out-of-range cell is a value nothing will ever read, and
    /// a duplicate is two answers to one question with the first quietly winning.
    pub fn validate(&self, owner: &str) -> Result<(), String> {
        let (dx, dy, dz) = self.div;
        if dx == 0 || dy == 0 || dz == 0 {
            return Err(format!(
                "`{owner}`'s subgrid divides {dx}x{dy}x{dz}; an axis with no divisions has no cells"
            ));
        }
        let mut seen: Vec<(u32, u32, u32)> = Vec::with_capacity(self.cells.len());
        for c in &self.cells {
            if c.at.0 >= dx || c.at.1 >= dy || c.at.2 >= dz {
                return Err(format!(
                    "`{owner}`'s subgrid cell {:?} is outside its {dx}x{dy}x{dz} lattice",
                    c.at
                ));
            }
            if seen.contains(&c.at) {
                return Err(format!(
                    "`{owner}`'s subgrid names cell {:?} twice — one cell, one answer",
                    c.at
                ));
            }
            seen.push(c.at);
        }
        Ok(())
    }
}

/// One cell of a [`Subgrid`]. See that type for what the three facets are for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SubCell {
    /// Which cell, `(x, y, z)`, zero-based.
    pub at: (u32, u32, u32),
    /// Solid space — what clearance and a flood fill must respect.
    pub solid: bool,
    /// What this cell presents to whatever is placed beside it. Matched face-to-face.
    pub edge: Option<String>,
    /// A role an interacting item may occupy here — `"diner"`, `"shelf-item"`.
    pub anchor: Option<String>,
}

/// Where a piece belongs, for whatever is placing it. **Not a semantic axis.**
///
/// `kind`/`effects`/`look` describe the thing itself; this describes where a *generator* should
/// consider putting it. The distinction matters because both are lists of opaque strings and it would
/// be very easy to have one list — which is precisely the free-text soup the audit found, where
/// `affordances`, `tags` and `group` were three unvalidated vocabularies nobody could tell apart.
///
/// These two survive the migration on their merits: `furnish::room_profile` matches `rooms` against
/// room types to choose a room's freestanding set, and items sharing a `group` are drawn together by
/// a soft `Near` relation. Unlike `category` and the four unread affordance tokens, something reads
/// them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Placement {
    /// Room-type tokens this piece suits — *a toilet suits a bathroom*. Cross-validated against the
    /// project's room types, the way `mycelia::validate_damp_coverage` already checks its own table
    /// against `config.ron:dungeon.room_types`.
    pub rooms: Vec<String>,
    /// Pieces sharing a token are drawn together (a toilet and a basin), as a soft relation rather
    /// than a hard constraint — a room that cannot fit both should still get one.
    pub group: Option<String>,
}

/// Every layer a piece can be put on, in the order an editor should cycle them.
///
/// The list is built from the project's surface vocabulary rather than hardcoded, because
/// `OnSurface` is the one variant whose payload is a *token* — a project with a `worktop` and a
/// `shelf` should offer both, and a project with neither should offer no surface mount at all rather
/// than one that can never match.
///
/// Order is deliberate: the common cases first. Most pieces stand on the floor, and an author
/// cycling to find `OnFloor` past four exotic variants is an author who will bind their own key.
pub fn mount_options(surfaces: &[String]) -> Vec<Mount> {
    let mut out = vec![
        Mount::OnFloor,
        Mount::OnSurface {
            class: String::new(),
        },
        Mount::OnWall { height: 1.8 },
        Mount::OnCeiling,
        Mount::Tiled,
        Mount::Overlay { on: OverlayHost::Floor },
        // The same default `OnWall` offers, because it is the same question: eye level for a sign.
        Mount::Overlay {
            on: OverlayHost::Wall { height: 1.8 },
        },
        Mount::Overlay {
            on: OverlayHost::Ceiling,
        },
        Mount::InOpening { clear: None },
    ];
    // Replace the placeholder with one entry per real class, so every offered mount can actually be
    // satisfied by something in this project.
    let at = out
        .iter()
        .position(|m| matches!(m, Mount::OnSurface { class } if class.is_empty()));
    if let Some(at) = at {
        out.remove(at);
        for (i, class) in surfaces.iter().enumerate() {
            out.insert(at + i, Mount::OnSurface { class: class.clone() });
        }
    }
    out
}

/// A short label for a mount, for a panel that has one line to say it in.
pub fn mount_label(mount: Option<&Mount>) -> String {
    match mount {
        None => "unset".to_owned(),
        Some(Mount::OnFloor) => "on floor".to_owned(),
        Some(Mount::OnSurface { class }) => format!("on {class}"),
        Some(Mount::OnWall { height }) => format!("on wall at {height:.1} m"),
        Some(Mount::OnCeiling) => "on ceiling".to_owned(),
        Some(Mount::Tiled) => "tiled".to_owned(),
        Some(Mount::Overlay { on }) => match on {
            OverlayHost::Floor => "overlay on floor".to_owned(),
            OverlayHost::Ceiling => "overlay on ceiling".to_owned(),
            OverlayHost::Wall { height } => format!("overlay on wall at {height:.1} m"),
        },
        Some(Mount::InOpening { clear }) => match clear {
            Some((w, h)) => format!("in opening {w:.2} x {h:.2} m"),
            None => "in opening".to_owned(),
        },
    }
}

/// Corrections for what the artist got wrong. Every one is measured, never dialled by eye — the kit's
/// own doc says so of `scale`, and the importer exists to make that true of the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Align {
    /// Uniform art correction: real-world size ÷ authored size.
    pub scale: Option<f32>,
    /// Stretch on Y alone to reach a target height, in metres.
    ///
    /// **Game policy, not an art fact** — a 2.0 m wall mesh made to reach a 2.4 m wall height is a
    /// statement about one game's architecture. It lives here for now because that is where the Site
    /// kit kept it (as a derived `y_scale`), but under the patch model it is more honestly a
    /// project-level layer over the descriptor base. Open question 3 in the plan.
    pub stretch_y: Option<f32>,
    /// Metres to lift off the ground plane. A geometric fix — the Ozea floor plate is 0.06 m thick and
    /// so are its inlays, so a decal at y = 0 is exactly coplanar and the depth winner is undefined.
    /// Not a depth bias.
    pub y_offset: Option<f32>,
    /// Local XZ offset of the mesh's bbox centre from its origin. Placement reasons about a footprint
    /// symmetric about the origin, so an off-centre mesh seated against a wall pokes through it.
    pub pivot: Option<(f32, f32)>,
    /// Degrees to add to an authored yaw to reach the engine convention (`forward = (sin, cos)`).
    ///
    /// `None` means *the mesh is symmetric and has no front*, which is a different claim from
    /// `Some(0.0)`. The kit records that distinction deliberately: a stool measures symmetric to
    /// within a centimetre, and "asserting a facing on a stool would be asserting a fact about the art
    /// that is not true."
    pub front: Option<f32>,
}

/// How much room it takes. Metres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Extent {
    /// (width, depth) before yaw.
    pub footprint: Option<(f32, f32)>,
    /// Top of the piece. For anything offering a surface, this is the plane props rest on.
    pub height: Option<f32>,
}

/// Where a piece attaches — the layering axis, replacing `Role` + `rests_on` + the floor-marking
/// height heuristic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Mount {
    /// Stands on the floor. The common case.
    OnFloor,
    /// Fixed to a wall face at this height in metres (a sconce at 1.8).
    OnWall { height: f32 },
    /// Hangs from the ceiling.
    OnCeiling,
    /// Fills a doorway. `clear` is the (width, height) of the hole in metres — not derivable from the
    /// mesh's own height, since jambs and a lintel sit inside its bounding box.
    ///
    /// `None` means **nobody has measured it**, which is the honest state of the shipped
    /// `furniture_kenney.ron` door: it carries `role: Anchor(host: Opening)` and no opening size,
    /// because the manifest schema has no field for one. The Site kit does record it
    /// (`DoorPiece::opening`, measured off the POSITION accessors), so a converted kit piece carries
    /// `Some`. Making this required would have forced the converter to invent a number for a row that
    /// has never had one.
    InOpening { clear: Option<(f32, f32)> },
    /// Rests on another piece that offers this surface class.
    OnSurface { class: String },
    /// **Lies flat on a host surface** — a decal, a floor marking, a wall poster, a ceiling stain.
    ///
    /// The case the old schemas could not express. `Overlay` claims no volume and never participates
    /// in the overlap rule: two decals may share a wall, which is the whole point of them.
    ///
    /// The host carries the height where a height is a thing that exists — see [`OverlayHost`].
    Overlay { on: OverlayHost },
    /// Laid on a grid by a tiling solver.
    Tiled,
}

/// The plane a decal lies on.
///
/// Its own enum rather than [`crate::placement::ir::Host`] for two reasons, and both are about not
/// being able to say something meaningless. `Opening` is not a plane you can stick a poster to. And a
/// **wall** needs a height while a floor and a ceiling do not: the map states where those are, and
/// nothing anywhere states how high a poster hangs.
///
/// That last one shipped as a hole. `Mount::OnWall` has carried a height since it was written, and
/// `Overlay` — the variant that exists *for* wall decals — had nowhere to put one, so a wall overlay
/// could be authored and then had no answer for where it went. Encoding it in the variant means the
/// unanswerable state is not constructible rather than merely rejected.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum OverlayHost {
    /// A floor marking — a hazard stripe, a painted lane, a stain. Lies on the map's floor.
    Floor,
    /// A ceiling stain or a painted marking. Lies on the map's ceiling.
    Ceiling,
    /// A poster, a sign, a scorch mark, at this height in metres — the same field `OnWall` carries,
    /// for the same reason.
    Wall { height: f32 },
}

/// Which way a clearance requirement points, relative to the piece's own front.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Dir {
    Front,
    Back,
    Left,
    Right,
    /// All four sides — a dining table wants room to pull chairs out on every edge.
    Around,
}

/// Space that must stay free, in metres.
///
/// Tutenel et al. (2010) distinguish **off-limits** features, which may overlap nothing, from
/// **clearance** features, which may overlap only other clearance features — so two chairs can share
/// the space they each need to be pulled out into, but neither may share it with a wall.
///
/// Numbers come from Merrell et al. 2011, already implemented here as `solvers::metropolis`: 0.91 m
/// beside a bed, 0.76 m in front of a seat, 0.61 m in front of shelving, 0.91 m around a dining table.
///
/// Without this the schema cannot forbid a chair flush against a wall with its seat socket inside the
/// wall — the same class of bug as the mug misclassification, one level up.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clearance {
    pub dir: Dir,
    pub dist: f32,
}

/// What a piece makes available to others.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Offers {
    /// Surface classes this piece's top provides, from the closed vocabulary in
    /// [`crate::placement::surfaces`].
    pub surfaces: Vec<String>,
    /// Named attachment points — where a thing goes, or where an agent stands.
    pub sockets: Vec<Socket>,
}

/// A named point on a piece, in its own local space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Socket {
    pub id: String,
    /// Which interaction role may occupy it — `"diner"`, `"merchant"`.
    ///
    /// Free now, expensive later. A table plus four chairs is *one* affordance with four seats, not
    /// four affordances (FFXV's smart locations; Smart Zones' main/supporting/extra strata), and role
    /// allocation needs somewhere to attach. Adding it after interactions ship is a schema migration.
    pub role: Option<String>,
    /// Local position (x, y, z), metres.
    pub at: (f32, f32, f32),
    /// Local yaw, degrees — which way an occupant faces.
    pub yaw: f32,
}

/// A descriptor with every inherited field filled in and the required ones proven present.
///
/// The runtime and the solvers consume this; [`Descriptor`] is only ever the authored form.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub id: String,
    pub mesh: String,
    pub scale: f32,
    pub stretch_y: Option<f32>,
    pub y_offset: f32,
    pub pivot: (f32, f32),
    pub front: Option<f32>,
    pub footprint: (f32, f32),
    pub height: f32,
    pub mount: Mount,
    pub clearance: Vec<Clearance>,
    pub offers: Offers,
    pub kind: Vec<String>,
    pub effects: Vec<String>,
    pub look: Vec<String>,
}

impl Descriptor {
    /// Layer `patch` over `self`, field by field. A field the patch leaves empty is inherited.
    ///
    /// List fields **replace** rather than concatenate. Appending would make a patch unable to
    /// *remove* a tag, and "add only" is the kind of one-way door that turns into a second mechanism
    /// for removal later.
    pub fn patched_with(&self, patch: &Descriptor) -> Descriptor {
        Descriptor {
            id: if patch.id.is_empty() {
                self.id.clone()
            } else {
                patch.id.clone()
            },
            mesh: patch.mesh.clone().or_else(|| self.mesh.clone()),
            align: Align {
                scale: patch.align.scale.or(self.align.scale),
                stretch_y: patch.align.stretch_y.or(self.align.stretch_y),
                y_offset: patch.align.y_offset.or(self.align.y_offset),
                pivot: patch.align.pivot.or(self.align.pivot),
                front: patch.align.front.or(self.align.front),
            },
            extent: Extent {
                footprint: patch.extent.footprint.or(self.extent.footprint),
                height: patch.extent.height.or(self.extent.height),
            },
            mount: patch.mount.clone().or_else(|| self.mount.clone()),
            clearance: pick(&self.clearance, &patch.clearance),
            offers: Offers {
                surfaces: pick(&self.offers.surfaces, &patch.offers.surfaces),
                sockets: pick(&self.offers.sockets, &patch.offers.sockets),
            },
            kind: pick(&self.kind, &patch.kind),
            effects: pick(&self.effects, &patch.effects),
            look: pick(&self.look, &patch.look),
            // A patch that states a lattice replaces it, on the same rule the lists follow: an
            // append could not remove a cell.
            subgrid: if patch.subgrid == Subgrid::default() {
                self.subgrid.clone()
            } else {
                patch.subgrid.clone()
            },
            placement: Placement {
                rooms: pick(&self.placement.rooms, &patch.placement.rooms),
                group: patch.placement.group.clone().or_else(|| self.placement.group.clone()),
            },
            // A patch that says nothing about the note inherits it. Replacing a note with silence
            // needs `note: Some("")` — deliberate, because a note is somebody's reasoning and losing
            // it should take an act.
            note: patch.note.clone().or_else(|| self.note.clone()),
        }
    }

    /// Fill in defaults and prove the required fields are present.
    ///
    /// One path, no fallback: a descriptor with no footprint is refused by id rather than resolved to
    /// `(0.0, 0.0)`. A zero reservation overlaps nothing and would let the piece sit inside a wall
    /// with every rule reporting success.
    pub fn resolve(&self) -> Result<Resolved, String> {
        if self.id.is_empty() {
            return Err("descriptor: an entry has no `id`".to_owned());
        }
        let need = |what: &str| format!("descriptor `{}`: missing `{what}`", self.id);

        let footprint = self.extent.footprint.ok_or_else(|| need("extent.footprint"))?;
        let mount = self.mount.clone().ok_or_else(|| need("mount"))?;

        // A decal has no meaningful height and never rests anything on itself, so it is the one shape
        // that may omit it. Everything else must state it: a surface's height IS the plane props sit
        // on, and defaulting that to zero would sink them into the floor.
        let height = match (self.extent.height, &mount) {
            (Some(h), _) => h,
            (None, Mount::Overlay { .. }) => 0.0,
            (None, _) => return Err(need("extent.height")),
        };

        for c in &self.clearance {
            if !c.dist.is_finite() || c.dist < 0.0 {
                return Err(format!(
                    "descriptor `{}`: clearance {:?} is {} m — must be finite and non-negative",
                    self.id, c.dir, c.dist
                ));
            }
        }

        let scale = self.align.scale.unwrap_or(1.0);
        if !scale.is_finite() || scale <= 0.0 {
            return Err(format!(
                "descriptor `{}`: align.scale is {scale} — must be finite and positive",
                self.id
            ));
        }

        Ok(Resolved {
            id: self.id.clone(),
            mesh: self.mesh.clone().ok_or_else(|| need("mesh"))?,
            scale,
            stretch_y: self.align.stretch_y,
            y_offset: self.align.y_offset.unwrap_or(0.0),
            pivot: self.align.pivot.unwrap_or((0.0, 0.0)),
            front: self.align.front,
            footprint,
            height,
            mount,
            clearance: self.clearance.clone(),
            offers: self.offers.clone(),
            kind: self.kind.clone(),
            effects: self.effects.clone(),
            look: self.look.clone(),
        })
    }
}

impl Resolved {
    /// Does this piece claim floor space that another may not share?
    ///
    /// Replaces `site::layout::occupies_floor` and its height threshold. An `Overlay` claims nothing
    /// (that is what a decal is), and something resting on a surface is the host's problem, not the
    /// floor's — two mugs on one table still collide with each other, which the overlap rule handles
    /// on the surface rather than here.
    pub fn occupies_floor(&self) -> bool {
        matches!(self.mount, Mount::OnFloor | Mount::Tiled)
    }
}

/// Patch semantics for a list: a non-empty patch replaces, an empty one inherits.
fn pick<T: Clone>(base: &[T], patch: &[T]) -> Vec<T> {
    if patch.is_empty() {
        base.to_vec()
    } else {
        patch.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The surface mount is the one whose payload is a project token, so the options have to come
    /// from the project — offering `OnSurface { "worktop" }` where nothing offers a worktop is
    /// offering a mount that can never be satisfied.
    #[test]
    fn mount_options_come_from_the_projects_surface_vocabulary() {
        let none = mount_options(&[]);
        assert!(
            !none.iter().any(|m| matches!(m, Mount::OnSurface { .. })),
            "a project with no surface classes must offer no surface mount"
        );

        let two = mount_options(&["support".to_owned(), "worktop".to_owned()]);
        let classes: Vec<&str> = two
            .iter()
            .filter_map(|m| match m {
                Mount::OnSurface { class } => Some(class.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(classes, ["support", "worktop"]);
        assert!(
            !two.iter().any(|m| matches!(m, Mount::OnSurface { class } if class.is_empty())),
            "the placeholder must not survive into the offered list"
        );
    }

    /// The common case comes first. An author cycling past four exotic variants to reach "on floor"
    /// will bind their own key instead.
    #[test]
    fn the_first_option_is_the_one_most_pieces_want() {
        assert_eq!(mount_options(&[])[0], Mount::OnFloor);
    }

    #[test]
    fn every_mount_has_a_label_short_enough_for_one_line() {
        for m in mount_options(&["worktop".to_owned()]) {
            let label = mount_label(Some(&m));
            assert!(!label.is_empty(), "{m:?} has no label");
            assert!(label.len() <= 24, "{m:?} label is too long: {label}");
        }
        assert_eq!(mount_label(None), "unset");
    }

    fn crate_desc() -> Descriptor {
        Descriptor {
            id: "crate".into(),
            mesh: Some("ozea/crate.glb".into()),
            extent: Extent {
                footprint: Some((0.6, 0.6)),
                height: Some(0.5),
            },
            mount: Some(Mount::OnFloor),
            kind: vec!["container".into()],
            ..Default::default()
        }
    }

    #[test]
    fn a_descriptor_resolves_its_defaults() {
        let r = crate_desc().resolve().expect("resolves");
        assert_eq!(r.scale, 1.0, "scale defaults to unity");
        assert_eq!(r.y_offset, 0.0);
        assert_eq!(r.pivot, (0.0, 0.0));
        assert_eq!(r.front, None, "no front is a claim, not a missing value");
        assert!(r.occupies_floor());
    }

    /// The failure this prevents is silent: a zero footprint overlaps nothing, so every placement rule
    /// reports success while the prop sits inside a wall.
    #[test]
    fn a_missing_footprint_is_refused_by_name() {
        let mut d = crate_desc();
        d.extent.footprint = None;
        let err = d.resolve().expect_err("must refuse");
        assert!(err.contains("crate") && err.contains("footprint"), "{err}");
    }

    #[test]
    fn a_missing_height_is_refused_unless_it_is_a_decal() {
        let mut d = crate_desc();
        d.extent.height = None;
        assert!(d.resolve().is_err(), "a solid piece must state its height");

        d.mount = Some(Mount::Overlay {
            on: OverlayHost::Wall { height: 1.8 },
        });
        let r = d.resolve().expect("a decal may omit height");
        assert_eq!(r.height, 0.0);
        assert!(!r.occupies_floor(), "an overlay claims no floor");
    }

    #[test]
    fn a_wall_decal_is_expressible() {
        // The thing neither old schema could say. `PropPlacement` had no Y, host or normal, so the
        // only wall-mounted object in the game was hardcoded in Rust.
        let d = Descriptor {
            id: "hazard_sign".into(),
            mesh: Some("signage/hazard.glb".into()),
            extent: Extent {
                footprint: Some((0.4, 0.4)),
                height: None,
            },
            mount: Some(Mount::Overlay {
                on: OverlayHost::Wall { height: 1.8 },
            }),
            ..Default::default()
        };
        let r = d.resolve().expect("resolves");
        assert_eq!(
            r.mount,
            Mount::Overlay {
                on: OverlayHost::Wall { height: 1.8 }
            }
        );
        assert!(!r.occupies_floor());
    }

    #[test]
    fn a_patch_inherits_what_it_does_not_state() {
        let base = crate_desc();
        let patch = Descriptor {
            id: String::new(),
            align: Align {
                front: Some(90.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let merged = base.patched_with(&patch);
        assert_eq!(merged.id, "crate", "an empty id inherits");
        assert_eq!(merged.mesh.as_deref(), Some("ozea/crate.glb"));
        assert_eq!(merged.extent.footprint, Some((0.6, 0.6)));
        assert_eq!(merged.align.front, Some(90.0), "the patch wins where it speaks");
    }

    /// Replace, not append — so a patch can take a tag away. An append-only list needs a second
    /// mechanism for removal the moment anyone wants one.
    #[test]
    fn a_patch_replaces_a_list_rather_than_appending() {
        let base = crate_desc();
        let patch = Descriptor {
            kind: vec!["debris".into()],
            ..Default::default()
        };
        assert_eq!(base.patched_with(&patch).kind, vec!["debris".to_string()]);

        let empty = Descriptor::default();
        assert_eq!(
            base.patched_with(&empty).kind,
            vec!["container".to_string()],
            "an empty list inherits rather than clearing"
        );
    }

    #[test]
    fn a_negative_clearance_is_refused() {
        let mut d = crate_desc();
        d.clearance = vec![Clearance {
            dir: Dir::Front,
            dist: -0.5,
        }];
        assert!(d.resolve().is_err());
    }

    #[test]
    fn the_schema_round_trips_through_ron() {
        let d = Descriptor {
            clearance: vec![Clearance {
                dir: Dir::Around,
                dist: 0.91,
            }],
            offers: Offers {
                surfaces: vec!["worktop".into()],
                sockets: vec![Socket {
                    id: "seat_n".into(),
                    role: Some("diner".into()),
                    at: (0.0, 0.75, -0.5),
                    yaw: 0.0,
                }],
            },
            effects: vec!["uses-electricity".into()],
            ..crate_desc()
        };
        let text = ron::ser::to_string_pretty(&d, ron::ser::PrettyConfig::default())
            .expect("serializes");
        let back: Descriptor = ron::from_str(&text).expect("parses");
        assert_eq!(d, back);
    }
}

#[cfg(test)]
mod subgrid_tests {
    use super::*;

    fn grid(div: (u32, u32, u32), cells: Vec<SubCell>) -> Subgrid {
        Subgrid { div, cells }
    }

    fn cell(at: (u32, u32, u32)) -> SubCell {
        SubCell {
            at,
            ..SubCell::default()
        }
    }

    /// The default is the whole point of the feature being cheap: a piece that says nothing about its
    /// inside costs one field and no rows.
    #[test]
    fn a_tile_says_nothing_about_its_inside_by_default() {
        let g = Subgrid::default();
        assert_eq!(g.div, (3, 3, 3));
        assert_eq!(g.volume(), 27);
        assert!(g.cells.is_empty());
        assert!(g.validate("x").is_ok());
    }

    /// All three facets on one cell — the thing this schema exists to allow.
    #[test]
    fn one_cell_can_be_solid_and_an_edge_and_an_anchor() {
        let c = SubCell {
            at: (2, 0, 1),
            solid: true,
            edge: Some("wall".into()),
            anchor: Some("diner".into()),
        };
        let g = grid((3, 3, 3), vec![c.clone()]);
        assert!(g.validate("table").is_ok());
        let got = g.at((2, 0, 1)).unwrap_or_else(|| panic!("no cell"));
        assert_eq!(got, &c);
        assert!(g.at((0, 0, 0)).is_none(), "unwritten cells are absent, not default rows");
    }

    #[test]
    fn a_cell_outside_the_lattice_is_refused() {
        let e = grid((3, 3, 3), vec![cell((3, 0, 0))])
            .validate("desk")
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("outside"), "{e}");
    }

    #[test]
    fn naming_one_cell_twice_is_refused() {
        let e = grid((3, 3, 3), vec![cell((1, 1, 1)), cell((1, 1, 1))])
            .validate("desk")
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("twice"), "{e}");
    }

    #[test]
    fn an_axis_with_no_divisions_is_refused() {
        let e = grid((3, 0, 3), vec![])
            .validate("desk")
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("no cells"), "{e}");
    }

    /// A lattice survives the file, which is what makes it authorable at all.
    #[test]
    fn a_lattice_round_trips_through_ron() {
        let before = Descriptor {
            id: "desk".into(),
            subgrid: grid(
                (3, 3, 3),
                vec![SubCell {
                    at: (0, 0, 2),
                    solid: true,
                    edge: Some("wall".into()),
                    anchor: None,
                }],
            ),
            ..Descriptor::default()
        };
        let text = ron::ser::to_string_pretty(&before, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|e| panic!("{e}"));
        let after: Descriptor = ron::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(before, after);
    }

    /// An old library has no `subgrid` key at all and must still parse — the field defaults.
    #[test]
    fn a_descriptor_written_before_the_lattice_still_parses() {
        let d: Descriptor = ron::from_str("(id: \"crate\")").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d.subgrid, Subgrid::default());
    }
}

#[cfg(test)]
mod subgrid_edit_tests {
    use super::*;

    #[test]
    fn toggling_a_cell_solid_and_back_leaves_no_trace() {
        let mut g = Subgrid::default();
        assert_eq!(g.toggle_solid((1, 0, 1)), Some(true));
        assert_eq!(g.cells.len(), 1);
        assert_eq!(g.toggle_solid((1, 0, 1)), Some(false));
        // The sparse invariant: a cell that says nothing is absent, not a row of `solid: false`.
        assert!(g.cells.is_empty(), "an unmarked cell must leave no row behind");
    }

    #[test]
    fn a_cell_outside_the_lattice_cannot_be_written() {
        let mut g = Subgrid::default();
        assert_eq!(g.toggle_solid((3, 0, 0)), None);
        assert_eq!(g.set_edge((0, 9, 0), "wall"), None);
        assert!(g.cells.is_empty(), "an out-of-range write must not append anything");
    }

    #[test]
    fn tokens_set_and_clear_on_the_same_cell() {
        let mut g = Subgrid::default();
        g.set_edge((0, 0, 2), "wall").unwrap_or_else(|| panic!("in range"));
        g.set_anchor((0, 0, 2), "diner").unwrap_or_else(|| panic!("in range"));
        let c = g.at((0, 0, 2)).unwrap_or_else(|| panic!("written"));
        assert_eq!(c.edge.as_deref(), Some("wall"));
        assert_eq!(c.anchor.as_deref(), Some("diner"));

        // Emptying both takes the row with it, since solid was never set.
        g.set_edge((0, 0, 2), "").unwrap_or_else(|| panic!("in range"));
        g.set_anchor((0, 0, 2), "  ").unwrap_or_else(|| panic!("in range"));
        assert!(g.cells.is_empty());
    }

    /// A cell keeps whichever facets are still set — clearing one must not drop the others.
    #[test]
    fn clearing_one_facet_keeps_the_rest() {
        let mut g = Subgrid::default();
        g.toggle_solid((1, 1, 1));
        g.set_edge((1, 1, 1), "wall").unwrap_or_else(|| panic!("in range"));
        g.set_edge((1, 1, 1), "").unwrap_or_else(|| panic!("in range"));
        let c = g.at((1, 1, 1)).unwrap_or_else(|| panic!("still solid"));
        assert!(c.solid);
        assert!(c.edge.is_none());
    }

    #[test]
    fn clear_forgets_the_whole_cell() {
        let mut g = Subgrid::default();
        g.toggle_solid((2, 2, 2));
        g.set_anchor((2, 2, 2), "seat").unwrap_or_else(|| panic!("in range"));
        g.clear((2, 2, 2));
        assert!(g.at((2, 2, 2)).is_none());
        assert!(g.validate("x").is_ok());
    }
}
