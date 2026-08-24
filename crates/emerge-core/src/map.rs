//! **The map** — what is placed where, and what can be done there.
//!
//! A map references [`crate::descriptor::Descriptor`]s by id and positions them. It also carries
//! **locations**, which is where interactions live.
//!
//! # Why interactions belong to a location and not to a prop
//!
//! The obvious design is `interactions: [...]` on each descriptor: a chair affords sitting, a bed
//! affords sleeping. It does not survive contact with a dining table.
//!
//! A table plus four chairs is **one** affordance with four seats, not five affordances. FINAL
//! FANTASY XV solved this with *smart locations* — *"invisible objects that refer to multiple
//! concrete objects. For example, a single smart location may refer to two chairs and a table. This
//! allows it not only to inform agents about the existence and usability of individual objects, but
//! also to capture relationships between them, such as furniture grouping… they essentially govern
//! the usage of the objects they refer to"* (Game AI Pro 3 ch.35). Smart Zones (Game AI Pro 2 ch.11)
//! add the role strata this reuses.
//!
//! A single-prop interaction is then the degenerate case — one prop, one `Main` role — so nothing is
//! lost by starting here, and retrofitting group ownership after interactions ship would be a schema
//! migration through every authored map.
//!
//! # Prose is a field, not a comment
//!
//! Every addressable thing here carries an optional `note:`. That is deliberate and it is what lets a
//! map be written by an ordinary serializer.
//!
//! This project already knows what happens otherwise. `assets/site/site67.ron` is 15% comments and its
//! props list carries more prose than data; `assets/config/config.ron` carries ~563 comment lines, and
//! on 2026-07-16 a `to_string_pretty` bake deleted 279 of them. The response there was
//! [`crate::ron_surgery`] — rewrite the file as text so the comments survive. That is the right answer
//! for a file a human authored and a tool visits.
//!
//! For a format being designed now it is the wrong problem to solve. If the reasoning is a **field**,
//! no serializer can lose it, no writer needs to be surgical, and the note survives a round-trip
//! through any tool that understands the schema. [`Placed::owned_because`] was already this idea in
//! one specific place — a reason stored as data precisely so nothing can strip it — and `note:`
//! generalises it.
//!
//! So: an emerge map is serialized normally and **never** text-spliced. The surgical writer stays for
//! `site67.ron` and `config.ron`, whose prose is a 48-line ASCII floor plan and paragraphs introducing
//! blocks of records — none of it attached to a record, so none of it with a field to live in.
//!
//! # Versioning: refuse what you cannot understand
//!
//! `persist.rs` states the rule this follows: a map from another schema is a loud error rather than a
//! guess. What that means precisely is **a floor, not an equality** — a file numbered *above* this
//! build is refused, and one at or below it is read and re-saved at the current number.
//!
//! It was an equality until [`Stamped`] arrived, and the difference is worth stating because it looks
//! like a loosening and is not. The hazard versioning exists to stop is an **old** build opening a
//! **new** file, understanding half of it, and writing the other half away — a map silently losing
//! every stamp in it. That is exactly what the floor still refuses. The equality additionally refused
//! the harmless direction, where a build that knows about stamps opens a file written before they
//! existed: that file says `stamps: []`, which is precisely what it meant. Refusing it destroyed a
//! map to prevent nothing.
//!
//! So the rule reads: **refuse if the file's version is greater than [`MAP_VERSION`].** One rule, one
//! direction, and no compatibility branch — a field added since is `#[serde(default)]` and its default
//! is what the older file already meant.

use serde::{Deserialize, Serialize};

use crate::descriptor::Descriptor;
use crate::placement::ir::Guard;

/// Bumped whenever the shape below changes. A file numbered above this is refused; at or below it is
/// read and re-saved here. See the module note on why that is a floor rather than an equality.
///
/// `2` added [`Map::stamps`]. `3` took `face_bands` and `snap_divisor` off the kit's policy, where
/// they described a lattice a *kit* does not have. `4` added `palette`, whose empty default
/// is what every earlier map already meant. **`5` gave the two lattice fields up again**, to
/// [`crate::kits::Lattice`]: a map does have exactly one lattice, but so does the project, and a
/// tile's adjacency contract cannot depend on which map happens to be open. That one is a
/// *removal*, so `deny_unknown_fields` means a map still carrying them is refused by name rather
/// than read with them ignored — which is the loud failure this schema's rules are for. **`6`
/// replaced `palette` with [`Map::bash`]** — the ad-hoc per-map list became a combination declared
/// once in `kits.ron`, so two maps can draw on the same one and it is validated in one place.
pub const MAP_VERSION: u32 = 6;

/// One authored world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    pub version: u32,
    /// What this map is called — **snake_case**, and it is also the file it lives in.
    ///
    /// One spelling, because a name is simultaneously a filename, a manifest key and something typed
    /// into a shell, and those have different tolerances: two names differing only by case are one
    /// file on some platforms and two on others. `crate::naming` forces it at the point of entry
    /// rather than complaining afterwards.
    #[serde(default)]
    pub name: String,
    /// Where this map sits in the world: **the centre of its floor.**
    ///
    /// X and Z are centred on it, Y runs upward from it. That asymmetry is the honest one — a map is
    /// a floor plan with headroom, and nobody builds below the floor — and centring the horizontal
    /// axes is what makes "a 32 metre map" mean 16 metres in every direction rather than a quadrant.
    ///
    /// Corner-at-origin was the first attempt and it failed the moment it was used: the editor opens
    /// looking at the origin, so half the visible ground was outside the map and a flood fill aimed at
    /// the middle of the screen was refused as out of bounds. The bug was the convention, not the fill.
    pub origin: (f32, f32, f32),
    /// **How big this map is**, in metres on each axis, measured from [`Self::origin`].
    ///
    /// A map without a stated size is not a smaller map, it is a map with no edges — and an edge is
    /// what several things need in order to be answerable. A flood fill needs somewhere to stop; a
    /// generator needs a domain to solve over; a validator needs to be able to say a placement is
    /// outside. Leaving it implicit makes each of those invent its own answer, which is how three
    /// slightly different ideas of "the level" end up in one codebase.
    ///
    /// Y is real and not decoration: a map is a volume, because `Mount::OnWall` and `OnCeiling` put
    /// things above the floor and the ceiling height is what decides where.
    #[serde(default = "default_bounds")]
    pub bounds: (f32, f32, f32),
    /// **The named combination of kits this map offers**, or `None` for every bound kit.
    ///
    /// # A filter on the palette, and never on what loads
    ///
    /// This does **not** decide what the map can resolve. Every kit the project binds is loaded
    /// whatever this says, so a piece already placed always resolves and a composition may still
    /// cross kits. `OpenMap::palette_namespaces` folds the namespaces the content already names
    /// back in, so naming a bash cannot strand a placement.
    ///
    /// The name is not checked here — a map validates in isolation and cannot see `kits.ron`.
    /// `OpenMap::open` refuses a name the project does not declare.
    ///
    /// **No `serde(default)`, and it would change nothing if there were one.** Serde deserializes a
    /// missing field of `Option` type as `None` (`serde::__private::de::missing_field`, which only
    /// answers `deserialize_option`), so an omitted `bash` and a written `bash: None` are the same
    /// value in every format — there is no hook that makes a bare `Option` field required. That is
    /// survivable here and would not be for [`crate::kits::Kits::bash`]: this field has exactly two
    /// meanings and the absent spelling maps onto the one that is also the new-map state, while an
    /// absent list of *declarations* would hide the difference between "none" and "not stated".
    /// Everything this program writes states it: nothing here is `skip_serializing_if`.
    pub bash: Option<String>,
    pub placements: Vec<Placed>,
    /// **Compositions this map stamps** — a reference each, never the rows they stand for.
    ///
    /// Expanded by [`crate::composition::expand`] at render, at validation and at load, and **never**
    /// written back into [`Self::placements`]. That is what makes editing a composition change every
    /// map that stamped it, and it is also what keeps this file's undo history addressable: an edit
    /// somewhere else cannot renumber rows that were never here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stamps: Vec<crate::composition::Stamped>,
    #[serde(default)]
    pub locations: Vec<Location>,
    /// What this map is and why it is laid out this way — the header prose, as data. See the module
    /// docs on why this is a field rather than a comment.
    #[serde(default)]
    pub note: Option<String>,
}

impl Default for Map {
    /// **A new, empty, valid map** — not a zeroed struct.
    ///
    /// The derived `Default` would give `version: 0` and zero bounds, which `validate` rejects on both
    /// counts. An editor starts a map from this, so "default" has to mean something an author can
    /// immediately work in rather than something they must first repair.
    fn default() -> Self {
        Map {
            version: MAP_VERSION,
            // Empty, not "untitled": a substituted name is a name nobody chose, and the second one
            // collides with the first. An unnamed map is a map that has to be named before it saves.
            name: String::new(),
            origin: (0.0, 0.0, 0.0),
            bounds: default_bounds(),
            bash: None,
            placements: Vec::new(),
            stamps: Vec::new(),
            locations: Vec::new(),
            note: None,
        }
    }
}

/// serde's `skip_serializing_if` needs a function, and these two say the only thing they exist to
/// say: an absent field and its default are the same fact, so a default is not written.
fn lift_is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn tip_is_zero(v: &(u8, u8)) -> bool {
    *v == (0, 0)
}

fn paint_is_zero(v: &i8) -> bool {
    *v == 0
}

/// A new map's size before anyone has said otherwise: 32 m square and one storey, so an author opens
/// on 16 m of workable ground in every direction.
///
/// Not zero. A zero-sized map is a map every placement is outside of, so the first thing an author
/// would do is see every piece flagged — a default should be somewhere to start work, not a puzzle.
/// The most a map may divide one tile.
fn default_bounds() -> (f32, f32, f32) {
    // **Ten metres square, four tall.** It was 32 x 32, which is 1,024 cells of floor an author
    // has to pan across before the map reads as a place rather than a plain — and the grid at the
    // finest rung over 32 m is the wash `draw_map_grid` records. Ten is a room-and-corridor's worth
    // of ground: big enough to lay a kit out on, small enough to see whole at the opening zoom.
    // Asked for at the keyboard, 2026-08-15. A map that wants more says so in its own file; this is
    // only what a NEW one starts at.
    (10.0, 4.0, 10.0)
}

/// One instance of a descriptor, somewhere.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Placed {
    /// Unique within the map. Referenced by [`Location::props`], so it must be stable across edits —
    /// which is why it is an authored string and not a vec index. An index would silently re-point
    /// every location the moment a placement above it was deleted.
    pub id: String,
    /// Which [`Descriptor`] this is an instance of.
    pub descriptor: String,
    /// Position in map space, metres. **The floor plan only** — the height comes from the
    /// descriptor's [`crate::descriptor::Mount`], never from the author, so a lamp cannot be authored
    /// hovering 3 cm above the table it is meant to be standing on. ([`Self::lift`] is the one
    /// deliberate amendment.)
    pub at: (f32, f32),
    pub yaw: f32,
    /// **Authored vertical offset, metres, layered on top of the resolved height.**
    ///
    /// The deliberate amendment to the rule on [`Self::at`]: `stack::resolve_y` still decides the
    /// datum — floor, wall height, the host's surface, so a lamp still follows its table — and this
    /// nudges the result afterwards. The editor's lift keys step it by one subgrid unit
    /// (`grid::SNAP / divisions`); it is stored in metres so the file keeps meaning if the project
    /// ever re-divides.
    ///
    /// Zero in every file written before the field existed, and skipped when zero so those files
    /// stay byte-identical. A build from before the field refuses a map that carries it
    /// (`deny_unknown_fields`) — loud, which is [`MAP_VERSION`]'s own rule.
    #[serde(default, skip_serializing_if = "lift_is_zero")]
    pub lift: f32,
    /// **Quarter turns tipping the piece over: (about X, about Z), each `0..=3`.**
    ///
    /// Set dressing — a tipped crate, a fallen chair. Quarter turns rather than free angles because
    /// a footprint stays answerable under axis swaps and under nothing else (`fill::cell_extents`
    /// records the lesson: a 30°-tipped rectangle has no honest cell). Applied in the piece's own
    /// frame *before* [`Self::yaw`] turns it, and the spawner re-seats the tipped bounds on the
    /// resolved height so tipping never buries a mesh.
    ///
    /// A tipped piece offers no surface — `stack::host_under` skips it, and the editor refuses to
    /// tip a piece while something rests on it — because "where is the tabletop of a table lying on
    /// its side" has no answer worth inventing.
    #[serde(default, skip_serializing_if = "tip_is_zero")]
    pub tip: (u8, u8),
    /// The [`Self::id`] of the placement this one **rests on**, for a descriptor that mounts
    /// `OnSurface`.
    ///
    /// A reference rather than a Y coordinate, and that is the whole point: move the table and the
    /// lamp goes with it. A stored height would be correct exactly until someone dragged the host, and
    /// then silently wrong in a way that reads as the lamp being badly authored.
    ///
    /// `None` for everything that stands on the floor, hangs from the ceiling or fills a doorway —
    /// those layers derive their height from the map, not from a neighbour.
    #[serde(default)]
    pub on: Option<String>,
    /// **Owned by the author** — a generator must route around it rather than through it.
    ///
    /// This is the lock in Smelik et al.'s sense and the `initial` domain in WFC's: an owned cell is a
    /// unary constraint, a cell whose domain is narrowed to one prototype before propagation. See
    /// `docs/2026-08-03-kitbash-editor.md`.
    #[serde(default)]
    pub owned: bool,
    /// Why it is owned. A **reason, never a bool** — the same call `PropPlacement::waive` makes, and
    /// for the same argument: a bool lets "I could not be bothered" and "this is the cell block's only
    /// entrance" look identical in a diff.
    #[serde(default)]
    pub owned_because: Option<String>,
    /// Per-instance overrides layered over the descriptor. Absence inherits.
    #[serde(default)]
    pub patch: Option<Descriptor>,
    /// **Paint order among things at the same spot** — higher draws in front.
    ///
    /// Purely cosmetic. It does **not** gate overlap (`stack::blocking` never reads it), does not
    /// change what a tile presents to its neighbours, and does not move anything: a floor, the grime
    /// on it and the marking over that are all at the same height and differ only here.
    ///
    /// # What it does and does not deliver
    ///
    /// Applied as `StandardMaterial::depth_bias`, which biases the **depth comparison**. That is the
    /// right tool for coplanar surfaces — two decals on one floor — and it is **not** a general
    /// stacking order: it will not lift something in front of unrelated geometry it sits well behind.
    /// Say so here, because the field's name promises more than the mechanism gives.
    ///
    /// `i8` on purpose. The renderer caches one material per `(base, paint)` pair, so an unbounded
    /// value is an unbounded cache — and a kit needs a handful of layers, not thousands.
    #[serde(default, skip_serializing_if = "paint_is_zero")]
    pub paint: i8,
    /// Why this prop is here — the trailing `// records desk` of the old format, as data.
    ///
    /// Distinct from [`Self::owned_because`], which answers a narrower question a generator has to
    /// respect. This one is for the reader.
    #[serde(default)]
    pub note: Option<String>,
}

/// An invisible thing that owns a group of props and governs their use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub id: String,
    /// [`Placed::id`]s this location governs. May be one.
    pub props: Vec<String>,
    pub interactions: Vec<Interaction>,
    /// What this grouping *is* — "the galley's near table", "the bunk nobody uses". A location is
    /// invisible, so without this it is the one thing in a map with no way to explain itself.
    #[serde(default)]
    pub note: Option<String>,
}

/// Something that can happen here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interaction {
    /// `"eat"`, `"sleep"`, `"repair"`. Opaque — matched, never interpreted.
    pub verb: String,
    pub roles: Vec<RoleSlot>,
    /// Precondition over world state. `ir::Guard` has been declared and unused since the constraint IR
    /// landed; this is the consumer it was reserved for.
    #[serde(default)]
    pub guard: Option<Guard>,
    pub effects: Vec<Effect>,
    /// Why this interaction exists here, and anything a reader would otherwise have to infer from the
    /// role counts.
    #[serde(default)]
    pub note: Option<String>,
}

/// A part an agent can play in an interaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleSlot {
    pub name: String,
    pub kind: RoleKind,
    pub min: u8,
    pub max: u8,
    /// Which [`crate::descriptor::Socket::role`] an occupant stands at.
    #[serde(default)]
    pub socket_role: Option<String>,
    /// What an occupant must be able to do — tokens from the `capabilities` axis, **all** of which
    /// are required.
    ///
    /// Empty means anybody, which is the honest reading of a role that states no requirement, and is
    /// right for an `Extra` standing around. A `Main` role usually wants something: a diner can eat, a
    /// server can cook.
    ///
    /// All rather than any: a role wanting somebody who can cook *and* carry wants both, and an
    /// "intersects" test would hand it somebody who can only carry. Game AI Pro 4 ch.4's mask compare,
    /// with the requirement side as the subset test it has to be.
    #[serde(default)]
    pub requires: Vec<String>,
}

/// Smart Zones' three strata, kept because they encode *when a scene may start*.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RoleKind {
    /// *"Main roles are essential… The scene won't start unless all the main roles are fulfilled."*
    Main,
    /// Favourable but not required.
    Supporting,
    /// Ambient bystanders.
    Extra,
}

/// What an interaction does.
///
/// Deliberately **closed** — there is no `Custom(String)` escape hatch. The IR's `Role::Custom` was
/// added for exactly that reason and has never been constructed in the life of the codebase; an open
/// variant that nothing produces is dead surface that every reader has to consider. Growing this enum
/// should be a deliberate edit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Effect {
    /// Refill a named drive at this rate per second while the interaction runs.
    Restore { drive: String, rate: f32 },
    /// Drain one, same units.
    Drain { drive: String, rate: f32 },
}

impl Map {
    /// Parse and validate. One path, no fallback — every failure names what and where.
    pub fn parse(text: &str) -> Result<Map, String> {
        let map: Map = ron::from_str(text).map_err(|e| format!("map: {e}"))?;
        map.validate()?;
        Ok(map)
    }

    /// Everything that must hold before a map is usable.
    ///
    /// `descriptors` is the set of known ids; pass an empty slice to skip that cross-check when
    /// validating a map in isolation.
    pub fn validate_against(&self, descriptors: &[String]) -> Result<(), String> {
        self.validate()?;
        if descriptors.is_empty() {
            return Ok(());
        }
        for p in &self.placements {
            if !descriptors.iter().any(|d| d == &p.descriptor) {
                return Err(format!(
                    "map: placement `{}` names descriptor `{}`, which does not exist",
                    p.id, p.descriptor
                ));
            }
        }
        Ok(())
    }

    /// The map's floor rectangle **in map space**: `(min_x, min_z, max_x, max_z)`.
    ///
    /// Map space, because that is the space [`Placed::at`] is in and this rectangle exists to say
    /// which `at` values are inside the map. It is a floor *plan*: centred on zero, and the same
    /// rectangle wherever the map is standing.
    ///
    /// It used to return world metres — origin ± half — while every caller compared it against an
    /// `at`. The two agree for a map at the origin, which is the only kind the editor authors, so the
    /// disagreement was invisible: a map moved to `(100, 0, 0)` would have called every one of its own
    /// placements out of bounds. Add [`Self::origin`] to draw it.
    ///
    /// One place computes this, because a convention re-derived at three call sites is a convention
    /// that will disagree with itself at one of them.
    pub fn floor_rect(&self) -> (f32, f32, f32, f32) {
        let (hx, hz) = (self.bounds.0 * 0.5, self.bounds.2 * 0.5);
        (-hx, -hz, hx, hz)
    }

    /// A world point on the ground, expressed in map space — what a cursor hit has to become before
    /// it can be compared against a [`Placed::at`] or written into one.
    pub fn to_map_space(&self, world_xz: (f32, f32)) -> (f32, f32) {
        (world_xz.0 - self.origin.0, world_xz.1 - self.origin.2)
    }

    /// Floor and ceiling heights in world metres. Y runs upward from the origin, not either side of
    /// it — see [`Self::origin`].
    pub fn height_span(&self) -> (f32, f32) {
        (self.origin.1, self.origin.1 + self.bounds.1)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !crate::naming::is_snake_case(&self.name) {
            let suggestion = crate::naming::to_snake_case(&self.name);
            return Err(if suggestion.is_empty() {
                format!(
                    "map: `{}` is not a usable name. Names are snake_case — lowercase letters, \
                     digits and single underscores, starting with a letter.",
                    self.name
                )
            } else {
                format!(
                    "map: `{}` is not snake_case. Call it `{suggestion}`.",
                    self.name
                )
            });
        }

        for (axis, v) in [
            ("x", self.bounds.0),
            ("y", self.bounds.1),
            ("z", self.bounds.2),
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(format!(
                    "map: bounds.{axis} is {v}. A map has to enclose something — a non-positive \
                     extent makes every placement out of bounds and leaves a flood fill nowhere to \
                     stop."
                ));
            }
        }

        if self.version > MAP_VERSION {
            return Err(format!(
                "map: version {} but this build reads {MAP_VERSION} — refusing a map written by a \
                 newer tool. Opening it would mean understanding part of it and writing the rest \
                 away.",
                self.version
            ));
        }

        let mut seen: Vec<&str> = Vec::with_capacity(self.placements.len());
        for p in &self.placements {
            if p.id.is_empty() {
                return Err("map: a placement has an empty id".to_owned());
            }
            if seen.contains(&p.id.as_str()) {
                return Err(format!(
                    "map: placement id `{}` is used twice — locations reference placements by id, so \
                     a duplicate makes `props` ambiguous",
                    p.id
                ));
            }
            seen.push(&p.id);
            if !p.lift.is_finite() {
                return Err(format!(
                    "map: placement `{}` has lift {} — a non-finite offset places it nowhere",
                    p.id, p.lift
                ));
            }
            if p.tip.0 > 3 || p.tip.1 > 3 {
                return Err(format!(
                    "map: placement `{}` has tip {:?} — quarter turns are 0..=3 per axis",
                    p.id, p.tip
                ));
            }
            // An owned placement without a reason is the bool-instead-of-reason shape this schema
            // refuses on purpose.
            if p.owned && p.owned_because.as_ref().is_none_or(|r| r.trim().is_empty()) {
                return Err(format!(
                    "map: placement `{}` is owned but says nothing about why. An owned placement \
                     constrains a generator; in six months only that sentence can say whether it \
                     still should.",
                    p.id
                ));
            }
        }

        // **What rests on what.** Checked after every id is known, because a host may be authored
        // below its guest in the file and order is not the author's problem.
        for p in &self.placements {
            let Some(host) = &p.on else { continue };
            if host == &p.id {
                return Err(format!(
                    "map: placement `{}` rests on itself. Its height would be its own height, which \
                     has no answer.",
                    p.id
                ));
            }
            if !seen.contains(&host.as_str()) {
                return Err(format!(
                    "map: placement `{}` rests on `{host}`, which does not exist. A piece whose host \
                     is missing has no floor to stand on — placing it at zero would put it through \
                     the ground.",
                    p.id
                ));
            }
        }
        self.no_stacking_cycles()?;

        // **Stamps share the placement id space.** The rows a stamp expands to are named
        // `<stamp>/<member>`, so a stamp sharing a name with a placement is a location's `props`
        // pointing at two different things.
        for st in &self.stamps {
            if st.id.is_empty() {
                return Err("map: a stamp has an empty id".to_owned());
            }
            if seen.contains(&st.id.as_str()) {
                return Err(format!(
                    "map: stamp `{}` shares its id with a placement. Every row a stamp expands to is \
                     named after it, so the two id spaces are one.",
                    st.id
                ));
            }
            if !st.at.0.is_finite() || !st.at.1.is_finite() || !st.yaw.is_finite() {
                return Err(format!(
                    "map: stamp `{}` is at a position that is not a number",
                    st.id
                ));
            }
            if st.owned
                && st
                    .owned_because
                    .as_ref()
                    .is_none_or(|r| r.trim().is_empty())
            {
                return Err(format!(
                    "map: stamp `{}` is owned but says nothing about why. An owned stamp constrains a \
                     generator; in six months only that sentence can say whether it still should.",
                    st.id
                ));
            }
        }
        let mut stamped: Vec<&str> = Vec::with_capacity(self.stamps.len());
        for st in &self.stamps {
            if stamped.contains(&st.id.as_str()) {
                return Err(format!(
                    "map: stamp id `{}` is used twice — the rows it expands to are named after it, so \
                     a duplicate would produce two rows with one id",
                    st.id
                ));
            }
            stamped.push(&st.id);
        }

        for loc in &self.locations {
            if loc.props.is_empty() {
                return Err(format!(
                    "map: location `{}` governs no props — it would advertise interactions with \
                     nothing to perform them on",
                    loc.id
                ));
            }
            for prop in &loc.props {
                if !seen.contains(&prop.as_str()) {
                    return Err(format!(
                        "map: location `{}` references placement `{prop}`, which does not exist",
                        loc.id
                    ));
                }
            }
            for i in &loc.interactions {
                if i.roles.is_empty() {
                    return Err(format!(
                        "map: interaction `{}` in location `{}` has no roles — nobody could perform it",
                        i.verb, loc.id
                    ));
                }
                for r in &i.roles {
                    if r.max < r.min {
                        return Err(format!(
                            "map: role `{}` in `{}`/`{}` has max {} below min {}",
                            r.name, loc.id, i.verb, r.max, r.min
                        ));
                    }
                    // Smart Zones: a Main role gates the scene starting. One that can be filled by
                    // nobody is either a Supporting role that was mislabelled or a scene that can
                    // never run — both worth failing at the door.
                    if r.kind == RoleKind::Main && r.min == 0 {
                        return Err(format!(
                            "map: role `{}` in `{}`/`{}` is Main with min 0. A Main role is what \
                             gates the scene starting; a Main role nobody has to fill is a \
                             Supporting role.",
                            r.name, loc.id, i.verb
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Nothing rests, however indirectly, on itself.
    ///
    /// A lamp on a table on that lamp has no height — the resolver would recurse forever looking for
    /// a floor. Refusing here means [`crate::stack::resolve_y`] can walk the chain without a depth
    /// cap it would have to invent a number for.
    fn no_stacking_cycles(&self) -> Result<(), String> {
        // Iterative rather than recursive: a map is author data and a deep chain must not blow the
        // stack of whatever is reading it.
        for start in &self.placements {
            let mut seen_ids: Vec<&str> = vec![start.id.as_str()];
            let mut at = start;
            while let Some(host_id) = &at.on {
                let Some(host) = self.placements.iter().find(|q| &q.id == host_id) else {
                    // Already refused above; reaching it means this was called on its own.
                    break;
                };
                if seen_ids.contains(&host.id.as_str()) {
                    seen_ids.push(&host.id);
                    return Err(format!(
                        "map: these placements rest on each other in a loop: {}. Nothing in the \
                         chain touches the floor, so none of them has a height.",
                        seen_ids.join(" → ")
                    ));
                }
                seen_ids.push(&host.id);
                at = host;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A map still carrying the lattice fields is refused by name, not read with them ignored.**
    ///
    /// `face_bands` and `snap_divisor` left this schema at version 5 for
    /// [`crate::kits::Lattice`] — a map has exactly one lattice, but so does the project, and a
    /// tile's adjacency contract cannot depend on which map happens to be open.
    ///
    /// A *removal* is the one schema change the version floor cannot absorb quietly: an old file
    /// carrying the fields would parse with them silently dropped, and the author would never learn
    /// that the number they set stopped meaning anything. `deny_unknown_fields` is what makes that
    /// loud, and this pins it — the shipped maps were migrated in the same commit.
    #[test]
    fn a_map_still_carrying_the_moved_lattice_is_refused_by_name() {
        let stale = r#"(version: 4, name: "m", origin: (0.0, 0.0, 0.0), face_bands: 2, placements: [], locations: [])"#;
        let e = Map::parse(stale).err().unwrap_or_default();
        assert!(e.contains("face_bands"), "the refusal names the field: {e}");

        let stale = r#"(version: 4, name: "m", origin: (0.0, 0.0, 0.0), snap_divisor: 4, placements: [], locations: [])"#;
        let e = Map::parse(stale).err().unwrap_or_default();
        assert!(e.contains("snap_divisor"), "the refusal names the field: {e}");

        // And one carrying neither of them still opens, meaning exactly what it meant.
        let before =
            r#"(version: 2, name: "m", origin: (0.0, 0.0, 0.0), placements: [], locations: [])"#;
        Map::parse(before).unwrap_or_else(|e| panic!("a pre-lattice map still opens: {e}"));
    }

    /// **A map states the combination it draws on, and `None` is every bound kit.**
    ///
    /// The absent spelling means the same thing, and cannot be made to mean anything else: serde
    /// answers a missing `Option` field with `None` in every format, so there is no version of this
    /// field that is both `Option<String>` and required. Pinned here because that is the fact the
    /// schema doc rests on — everything this program writes states `bash`, and a hand-written file
    /// that leaves it out has written the new-map state rather than something nobody chose.
    #[test]
    fn a_map_names_a_bash_or_says_none_and_none_is_every_bound_kit() {
        let stated =
            r#"(version: 6, name: "m", origin: (0.0, 0.0, 0.0), bash: None, placements: [], locations: [])"#;
        let m = Map::parse(stated).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.bash, None, "`None` is every bound kit, and it is written down");
        assert_eq!(Map::default().bash, None, "a new map starts there too");

        let silent = r#"(version: 6, name: "m", origin: (0.0, 0.0, 0.0), placements: [], locations: [])"#;
        let m = Map::parse(silent).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.bash, None, "and omitting it is the same value, not a second meaning");

        let named =
            r#"(version: 6, name: "m", origin: (0.0, 0.0, 0.0), bash: Some("hub"), placements: [], locations: [])"#;
        let m = Map::parse(named).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.bash.as_deref(), Some("hub"));

        // **Written back out every time.** No `skip_serializing_if`, so a map saved from this
        // program always says which combination it draws on.
        let out = ron::ser::to_string(&Map { bash: Some("hub".into()), ..Map::default() })
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(out.contains("bash:Some(\"hub\")") || out.contains("bash: Some(\"hub\")"), "{out}");
        let out = ron::ser::to_string(&Map::default()).unwrap_or_else(|e| panic!("{e}"));
        assert!(out.contains("bash:None") || out.contains("bash: None"), "{out}");
    }

    fn table_map() -> Map {
        Map {
            version: MAP_VERSION,
            name: "galley".into(),
            origin: (0.0, 0.0, 0.0),
            bounds: default_bounds(),
            bash: None,
            stamps: Vec::new(),
            placements: vec![
                Placed {
                    id: "table_1".into(),
                    descriptor: "mess_table".into(),
                    at: (4.0, 4.0),
                    yaw: 0.0,
                    ..Placed::default()
                },
                Placed {
                    id: "stool_1".into(),
                    descriptor: "stool".into(),
                    at: (4.0, 3.0),
                    yaw: 180.0,
                    ..Placed::default()
                },
            ],
            locations: vec![Location {
                id: "galley_table".into(),
                props: vec!["table_1".into(), "stool_1".into()],
                note: Some("the galley's near table".into()),
                interactions: vec![Interaction {
                    verb: "eat".into(),
                    roles: vec![RoleSlot {
                        name: "diner".into(),
                        kind: RoleKind::Main,
                        min: 1,
                        max: 4,
                        socket_role: Some("diner".into()),
                        requires: vec!["eat".into()],
                    }],
                    guard: None,
                    effects: vec![Effect::Restore {
                        drive: "stamina".into(),
                        rate: 0.2,
                    }],
                    note: None,
                }],
            }],
            note: Some("a galley, for the schema tests".into()),
        }
    }

    #[test]
    fn a_table_and_its_stools_are_one_affordance() {
        let m = table_map();
        m.validate().expect("valid");
        // The point of the whole design: two props, one interaction.
        assert_eq!(m.locations[0].props.len(), 2);
        assert_eq!(m.locations[0].interactions.len(), 1);
    }

    /// **A map from a newer tool is refused, never half-read.**
    ///
    /// The direction that matters: this build would understand the fields it knows, drop the ones it
    /// does not, and write the file back without them. A map silently losing every stamp in it is the
    /// failure the version number exists for.
    #[test]
    fn a_map_from_a_newer_schema_is_refused_not_migrated() {
        let mut m = table_map();
        m.version = MAP_VERSION + 1;
        let err = m.validate().expect_err("must refuse");
        assert!(err.contains("newer tool"), "{err}");
    }

    /// **A map from before a field existed still loads**, because its absence is what it always meant.
    ///
    /// The other half of the floor rule, and the reason it is a floor. Refusing this direction would
    /// destroy a map to prevent nothing: a file written before [`Map::stamps`] says `stamps: []`,
    /// which is exactly what it said then.
    #[test]
    fn a_map_from_before_a_field_existed_still_loads() {
        let mut m = table_map();
        m.version = MAP_VERSION - 1;
        m.validate()
            .expect("an older map means what it always meant");
        assert!(m.stamps.is_empty());
    }

    #[test]
    fn a_location_cannot_reference_a_placement_that_is_not_there() {
        let mut m = table_map();
        m.locations[0].props.push("ghost_chair".into());
        let err = m.validate().expect_err("must refuse");
        assert!(err.contains("ghost_chair"), "{err}");
    }

    /// Placement ids are how locations name their props, so a duplicate is an ambiguity, not a typo.
    #[test]
    fn duplicate_placement_ids_are_refused() {
        let mut m = table_map();
        m.placements[1].id = "table_1".into();
        assert!(m.validate().is_err());
    }

    #[test]
    fn an_owned_placement_must_say_why() {
        let mut m = table_map();
        m.placements[0].owned = true;
        let err = m.validate().expect_err("must refuse a reasonless lock");
        assert!(err.contains("why"), "{err}");

        m.placements[0].owned_because = Some("the cell block's only entrance".into());
        m.validate().expect("a reason satisfies it");
    }

    /// A Main role with min 0 cannot gate anything, which is the only thing Main means.
    #[test]
    fn a_main_role_nobody_must_fill_is_refused() {
        let mut m = table_map();
        m.locations[0].interactions[0].roles[0].min = 0;
        let err = m.validate().expect_err("must refuse");
        assert!(err.contains("Supporting"), "{err}");
    }

    #[test]
    fn a_placement_naming_an_unknown_descriptor_is_refused() {
        let m = table_map();
        let known = vec!["mess_table".to_string()];
        let err = m.validate_against(&known).expect_err("stool is unknown");
        assert!(err.contains("stool"), "{err}");

        let known = vec!["mess_table".to_string(), "stool".to_string()];
        m.validate_against(&known).expect("both known");
    }

    #[test]
    fn the_map_round_trips_through_ron() {
        let m = table_map();
        let text =
            ron::ser::to_string_pretty(&m, ron::ser::PrettyConfig::default()).expect("serializes");
        assert_eq!(Map::parse(&text).expect("parses"), m);
    }

    /// **The reason `note:` is a field.** The same `to_string_pretty` that deleted 279 lines of
    /// rationale from `config.ron` on 2026-07-16 carries these through untouched, because they are
    /// data. No surgical writer, no comment-preserving pass, nothing to forget.
    ///
    /// The assertion is deliberately on the *serialized text* as well as the parsed value: a
    /// round-trip through `PartialEq` would still pass if a future serializer config dropped the
    /// field and the parser defaulted it back to `None` on both sides.
    #[test]
    fn prose_survives_an_ordinary_serializer_because_it_is_a_field() {
        let mut m = table_map();
        m.placements[0].note = Some("the slab the specimen goes on".into());

        let text =
            ron::ser::to_string_pretty(&m, ron::ser::PrettyConfig::default()).expect("serializes");
        for prose in [
            "a galley, for the schema tests",
            "the slab the specimen goes on",
        ] {
            assert!(
                text.contains(prose),
                "`{prose}` was lost by the serializer:\n{text}"
            );
        }

        let back = Map::parse(&text).expect("parses");
        assert_eq!(
            back, m,
            "a note must survive the round trip, not just the write"
        );
        assert_eq!(
            back.placements[0].note.as_deref(),
            Some("the slab the specimen goes on")
        );
    }

    /// A note is prose, so it may contain the things prose contains — including the `//` that would
    /// have ended a comment, and quotes.
    ///
    /// **The serializer escapes more than it needs to.** Measured against `ron 0.12.2`: an apostrophe
    /// inside a double-quoted string comes out as `\'`, so "the galley's near table" is written
    /// `"the galley\'s near table"`. It parses back identically, which is the property that matters —
    /// but it means a *textual* grep of a map file for an author's note can miss it, and a tool that
    /// diffs notes must compare parsed values rather than bytes.
    #[test]
    fn a_note_may_contain_anything_prose_contains() {
        for prose in [
            r#"see docs/ui.md §5 // and the "kit" notes"#,
            "the galley's near table",
            "a\nnote\nover several lines",
        ] {
            let mut m = table_map();
            m.note = Some(prose.to_owned());
            let text = ron::ser::to_string_pretty(&m, ron::ser::PrettyConfig::default())
                .expect("serializes");
            assert_eq!(
                Map::parse(&text).expect("parses").note.as_deref(),
                Some(prose),
                "prose did not survive:\n{text}"
            );
        }
    }

    /// Absence is the normal case — a map with nothing to explain must not be forced to say so.
    #[test]
    fn a_note_is_optional_everywhere() {
        let text = r#"(
            version: 1,
            name: "bare",
            origin: (0.0, 0.0, 0.0),
            placements: [
                ( id: "a", descriptor: "crate", at: (1.0, 1.0), yaw: 0.0 ),
            ],
        )"#;
        let m = Map::parse(text).expect("a map with no notes must parse");
        assert_eq!(m.note, None);
        assert_eq!(m.placements[0].note, None);
        m.validate().expect("and validate");
    }
}
