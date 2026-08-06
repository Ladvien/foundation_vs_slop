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

    /// The tile's internal lattice, or `None` for a piece that says nothing about its inside.
    ///
    /// **An `Option` because a patch has to be able to say nothing.** [`Self::patched_with`] used a
    /// bare `Subgrid` and treated `Subgrid::default()` as "unset" — but that is also the commonest
    /// legal value, so a patch deliberately clearing a lattice was indistinguishable from a patch
    /// with no opinion about it, and the second reading silently won. `None` is "no opinion";
    /// `Some(Subgrid::default())` is "this piece has no marked cells".
    pub subgrid: Option<Subgrid>,

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
/// # The divisions are not stored here
///
/// **A lattice knows its cells; the project knows how finely a tile divides.** `div` used to be a
/// per-descriptor field defaulting to 3×3×3, which meant a 3 m wall had 1 m cells and a 0.5 m chair
/// had 0.167 m ones — two faces that [`crate::adjacency::seam`] compares cell against cell and
/// that could never mean the same thing. Merrell & Manocha's model synthesis is explicit about why
/// that cannot work: the grid is three sets of parallel planes and *"all planes within each set are
/// parallel and evenly spaced"*, so a spacing that varies per object is not a grid at all
/// (Merrell & Manocha 2009, *Constraint-Based Model Synthesis*, §4.4).
///
/// So the spacing is one project-level number — [`crate::policy::Policy::divisions`] — and a piece's
/// divisions are **derived** from its own size by [`divisions`]. The same paper names this exact
/// remedy for objects that do not land on round multiples: *"the planes could be spaced more closely.
/// If they are spaced twice as close, an object that was 1.5-plane spaces wide would become three
/// planes wide"* (§4.5).
///
/// # Sparse, because most tiles have nothing to say
///
/// The shipped library is 43 mostly-rectangular props. Writing every cell would be thousands of rows
/// of `solid: false` — so only cells that differ from open-and-unlabelled are written, and a piece
/// with no lattice detail costs nothing at all.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Subgrid {
    /// Only the cells that are not plain open space. See the note on sparseness.
    pub cells: Vec<SubCell>,
}

/// **How finely a piece divides**, given the project's divisions-per-tile.
///
/// A subunit is a `grid::SNAP / per_tile` **cube**: each axis is the whole number of authoring cells
/// the piece spans, times `per_tile`. So every piece in a project is measured against the same
/// spacing on every axis, which is what makes an edge token on a 3 m wall mean the same thing as one
/// on a 0.5 m chair.
///
/// # The piece as placed, not the mesh as measured
///
/// The height is `extent.height * align.stretch_y`, because that is how tall the piece **stands**.
/// Reading `extent.height` alone made the same piece id derive a different lattice in two kits: the
/// Site kit's wall is authored at 2.40 m and stretched 1.0, so five layers; `site_greybox`'s is a 1 m
/// module stretched 2.4x, so it derived **two** — a lattice describing a piece a third of the height
/// of the one actually in the world. Found by authoring the first real tokens, which is the only way
/// it could have been found.
///
/// **`align.scale` is not applied**, because `extent` already carries it: the extents are recorded
/// post-scale (see [`placed_footprint`], where the contract and the `site/books` datum that proves it
/// are written out), so the lattice derives from `extent` and `stretch_y` alone and agrees with
/// `stack::covers`, `fill` and the drawn mesh by construction.
///
/// `grid::cells` never returns zero, so a decal with no height gets one layer rather than a
/// degenerate lattice — no special case needed.
///
/// A missing `footprint` is refused by id rather than resolved to `(0, 0)`, the same call
/// [`Descriptor::resolve`] makes and for the same reason: a zero lattice has no cells and every rule
/// reading it would silently report success.
pub fn divisions(d: &Descriptor, per_tile: u32) -> Result<(u32, u32, u32), String> {
    let owner = &d.id;
    if per_tile == 0 {
        return Err(format!(
            "`{owner}`: the project divides each tile 0 ways; an axis with no divisions has no cells"
        ));
    }
    let (w, dep) = placed_footprint(d).ok_or_else(|| {
        format!("`{owner}`: no `extent.footprint`, so its lattice cannot be derived")
    })?;
    // A decal may legitimately omit its height — `Descriptor::resolve` says so — and `grid::cells(0)`
    // is one cell, so the lattice is one layer deep rather than empty.
    let h = placed_height(d).unwrap_or(0.0);
    let axis = |span: f32| crate::grid::cells(span).0 * per_tile;
    Ok((axis(w), axis(h), axis(dep)))
}

/// **The footprint a piece actually occupies**, in metres — which is `extent.footprint`, verbatim.
///
/// # `align.scale` is deliberately NOT applied, and this time that is the contract
///
/// `extent` records the piece **as placed**: `src/site/visuals.rs` states it in as many words —
/// *"the kit's `height`/`footprint` are the post-scale values because every placement rule reads
/// them"* — and the one shipped non-unity scale proves it. `site/books`' raw mesh measures 0.5096 m
/// wide; the file records `footprint (0.306, 0.106)` with `scale: Some(0.6)`, and 0.5096 × 0.6 is
/// 0.306. **`scale` maps the authored mesh onto the recorded extent** — a render instruction, exactly
/// like [`Align::rotate`], whose extents are likewise baked at import so no reader learns it exists.
///
/// A previous version multiplied by `scale` here, on the argument that the vertical axis already did
/// (`stack::drawn_height` was `h × scale × stretch_y`). That was a double-application: it shrank
/// `books`' every space answer to 0.184 m while the mesh drew at 0.306 m — the exact
/// drawn-versus-reserved disagreement it claimed to fix, created rather than closed. The editor's
/// `SIZE (m)` field therefore **bakes**: it rewrites `extent` and composes `scale`, so this function
/// stays a plain read.
///
/// It exists as a function rather than a field access so the question "how much room" has one name —
/// [`divisions`], `stack::covers`, `fill::cell_extents`, `adjacency::faults` and the editor all come
/// through here, and the contract above is written at the one place they share.
///
/// `None` for an unmeasured piece, which is propagated rather than defaulted. A zero footprint
/// overlaps nothing and would let the piece sit inside a wall with every rule reporting success —
/// [`Descriptor::resolve`] makes the same call for the same reason.
pub fn placed_footprint(d: &Descriptor) -> Option<(f32, f32)> {
    d.extent.footprint
}

/// **How tall a piece stands**, in metres — `extent.height × stretch_y`.
///
/// The vertical sibling of [`placed_footprint`], under the same contract: `extent.height` is already
/// the post-scale value, so only [`Align::stretch_y`] — game policy applied on top of the art, never
/// baked — multiplies it. This is the rule [`divisions`] has always used for the lattice.
///
/// It replaced `stack::drawn_height`, which multiplied by `scale` as well. For every shipped piece
/// but one that factor was 1.0 and harmless; for `site/books` it was a latent double-application —
/// never observable only because `books` offers no surfaces, so nothing ever rested on the answer.
///
/// `None` when the descriptor records no height — an **unmeasured** piece, not a flat one. Nothing may
/// rest on it, and saying so is better than treating unknown as zero and stacking a lamp at floor
/// level on top of a bookcase.
pub fn placed_height(d: &Descriptor) -> Option<f32> {
    let h = d.extent.height?;
    Some(h * d.align.stretch_y.unwrap_or(1.0))
}

/// The divisions after `quarter` 90° turns about +Y — x and z swap on every odd turn.
///
/// Separate from [`Subgrid::rotated`] because the lattice no longer carries its own divisions: a
/// caller that turns a tile needs both halves, and having to ask for both is what keeps them in step.
pub fn rotate_div(div: (u32, u32, u32), quarter: u8) -> (u32, u32, u32) {
    if quarter % 2 == 0 {
        div
    } else {
        (div.2, div.1, div.0)
    }
}

impl Subgrid {
    /// How many cells the lattice has, if it were written out in full.
    pub fn volume(div: (u32, u32, u32)) -> u32 {
        div.0.saturating_mul(div.1).saturating_mul(div.2)
    }

    /// What is at `at`, or `None` for a cell nobody has said anything about.
    ///
    /// The one accessor that does **not** need the divisions: it searches what was written rather
    /// than asking what could exist.
    pub fn at(&self, at: (u32, u32, u32)) -> Option<&SubCell> {
        self.cells.iter().find(|c| c.at == at)
    }

    /// Is `at` inside a lattice of `div`?
    pub fn holds(at: (u32, u32, u32), div: (u32, u32, u32)) -> bool {
        at.0 < div.0 && at.1 < div.1 && at.2 < div.2
    }

    /// The cell at `at`, created empty if nobody has written one yet.
    ///
    /// `None` when `at` is outside the lattice — a caller asking about a cell that cannot exist gets
    /// an answer, not a row appended somewhere unreachable.
    fn entry(&mut self, at: (u32, u32, u32), div: (u32, u32, u32)) -> Option<&mut SubCell> {
        if !Subgrid::holds(at, div) {
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
    pub fn toggle_solid(&mut self, at: (u32, u32, u32), div: (u32, u32, u32)) -> Option<bool> {
        let now = {
            let cell = self.entry(at, div)?;
            cell.solid = !cell.solid;
            cell.solid
        };
        self.prune();
        Some(now)
    }

    /// Mark a cell solid, whatever it was. Returns `None` if `at` is outside.
    ///
    /// Distinct from [`Self::toggle_solid`] because a mesh scan states a fact rather than flipping
    /// one: running it twice must leave the same lattice, which a toggle would not.
    pub fn set_solid(&mut self, at: (u32, u32, u32), div: (u32, u32, u32)) -> Option<()> {
        self.entry(at, div)?.solid = true;
        self.prune();
        Some(())
    }

    /// Set or clear a cell's edge label. An empty string clears it — the same keystroke that types a
    /// token has to be able to take it back.
    pub fn set_edge(&mut self, at: (u32, u32, u32), div: (u32, u32, u32), token: &str) -> Option<()> {
        let cell = self.entry(at, div)?;
        cell.edge = (!token.trim().is_empty()).then(|| token.trim().to_owned());
        self.prune();
        Some(())
    }

    /// Set or clear a cell's anchor role.
    pub fn set_anchor(&mut self, at: (u32, u32, u32), div: (u32, u32, u32), token: &str) -> Option<()> {
        let cell = self.entry(at, div)?;
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
    /// `div` is the lattice's divisions **before** the turn; the turned divisions are
    /// [`rotate_div`]'s job. Two calls rather than one because the lattice no longer carries its
    /// divisions, and a caller forced to name both cannot let them drift apart.
    ///
    /// Lives here rather than beside the matcher because the schema owns its own transforms for the
    /// same reason it owns its own edits: the sparse invariant is this type's to keep.
    pub fn rotated(&self, quarter: u8, div: (u32, u32, u32)) -> Subgrid {
        let mut out = self.clone();
        let mut at_div = div;
        for _ in 0..(quarter % 4) {
            let (dx, dy, dz) = at_div;
            out = Subgrid {
                cells: out
                    .cells
                    .iter()
                    .map(|c| SubCell {
                        at: (c.at.2, c.at.1, dx.saturating_sub(1) - c.at.0.min(dx.saturating_sub(1))),
                        ..c.clone()
                    })
                    .collect(),
            };
            at_div = (dz, dy, dx);
        }
        out
    }

    /// Refuse a lattice that cannot be true.
    ///
    /// Every rule here is one whose violation is silent: a zero division makes the lattice empty
    /// while still claiming to have one, an out-of-range cell is a value nothing will ever read, and
    /// a duplicate is two answers to one question with the first quietly winning.
    pub fn validate(&self, owner: &str, div: (u32, u32, u32)) -> Result<(), String> {
        let (dx, dy, dz) = div;
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
    /// Solid space, as rasterised from the mesh by [`crate::import::occupancy`].
    ///
    /// **Nothing reads this to decide anything, and that is a ruling rather than an omission.** It was
    /// meant to refine `stack::covers` so clearance would respect a piece's shape instead of its
    /// bounding box — `FVS-Q-9`, closed *no* on 2026-08-05 after being built and measured. At the
    /// shipped `divisions: 1` a lattice-aware `covers` agrees with the bounding box 96% of the time,
    /// because the props are mostly smaller than a few cells; resolution fine enough to hold a shape
    /// needs `divisions: 3`, which makes a wall 810 cells. And `divisions` cannot be raised for this
    /// alone, because it is one project-wide number precisely so that two faces are comparable — see
    /// [`Subgrid`]. Coarse-for-matching and fine-for-clearance cannot be the same number.
    ///
    /// What it *is* for: the author's confirmation that the lattice lines up with the mesh. The editor
    /// marks it with `rescan mesh` and draws it, which is how you see that a wall's lattice really does
    /// fill the wall. `Descriptor::clearance` is the field that decides anything about space.
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

/// **The height a mount carries**, metres, or `None` for one where the question does not arise.
///
/// Two variants carry a height and they carry the same one for the same reason — a wall's height is
/// nobody else's to state, unlike the floor and the ceiling, which the map states. [`OverlayHost`]'s
/// own doc says so.
///
/// This exists because the editor could put a piece **on** a wall and then not say how far up: the
/// mount cycles through [`mount_options`], every entry of which is a literal, so `1.8` was the only
/// wall height reachable without hand-editing the RON.
pub fn mount_height(m: &Mount) -> Option<f32> {
    match m {
        Mount::OnWall { height } => Some(*height),
        Mount::Overlay {
            on: OverlayHost::Wall { height },
        } => Some(*height),
        _ => None,
    }
}

/// The same mount at a different height — or `None` when it has no height to set.
///
/// Returns a new mount rather than mutating one, so a caller that asks the wrong question gets an
/// answer it has to handle instead of a silent no-op on a piece it thought it had changed.
pub fn with_mount_height(m: &Mount, height: f32) -> Option<Mount> {
    match m {
        Mount::OnWall { .. } => Some(Mount::OnWall { height }),
        Mount::Overlay {
            on: OverlayHost::Wall { .. },
        } => Some(Mount::Overlay {
            on: OverlayHost::Wall { height },
        }),
        _ => None,
    }
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

/// **One of a tile's four horizontal faces**, in the vocabulary the lattice already uses.
///
/// `crate::adjacency::face` reads a face; [`Align::front`] names one; and cell picking will report
/// the one a ray entered. Three things about the same four directions, so they are one type rather
/// than three spellings of a quarter turn.
///
/// The world meaning is `crate::wfc`'s, which is `grammar::learn`'s step table: **North is −Z, East
/// is +X, South is +Z, West is −X.**
///
/// # Why a face and not degrees
///
/// `front` was a yaw in degrees, and degrees can express things a tile cannot have. A front at 37°
/// points at a corner: there is no column of cells there, so nothing can read it, and
/// `adjacency::quarter_turns` already refuses off-square yaws for exactly that reason. Naming a face
/// makes the quantisation part of the type instead of a rule some later caller forgets.
///
/// It is also lossless for the shipped data — every `front` in the repo is `90.0`, which is [`Self::East`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Face {
    North,
    East,
    South,
    West,
}

impl Face {
    /// The yaw, in degrees, whose forward vector points out of this face.
    ///
    /// The engine convention is `forward = (sin yaw, cos yaw)`, so yaw 0 is +Z — [`Self::South`] —
    /// and the quarter turns follow from there. This is what every reader that composes a facing
    /// with a placement's yaw wants.
    pub fn yaw_degrees(self) -> f32 {
        match self {
            Face::South => 0.0,
            Face::East => 90.0,
            Face::North => 180.0,
            Face::West => 270.0,
        }
    }

    /// The face a yaw points out of, to the nearest quarter turn.
    ///
    /// **Snapping is the point, not a loss.** `glb::front_detail` measures a continuous angle out of
    /// centroid asymmetry, and that angle is evidence about which face is the front rather than a
    /// facing in its own right — a chair modelled 3° off square still fronts +X. The importer shows
    /// the raw measurement beside this so an author can overrule a borderline call.
    pub fn from_yaw(deg: f32) -> Face {
        if !deg.is_finite() {
            return Face::South;
        }
        match (deg / 90.0).round().rem_euclid(4.0) as u8 {
            1 => Face::East,
            2 => Face::North,
            3 => Face::West,
            _ => Face::South,
        }
    }

    /// This face as [`crate::wfc`]'s edge index, so a front and a lattice face are the same four
    /// numbers to everything downstream.
    pub fn dir(self) -> crate::placement::ir::Dir {
        match self {
            Face::North => crate::wfc::N,
            Face::East => crate::wfc::E,
            Face::South => crate::wfc::S,
            Face::West => crate::wfc::W,
        }
    }

    /// The face opposite this one.
    pub fn opposite(self) -> Face {
        match self {
            Face::North => Face::South,
            Face::East => Face::West,
            Face::South => Face::North,
            Face::West => Face::East,
        }
    }

    /// How an author reads it, matching `adjacency`'s fault messages.
    pub fn label(self) -> &'static str {
        match self {
            Face::North => "N",
            Face::East => "E",
            Face::South => "S",
            Face::West => "W",
        }
    }
}

/// **[`Align::rotate`] as quarter turns**, or a refusal naming the piece and the angle.
///
/// The same rule `adjacency::quarter_turns` holds for a placement's yaw, for the same reason: a tile
/// is square to the world, so a rotation that is not a quarter turn leaves it with no face any rule
/// can read. Refused rather than rounded — rounding would silently store an orientation the author
/// did not ask for, and the mesh would render at one angle while every measurement described another.
pub fn quarter_turns_xyz(rotate: (i32, i32, i32), owner: &str) -> Result<(u8, u8, u8), String> {
    let axis = |deg: i32, name: &str| -> Result<u8, String> {
        if deg % 90 != 0 {
            return Err(format!(
                "`{owner}`'s rotation is {deg} degrees about {name}; a tile only sits square to the \
                 world, so a rotation must be a multiple of 90"
            ));
        }
        Ok((deg.rem_euclid(360) / 90) as u8)
    };
    Ok((
        axis(rotate.0, "X")?,
        axis(rotate.1, "Y")?,
        axis(rotate.2, "Z")?,
    ))
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
    /// **A default rotation for a mesh authored the wrong way up**, in degrees per axis, applied X
    /// then Y then Z.
    ///
    /// Every value is a multiple of 90 — [`quarter_turns_xyz`] refuses anything else. A tile's
    /// lattice, its faces and its footprint are all square to the world; a mesh dropped in at 37°
    /// has no honest extent, so the schema cannot express one.
    ///
    /// # The extent stored beside this is already rotated
    ///
    /// **This is a render instruction, not a measurement correction.** The importer measures the
    /// mesh, applies this rotation to the bounds ([`crate::glb::Measured::rotated`]), and writes the
    /// *rotated* `extent`, `pivot` and `y_offset`. So every reader of `extent` — `stack`, `fill`,
    /// `thumbs`, [`divisions`] — sees the piece as it will stand in the world, and none of them
    /// needs to know this field exists.
    ///
    /// The cost of that choice is an invariant a file can break: editing `rotate` by hand leaves
    /// `extent` describing the old orientation, and nothing downstream can tell. Change it through
    /// the editor, which re-measures.
    pub rotate: Option<(i32, i32, i32)>,
    /// **Which of the mesh's own faces is its front**, in its local space.
    ///
    /// Composed with a placement's yaw by whoever needs a world facing:
    /// `placement.yaw + front.yaw_degrees()`. See [`Face`] for why this is a face rather than the
    /// arbitrary angle it used to be.
    ///
    /// `None` means *the mesh is symmetric and has no front*, which is a different claim from
    /// `Some(Face::South)`. The kit records that distinction deliberately: a stool measures symmetric
    /// to within a centimetre, and "asserting a facing on a stool would be asserting a fact about the
    /// art that is not true."
    pub front: Option<Face>,
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
    pub front: Option<Face>,
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
                rotate: patch.align.rotate.or(self.align.rotate),
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
            // append could not remove a cell. `None` is the patch saying nothing — which is a
            // different claim from `Some(Subgrid::default())`, the patch clearing every cell.
            subgrid: patch.subgrid.clone().or_else(|| self.subgrid.clone()),
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

    /// **The lattice to write into**, created empty if this is the first thing said about it.
    ///
    /// Pair every batch of edits with [`Self::settle_lattice`], or a piece whose last cell was
    /// cleared keeps an empty lattice where it used to have nothing.
    pub fn lattice_mut(&mut self) -> &mut Subgrid {
        self.subgrid.get_or_insert_with(Subgrid::default)
    }

    /// **Drop a lattice that has gone back to saying nothing.**
    ///
    /// `Subgrid::prune` keeps the *cells* sparse; this keeps the *field* sparse, and it is the same
    /// argument one level up: a piece an author poked at and undid must leave the file as it was.
    ///
    /// `None` and `Some(Subgrid::default())` are only interchangeable **here**, in a base
    /// descriptor, where both can mean nothing but "no cells". In a patch they are two different
    /// claims — silence versus "clear them" — which is why [`Self::patched_with`] distinguishes
    /// them and this does not.
    pub fn settle_lattice(&mut self) {
        if self.subgrid.as_ref().is_some_and(|g| g.cells.is_empty()) {
            self.subgrid = None;
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

    /// **The contract: `extent` is the placed size, and `scale` never multiplies it again.**
    ///
    /// `site/books` is the proof datum — raw mesh 0.5096 m wide, recorded `footprint (0.306, ..)`
    /// with `scale 0.6`, and 0.5096 × 0.6 = 0.306 — and `src/site/visuals.rs` states it in words:
    /// the extents are *"the post-scale values because every placement rule reads them"*. A previous
    /// version of these helpers multiplied by scale here, which shrank every space answer for `books`
    /// to 0.6× of a value that was already 0.6× — clicks missed the visible mesh and its reservation
    /// no longer covered what it drew. If this test ever fails, that double-application is back.
    #[test]
    fn everything_that_asks_how_much_room_reads_the_extent_as_placed() {
        // The books datum, verbatim from the site kit.
        let mut d = crate_desc();
        d.extent.footprint = Some((0.306, 0.106));
        d.extent.height = Some(0.178);
        d.align.scale = Some(0.6);

        assert_eq!(placed_footprint(&d), Some((0.306, 0.106)), "extent IS the answer");
        assert_eq!(placed_height(&d), Some(0.178));

        // The reservation covers the drawn mesh exactly: raw 0.5096 × scale 0.6 = 0.306 drawn, and
        // covers() must reach the drawn half-width and no further.
        assert!(crate::stack::covers(&d, (0.0, 0.0), 0.0, (0.15, 0.0)));
        assert!(!crate::stack::covers(&d, (0.0, 0.0), 0.0, (0.16, 0.0)));

        // And the lattice derives from the same numbers.
        let div = divisions(&d, 1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            div,
            (
                crate::grid::cells(0.306).0,
                crate::grid::cells(0.178).0,
                crate::grid::cells(0.106).0
            )
        );

        // Explicit unity, absent, and any other scale are all the same answer — scale is a render
        // instruction, invisible to every space question by construction.
        for s in [None, Some(1.0), Some(0.6), Some(2.0)] {
            let mut v = d.clone();
            v.align.scale = s;
            assert_eq!(placed_footprint(&v), Some((0.306, 0.106)), "scale {s:?} leaked into space");
            assert_eq!(divisions(&v, 3), divisions(&d, 3));
        }
    }

    /// A stretched wall is taller than its extent — `stretch_y` is game policy layered on top of the
    /// art, the one factor that is NOT baked in — and it never touches the footprint.
    #[test]
    fn stretch_reaches_the_placed_height_and_scale_does_not() {
        let mut d = crate_desc();
        d.extent.footprint = Some((1.0, 1.0));
        d.extent.height = Some(2.0);
        d.align.scale = Some(0.5);
        d.align.stretch_y = Some(1.2);
        assert_eq!(placed_height(&d), Some(2.4), "2.0 * 1.2 — the scale is already in the 2.0");
        assert_eq!(placed_footprint(&d), Some((1.0, 1.0)));
    }

    /// An unmeasured piece stays unmeasured — no default may invent a zero, which is the value that
    /// overlaps nothing and lets a prop sit inside a wall.
    #[test]
    fn an_unmeasured_piece_yields_nothing_rather_than_zero() {
        let mut d = crate_desc();
        d.extent.footprint = None;
        d.align.scale = Some(0.6);
        assert_eq!(placed_footprint(&d), None);
        assert!(divisions(&d, 1).is_err(), "and the lattice is refused by name");
    }

    /// **Both wall mounts carry a height, and nothing else does.** The floor and the ceiling are the
    /// map's to state; a wall's is not, which is why these two carry one and `OnFloor` does not.
    #[test]
    fn the_two_wall_mounts_are_the_ones_with_a_height() {
        assert_eq!(mount_height(&Mount::OnWall { height: 1.6 }), Some(1.6));
        assert_eq!(
            mount_height(&Mount::Overlay {
                on: OverlayHost::Wall { height: 2.1 }
            }),
            Some(2.1)
        );
        for no_height in [
            Mount::OnFloor,
            Mount::OnCeiling,
            Mount::Tiled,
            Mount::InOpening { clear: None },
            Mount::OnSurface { class: "worktop".into() },
            Mount::Overlay { on: OverlayHost::Floor },
            Mount::Overlay { on: OverlayHost::Ceiling },
        ] {
            assert_eq!(mount_height(&no_height), None, "{no_height:?}");
            assert_eq!(
                with_mount_height(&no_height, 1.0),
                None,
                "{no_height:?} has no height to set, and must say so rather than no-op"
            );
        }
    }

    /// Setting a height keeps the mount it was set on. A poster must not become a sconce because
    /// somebody typed a number at it.
    #[test]
    fn setting_a_height_does_not_change_which_mount_it_is() {
        let poster = Mount::Overlay {
            on: OverlayHost::Wall { height: 1.8 },
        };
        let moved = with_mount_height(&poster, 1.2).unwrap_or_else(|| panic!("has a height"));
        assert!(matches!(
            moved,
            Mount::Overlay {
                on: OverlayHost::Wall { .. }
            }
        ));
        assert_eq!(mount_height(&moved), Some(1.2));

        let sconce = with_mount_height(&Mount::OnWall { height: 1.8 }, 2.4)
            .unwrap_or_else(|| panic!("has a height"));
        assert!(matches!(sconce, Mount::OnWall { .. }));
        assert_eq!(mount_height(&sconce), Some(2.4));
    }

    /// Every wall mount `mount_options` offers can be re-heighted — otherwise the editor could cycle
    /// onto one it then had no way to adjust, which is the gap this pair closes.
    #[test]
    fn every_offered_wall_mount_can_be_re_heighted() {
        for m in mount_options(&["worktop".to_owned()]) {
            if mount_height(&m).is_some() {
                assert!(
                    with_mount_height(&m, 1.0).is_some(),
                    "{m:?} reports a height but cannot be given one"
                );
            }
        }
    }

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
                front: Some(Face::East),
                ..Default::default()
            },
            ..Default::default()
        };
        let merged = base.patched_with(&patch);
        assert_eq!(merged.id, "crate", "an empty id inherits");
        assert_eq!(merged.mesh.as_deref(), Some("ozea/crate.glb"));
        assert_eq!(merged.extent.footprint, Some((0.6, 0.6)));
        assert_eq!(merged.align.front, Some(Face::East), "the patch wins where it speaks");
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
mod face_tests {
    use super::*;

    /// The engine convention is `forward = (sin yaw, cos yaw)`, so yaw 0 points at +Z — which is
    /// South in `wfc`'s naming. Everything else follows, and getting this backwards would turn every
    /// seat in the game a quarter turn.
    #[test]
    fn a_faces_yaw_points_out_of_it() {
        for face in [Face::North, Face::East, Face::South, Face::West] {
            let yaw = face.yaw_degrees().to_radians();
            let (x, z) = (yaw.sin(), yaw.cos());
            let want = match face {
                Face::North => (0.0, -1.0),
                Face::East => (1.0, 0.0),
                Face::South => (0.0, 1.0),
                Face::West => (-1.0, 0.0),
            };
            assert!(
                (x - want.0).abs() < 1e-5 && (z - want.1).abs() < 1e-5,
                "{}: forward is ({x:.3}, {z:.3}), wanted {want:?}",
                face.label()
            );
        }
    }

    /// Round trip, and the snap: an angle within an eighth turn of a face resolves to it.
    #[test]
    fn an_angle_snaps_to_the_face_it_is_nearest() {
        for face in [Face::North, Face::East, Face::South, Face::West] {
            assert_eq!(Face::from_yaw(face.yaw_degrees()), face);
            // Off square by up to 44 degrees either way, and wrapped a full turn, still the same face.
            for drift in [-44.0, -1.0, 1.0, 44.0] {
                assert_eq!(Face::from_yaw(face.yaw_degrees() + drift), face, "{drift}");
                assert_eq!(Face::from_yaw(face.yaw_degrees() + drift + 360.0), face);
                assert_eq!(Face::from_yaw(face.yaw_degrees() + drift - 360.0), face);
            }
        }
        // The shipped value, which is what the four hand-measured fronts were written as.
        assert_eq!(Face::from_yaw(90.0), Face::East);
    }

    /// A yaw that is not a number cannot pick a face, and picking one at random would be a facing
    /// nobody authored. South is the identity — the zero-degree face — so it is the honest answer.
    #[test]
    fn a_yaw_that_is_not_an_angle_falls_to_the_identity_face() {
        assert_eq!(Face::from_yaw(f32::NAN), Face::South);
        assert_eq!(Face::South.yaw_degrees(), 0.0);
    }

    #[test]
    fn opposite_is_its_own_inverse_and_never_itself() {
        for face in [Face::North, Face::East, Face::South, Face::West] {
            assert_eq!(face.opposite().opposite(), face);
            assert_ne!(face.opposite(), face);
        }
    }

    /// A front and a lattice face are the same four numbers, so a piece's front can be used to read
    /// the cells it presents without a second mapping in between.
    #[test]
    fn a_face_is_the_same_direction_the_lattice_uses() {
        assert_eq!(Face::North.dir(), crate::wfc::N);
        assert_eq!(Face::East.dir(), crate::wfc::E);
        assert_eq!(Face::South.dir(), crate::wfc::S);
        assert_eq!(Face::West.dir(), crate::wfc::W);
    }

    /// A face survives the file it is written in.
    #[test]
    fn a_front_round_trips_through_ron() {
        let before = Descriptor {
            id: "chair".into(),
            align: Align {
                front: Some(Face::East),
                ..Align::default()
            },
            ..Descriptor::default()
        };
        let text = ron::ser::to_string_pretty(&before, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(text.contains("front: Some(East)"), "{text}");
        let after: Descriptor = ron::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(before, after);
    }
}

#[cfg(test)]
mod subgrid_tests {
    use super::*;

    fn grid(cells: Vec<SubCell>) -> Subgrid {
        Subgrid { cells }
    }

    fn cell(at: (u32, u32, u32)) -> SubCell {
        SubCell {
            at,
            ..SubCell::default()
        }
    }

    const D3: (u32, u32, u32) = (3, 3, 3);

    /// A descriptor of exactly this size, unstretched — the shape most of these tests want.
    fn sized(w: f32, h: f32, d: f32) -> Descriptor {
        Descriptor {
            id: "x".into(),
            extent: Extent {
                footprint: Some((w, d)),
                height: Some(h),
            },
            ..Descriptor::default()
        }
    }

    /// The default is the whole point of the feature being cheap: a piece that says nothing about its
    /// inside costs no rows at all.
    #[test]
    fn a_tile_says_nothing_about_its_inside_by_default() {
        let g = Subgrid::default();
        assert_eq!(Subgrid::volume(D3), 27);
        assert!(g.cells.is_empty());
        assert!(g.validate("x", D3).is_ok());
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
        let g = grid(vec![c.clone()]);
        assert!(g.validate("table", D3).is_ok());
        let got = g.at((2, 0, 1)).unwrap_or_else(|| panic!("no cell"));
        assert_eq!(got, &c);
        assert!(g.at((0, 0, 0)).is_none(), "unwritten cells are absent, not default rows");
    }

    #[test]
    fn a_cell_outside_the_lattice_is_refused() {
        let e = grid(vec![cell((3, 0, 0))])
            .validate("desk", D3)
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("outside"), "{e}");
    }

    #[test]
    fn naming_one_cell_twice_is_refused() {
        let e = grid(vec![cell((1, 1, 1)), cell((1, 1, 1))])
            .validate("desk", D3)
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("twice"), "{e}");
    }

    #[test]
    fn an_axis_with_no_divisions_is_refused() {
        let e = grid(vec![])
            .validate("desk", (3, 0, 3))
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("no cells"), "{e}");
    }

    /// A lattice survives the file, which is what makes it authorable at all.
    #[test]
    fn a_lattice_round_trips_through_ron() {
        let before = Descriptor {
            id: "desk".into(),
            subgrid: Some(grid(vec![SubCell {
                at: (0, 0, 2),
                solid: true,
                edge: Some("wall".into()),
                anchor: None,
            }])),
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
        assert_eq!(d.subgrid, None);
    }

    /// **The point of deriving divisions.** Two pieces of the same height present faces of the same
    /// length, so their edge tokens can be compared; two of different heights do not, and refusing is
    /// the honest answer rather than matching on a prefix.
    ///
    /// The numbers are the ones the plan was approved on, at the shipped `divisions: 1`.
    #[test]
    fn a_pieces_divisions_come_from_its_own_size() {
        let at = |d: Descriptor| divisions(&d, 1).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(at(sized(0.5, 0.5, 0.5)), (1, 1, 1), "crate");
        assert_eq!(at(sized(0.5, 0.9, 0.5)), (1, 2, 1), "chair");
        assert_eq!(at(sized(1.0, 0.75, 1.0)), (2, 2, 2), "table");
        assert_eq!(at(sized(3.0, 2.4, 0.5)), (6, 5, 1), "wall");
        assert_eq!(at(sized(1.0, 2.4, 0.5)), (2, 5, 1), "doorway");

        // A wall and a doorway are both 2.4 m, so both present five rows and can agree.
        let wall = at(sized(3.0, 2.4, 0.5));
        let doorway = at(sized(1.0, 2.4, 0.5));
        assert_eq!(wall.1, doorway.1);
        // A crate is not, and must not silently match the bottom of a wall.
        assert_ne!(at(sized(0.5, 0.5, 0.5)).1, wall.1);
    }

    /// Raising the project's number refines every axis of every piece by the same factor — the
    /// remedy Merrell & Manocha §4.5 names for objects that do not land on round multiples.
    #[test]
    fn dividing_a_tile_more_finely_refines_every_piece_equally() {
        let wall = sized(3.0, 2.4, 0.5);
        let one = divisions(&wall, 1).unwrap_or_else(|e| panic!("{e}"));
        let three = divisions(&wall, 3).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(three, (one.0 * 3, one.1 * 3, one.2 * 3));
    }

    /// A decal may omit its height (`Descriptor::resolve` says so) and still gets one layer rather
    /// than a degenerate lattice — `grid::cells` never returns zero.
    #[test]
    fn a_piece_with_no_height_still_has_one_layer() {
        let mut d = sized(1.0, 0.0, 1.0);
        d.extent.height = None;
        assert_eq!(divisions(&d, 1).unwrap_or_else(|err| panic!("{err}")), (2, 1, 2));
    }

    /// No footprint is refused by id, not resolved to a zero lattice that every rule reads as fine.
    #[test]
    fn a_piece_with_no_footprint_is_refused_by_name() {
        let d = Descriptor {
            id: "mystery".into(),
            ..Descriptor::default()
        };
        let err = divisions(&d, 1).err().unwrap_or_default();
        assert!(err.contains("mystery") && err.contains("footprint"), "{err}");
    }

    /// Zero divisions-per-tile is refused here too, so no caller can build a lattice with no cells.
    #[test]
    fn a_project_that_divides_a_tile_zero_ways_is_refused() {
        let err = divisions(&sized(1.0, 1.0, 1.0), 0).err().unwrap_or_default();
        assert!(err.contains("no cells"), "{err}");
    }

    /// A quarter turn swaps x and z; two turns are back to square; four are the identity.
    #[test]
    fn turning_swaps_the_x_and_z_divisions() {
        let div = (6, 5, 1);
        assert_eq!(rotate_div(div, 0), div);
        assert_eq!(rotate_div(div, 1), (1, 5, 6));
        assert_eq!(rotate_div(div, 2), div);
        assert_eq!(rotate_div(div, 3), (1, 5, 6));
        assert_eq!(rotate_div(div, 4), div);
    }
}

#[cfg(test)]
mod subgrid_edit_tests {
    use super::*;

    const D3: (u32, u32, u32) = (3, 3, 3);

    #[test]
    fn toggling_a_cell_solid_and_back_leaves_no_trace() {
        let mut g = Subgrid::default();
        assert_eq!(g.toggle_solid((1, 0, 1), D3), Some(true));
        assert_eq!(g.cells.len(), 1);
        assert_eq!(g.toggle_solid((1, 0, 1), D3), Some(false));
        // The sparse invariant: a cell that says nothing is absent, not a row of `solid: false`.
        assert!(g.cells.is_empty(), "an unmarked cell must leave no row behind");
    }

    /// A mesh scan states a fact rather than flipping one, so running it twice is running it once.
    #[test]
    fn marking_solid_is_idempotent_where_toggling_is_not() {
        let mut g = Subgrid::default();
        g.set_solid((1, 0, 1), D3).unwrap_or_else(|| panic!("in range"));
        g.set_solid((1, 0, 1), D3).unwrap_or_else(|| panic!("in range"));
        assert_eq!(g.cells.len(), 1);
        assert!(g.at((1, 0, 1)).is_some_and(|c| c.solid), "a second scan must not unmark it");
    }

    #[test]
    fn a_cell_outside_the_lattice_cannot_be_written() {
        let mut g = Subgrid::default();
        assert_eq!(g.toggle_solid((3, 0, 0), D3), None);
        assert_eq!(g.set_edge((0, 9, 0), D3, "wall"), None);
        assert!(g.cells.is_empty(), "an out-of-range write must not append anything");
    }

    #[test]
    fn tokens_set_and_clear_on_the_same_cell() {
        let mut g = Subgrid::default();
        g.set_edge((0, 0, 2), D3, "wall").unwrap_or_else(|| panic!("in range"));
        g.set_anchor((0, 0, 2), D3, "diner").unwrap_or_else(|| panic!("in range"));
        let c = g.at((0, 0, 2)).unwrap_or_else(|| panic!("written"));
        assert_eq!(c.edge.as_deref(), Some("wall"));
        assert_eq!(c.anchor.as_deref(), Some("diner"));

        // Emptying both takes the row with it, since solid was never set.
        g.set_edge((0, 0, 2), D3, "").unwrap_or_else(|| panic!("in range"));
        g.set_anchor((0, 0, 2), D3, "  ").unwrap_or_else(|| panic!("in range"));
        assert!(g.cells.is_empty());
    }

    /// A cell keeps whichever facets are still set — clearing one must not drop the others.
    #[test]
    fn clearing_one_facet_keeps_the_rest() {
        let mut g = Subgrid::default();
        g.toggle_solid((1, 1, 1), D3);
        g.set_edge((1, 1, 1), D3, "wall").unwrap_or_else(|| panic!("in range"));
        g.set_edge((1, 1, 1), D3, "").unwrap_or_else(|| panic!("in range"));
        let c = g.at((1, 1, 1)).unwrap_or_else(|| panic!("still solid"));
        assert!(c.solid);
        assert!(c.edge.is_none());
    }

    #[test]
    fn clear_forgets_the_whole_cell() {
        let mut g = Subgrid::default();
        g.toggle_solid((2, 2, 2), D3);
        g.set_anchor((2, 2, 2), D3, "seat").unwrap_or_else(|| panic!("in range"));
        g.clear((2, 2, 2));
        assert!(g.at((2, 2, 2)).is_none());
        assert!(g.validate("x", D3).is_ok());
    }

    /// A patch saying nothing about the lattice inherits it; a patch stating one replaces it, and an
    /// **empty** stated lattice clears every cell. Those last two were indistinguishable before
    /// `subgrid` became an `Option`, and the inherit reading silently won.
    #[test]
    fn a_patch_can_state_an_empty_lattice_without_it_meaning_silence() {
        let base = Descriptor {
            id: "wall".into(),
            subgrid: Some(Subgrid {
                cells: vec![SubCell {
                    at: (0, 0, 0),
                    solid: true,
                    ..SubCell::default()
                }],
            }),
            ..Descriptor::default()
        };

        let silent = base.patched_with(&Descriptor::default());
        assert_eq!(silent.subgrid, base.subgrid, "no opinion inherits");

        let cleared = base.patched_with(&Descriptor {
            subgrid: Some(Subgrid::default()),
            ..Descriptor::default()
        });
        assert_eq!(
            cleared.subgrid,
            Some(Subgrid::default()),
            "an explicitly empty lattice clears the cells rather than inheriting them"
        );
    }
}

/// The rotation field: what it accepts, what it refuses, and how it layers.
#[cfg(test)]
mod rotate_tests {
    use super::*;

    #[test]
    fn a_rotation_is_read_as_quarter_turns() {
        assert_eq!(quarter_turns_xyz((0, 0, 0), "x"), Ok((0, 0, 0)));
        assert_eq!(quarter_turns_xyz((90, 180, 270), "x"), Ok((1, 2, 3)));
        // Wrapped either way — an author writing -90 means three quarters, not an error.
        assert_eq!(quarter_turns_xyz((-90, 360, 720), "x"), Ok((3, 0, 0)));
    }

    /// Refused rather than rounded. Rounding would draw the mesh at one angle while every
    /// measurement beside it described another.
    #[test]
    fn a_rotation_that_is_not_a_quarter_turn_is_refused_by_name() {
        let err = quarter_turns_xyz((0, 45, 0), "lamp").err().unwrap_or_default();
        assert!(err.contains("lamp") && err.contains("45") && err.contains("about Y"), "{err}");
    }

    /// A patch may state a rotation, and silence inherits — the rule every other `Align` field holds.
    #[test]
    fn a_rotation_layers_like_every_other_correction() {
        let base = Descriptor {
            id: "door".into(),
            align: Align {
                rotate: Some((90, 0, 0)),
                ..Align::default()
            },
            ..Descriptor::default()
        };
        assert_eq!(
            base.patched_with(&Descriptor::default()).align.rotate,
            Some((90, 0, 0)),
            "silence inherits"
        );
        assert_eq!(
            base.patched_with(&Descriptor {
                align: Align {
                    rotate: Some((0, 90, 0)),
                    ..Align::default()
                },
                ..Descriptor::default()
            })
            .align
            .rotate,
            Some((0, 90, 0)),
            "a stated rotation wins"
        );
    }

    /// A rotation survives the file, and reads as degrees rather than as a count nobody can picture.
    #[test]
    fn a_rotation_round_trips_through_ron() {
        let before = Descriptor {
            id: "door".into(),
            align: Align {
                rotate: Some((90, 0, 180)),
                ..Align::default()
            },
            ..Descriptor::default()
        };
        let text = ron::ser::to_string_pretty(&before, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(text.contains("rotate: Some((90, 0, 180))"), "{text}");
        assert_eq!(
            ron::from_str::<Descriptor>(&text).unwrap_or_else(|e| panic!("{e}")),
            before
        );
    }
}

/// **Which lattice cell a ray enters, and through which face.**
///
/// The lattice is an axis-aligned box from `origin` spanning `size`, divided `div` ways per axis.
/// `dir` need not be normalised. Returns `None` when the ray misses, or when it starts inside the box
/// — a camera inside the piece it is authoring has no "entry" to report, and guessing one would put
/// the cursor on a face the author cannot see.
///
/// # What the face means, and what it does not
///
/// The face is **where you are looking from**, which is what makes a token's audience legible: an
/// author putting `seam` on a cell wants to know which neighbour will read it. It is not a new place
/// to store anything — [`SubCell::edge`] is one token per *cell*, and
/// [`crate::adjacency::face`] collects the tokens of whichever cells lie on the face being compared.
/// A corner cell is on two faces and presents the same token to both, by design.
///
/// `None` for the face means the ray came in through the top or the bottom. Those are real surfaces
/// to click on — you pick cells from above constantly — but they are not faces any rule reads, since
/// adjacency is horizontal. Saying `None` is more honest than naming a direction that matches nothing.
///
/// Slab method: clip the ray against each pair of parallel planes and keep the latest entry. The axis
/// that produced that entry is the face it came through.
pub fn pick_cell(
    ray_origin: [f32; 3],
    dir: [f32; 3],
    origin: [f32; 3],
    size: [f32; 3],
    div: (u32, u32, u32),
) -> Option<((u32, u32, u32), Option<Face>)> {
    let n = [div.0, div.1, div.2];
    if n.iter().any(|d| *d == 0) || size.iter().any(|s| *s <= 0.0) {
        return None;
    }
    if ray_origin.iter().chain(dir.iter()).any(|v| !v.is_finite()) {
        return None;
    }

    let mut t_enter = f32::NEG_INFINITY;
    let mut t_exit = f32::INFINITY;
    // Which axis let the ray in last, and from which side. `None` until a slab actually bounds it —
    // a ray parallel to every axis but one still has a face.
    let mut entry: Option<(usize, bool)> = None;

    for a in 0..3 {
        let (lo, hi) = (origin[a], origin[a] + size[a]);
        if dir[a].abs() < 1e-9 {
            // Parallel to this slab: inside it forever, or outside it forever.
            if ray_origin[a] < lo || ray_origin[a] > hi {
                return None;
            }
            continue;
        }
        let (mut t1, mut t2) = ((lo - ray_origin[a]) / dir[a], (hi - ray_origin[a]) / dir[a]);
        // `from_low` tracks which plane t1 is, before the swap loses that.
        let from_low = t1 <= t2;
        if !from_low {
            std::mem::swap(&mut t1, &mut t2);
        }
        if t1 > t_enter {
            t_enter = t1;
            entry = Some((a, from_low));
        }
        t_exit = t_exit.min(t2);
        if t_enter > t_exit {
            return None;
        }
    }

    // Behind the camera, or starting inside. Both are "no entry face to report".
    if t_enter <= 0.0 {
        return None;
    }

    let hit = [
        ray_origin[0] + dir[0] * t_enter,
        ray_origin[1] + dir[1] * t_enter,
        ray_origin[2] + dir[2] * t_enter,
    ];
    let mut cell = [0u32; 3];
    for a in 0..3 {
        let frac = (hit[a] - origin[a]) / size[a];
        // Clamped rather than refused: the entry point sits exactly on a boundary by construction, and
        // floating point puts it a hair outside about half the time.
        cell[a] = ((frac * n[a] as f32) as i64).clamp(0, n[a] as i64 - 1) as u32;
    }

    let face = entry.and_then(|(axis, from_low)| match (axis, from_low) {
        // Entering through the low-X plane means looking at the piece's WEST face.
        (0, true) => Some(Face::West),
        (0, false) => Some(Face::East),
        // North is −Z, so the low-Z plane is the north face.
        (2, true) => Some(Face::North),
        (2, false) => Some(Face::South),
        // Top or bottom: a surface, but not a face adjacency reads.
        _ => None,
    });
    Some(((cell[0], cell[1], cell[2]), face))
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    const ORIGIN: [f32; 3] = [0.0, 0.0, 0.0];
    const SIZE: [f32; 3] = [3.0, 2.4, 0.5];
    /// The shipped wall's lattice.
    const DIV: (u32, u32, u32) = (6, 5, 1);

    /// Looking at the wall from +X hits its EAST face and the last x cell.
    #[test]
    fn a_ray_from_the_east_reports_the_east_face() {
        let got = pick_cell([10.0, 1.2, 0.25], [-1.0, 0.0, 0.0], ORIGIN, SIZE, DIV);
        let ((x, _, _), face) = got.unwrap_or_else(|| panic!("must hit"));
        assert_eq!(x, DIV.0 - 1, "the near column, not the far one");
        assert_eq!(face, Some(Face::East));
    }

    /// And from −X, the west face and cell zero. The pair together is the check that the sign of the
    /// slab is not inverted — one of them alone would pass either way.
    #[test]
    fn a_ray_from_the_west_reports_the_west_face() {
        let got = pick_cell([-10.0, 1.2, 0.25], [1.0, 0.0, 0.0], ORIGIN, SIZE, DIV);
        let ((x, _, _), face) = got.unwrap_or_else(|| panic!("must hit"));
        assert_eq!(x, 0);
        assert_eq!(face, Some(Face::West));
    }

    /// North is −Z and South is +Z, matching `crate::wfc`. Getting this backwards would put every
    /// token on the wrong side of every wall.
    #[test]
    fn the_z_faces_follow_the_projects_own_compass() {
        let from_north = pick_cell([1.5, 1.2, -10.0], [0.0, 0.0, 1.0], ORIGIN, SIZE, DIV);
        assert_eq!(from_north.and_then(|(_, f)| f), Some(Face::North));
        let from_south = pick_cell([1.5, 1.2, 10.0], [0.0, 0.0, -1.0], ORIGIN, SIZE, DIV);
        assert_eq!(from_south.and_then(|(_, f)| f), Some(Face::South));
    }

    /// **Looking down picks a cell but names no face.** Adjacency is horizontal, so there is no face
    /// here that any rule reads — and inventing one would put a token where nothing looks for it.
    #[test]
    fn looking_down_gives_a_cell_and_no_face() {
        let got = pick_cell([1.5, 10.0, 0.25], [0.0, -1.0, 0.0], ORIGIN, SIZE, DIV);
        let ((_, y, _), face) = got.unwrap_or_else(|| panic!("must hit"));
        assert_eq!(y, DIV.1 - 1, "the top layer");
        assert_eq!(face, None, "top and bottom are surfaces, not faces");
    }

    /// The cell tracks where along the face the ray landed, which is the whole point of picking.
    #[test]
    fn the_cell_follows_the_hit_along_the_face() {
        // Each x cell is 0.5 m wide; aim at the middle of the third.
        let got = pick_cell([1.25, 0.25, 10.0], [0.0, 0.0, -1.0], ORIGIN, SIZE, DIV);
        let ((x, y, z), _) = got.unwrap_or_else(|| panic!("must hit"));
        assert_eq!((x, y, z), (2, 0, 0), "third column, bottom layer");
    }

    /// A miss is a miss, in every direction it can be one.
    #[test]
    fn a_ray_that_misses_reports_nothing() {
        // Past the end of the wall.
        assert!(pick_cell([10.0, 1.2, 9.0], [-1.0, 0.0, 0.0], ORIGIN, SIZE, DIV).is_none());
        // Above it.
        assert!(pick_cell([10.0, 9.0, 0.25], [-1.0, 0.0, 0.0], ORIGIN, SIZE, DIV).is_none());
        // Pointing away from it.
        assert!(pick_cell([10.0, 1.2, 0.25], [1.0, 0.0, 0.0], ORIGIN, SIZE, DIV).is_none());
        // Starting inside: there is no entry face to name.
        assert!(pick_cell([1.5, 1.2, 0.25], [0.0, 0.0, -1.0], ORIGIN, SIZE, DIV).is_none());
    }

    /// Degenerate input is refused rather than answered with cell zero.
    #[test]
    fn a_lattice_or_ray_that_cannot_be_picked_is_refused() {
        assert!(pick_cell([0.0, 0.0, 10.0], [0.0, 0.0, -1.0], ORIGIN, SIZE, (0, 1, 1)).is_none());
        assert!(pick_cell([0.0, 0.0, 10.0], [0.0, 0.0, -1.0], ORIGIN, [0.0; 3], DIV).is_none());
        assert!(pick_cell([f32::NAN, 0.0, 10.0], [0.0, 0.0, -1.0], ORIGIN, SIZE, DIV).is_none());
    }

    /// **Every cell is reachable**, and the cell a ray picks is the cell whose box contains the hit.
    /// Swept rather than sampled at one point, because an off-by-one in the clamp would show up only
    /// at the edges.
    #[test]
    fn sweeping_the_face_walks_every_column_in_order() {
        let mut seen = Vec::new();
        for i in 0..DIV.0 {
            // The middle of each column.
            let x = (i as f32 + 0.5) * SIZE[0] / DIV.0 as f32;
            let got = pick_cell([x, 0.25, 10.0], [0.0, 0.0, -1.0], ORIGIN, SIZE, DIV);
            let ((cx, _, _), _) = got.unwrap_or_else(|| panic!("column {i} must hit"));
            seen.push(cx);
        }
        assert_eq!(seen, (0..DIV.0).collect::<Vec<_>>());
    }
}
