//! **A named, reusable group of placements** — and the one function that turns a reference to one
//! back into ordinary map rows.
//!
//! Before this, the smallest thing an author could reuse was a single [`crate::descriptor::Descriptor`].
//! A nurse station, a meal setting, a wall-with-its-header — anything made of more than one piece — had
//! to be re-placed by hand every time. The editor's clone tool got as far as holding such a set *in
//! hand*, repointing the `on:` hosts inside it, and then losing the whole thing on `Esc`. This is that
//! set given a name, a file, and a reference somebody else's map can hold.
//!
//! # Reference, not bake
//!
//! A map stores a [`Stamped`] — *which* composition, *where*, and what it overrides. It does **not**
//! store the rows that come out. So editing a composition changes every map that stamped it, which is
//! the entire reason this shape was chosen over flattening at save time.
//!
//! The precedent is OpenUSD, which composes by reference with sparse overrides and a fixed strength
//! order. Two of its rules are adopted verbatim, because both name a failure that has been observed in
//! shipped tools:
//!
//! * **A fixed, documented strength order**, so where an opinion wins is answerable rather than
//!   discovered: `library.ron` < `project.ron` patch < [`Member`] patch < [`Override`]. Nothing here
//!   applies those layers itself — the two patch layers are merged into the one
//!   [`crate::map::Placed::patch`] the map schema already has, so there is exactly one place a patch is
//!   ever applied.
//! * **Encapsulation.** A stamp may override a member's *values*. It may not delete a member, re-parent
//!   one, or reach into a nested composition's internals. Unity's nested-prefab overrides are the
//!   cautionary tale — they evaporate when the thing they point at moves — and the discipline that
//!   prevents it is that the referenced structure is immutable from outside. It is also what makes
//!   [`expand`] properly recursive rather than a special case per depth.
//!
//! # One arbitrary-depth type, not a ladder of levels
//!
//! [`Composition`] is one type that can contain another ([`Body::Composition`]), rather than a fixed
//! ladder of Assembly-inside-Arrangement-inside-Map. StructureNet (Mo, Guerrero, Yi, Su, Wonka, Mitra
//! & Guibas, *ACM TOG* 38(6):242, 2019) is the reason: it abandoned GRASS's fixed binary hierarchy
//! because the imposed arity *"can introduce arbitrary ordering in nodes and make the hierarchy
//! inconsistent"*, and its fix was an encoding invariant to sibling order.
//!
//! That paper also names the risk of going the other way, and it is guarded here rather than hoped
//! about: **one uniform recursive type makes the same set expressible as several different trees.**
//! Two authors — or one author twice — would then produce diffs that differ without meaning to. So
//! [`Composition::members`] is stored in a canonical order (by [`Member::id`]) and parenthood is
//! author-declared through `on`, never inferred from an ordering heuristic.
//!
//! [`crate::map::Map`] deliberately stays a **different** type that *holds* stamps rather than being
//! one. USD keeps a distinct stage and Unreal keeps a distinct level for the same reason, and the
//! asymmetry decides it: a distinct Map can be promoted later by giving it an envelope, whereas
//! unifying now and finding that bounds and layers do not fit is a migration through every authored
//! file.
//!
//! # There is no socket type here, and that is deliberate
//!
//! The case that looks like it wants attachment points — a plate on a table, peas on the plate — is
//! already answered by [`Body::Descriptor::on`] plus [`Member::lift`] and
//! [`crate::stack::resolve_y`]. A socket family would be a second way to name a position, and
//! `Offers::sockets` already means something else entirely: a *seat*, where an actor stands to use a
//! thing. Edge tokens remain the one connection model.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::descriptor::{Descriptor, Dir as ClearDir, Mount, DecalHost, Subgrid};
use crate::library::Library;
use crate::map::{Location, Map, Placed};
use crate::placement::ir::Dir;
use crate::wfc::{E, N, S, W};

/// How deep compositions may nest.
///
/// The cycle check catches a loop; it does not catch forty honest levels, and every level multiplies
/// what one stamp expands to. Refused with the chain named rather than truncated, on the rule the
/// solver's own prototype ceiling follows — a limit that silently drops content reads as the tool
/// working.
pub const MAX_COMPOSITION_DEPTH: u32 = 8;

/// How many rows one stamp may expand to.
///
/// The editor's ghost draws a stamp's **real contents** rather than a proxy box, so this is the number
/// that keeps that promise affordable. As with the depth cap, over it is a refusal naming the count,
/// never a sample of the first N.
pub const MAX_RESOLVED_MEMBERS: usize = 256;

/// A named, reusable group.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    /// Same shape as a descriptor id — `naming::is_id`, kit namespace and all.
    pub id: String,
    #[serde(default)]
    pub envelope: Envelope,
    /// **Canonical order: sorted by [`Member::id`].** See the module note — this is what stops the
    /// same set from having several encodings.
    pub members: Vec<Member>,
    /// Affordances that travel with the group. `props` name [`Member::id`]s; [`expand`] repoints them
    /// to the ids the stamp produced.
    #[serde(default)]
    pub locations: Vec<Location>,
    #[serde(default)]
    pub note: Option<String>,
}

/// What kind of space a composition claims.
///
/// The one parameter that distinguishes the two things authors actually build. There is deliberately
/// no second parameter: a field with one legal value is a stub, and this schema does not carry stubs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Envelope {
    /// Positioned relative to its anchor and claiming no tile. A table and its chairs.
    #[default]
    Anchored,
    /// Fills a tile-shaped region, `(width, height, depth)` in metres, centred on the anchor in X/Z
    /// and rising from it in Y — the same reading [`crate::map::Map::origin`] has.
    ///
    /// Only a `Bounded` composition has a derived edge interface, and therefore only a `Bounded` one
    /// can ever be a solver prototype.
    Bounded { size: (f32, f32, f32) },
}

/// One thing a composition is made of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    /// Unique within the composition, and **the stable target an [`Override`] names**. Never an index:
    /// a member reordered by a rename would silently reassign every override pointing past it, which
    /// is the failure Unity's file-id overrides are known for.
    pub id: String,
    pub body: Body,
    /// Position relative to the composition's anchor, metres. Floor plan only, exactly as
    /// [`crate::map::Placed::at`] is.
    pub at: (f32, f32),
    #[serde(default)]
    pub yaw: f32,
    /// Vertical nudge on top of whatever the mount resolves to, metres — the same amendment
    /// [`crate::map::Placed::lift`] is, and it survives nesting by addition.
    #[serde(default)]
    pub lift: f32,
    /// **Paint order among things at the same spot** — higher draws in front.
    ///
    /// The visual sibling of [`Self::lift`]: `lift` moves a member, this only decides what is seen
    /// when two of them are in the same place. Copied onto [`crate::map::Placed::paint`] by
    /// [`expand`], which carries the full note on what `depth_bias` does and does not deliver.
    #[serde(default, skip_serializing_if = "paint_is_zero")]
    pub paint: i8,
    /// **The fingerprint of the interface this member was built against.**
    ///
    /// A verifying trace in the sense of Mokhov, Mitchell & Peyton Jones (*Build Systems à la Carte*,
    /// Proc. ACM PL 2(ICFP):79, 2018): record what a dependency looked like, and call the dependent
    /// dirty when a recomputation disagrees. Mismatch means STALE — a fact for the editor to show, never
    /// a reason to refuse a load, because [`expand`] always recomputes from source and so cannot be
    /// wrong, only out of date.
    ///
    /// **`None` means never recorded, and that is a different fact from stale.** It was a bare `u64`
    /// defaulting to zero for one afternoon, and every hand-written group in the shipped file read
    /// STALE against `recorded 0x0000000000000000` — a group that has never been measured is not a
    /// group that drifted. Zero is also a legal hash, which is the `Subgrid::default()`-as-sentinel
    /// mistake `descriptor.rs` already records: a sentinel that is also a legal value is a bug
    /// waiting for its input.
    #[serde(default)]
    pub of_fingerprint: Option<u64>,
    #[serde(default)]
    pub note: Option<String>,
}

fn paint_is_zero(v: &i8) -> bool {
    *v == 0
}

/// What a member *is* — and, for a descriptor, everything only a descriptor can carry.
///
/// The three fields below live in this variant rather than on [`Member`] so that the states they would
/// otherwise make expressible are unrepresentable instead. A composition has no mesh to tip, no
/// `Descriptor` shape for a patch to decal, and its own members answer their own hosts; a `tip`,
/// `patch` or `on` on a group would each be a field with no meaning that some reader would eventually
/// act on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Body {
    Descriptor {
        /// A [`crate::descriptor::Descriptor`] id.
        id: String,
        /// Quarter turns about X and Z, applied in the piece's own frame *before* any yaw — which is
        /// what lets a stamp's yaw simply add to the member's.
        #[serde(default)]
        tip: (u8, u8),
        /// A **sibling** [`Member::id`] this rests on. `None` means "find a host outside this group",
        /// which [`expand`] resolves against the map the stamp lands in.
        #[serde(default)]
        on: Option<String>,
        /// Per-member decal on the library entry. Sparse: [`Descriptor`] is a patch type, so absence
        /// inherits.
        #[serde(default)]
        patch: Option<Descriptor>,
    },
    /// Another composition, in full. Its internals are immutable from here — see the module note.
    Composition { id: String },
}

impl Body {
    /// The id of whatever this refers to, for messages and lookups.
    pub fn target(&self) -> &str {
        match self {
            Body::Descriptor { id, .. } | Body::Composition { id } => id,
        }
    }
}

/// A composition placed in a map.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stamped {
    /// Unique within the map, and shared with no placement — the rows this expands to are named
    /// `<stamp>/<member>`, so a collision would make a location's `props` ambiguous.
    pub id: String,
    /// The [`Composition::id`] this is an instance of.
    pub of: String,
    pub at: (f32, f32),
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub overrides: Vec<Override>,
    /// **What the composition looked like when this was stamped**, so the editor can say the map needs
    /// looking at *before* it stops loading.
    ///
    /// The case this exists for is narrow and real: an author deletes a member that some map's stamp
    /// overrides. Validation catches the dangling override — at load, by refusing — so without this the
    /// damage is done in one session and discovered in another. Covers the member-id set, which is
    /// exactly what an override can dangle against.
    ///
    /// `None` means never recorded — see [`Member::of_fingerprint`].
    #[serde(default)]
    pub of_fingerprint: Option<u64>,
    /// Owned by the author: a generator routes around the whole group. Inherited by every row this
    /// expands to.
    #[serde(default)]
    pub owned: bool,
    #[serde(default)]
    pub owned_because: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// One authored opinion layered over a member of the stamped composition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Override {
    /// A [`Member::id`] of the composition being stamped. **Its own members only** — reaching into a
    /// nested composition is the restructuring encapsulation forbids.
    pub member: String,
    pub patch: Descriptor,
    /// Why the composition's own answer was not enough. A reason, never a bare edit — the same call
    /// [`crate::map::Placed::owned_because`] and `policy::Patch::because` already make, and for the
    /// same argument: without it a diff cannot tell a deliberate exception from a stray keystroke.
    pub because: String,
}

/// What a map's stamps come out as.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Expansion {
    /// In `(stamp id, member path)` order — a total order, so two runs and two `App` instances agree.
    pub placements: Vec<Placed>,
    pub locations: Vec<Location>,
    /// Placement id → `(stamp id, member path)`.
    ///
    /// The editor reads this to answer "what is this row part of" for selection, break-link and the
    /// STALE badge; the game ignores it. It exists so that **no consumer parses an id back apart** —
    /// the provenance is returned as structure rather than encoded in a string and decoded again.
    pub from: BTreeMap<String, (String, String)>,
}

/// Where a composition's members disagree about what its boundary presents.
#[derive(Clone, Debug, PartialEq)]
pub struct InterfaceFault {
    pub dir: Dir,
    /// The two member paths that both reach this face here.
    pub a: String,
    pub b: String,
    pub a_token: Option<String>,
    pub b_token: Option<String>,
    pub message: String,
}

/// **One rectangle of a face that presents a single token**, in metres.
///
/// A face is described by its bands rather than by its cells, which is why reading one does not
/// require knowing how finely the project divides a tile: a 2.4 m wall is one band whether it is
/// divided five ways or fifty. Positions are **metres, not fractions** — [`crate::adjacency::seam`]
/// already settled that comparing faces is a question about *where two pieces physically touch*
/// rather than about whether they are the same shape, and normalised coordinates would reintroduce
/// exactly the defect it was changed to fix.
///
/// The decomposition is the component split of Müller et al.'s CGA Shape (`10.1145/1179352.1141931`)
/// carried one dimension further than the 2-D face: the face splits into horizontal strips, and each
/// strip into lateral runs. Taking them in that order is what makes it **canonical** — a greedy
/// rectangle cover of the same cells has several answers and this has one, so two runs and two `App`
/// instances agree.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    /// Height this band spans, in metres above the composition's floor.
    pub y: (f32, f32),
    /// Extent along the face: `x` for a north or south face, `z` for an east or west one.
    ///
    /// **In the envelope's own coordinates — the ones [`Member::at`] is written in**, so it runs from
    /// `-size/2` to `+size/2` rather than from zero. Quoting it on the envelope's axes rather than
    /// from "the left end" means it does not depend on which side you imagine standing, which is the
    /// kind of thing that reads fine and mirrors a face.
    pub lat: (f32, f32),
    /// What this rectangle presents. `None` is a token in its own right and matches only `None` —
    /// [`crate::adjacency`]'s rule, so a composition and a plain tile answer the same way.
    pub token: Option<String>,
}

/// A `Bounded` composition's derived edge interface — what it presents to whatever abuts it.
///
/// **Read off the members, never authored.** There is no field anywhere for a hand-written interface,
/// which is what makes an inconsistent one unrepresentable rather than merely discouraged. Sturgeon
/// (Cooper, AIIDE 2022) is the precedent: the tokens that constrain placement are a derived layer over
/// a functional one, not a parallel thing to keep in step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interface {
    /// Indexed by [`crate::wfc`]'s `N`/`E`/`S`/`W`. Bands in `y` then `lat` ascending — the same
    /// order [`crate::adjacency::face`] reads cells in, one dimension up.
    ///
    /// **Not one token per side.** Measured across both shipped kits on 2026-08-09: 192 faces carry
    /// a subgrid and four of them present two tokens at once — `site/wall_doorway`,
    /// `site/wall_doorway_wide`, `site/wall_window` and `site_greybox`'s `wall_doorway_wide` all read
    /// `wall` at the jambs and nothing through the opening. A single token per side would have to
    /// fault every doorway or pick a winner, and picking is exactly what the faults below exist to
    /// avoid.
    pub faces: [Vec<Band>; 4],
    /// Members disagreeing about a face. **Derived and reported, never resolved by picking one** — the
    /// call `adjacency::faults` already makes, because silently choosing a winner is how a tool ends up
    /// modelling something other than what the author has in their head.
    pub faults: Vec<InterfaceFault>,
}

impl Interface {
    /// Whether this interface is fit to be a solver prototype.
    ///
    /// A composition whose members contradict each other about a face has no single answer for what
    /// that face presents, so it cannot constrain a neighbour. It can still be stamped by hand: the
    /// author asked for those pieces in those places, and that is not this function's business.
    pub fn is_clean(&self) -> bool {
        self.faults.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------
// Geometry shared by expansion and interface derivation
// ---------------------------------------------------------------------------------------------

/// Turn a local offset into a world one, `yaw` degrees about +Y.
///
/// The exact inverse of the rotation [`crate::stack::covers`] uses to go the other way, and written
/// against it rather than re-derived: Bevy's yaw turns +X toward −Z, so getting the sign wrong here
/// mirrors every composition without failing anything.
///
/// **Public because a turned *loose set* has to agree with a turned stamp.** `emerge-mapper`'s clone
/// tool rotates a captured group about its anchor, and a second copy of this sign convention is how
/// a set stamped at 90° would come out mirrored against a composition stamped at 90° — the failure
/// this comment already warns about, one caller further out.
pub fn rotate_xz(local: (f32, f32), yaw_deg: f32) -> (f32, f32) {
    let (s, c) = yaw_deg.to_radians().sin_cos();
    (local.0 * c + local.1 * s, -local.0 * s + local.1 * c)
}

/// Yaws add, and the sum is brought back into `[0, 360)`. Public for [`rotate_xz`]'s reason.
pub fn add_yaw(a: f32, b: f32) -> f32 {
    (a + b).rem_euclid(360.0)
}

/// One member of a composition, with every enclosing composition's transform already folded in.
///
/// Nesting exists only here. Everything downstream — expansion, interface derivation — sees a flat
/// list, which is what keeps [`expand`] from growing a branch per depth.
#[derive(Clone, Debug)]
struct Flat {
    /// `outer/inner/…`, the member path within the composition being expanded.
    path: String,
    descriptor: String,
    at: (f32, f32),
    yaw: f32,
    lift: f32,
    /// **Paint order, summed through nesting** the way `lift` is added: a group given a paint offset
    /// carries every member in it forward by that much, so nesting cannot reorder a child against
    /// itself. Saturating, because `i8` is deliberately narrow.
    paint: i8,
    tip: (u8, u8),
    /// A sibling **path**, already prefixed to match `path`.
    on: Option<String>,
    patch: Option<Descriptor>,
    note: Option<String>,
}

/// Look a composition up by id, or say which one is missing and who wanted it.
fn find<'a>(compositions: &'a [Composition], id: &str, who: &str) -> Result<&'a Composition, String> {
    compositions.iter().find(|c| c.id == id).ok_or_else(|| {
        format!("composition: `{who}` refers to composition `{id}`, which does not exist")
    })
}

/// Flatten one composition into world-independent members, folding nested transforms in.
///
/// `overrides` apply at the top level only — see [`Override::member`].
fn flatten(
    comp: &Composition,
    compositions: &[Composition],
    overrides: &[Override],
    stack: &mut Vec<String>,
    depth: u32,
) -> Result<(Vec<Flat>, Vec<Location>), String> {
    if stack.contains(&comp.id) {
        stack.push(comp.id.clone());
        return Err(format!(
            "composition: `{}` contains itself — {}. A group inside itself has no members to end at.",
            comp.id,
            stack.join(" → ")
        ));
    }
    if depth > MAX_COMPOSITION_DEPTH {
        return Err(format!(
            "composition: `{}` nests deeper than {MAX_COMPOSITION_DEPTH} — {}. Flatten one of these \
             levels rather than raising the cap; every level multiplies what one stamp costs.",
            comp.id,
            stack.join(" → ")
        ));
    }
    stack.push(comp.id.clone());

    let mut flats: Vec<Flat> = Vec::new();
    let mut locs: Vec<Location> = Vec::new();

    for m in &comp.members {
        match &m.body {
            Body::Descriptor {
                id,
                tip,
                on,
                patch,
            } => {
                // **The strength order, applied once.** The two patch layers are merged into the one
                // `Placed::patch` the map already has, rather than resolved against the library here —
                // so there stays exactly one place in this codebase where a patch meets a descriptor.
                let over = overrides.iter().find(|o| o.member == m.id);
                let merged = match (patch, over) {
                    (Some(p), Some(o)) => Some(p.patched_with(&o.patch)),
                    (Some(p), None) => Some(p.clone()),
                    (None, Some(o)) => Some(o.patch.clone()),
                    (None, None) => None,
                };
                flats.push(Flat {
                    path: m.id.clone(),
                    descriptor: id.clone(),
                    at: m.at,
                    yaw: m.yaw,
                    lift: m.lift,
                    paint: m.paint,
                    tip: *tip,
                    on: on.clone(),
                    patch: merged,
                    note: m.note.clone(),
                });
            }
            Body::Composition { id } => {
                let inner = find(compositions, id, &comp.id)?;
                // Nested members take no overrides: an override names a member of the composition
                // being stamped, and reaching past that is the restructuring encapsulation forbids.
                let (inner_flats, inner_locs) =
                    flatten(inner, compositions, &[], stack, depth + 1)?;
                for f in inner_flats {
                    let at = rotate_xz(f.at, m.yaw);
                    flats.push(Flat {
                        path: format!("{}/{}", m.id, f.path),
                        descriptor: f.descriptor,
                        at: (m.at.0 + at.0, m.at.1 + at.1),
                        yaw: add_yaw(f.yaw, m.yaw),
                        // Lifts add: a group nudged up carries everything in it.
                        lift: m.lift + f.lift,
                        // Paint adds the same way, and saturates rather than wrapping: a wrap would
                        // send the front-most member behind everything, which is the one outcome an
                        // author would never intend.
                        paint: m.paint.saturating_add(f.paint),
                        tip: f.tip,
                        on: f.on.map(|o| format!("{}/{}", m.id, o)),
                        patch: f.patch,
                        note: f.note,
                    });
                }
                for l in inner_locs {
                    locs.push(Location {
                        id: format!("{}/{}", m.id, l.id),
                        props: l.props.iter().map(|p| format!("{}/{}", m.id, p)).collect(),
                        interactions: l.interactions,
                        note: l.note,
                    });
                }
            }
        }
    }

    locs.extend(comp.locations.iter().cloned());
    stack.pop();

    if flats.len() > MAX_RESOLVED_MEMBERS {
        return Err(format!(
            "composition: `{}` expands to {} rows, over the {MAX_RESOLVED_MEMBERS} a stamp may \
             carry. Split it into named groups rather than raising the cap — the editor's ghost draws \
             the real contents of every stamp.",
            comp.id,
            flats.len()
        ));
    }
    Ok((flats, locs))
}

// ---------------------------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------------------------

impl Composition {
    /// Everything answerable without looking at the other compositions.
    pub fn validate_shape(&self) -> Result<(), String> {
        if !crate::naming::is_id(&self.id) {
            return Err(format!(
                "composition: `{}` is not a usable id. Ids are snake_case, with `/` separating a kit \
                 from a piece — `ward/nurse_station`.",
                self.id
            ));
        }
        // **Refused here again, and the round trip is worth recording.**
        //
        // This check was moved out to [`expand`] earlier the same day, on the argument that its own
        // reason — "an empty group stamps nothing, which looks exactly like a stamp that failed" —
        // is about *stamping*. That was true, and it was the wrong conclusion, for two reasons found
        // afterwards.
        //
        // **The blast radius is the map, not the stamp.** `editor::redraw_stamps` and `emerge-bevy`
        // both expand a map's stamps in ONE call, and the editor despawns every stamped row *before*
        // that call can fail — so a single empty composition takes every stamped row in the map off
        // the screen, and the saved file will not load in the game either. A per-stamp refusal
        // downstream cannot be contained to the stamp that caused it.
        //
        // **And the thing it was moved for is gone.** It was blocking the Compose tab's NEW verb,
        // which created an empty tile and then asked you to fill it. Authoring moved to the Map,
        // where a composition is captured from a box selection and an empty box is already refused
        // — so nothing creates an empty composition any more, and refusing one at the door costs
        // nothing and catches a hand-edited file before it reaches a map.
        //
        // [`expand`] keeps its own check as a precondition on a public function, for a caller that
        // builds compositions in memory without coming through here.
        if self.members.is_empty() {
            return Err(format!(
                "composition: `{}` has no members. An empty composition stamps nothing, which looks \
                 exactly like a stamp that failed.",
                self.id
            ));
        }
        if let Envelope::Bounded { size } = self.envelope {
            for (axis, v) in [("x", size.0), ("y", size.1), ("z", size.2)] {
                if !(v.is_finite() && v > 0.0) {
                    return Err(format!(
                        "composition: `{}` has envelope {axis} of {v}. A bounded group has to enclose \
                         something, and its edge tokens are read off that boundary.",
                        self.id
                    ));
                }
            }
        }

        let mut seen: Vec<&str> = Vec::with_capacity(self.members.len());
        for m in &self.members {
            if !crate::naming::is_id(&m.id) {
                return Err(format!(
                    "composition: `{}` has a member named `{}`, which is not a usable id",
                    self.id, m.id
                ));
            }
            if seen.contains(&m.id.as_str()) {
                return Err(format!(
                    "composition: `{}` uses member id `{}` twice — an override names a member by id, \
                     so a duplicate makes it ambiguous which one it means",
                    self.id, m.id
                ));
            }
            seen.push(&m.id);
            if !m.at.0.is_finite() || !m.at.1.is_finite() || !m.yaw.is_finite() || !m.lift.is_finite()
            {
                return Err(format!(
                    "composition: `{}` member `{}` has a position that is not a number",
                    self.id, m.id
                ));
            }
            if let Body::Descriptor { tip, .. } = &m.body
                && (tip.0 > 3 || tip.1 > 3)
            {
                return Err(format!(
                    "composition: `{}` member `{}` has tip {tip:?} — quarter turns are 0..=3 per axis",
                    self.id, m.id
                ));
            }
        }

        // **Canonical order.** Not a style rule: without it the same group has several encodings, and
        // two authors building the same thing produce diffs that differ without meaning to.
        let mut sorted: Vec<&str> = seen.clone();
        sorted.sort_unstable();
        if sorted != seen {
            return Err(format!(
                "composition: `{}` lists its members out of order. They are stored sorted by id so \
                 that one group has one encoding; write them as: {}",
                self.id,
                sorted.join(", ")
            ));
        }

        for m in &self.members {
            let Body::Descriptor { on: Some(host), .. } = &m.body else {
                continue;
            };
            if host == &m.id {
                return Err(format!(
                    "composition: `{}` member `{}` rests on itself",
                    self.id, m.id
                ));
            }
            if !seen.contains(&host.as_str()) {
                return Err(format!(
                    "composition: `{}` member `{}` rests on `{host}`, which is not a member of it. A \
                     host outside the group is written as no host at all — `on: None` asks the map it \
                     lands in.",
                    self.id, m.id
                ));
            }
        }
        self.no_resting_cycles()?;

        for l in &self.locations {
            if l.props.is_empty() {
                return Err(format!(
                    "composition: `{}` location `{}` governs no props",
                    self.id, l.id
                ));
            }
            for p in &l.props {
                if !seen.contains(&p.as_str()) {
                    return Err(format!(
                        "composition: `{}` location `{}` references member `{p}`, which does not exist",
                        self.id, l.id
                    ));
                }
            }
        }
        Ok(())
    }

    /// Nothing rests, however indirectly, on itself — [`crate::map::Map`]'s rule, inside a group.
    fn no_resting_cycles(&self) -> Result<(), String> {
        for start in &self.members {
            let mut chain: Vec<&str> = vec![start.id.as_str()];
            let mut at = start;
            loop {
                let Body::Descriptor { on: Some(host), .. } = &at.body else {
                    break;
                };
                let Some(next) = self.members.iter().find(|m| &m.id == host) else {
                    break; // dangling; already reported above
                };
                if chain.contains(&next.id.as_str()) {
                    chain.push(&next.id);
                    return Err(format!(
                        "composition: `{}` has a resting cycle — {}. Its height would be its own.",
                        self.id,
                        chain.join(" → ")
                    ));
                }
                chain.push(&next.id);
                at = next;
            }
        }
        Ok(())
    }
}

/// Validate a whole set together: shapes, unique ids, every reference resolving, and no cycles.
///
/// Whole-set rather than per-composition because containment is a property of the set — the same
/// reason `Library::resolve` checks surface classes across every descriptor at once.
pub fn validate(compositions: &[Composition], library: &Library) -> Result<(), String> {
    let mut seen: Vec<&str> = Vec::with_capacity(compositions.len());
    for c in compositions {
        c.validate_shape()?;
        if seen.contains(&c.id.as_str()) {
            return Err(format!(
                "composition: id `{}` is used twice — a stamp names one by id, so a duplicate makes \
                 it ambiguous which is stamped",
                c.id
            ));
        }
        seen.push(&c.id);
    }
    for c in compositions {
        for m in &c.members {
            match &m.body {
                Body::Descriptor { id, .. } => {
                    if library.get(id).is_none() {
                        return Err(format!(
                            "composition: `{}` member `{}` places descriptor `{id}`, which the \
                             library does not define",
                            c.id, m.id
                        ));
                    }
                }
                Body::Composition { id } => {
                    find(compositions, id, &c.id)?;
                }
            }
        }
    }
    // Depth and cycles are answered by the one function that walks containment, rather than by a
    // second walk that could disagree with it.
    for c in compositions {
        flatten(c, compositions, &[], &mut Vec::new(), 0)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Fingerprints
// ---------------------------------------------------------------------------------------------

/// A canonical byte encoding, hashed once at the end.
///
/// Floats go in as their **bits**, not their text: a formatting change in the toolchain would
/// otherwise silently re-fingerprint the whole corpus and turn a badge designed to be truthful into
/// noise. Strings go in length-prefixed so that `("ab", "c")` and `("a", "bc")` cannot collide.
#[derive(Default)]
struct Fp(Vec<u8>);

impl Fp {
    fn tag(&mut self, t: u8) -> &mut Self {
        self.0.push(t);
        self
    }
    fn f32(&mut self, v: f32) -> &mut Self {
        self.0.extend_from_slice(&v.to_bits().to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn str(&mut self, s: &str) -> &mut Self {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
        self
    }
    fn opt_f32(&mut self, v: Option<f32>) -> &mut Self {
        match v {
            Some(v) => self.tag(1).f32(v),
            None => self.tag(0),
        }
    }
    fn opt_str(&mut self, v: Option<&str>) -> &mut Self {
        match v {
            Some(v) => self.tag(1).str(v),
            None => self.tag(0),
        }
    }
    fn finish(&self) -> u64 {
        crate::glb::fnv1a(&self.0)
    }
}

/// **What a descriptor presents to whatever composes it**, hashed.
///
/// Deliberately narrower than the descriptor: `note` and `look` are excluded because nothing about
/// composition depends on them, and a fingerprint that moves when a comment is reworded is a badge
/// nobody reads twice. What is in: extent, alignment, mount, what it offers, and the lattice cells
/// carrying an edge token.
pub fn descriptor_fingerprint(d: &Descriptor) -> u64 {
    let mut f = Fp::default();
    f.str(&d.id);
    f.opt_f32(d.extent.footprint.map(|x| x.0));
    f.opt_f32(d.extent.footprint.map(|x| x.1));
    f.opt_f32(d.extent.height);
    f.opt_f32(d.align.scale);
    f.opt_f32(d.align.stretch_y);
    f.opt_f32(d.align.y_offset);
    f.opt_f32(d.align.pivot.map(|p| p.0));
    f.opt_f32(d.align.pivot.map(|p| p.1));
    match d.align.rotate {
        Some((x, y, z)) => {
            f.tag(1);
            f.u32(x as u32).u32(y as u32).u32(z as u32);
        }
        None => {
            f.tag(0);
        }
    }
    // **Encoded by hand, not by `Debug`.** These were `format!("{x:?}")`, which is the same mistake
    // in kind as hashing a float's text: `Debug` output is not a stability contract, so renaming a
    // `Mount` field or reordering `DecalHost` would re-fingerprint every descriptor carrying one
    // and flip every composition using it to STALE with no real drift. A discriminant plus the
    // fields that matter says what it means and only changes when the meaning does.
    match d.align.front {
        None => {
            f.tag(0);
        }
        Some(face) => {
            f.tag(1).u32(face.dir() as u32);
        }
    }
    match &d.mount {
        None => {
            f.tag(0);
        }
        Some(Mount::OnFloor) => {
            f.tag(1);
        }
        Some(Mount::OnWall { height }) => {
            f.tag(2).f32(*height);
        }
        Some(Mount::OnCeiling) => {
            f.tag(3);
        }
        Some(Mount::InOpening { clear }) => {
            f.tag(4);
            f.opt_f32(clear.map(|c| c.0));
            f.opt_f32(clear.map(|c| c.1));
        }
        Some(Mount::OnSurface { class }) => {
            f.tag(5).str(class);
        }
        Some(Mount::Decal { on }) => {
            f.tag(6);
            match on {
                DecalHost::Floor => f.tag(0),
                DecalHost::Ceiling => f.tag(1),
                DecalHost::Wall { height } => f.tag(2).f32(*height),
            };
        }
        Some(Mount::Tiled) => {
            f.tag(7);
        }
    }
    f.u32(d.clearance.len() as u32);
    for c in &d.clearance {
        let dir = match c.dir {
            ClearDir::Front => 0u8,
            ClearDir::Back => 1,
            ClearDir::Left => 2,
            ClearDir::Right => 3,
            ClearDir::Around => 4,
        };
        f.tag(dir).f32(c.dist);
    }
    f.u32(d.offers.surfaces.len() as u32);
    for s in &d.offers.surfaces {
        f.str(s);
    }
    f.u32(d.offers.sockets.len() as u32);
    for s in &d.offers.sockets {
        f.str(&s.id).opt_str(s.role.as_deref());
        f.f32(s.at.0).f32(s.at.1).f32(s.at.2).f32(s.yaw);
    }
    // Only the cells that say something about an edge, in a fixed order — a lattice reordered in the
    // file is the same lattice.
    let mut edges: Vec<(&(u32, u32, u32), &str)> = d
        .subgrid
        .as_ref()
        .map(|g| {
            g.cells
                .iter()
                .filter_map(|c| c.edge.as_deref().map(|e| (&c.at, e)))
                .collect()
        })
        .unwrap_or_default();
    edges.sort_unstable();
    f.u32(edges.len() as u32);
    for (at, e) in edges {
        f.u32(at.0).u32(at.1).u32(at.2).str(e);
    }
    f.finish()
}

/// A composition's fingerprint: its envelope, its members' transforms, and each member's body
/// fingerprint, folded in canonical order.
///
/// Recursive by the same rule the module note gives — a parent sees a child's fingerprint, never a
/// child's internals.
pub fn composition_fingerprint(
    comp: &Composition,
    compositions: &[Composition],
    library: &Library,
) -> Result<u64, String> {
    fingerprint_inner(comp, compositions, library, &mut Vec::new(), 0)
}

fn fingerprint_inner(
    comp: &Composition,
    compositions: &[Composition],
    library: &Library,
    stack: &mut Vec<String>,
    depth: u32,
) -> Result<u64, String> {
    if stack.contains(&comp.id) || depth > MAX_COMPOSITION_DEPTH {
        return Err(format!(
            "composition: cannot fingerprint `{}` — it contains itself or nests too deep",
            comp.id
        ));
    }
    stack.push(comp.id.clone());
    let mut f = Fp::default();
    f.str(&comp.id);
    match comp.envelope {
        Envelope::Anchored => {
            f.tag(0);
        }
        Envelope::Bounded { size } => {
            f.tag(1).f32(size.0).f32(size.1).f32(size.2);
        }
    }
    f.u32(comp.members.len() as u32);
    for m in &comp.members {
        f.str(&m.id).f32(m.at.0).f32(m.at.1).f32(m.yaw).f32(m.lift);
        // **Paint is folded in even though it is cosmetic.** A stamp records what it was made
        // against, and an author who reorders two decals and sees no STALE badge has been told the
        // group did not change when it did. `i8` widened to i32 so the cast is total.
        f.u32(m.paint as i32 as u32);
        f.u64(body_fingerprint(&m.body, compositions, library, stack, depth)?);
    }
    stack.pop();
    Ok(f.finish())
}

/// The fingerprint of whatever a member refers to — a descriptor's interface, or a nested
/// composition's whole fingerprint. This is what [`Member::of_fingerprint`] records.
pub fn body_fingerprint_of(
    body: &Body,
    compositions: &[Composition],
    library: &Library,
) -> Result<u64, String> {
    body_fingerprint(body, compositions, library, &mut Vec::new(), 0)
}

fn body_fingerprint(
    body: &Body,
    compositions: &[Composition],
    library: &Library,
    stack: &mut Vec<String>,
    depth: u32,
) -> Result<u64, String> {
    match body {
        Body::Descriptor { id, tip, on, patch } => {
            let base = library.get(id).ok_or_else(|| {
                format!("composition: cannot fingerprint descriptor `{id}` — the library does not define it")
            })?;
            let resolved = match patch {
                Some(p) => base.patched_with(p),
                None => base.clone(),
            };
            let mut f = Fp::default();
            f.u64(descriptor_fingerprint(&resolved));
            f.u32(tip.0 as u32).u32(tip.1 as u32);
            f.opt_str(on.as_deref());
            Ok(f.finish())
        }
        Body::Composition { id } => {
            let inner = find(compositions, id, "a member")?;
            fingerprint_inner(inner, compositions, library, stack, depth + 1)
        }
    }
}

/// What a member's recorded fingerprint says about it now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    /// Recorded, and what was recorded still holds.
    Fresh,
    /// Recorded, and the body has changed underneath it.
    Stale,
    /// **Never recorded** — a hand-written group, or one whose member was added by hand. Not a
    /// problem and not a drift; there is simply nothing to compare against yet.
    Unrecorded,
}

/// One member, and whether what was recorded about it still holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Stale {
    pub composition: String,
    pub member: String,
    pub freshness: Freshness,
    /// `None` when nothing was ever recorded.
    pub recorded: Option<u64>,
    pub measured: u64,
}

/// Which members of a composition are **not** confirmed fresh, and nothing beyond that.
///
/// This is the early cutoff from *Build Systems à la Carte* stated as a signature rather than a
/// convention: a caller asks one composition whether its own recorded dependencies still hold. If the
/// answer is empty and the composition's own fingerprint is unchanged, there is nothing to propagate —
/// which is precisely why this returns per-member facts rather than walking dependents itself.
pub fn stale_members(
    comp: &Composition,
    compositions: &[Composition],
    library: &Library,
) -> Result<Vec<Stale>, String> {
    let mut out = Vec::new();
    for m in &comp.members {
        let measured = body_fingerprint_of(&m.body, compositions, library)?;
        let freshness = match m.of_fingerprint {
            None => Freshness::Unrecorded,
            Some(r) if r == measured => Freshness::Fresh,
            Some(_) => Freshness::Stale,
        };
        if freshness != Freshness::Fresh {
            out.push(Stale {
                composition: comp.id.clone(),
                member: m.id.clone(),
                freshness,
                recorded: m.of_fingerprint,
                measured,
            });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Expansion
// ---------------------------------------------------------------------------------------------

/// **Turn a map's stamps into the rows and locations they stand for.**
///
/// The one expander. The editor's ghost, the editor's save and the game's loader all come through
/// here, so a stamp cannot look one way in the tool and another in the game — the failure that made
/// `bake.rs` and `site_editor::source_map` grow two RON writers that drifted.
///
/// # Where a member's host comes from
///
/// Two cases, and the schema tells them apart rather than a heuristic:
///
/// * `on: Some(sibling)` — resolved inside the group, repointed to the row the sibling became.
/// * `on: None` — asked of the map, through [`crate::stack::placement_at`], which is the same question
///   a click asks. Resolved **before** this stamp's own rows are added, so `on: None` means exactly
///   "a host outside this group". A piece that needs a surface and finds none refuses the whole stamp
///   rather than landing at floor level.
///
/// Stamps are processed in id order and their rows emitted in member-path order, so the output is a
/// total order that does not depend on the order the file happens to list them in.
///
/// **One consequence, stated because it would otherwise be discovered:** a stamp may rest on a row
/// produced by another stamp only when that other stamp's id sorts earlier, since hosts are looked up
/// among the rows already emitted. It refuses by name rather than resolving differently on a
/// re-order, which is the property that matters — but if cross-stamp hosting ever becomes a thing
/// authors do, that ordering rule is the first thing to revisit.
pub fn expand(
    map: &Map,
    stamps: &[Stamped],
    compositions: &[Composition],
    library: &Library,
) -> Result<Expansion, String> {
    let mut out = Expansion::default();
    if stamps.is_empty() {
        return Ok(out);
    }

    // **The id space is shared with the rows an author placed by hand.** A placement id is not
    // shape-checked — `fridge@1` ships — so nothing stops one being named `mess_a/table`, which is
    // exactly what a stamp called `mess_a` produces. Caught here rather than at `Map::validate`
    // downstream, because this is the function that mints the name.
    let hand_placed: BTreeSet<&str> = map.placements.iter().map(|p| p.id.as_str()).collect();

    let mut order: Vec<&Stamped> = stamps.iter().collect();
    order.sort_by(|a, b| a.id.cmp(&b.id));
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for s in &order {
        if !seen.insert(s.id.as_str()) {
            return Err(format!(
                "map: stamp id `{}` is used twice — the rows it expands to are named after it, so a \
                 duplicate would produce two rows with one id",
                s.id
            ));
        }
    }

    // The rows a host may be found among: everything authored by hand, plus every stamp already
    // expanded. Grown as we go, so a stamp can rest on one processed before it and the answer does not
    // depend on file order.
    let mut working = map.clone();

    for s in order {
        let comp = find(compositions, &s.of, &s.id)?;
        let (flats, locs) = flatten(comp, compositions, &s.overrides, &mut Vec::new(), 0)?;

        // **Where the empty-composition refusal lives**, moved down from `validate_shape`. A group
        // with no members is a legitimate thing to be authoring and an illegitimate thing to have
        // stamped: it puts nothing in the map, which looks exactly like a stamp that failed. Naming
        // both the stamp and the composition, because at this point the author has one of each.
        if flats.is_empty() {
            return Err(format!(
                "map: stamp `{}` places `{}`, which has no members. An empty composition stamps \
                 nothing, which looks exactly like a stamp that failed.",
                s.id, comp.id
            ));
        }

        // Every override has to name a member that exists. A dangling one is the failure
        // `Stamped::of_fingerprint` exists to catch a session earlier than this.
        let known: BTreeSet<&str> = comp.members.iter().map(|m| m.id.as_str()).collect();
        for o in &s.overrides {
            if !known.contains(o.member.as_str()) {
                return Err(format!(
                    "map: stamp `{}` overrides member `{}` of `{}`, which has no such member. It has: \
                     {}.",
                    s.id,
                    o.member,
                    comp.id,
                    known.iter().copied().collect::<Vec<_>>().join(", ")
                ));
            }
            if o.because.trim().is_empty() {
                return Err(format!(
                    "map: stamp `{}` overrides member `{}` but says nothing about why. An override is \
                     the one place authored data re-enters a derived group; in six months only that \
                     sentence can say whether it still should.",
                    s.id, o.member
                ));
            }
            let member = comp.members.iter().find(|m| m.id == o.member);
            if matches!(member.map(|m| &m.body), Some(Body::Composition { .. })) {
                return Err(format!(
                    "map: stamp `{}` overrides member `{}` of `{}`, which is a composition. A group \
                     has no descriptor for a patch to decal — override one of its own members, or \
                     edit the group.",
                    s.id, o.member, comp.id
                ));
            }
        }

        // **Hosts first, against the world as it stands.** Done before any of this stamp's rows join
        // it, so `on: None` cannot silently pick up a sibling and mean two things.
        let y = crate::stack::resolve_y(&working, library)?;
        let mut hosts: Vec<Option<String>> = Vec::with_capacity(flats.len());
        for f in &flats {
            match &f.on {
                Some(sibling) => hosts.push(Some(format!("{}/{}", s.id, sibling))),
                None => {
                    let base = library.get(&f.descriptor).ok_or_else(|| {
                        format!(
                            "map: stamp `{}` places descriptor `{}`, which the library does not \
                             define",
                            s.id, f.descriptor
                        )
                    })?;
                    let d = match &f.patch {
                        Some(p) => base.patched_with(p),
                        None => base.clone(),
                    };
                    let at = rotate_xz(f.at, s.yaw);
                    let probe = (s.at.0 + at.0, s.at.1 + at.1);
                    let host = crate::stack::placement_at(&working, library, &y, &d, probe)
                        .map_err(|e| format!("map: stamp `{}` member `{}`: {e}", s.id, f.path))?;
                    hosts.push(host.1.map(|p| p.id.clone()));
                }
            }
        }

        for (f, on) in flats.into_iter().zip(hosts) {
            let at = rotate_xz(f.at, s.yaw);
            let id = format!("{}/{}", s.id, f.path);
            if hand_placed.contains(id.as_str()) {
                return Err(format!(
                    "map: stamp `{}` expands to a row named `{id}`, and the map already places one \
                     under that name by hand. A location's `props` could not say which it meant — \
                     rename the stamp or the placement.",
                    s.id
                ));
            }
            out.from
                .insert(id.clone(), (s.id.clone(), f.path.clone()));
            let placed = Placed {
                id,
                descriptor: f.descriptor,
                at: (s.at.0 + at.0, s.at.1 + at.1),
                yaw: add_yaw(f.yaw, s.yaw),
                lift: f.lift,
                paint: f.paint,
                tip: f.tip,
                on,
                // An owned stamp is owned whole: a generator routing around half a group would leave
                // the other half of it to be overwritten.
                owned: s.owned,
                owned_because: s.owned_because.clone(),
                patch: f.patch,
                note: f.note,
            };
            working.placements.push(placed.clone());
            out.placements.push(placed);
        }

        for l in locs {
            out.locations.push(Location {
                id: format!("{}/{}", s.id, l.id),
                props: l.props.iter().map(|p| format!("{}/{}", s.id, p)).collect(),
                interactions: l.interactions,
                note: l.note,
            });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// The derived edge interface
// ---------------------------------------------------------------------------------------------

/// One flattened member with its local box and its lattice as it stands.
struct Boxed {
    path: String,
    x: (f32, f32),
    y: (f32, f32),
    z: (f32, f32),
    div: (u32, u32, u32),
    grid: Subgrid,
}

/// **What a bounded composition presents on each of its four sides**, read off its members.
///
/// `None` for an [`Envelope::Anchored`] composition: it claims no tile, so it has no face for anything
/// to abut. That is not a missing feature — an interface on something with no boundary would be a
/// number invented to fill a field.
///
/// A face position no member reaches reads `None`, and `None` matches only `None`. That is not a
/// wildcard, and the rule is not new here — [`crate::adjacency`] already decided it, so a composition
/// and a plain tile answer the same way.
///
/// # Nested compositions are read through, not read off
///
/// A nested group contributes through [`flatten`], so this reads its members' cells rather than
/// calling itself on the child and splicing the result. The two give the **same tokens** — a child
/// flush with a parent face presents the cells its own interface would report, and a child sitting
/// inside the parent reaches no face either way — and going through `flatten` means there is one
/// walk of containment rather than two that could disagree about depth or order.
///
/// Encapsulation is not weakened by that, because encapsulation is about the *mutation* direction:
/// [`expand`] refuses an [`Override`] naming anything but the stamped composition's own members, which
/// is where reaching into a child would actually do damage.
pub fn interface(
    comp: &Composition,
    compositions: &[Composition],
    library: &Library,
    per_tile: u32,
) -> Result<Option<Interface>, String> {
    let Envelope::Bounded { size } = comp.envelope else {
        return Ok(None);
    };
    if per_tile == 0 {
        return Err(format!(
            "composition: `{}` cannot derive an interface — the project divides each tile 0 ways",
            comp.id
        ));
    }
    let (flats, _) = flatten(comp, compositions, &[], &mut Vec::new(), 0)?;

    // The members' own heights come from their mounts, and a mount is only answerable against a map.
    // So the envelope becomes one: a floor at zero, the declared bounds, and the members on it — which
    // means `stack::resolve_y` answers here exactly as it will in the game.
    let mut scratch = Map {
        version: crate::map::MAP_VERSION,
        name: "composition_envelope".to_owned(),
        origin: (0.0, 0.0, 0.0),
        bounds: size,
        placements: Vec::new(),
        stamps: Vec::new(),
        locations: Vec::new(),
        note: None,
    };
    for f in &flats {
        scratch.placements.push(Placed {
            // **Zero, deliberately.** A face is read off geometry; paint order only decides what is
            // seen where two things coincide, so it must not reach the interface. This is the
            // "seating precision does not become token precision" rule in its other direction.
            paint: 0,
            id: f.path.clone(),
            descriptor: f.descriptor.clone(),
            at: f.at,
            yaw: f.yaw,
            lift: f.lift,
            tip: f.tip,
            on: f.on.clone(),
            owned: false,
            owned_because: None,
            patch: f.patch.clone(),
            note: None,
        });
    }
    let ys = crate::stack::resolve_y(&scratch, library).map_err(|e| {
        format!(
            "composition: `{}` cannot derive an interface — {e}. Every member has to have a height \
             before the boundary can be read.",
            comp.id
        )
    })?;

    let mut boxes: Vec<Boxed> = Vec::new();
    for (i, f) in flats.iter().enumerate() {
        let base = library.get(&f.descriptor).ok_or_else(|| {
            format!(
                "composition: `{}` member `{}` places descriptor `{}`, which the library does not \
                 define",
                comp.id, f.path, f.descriptor
            )
        })?;
        let d = match &f.patch {
            Some(p) => base.patched_with(p),
            None => base.clone(),
        };
        let Some(g) = d.subgrid.clone() else { continue };
        if !g.cells.iter().any(|c| c.edge.is_some()) {
            continue; // says nothing about its edges, so it contributes nothing to say
        }
        let quarter = crate::adjacency::quarter_turns(&f.path, f.yaw)?;
        let div = crate::descriptor::divisions(&d, per_tile)?;
        let (w, h, dep) = crate::descriptor::tipped_extents(&d, f.tip).ok_or_else(|| {
            format!(
                "composition: `{}` member `{}` is unmeasured, so its boundary cannot be read",
                comp.id, f.path
            )
        })?;
        let turned_div = crate::descriptor::rotate_div(div, quarter);
        // Both halves turned together: a face read off a rotated lattice with unrotated divisions is a
        // face of the wrong length.
        let grid = g.rotated(quarter, div);
        let (sw, sd) = if quarter % 2 == 1 { (dep, w) } else { (w, dep) };
        let y0 = ys.get(i).copied().unwrap_or(0.0);
        boxes.push(Boxed {
            path: f.path.clone(),
            x: (f.at.0 - sw * 0.5, f.at.0 + sw * 0.5),
            y: (y0, y0 + h),
            z: (f.at.1 - sd * 0.5, f.at.1 + sd * 0.5),
            div: turned_div,
            grid,
        });
    }

    let subunit = crate::grid::SNAP / per_tile as f32;
    let half = (size.0 * 0.5, size.2 * 0.5);
    let env = (
        (-half.0, half.0),
        (0.0, size.1),
        (-half.1, half.1),
    );
    let mut faces: [Vec<Band>; 4] = Default::default();
    let mut faults = Vec::new();

    for dir in [N, E, S, W] {
        let lateral_is_z = dir == E || dir == W;
        let lat = if lateral_is_z { env.2 } else { env.0 };
        let steps = |span: (f32, f32)| (((span.1 - span.0) / subunit).round() as u32).max(1);
        let (n_lat, n_y) = (steps(lat), steps(env.1));
        // A row per sampled height, rather than one flat vector with a stride to remember. The shape
        // is then carried by the type, so banding it needs no length invariant to check and has no
        // wrong answer to give if one were ever violated.
        let mut rows: Vec<Vec<Option<String>>> = Vec::with_capacity(n_y as usize);

        for iy in 0..n_y {
            let wy = env.1.0 + (iy as f32 + 0.5) * (env.1.1 - env.1.0) / n_y as f32;
            let mut row: Vec<Option<String>> = Vec::with_capacity(n_lat as usize);
            for il in 0..n_lat {
                let wl = lat.0 + (il as f32 + 0.5) * (lat.1 - lat.0) / n_lat as f32;
                let mut found: Option<(&str, Option<&str>)> = None;
                for b in &boxes {
                    // On this face, and covering this sample.
                    let on_face = match dir {
                        E => (b.x.1 - env.0.1).abs() <= crate::adjacency::EDGE_EPSILON,
                        W => (b.x.0 - env.0.0).abs() <= crate::adjacency::EDGE_EPSILON,
                        N => (b.z.0 - env.2.0).abs() <= crate::adjacency::EDGE_EPSILON,
                        S => (b.z.1 - env.2.1).abs() <= crate::adjacency::EDGE_EPSILON,
                        _ => false,
                    };
                    if !on_face || wy < b.y.0 || wy > b.y.1 {
                        continue;
                    }
                    let (lo, hi) = if lateral_is_z { b.z } else { b.x };
                    if wl < lo || wl > hi {
                        continue;
                    }
                    let ay = crate::adjacency::index(wy, b.y, b.div.1);
                    let al = crate::adjacency::index(wl, (lo, hi), if lateral_is_z { b.div.2 } else { b.div.0 });
                    let last = |n: u32| n.saturating_sub(1);
                    let cell = match dir {
                        E => (last(b.div.0), ay, al),
                        W => (0, ay, al),
                        N => (al, ay, 0),
                        S => (al, ay, last(b.div.2)),
                        _ => continue,
                    };
                    let token = b.grid.at(cell).and_then(|c| c.edge.as_deref());
                    match found {
                        None => found = Some((b.path.as_str(), token)),
                        Some((other, other_token)) if other_token != token => {
                            faults.push(InterfaceFault {
                                dir,
                                a: other.to_owned(),
                                b: b.path.clone(),
                                a_token: other_token.map(str::to_owned),
                                b_token: token.map(str::to_owned),
                                message: format!(
                                    "`{}`: `{other}` presents {} on its {} face where `{}` presents \
                                     {} — the group has no single answer for what abuts it there",
                                    comp.id,
                                    other_token.unwrap_or("nothing"),
                                    dir_name(dir),
                                    b.path,
                                    token.unwrap_or("nothing"),
                                ),
                            });
                        }
                        Some(_) => {}
                    }
                }
                row.push(found.and_then(|(_, t)| t.map(str::to_owned)));
            }
            rows.push(row);
        }
        faces[dir] = into_bands(&rows, lat, env.1);
    }
    faults.sort_by(|p, q| (p.dir, &p.a, &p.b).cmp(&(q.dir, &q.a, &q.b)));
    faults.dedup_by(|p, q| p.dir == q.dir && p.a == q.a && p.b == q.b);
    Ok(Some(Interface { faces, faults }))
}

/// **Collapse a face's samples into the rectangles that make it up.**
///
/// Strips first, then runs within a strip: rows that read identically all the way across merge into
/// one horizontal strip, and each strip splits at every point its token changes. Doing it in that
/// order is what makes the answer **canonical** — a greedy rectangle cover of the same cells has
/// several valid answers and would let two runs describe one face two ways.
///
/// The sample count disappears here, which is the point: the same wall divided five ways and fifty
/// bands identically, so how finely a project subdivides a tile stops leaking into what its pieces
/// say about each other.
///
/// `rows` is outer-to-inner as [`interface`] samples it — one entry per height, each holding one
/// entry per lateral step. Rows of unequal length compare unequal and therefore simply do not merge,
/// so there is no shape to assert and no degraded answer to return.
fn into_bands(rows: &[Vec<Option<String>>], lat: (f32, f32), yspan: (f32, f32)) -> Vec<Band> {
    let n_y = rows.len();
    let n_lat = rows.first().map_or(0, Vec::len);
    if n_y == 0 || n_lat == 0 {
        return Vec::new();
    }
    let y_at = |i: usize| yspan.0 + (yspan.1 - yspan.0) * i as f32 / n_y as f32;
    let l_at = |i: usize| lat.0 + (lat.1 - lat.0) * i as f32 / n_lat as f32;

    let mut out = Vec::new();
    let mut iy = 0;
    while iy < n_y {
        let Some(row) = rows.get(iy) else { break };
        let mut ny = 1;
        while rows.get(iy + ny).is_some_and(|r| r == row) {
            ny += 1;
        }
        let y = (y_at(iy), y_at(iy + ny));
        let mut il = 0;
        while il < n_lat {
            let token = row.get(il).cloned().flatten();
            let mut nl = 1;
            while il + nl < n_lat && row.get(il + nl).cloned().flatten() == token {
                nl += 1;
            }
            out.push(Band { y, lat: (l_at(il), l_at(il + nl)), token });
            il += nl;
        }
        iy += ny;
    }
    out
}

fn dir_name(dir: Dir) -> &'static str {
    match dir {
        N => "north",
        E => "east",
        S => "south",
        W => "west",
        _ => "unknown",
    }
}

/// Bumped when the shape of the file changes. Read under the same floor rule
/// [`crate::map::MAP_VERSION`] states: a build reads what it understands and refuses what it cannot.
pub const COMPOSITIONS_VERSION: u32 = 1;

/// **Every composition a project can stamp, in one file.**
///
/// One file rather than a directory, for the reasons `crate::library` already gives about the same
/// choice: a directory makes ordering into filesystem ordering, makes a duplicate id into two files
/// that each look right alone, and does not help anyway because containment is a property of the whole
/// set.
///
/// # It is rewritten by an ordinary serializer, so it must carry no comments
///
/// The Compose tab's record verb reserializes this file to write fingerprints back. That deletes every
/// `//` comment in it — the same `to_string_pretty` bake `crate::map`'s module note records losing 279
/// comments to on 2026-07-16. The shipped file was authored with comments and lost them exactly that
/// way, once, which is why this paragraph exists.
///
/// **Prose goes in a `note` field**, on the set, on a composition, on a member, on a location, on an
/// interaction. `crate::map` already made this choice for the map format and gives the argument: if
/// the reasoning is a field, no serializer can lose it and no writer has to be surgical.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compositions {
    pub version: u32,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub compositions: Vec<Composition>,
}

impl Compositions {
    pub fn parse(text: &str) -> Result<Compositions, String> {
        let set: Compositions = ron::from_str(text).map_err(|e| format!("compositions: {e}"))?;
        if set.version > COMPOSITIONS_VERSION {
            return Err(format!(
                "compositions: version {} but this build reads {COMPOSITIONS_VERSION} — refusing a \
                 file written by a newer tool rather than dropping what it says",
                set.version
            ));
        }
        Ok(set)
    }

    pub fn to_ron(&self) -> Result<String, String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("compositions: serialize: {e}"))
    }

    /// The file's own name, beside `library.ron` and `project.ron` in a project directory.
    pub const FILE: &'static str = "compositions.ron";
}

/// **Write down what every member currently presents**, so later drift is measurable.
///
/// The verb behind the STALE badge: a group that has never been measured has nothing to be stale
/// against, and this is what turns "unrecorded" into "fresh". Returns how many it changed, so a
/// caller can say nothing-to-do rather than claiming work it did not do.
pub fn record_fingerprints(
    comp: &mut Composition,
    compositions: &[Composition],
    library: &Library,
) -> Result<usize, String> {
    let mut changed = 0;
    for m in &mut comp.members {
        let measured = body_fingerprint_of(&m.body, compositions, library)?;
        if m.of_fingerprint != Some(measured) {
            m.of_fingerprint = Some(measured);
            changed += 1;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{Extent, Mount, Offers, SubCell};
    use crate::library::LIBRARY_VERSION;

    /// A measured piece with no opinions beyond its size.
    fn piece(id: &str, w: f32, d: f32, h: f32) -> Descriptor {
        Descriptor {
            id: id.to_owned(),
            extent: Extent { footprint: Some((w, d)), height: Some(h) },
            ..Default::default()
        }
    }

    /// A piece that presents `token` on every cell of its lattice.
    fn tiled(id: &str, w: f32, d: f32, h: f32, token: &str) -> Descriptor {
        tiled_divided(id, w, d, h, token, 1)
    }

    /// The same piece, with its cells authored at `per_tile` divisions.
    ///
    /// A subgrid is indexed at [`crate::descriptor::divisions`] for the project it belongs to, so
    /// authoring at one density and reading at another is not a finer view of the same piece — it is
    /// a piece most of whose cells are simply absent. Anything comparing two densities has to author
    /// both.
    fn tiled_divided(id: &str, w: f32, d: f32, h: f32, token: &str, per_tile: u32) -> Descriptor {
        let mut p = piece(id, w, d, h);
        let div = crate::descriptor::divisions(&p, per_tile).expect("measured");
        let mut cells = Vec::new();
        for x in 0..div.0 {
            for y in 0..div.1 {
                for z in 0..div.2 {
                    cells.push(SubCell {
                        at: (x, y, z),
                        solid: true,
                        edge: Some(token.to_owned()),
                        anchor: None,
                    });
                }
            }
        }
        p.subgrid = Some(Subgrid { cells });
        p
    }

    /// **An empty composition is refused at the door, and again at the stamp.**
    ///
    /// The refusal lived in `validate_shape`, moved to `expand` to unblock a verb that created empty
    /// tiles, and came back when that verb did not survive: authoring moved to the Map, where an
    /// empty box selection is already refused, so nothing makes one any more.
    ///
    /// Both halves are pinned because they catch different things. `validate_shape` catches a
    /// hand-edited file **at load, naming the empty composition** — including one nested inside
    /// another, which the stamp-time check can only report against the outer id. `expand` catches a
    /// caller that built compositions in memory and never came through the door.
    ///
    /// The reason the load-time check is the important one: `redraw_stamps` and `emerge-bevy` expand
    /// a whole map in ONE call, and the editor despawns every stamped row before that call can fail.
    /// One empty composition therefore costs every stamped row in the map, not just its own.
    #[test]
    fn an_empty_composition_is_refused_at_the_door_and_at_the_stamp() {
        let comp = Composition {
            id: "half_built".to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 2.4, 1.0) },
            members: Vec::new(),
            locations: Vec::new(),
            note: None,
        };
        let lib = library(vec![piece("floor", 1.0, 1.0, 0.1)]);

        // The door, naming the composition an author has to go and fix.
        let err = comp.validate_shape().expect_err("an empty composition must not load");
        assert!(err.contains("half_built"), "{err}");
        assert!(err.contains("no members"), "{err}");
        let err = validate(std::slice::from_ref(&comp), &lib)
            .expect_err("nor pass the whole-set validation the loader runs");
        assert!(err.contains("half_built"), "{err}");

        // And the backstop, for a caller that built one in memory. It names the stamp AND the
        // composition, because at that point there is one of each.
        let stamps = vec![Stamped {
            id: "s1".to_owned(),
            of: "half_built".to_owned(),
            ..Default::default()
        }];
        let err = expand(&empty_map(), &stamps, std::slice::from_ref(&comp), &lib)
            .expect_err("stamping an empty composition has to refuse");
        assert!(err.contains("s1"), "the stamp is not named: {err}");
        assert!(err.contains("half_built"), "the composition is not named: {err}");
    }

    /// **A nested empty composition is named by the one an author has to edit.**
    ///
    /// The stamp-time check can only see that the whole expansion came out empty, so it reports the
    /// OUTER composition — which is not the one to go and fix. The load-time check walks every
    /// composition in the set, so it names the inner one directly. This is why moving the check
    /// downstream lost information, not just timing.
    #[test]
    fn a_nested_empty_composition_is_named_rather_than_its_parent() {
        let alcove = Composition {
            id: "alcove".to_owned(),
            envelope: Envelope::Anchored,
            members: Vec::new(),
            locations: Vec::new(),
            note: None,
        };
        let room = Composition {
            id: "room".to_owned(),
            envelope: Envelope::Anchored,
            members: vec![Member {
                id: "nook".to_owned(),
                body: Body::Composition { id: "alcove".to_owned() },
                at: (0.0, 0.0),
                yaw: 0.0,
                lift: 0.0,
                paint: 0,
                of_fingerprint: None,
                note: None,
            }],
            locations: Vec::new(),
            note: None,
        };
        let lib = library(vec![piece("floor", 1.0, 1.0, 0.1)]);
        let err = validate(&[alcove, room], &lib).expect_err("the empty inner one must refuse");
        assert!(
            err.contains("alcove"),
            "the refusal has to name the composition to go and edit, not its parent: {err}"
        );
    }

    fn library(descriptors: Vec<Descriptor>) -> Library {
        Library { version: LIBRARY_VERSION, note: None, descriptors }
    }

    fn empty_map() -> Map {
        Map {
            version: crate::map::MAP_VERSION,
            name: "scratch".to_owned(),
            origin: (0.0, 0.0, 0.0),
            bounds: (32.0, 4.0, 32.0),
            placements: Vec::new(),
            stamps: Vec::new(),
            locations: Vec::new(),
            note: None,
        }
    }

    fn member(id: &str, descriptor: &str, at: (f32, f32)) -> Member {
        Member {
            id: id.to_owned(),
            paint: 0,
            body: Body::Descriptor {
                id: descriptor.to_owned(),
                tip: (0, 0),
                on: None,
                patch: None,
            },
            at,
            yaw: 0.0,
            lift: 0.0,
            of_fingerprint: None,
            note: None,
        }
    }

    fn stamp(id: &str, of: &str, at: (f32, f32), yaw: f32) -> Stamped {
        Stamped {
            id: id.to_owned(),
            of: of.to_owned(),
            at,
            yaw,
            ..Default::default()
        }
    }

    /// Write down what every member's body currently looks like — what an editor does the moment a
    /// composition is authored, and what makes anything later a measurable change rather than a guess.
    fn record_all(comps: &mut Vec<Composition>, lib: &Library) {
        let snapshot = comps.clone();
        for c in comps.iter_mut() {
            super::record_fingerprints(c, &snapshot, lib).expect("records");
        }
    }

    /// Two members, one library entry each, no nesting — the base case everything else varies from.
    fn simple() -> (Library, Vec<Composition>) {
        let lib = library(vec![piece("desk", 1.0, 1.0, 0.8), piece("chair", 0.5, 0.5, 1.0)]);
        let comp = Composition {
            id: "workstation".to_owned(),
            envelope: Envelope::Anchored,
            members: vec![member("chair", "chair", (0.0, 1.0)), member("desk", "desk", (0.0, 0.0))],
            locations: Vec::new(),
            note: None,
        };
        (lib, vec![comp])
    }

    // -------------------------------------------------------------------------------------
    // Expansion
    // -------------------------------------------------------------------------------------

    /// **The output does not depend on the order the file lists stamps in.**
    ///
    /// The project's determinism lint exists because a "stable enough" ordering is how one input came
    /// out two ways. A map is a file somebody hand-edits, so the order of its stamps is exactly the
    /// kind of incidental fact that must not reach the output.
    #[test]
    fn expansion_does_not_depend_on_stamp_order() {
        let (lib, comps) = simple();
        let map = empty_map();
        let a = stamp("alpha", "workstation", (2.0, 0.0), 0.0);
        let b = stamp("beta", "workstation", (-2.0, 0.0), 90.0);

        let one = expand(&map, &[a.clone(), b.clone()], &comps, &lib).expect("expands");
        let two = expand(&map, &[b, a], &comps, &lib).expect("expands");
        assert_eq!(one.placements, two.placements);
        assert_eq!(one.from, two.from);
        assert_eq!(one.placements.len(), 4);
    }

    /// Rows are named after the stamp and the member, and the provenance comes back as structure —
    /// nobody has to parse the id apart to learn what a row belongs to.
    #[test]
    fn every_row_says_which_stamp_and_member_it_came_from() {
        let (lib, comps) = simple();
        let out = expand(&empty_map(), &[stamp("a1", "workstation", (0.0, 0.0), 0.0)], &comps, &lib)
            .expect("expands");
        let ids: Vec<&str> = out.placements.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["a1/chair", "a1/desk"]);
        assert_eq!(
            out.from.get("a1/desk"),
            Some(&("a1".to_owned(), "desk".to_owned()))
        );
    }

    /// A stamp's yaw turns the whole group about its anchor, and adds to each member's own yaw.
    ///
    /// Pinned against `stack::covers`' convention rather than re-derived: a positive yaw turns +X
    /// toward −Z, so a member one metre along +Z lands one metre along +X at 90°.
    #[test]
    fn a_stamps_yaw_turns_the_group_about_its_anchor() {
        let (lib, comps) = simple();
        let out = expand(&empty_map(), &[stamp("a1", "workstation", (0.0, 0.0), 90.0)], &comps, &lib)
            .expect("expands");
        let chair = out.placements.iter().find(|p| p.id == "a1/chair").expect("chair");
        assert!((chair.at.0 - 1.0).abs() < 1e-5, "chair x was {}", chair.at.0);
        assert!(chair.at.1.abs() < 1e-5, "chair z was {}", chair.at.1);
        assert!((chair.yaw - 90.0).abs() < 1e-5);
    }

    /// A member resting on a sibling is repointed to the row that sibling became — the promise
    /// `Placed::on` has always made, now kept across a stamp.
    #[test]
    fn a_sibling_host_is_repointed_to_the_row_it_became() {
        let mut desk = piece("desk", 1.0, 1.0, 0.8);
        desk.offers = Offers { surfaces: vec!["desk".to_owned()], sockets: Vec::new() };
        let mut lamp = piece("lamp", 0.2, 0.2, 0.4);
        lamp.mount = Some(Mount::OnSurface { class: "desk".to_owned() });
        let lib = library(vec![desk, lamp]);
        let comp = Composition {
            id: "lit_desk".to_owned(),
            envelope: Envelope::Anchored,
            members: vec![
                member("desk", "desk", (0.0, 0.0)),
                Member {
                    body: Body::Descriptor {
                        id: "lamp".to_owned(),
                        tip: (0, 0),
                        on: Some("desk".to_owned()),
                        patch: None,
                    },
                    ..member("lamp", "lamp", (0.0, 0.0))
                },
            ],
            locations: Vec::new(),
            note: None,
        };
        let out = expand(&empty_map(), &[stamp("a1", "lit_desk", (0.0, 0.0), 0.0)], &vec![comp], &lib)
            .expect("expands");
        let lamp = out.placements.iter().find(|p| p.id == "a1/lamp").expect("lamp");
        assert_eq!(lamp.on.as_deref(), Some("a1/desk"));
    }

    /// A member that needs a surface and finds none refuses the **whole** stamp, naming the member.
    ///
    /// The alternative — the piece at floor level — is a stamp that looks like it worked and is wrong
    /// in a way nobody notices until they walk into the room.
    #[test]
    fn a_member_with_nowhere_to_rest_refuses_the_whole_stamp() {
        let mut lamp = piece("lamp", 0.2, 0.2, 0.4);
        lamp.mount = Some(Mount::OnSurface { class: "desk".to_owned() });
        let lib = library(vec![lamp]);
        let comp = Composition {
            id: "floating".to_owned(),
            envelope: Envelope::Anchored,
            members: vec![member("lamp", "lamp", (0.0, 0.0))],
            locations: Vec::new(),
            note: None,
        };
        let err = expand(&empty_map(), &[stamp("a1", "floating", (0.0, 0.0), 0.0)], &vec![comp], &lib)
            .expect_err("refuses");
        assert!(err.contains("a1"), "{err}");
        assert!(err.contains("lamp"), "{err}");
    }

    /// **A hand-placed row and a stamped one cannot share a name.**
    ///
    /// Placement ids are not shape-checked — `fridge@1` ships, and `@` is not a legal id character —
    /// so nothing prevents someone naming a row `mess_a/table`, which is exactly what a stamp called
    /// `mess_a` mints. A duplicate makes a location's `props` ambiguous, which is the same failure
    /// `Map::validate` refuses duplicates for.
    #[test]
    fn an_expanded_id_colliding_with_a_hand_placed_row_refuses() {
        let (lib, comps) = simple();
        let mut map = empty_map();
        map.placements.push(Placed {
            id: "a1/desk".to_owned(),
            descriptor: "desk".to_owned(),
            ..Default::default()
        });
        let err = expand(&map, &[stamp("a1", "workstation", (0.0, 0.0), 0.0)], &comps, &lib)
            .expect_err("refuses");
        assert!(err.contains("a1/desk"), "{err}");
        assert!(err.contains("already places one"), "{err}");
    }

    /// Locations travel with the group, and their props point at the rows the stamp produced.
    #[test]
    fn a_locations_props_follow_the_rows_the_stamp_made() {
        let (lib, mut comps) = simple();
        comps[0].locations = vec![Location {
            id: "seat".to_owned(),
            props: vec!["chair".to_owned(), "desk".to_owned()],
            interactions: Vec::new(),
            note: None,
        }];
        let out = expand(&empty_map(), &[stamp("a1", "workstation", (0.0, 0.0), 0.0)], &comps, &lib)
            .expect("expands");
        assert_eq!(out.locations.len(), 1);
        assert_eq!(out.locations[0].id, "a1/seat");
        assert_eq!(out.locations[0].props, ["a1/chair", "a1/desk"]);
    }

    // -------------------------------------------------------------------------------------
    // The strength order
    // -------------------------------------------------------------------------------------

    /// **library < member patch < override**, and the two authored layers arrive merged into the one
    /// `Placed::patch` the map already has — so there stays exactly one place a patch meets a
    /// descriptor.
    #[test]
    fn an_override_wins_over_a_member_patch_and_both_over_the_library() {
        let lib = library(vec![piece("crate", 1.0, 1.0, 1.0)]);
        let comp = Composition {
            id: "stack".to_owned(),
            envelope: Envelope::Anchored,
            members: vec![Member {
                body: Body::Descriptor {
                    id: "crate".to_owned(),
                    tip: (0, 0),
                    on: None,
                    patch: Some(Descriptor {
                        look: vec!["rusted".to_owned()],
                        note: Some("the group's own opinion".to_owned()),
                        ..Default::default()
                    }),
                },
                ..member("box", "crate", (0.0, 0.0))
            }],
            locations: Vec::new(),
            note: None,
        };
        let s = Stamped {
            overrides: vec![Override {
                member: "box".to_owned(),
                patch: Descriptor { look: vec!["scorched".to_owned()], ..Default::default() },
                because: "this one is the fire-damaged corner".to_owned(),
            }],
            ..stamp("a1", "stack", (0.0, 0.0), 0.0)
        };
        let out = expand(&empty_map(), &[s], &vec![comp], &lib).expect("expands");
        let patch = out.placements[0].patch.as_ref().expect("carries a patch");
        // The override wins its own field...
        assert_eq!(patch.look, ["scorched"]);
        // ...and says nothing about the rest, so the member's opinion survives underneath it.
        assert_eq!(patch.note.as_deref(), Some("the group's own opinion"));
        // And the library entry is still what it always was — nothing was resolved into it here.
        assert!(lib.get("crate").expect("in library").look.is_empty());
    }

    /// An override naming a member that does not exist refuses, and lists the members that do.
    #[test]
    fn an_override_on_a_missing_member_refuses_and_names_what_there_is() {
        let (lib, comps) = simple();
        let s = Stamped {
            overrides: vec![Override {
                member: "stool".to_owned(),
                patch: Descriptor::default(),
                because: "wrong name".to_owned(),
            }],
            ..stamp("a1", "workstation", (0.0, 0.0), 0.0)
        };
        let err = expand(&empty_map(), &[s], &comps, &lib).expect_err("refuses");
        assert!(err.contains("stool"), "{err}");
        assert!(err.contains("chair") && err.contains("desk"), "{err}");
    }

    /// An override with no reason is refused for the same argument `owned_because` is.
    #[test]
    fn an_override_without_a_reason_is_refused() {
        let (lib, comps) = simple();
        let s = Stamped {
            overrides: vec![Override {
                member: "desk".to_owned(),
                patch: Descriptor::default(),
                because: "   ".to_owned(),
            }],
            ..stamp("a1", "workstation", (0.0, 0.0), 0.0)
        };
        let err = expand(&empty_map(), &[s], &comps, &lib).expect_err("refuses");
        assert!(err.contains("says nothing about why"), "{err}");
    }

    /// **Encapsulation.** A stamp may not reach into a nested group — that is restructuring, and it is
    /// the thing that makes Unity's nested-prefab overrides evaporate.
    #[test]
    fn a_stamp_cannot_override_a_nested_composition() {
        let (lib, mut comps) = simple();
        comps.push(Composition {
            id: "office".to_owned(),
            envelope: Envelope::Anchored,
            members: vec![Member {
                body: Body::Composition { id: "workstation".to_owned() },
                ..member("station", "unused", (0.0, 0.0))
            }],
            locations: Vec::new(),
            note: None,
        });
        let s = Stamped {
            overrides: vec![Override {
                member: "station".to_owned(),
                patch: Descriptor::default(),
                because: "reaching in".to_owned(),
            }],
            ..stamp("a1", "office", (0.0, 0.0), 0.0)
        };
        let err = expand(&empty_map(), &[s], &comps, &lib).expect_err("refuses");
        assert!(err.contains("is a composition"), "{err}");
    }

    // -------------------------------------------------------------------------------------
    // Nesting
    // -------------------------------------------------------------------------------------

    /// A nested group's transform folds into its members', so depth costs the reader nothing.
    #[test]
    fn a_nested_groups_transform_folds_into_its_members() {
        let (lib, mut comps) = simple();
        comps.push(Composition {
            id: "office".to_owned(),
            envelope: Envelope::Anchored,
            members: vec![Member {
                body: Body::Composition { id: "workstation".to_owned() },
                yaw: 90.0,
                ..member("station", "unused", (5.0, 0.0))
            }],
            locations: Vec::new(),
            note: None,
        });
        let out = expand(&empty_map(), &[stamp("a1", "office", (0.0, 0.0), 0.0)], &comps, &lib)
            .expect("expands");
        let chair = out
            .placements
            .iter()
            .find(|p| p.id == "a1/station/chair")
            .expect("the nested chair");
        // Local (0, 1) turned 90° is (1, 0), then offset by the member's own (5, 0).
        assert!((chair.at.0 - 6.0).abs() < 1e-5, "x was {}", chair.at.0);
        assert!(chair.at.1.abs() < 1e-5, "z was {}", chair.at.1);
        assert!((chair.yaw - 90.0).abs() < 1e-5);
    }

    /// A group inside itself refuses, and the message is the chain rather than "a cycle was found".
    #[test]
    fn a_group_containing_itself_refuses_and_names_the_loop() {
        let lib = library(vec![piece("desk", 1.0, 1.0, 0.8)]);
        let a = Composition {
            id: "a".to_owned(),
            envelope: Envelope::Anchored,
            members: vec![Member {
                body: Body::Composition { id: "b".to_owned() },
                ..member("inner", "unused", (0.0, 0.0))
            }],
            locations: Vec::new(),
            note: None,
        };
        let b = Composition {
            id: "b".to_owned(),
            members: vec![Member {
                body: Body::Composition { id: "a".to_owned() },
                ..member("inner", "unused", (0.0, 0.0))
            }],
            ..a.clone()
        };
        let err = validate(&[a, b], &lib).expect_err("refuses");
        assert!(err.contains("contains itself"), "{err}");
        assert!(err.contains("a → b → a"), "{err}");
    }

    /// Deeper than the cap refuses with the chain — never truncated to the first eight levels.
    #[test]
    fn nesting_past_the_depth_cap_refuses_and_names_the_chain() {
        let lib = library(vec![piece("desk", 1.0, 1.0, 0.8)]);
        let deepest = MAX_COMPOSITION_DEPTH as usize + 3;
        let mut comps: Vec<Composition> = Vec::new();
        for i in 0..deepest {
            let body = if i + 1 == deepest {
                Body::Descriptor { id: "desk".to_owned(), tip: (0, 0), on: None, patch: None }
            } else {
                Body::Composition { id: format!("c{}", i + 1) }
            };
            comps.push(Composition {
                id: format!("c{i}"),
                envelope: Envelope::Anchored,
                members: vec![Member { body, ..member("inner", "unused", (0.0, 0.0)) }],
                locations: Vec::new(),
                note: None,
            });
        }
        let err = validate(&comps, &lib).expect_err("refuses");
        assert!(err.contains("nests deeper"), "{err}");
        assert!(err.contains(&format!("{MAX_COMPOSITION_DEPTH}")), "{err}");
        assert!(err.contains("c0 → c1"), "{err}");
    }

    /// More rows than a stamp may carry refuses **with the count**, so the author knows how far over
    /// they are rather than only that they are.
    #[test]
    fn a_group_over_the_row_cap_refuses_with_the_count() {
        let lib = library(vec![piece("desk", 1.0, 1.0, 0.8)]);
        let over = MAX_RESOLVED_MEMBERS + 1;
        let members: Vec<Member> = (0..over)
            .map(|i| member(&format!("m{i:04}"), "desk", (i as f32, 0.0)))
            .collect();
        let comp = Composition {
            id: "too_much".to_owned(),
            envelope: Envelope::Anchored,
            members,
            locations: Vec::new(),
            note: None,
        };
        let err = validate(&[comp], &lib).expect_err("refuses");
        assert!(err.contains(&over.to_string()), "{err}");
        assert!(err.contains(&MAX_RESOLVED_MEMBERS.to_string()), "{err}");
    }

    // -------------------------------------------------------------------------------------
    // Validation
    // -------------------------------------------------------------------------------------

    /// Members out of canonical order refuse, and the message is the order to write.
    ///
    /// Not a style rule — see the module note. Without it the same group has several encodings and two
    /// authors produce diffs that differ without meaning to.
    #[test]
    fn members_out_of_canonical_order_refuse_and_say_the_order() {
        let (lib, mut comps) = simple();
        comps[0].members.reverse();
        let err = validate(&comps, &lib).expect_err("refuses");
        assert!(err.contains("out of order"), "{err}");
        assert!(err.contains("chair, desk"), "{err}");
    }

    /// A member resting on something outside the group is written as no host at all.
    #[test]
    fn a_host_that_is_not_a_member_refuses() {
        let (lib, mut comps) = simple();
        comps[0].members[0].body = Body::Descriptor {
            id: "chair".to_owned(),
            tip: (0, 0),
            on: Some("bench".to_owned()),
            patch: None,
        };
        let err = validate(&comps, &lib).expect_err("refuses");
        assert!(err.contains("bench"), "{err}");
        assert!(err.contains("no host at all"), "{err}");
    }

    /// A member placing a descriptor nothing defines refuses at validation, not at expansion.
    #[test]
    fn a_member_naming_an_unknown_descriptor_refuses() {
        let (lib, mut comps) = simple();
        comps[0].members.push(member("zzz", "hovercraft", (0.0, 0.0)));
        let err = validate(&comps, &lib).expect_err("refuses");
        assert!(err.contains("hovercraft"), "{err}");
    }

    /// Two stamps with one id would make two rows with one name.
    #[test]
    fn two_stamps_sharing_an_id_refuse() {
        let (lib, comps) = simple();
        let a = stamp("a1", "workstation", (0.0, 0.0), 0.0);
        let err = expand(&empty_map(), &[a.clone(), a], &comps, &lib).expect_err("refuses");
        assert!(err.contains("used twice"), "{err}");
    }

    // -------------------------------------------------------------------------------------
    // Fingerprints and staleness
    // -------------------------------------------------------------------------------------

    /// **The encoding is pinned.**
    ///
    /// The value below was measured, not derived — its job is to fail if the byte encoding or the hash
    /// ever changes. Both would silently re-fingerprint every composition in the corpus and turn a
    /// STALE badge designed to be truthful into noise, which is exactly the failure that is invisible
    /// without a test like this one.
    #[test]
    fn the_fingerprint_encoding_is_pinned() {
        let d = piece("desk", 1.5, 0.75, 0.8);
        assert_eq!(descriptor_fingerprint(&d), 0xca04_5f8b_b62c_b44f);
    }

    /// **The hand-encoded half is pinned too.**
    ///
    /// The first version of this test used a descriptor with no mount and no clearance, so it pinned
    /// only the paths that were already bytes and said nothing about the ones encoded through
    /// `Debug` — which is precisely where a rename would have re-fingerprinted the corpus silently.
    #[test]
    fn the_fingerprint_encoding_is_pinned_for_a_mounted_piece() {
        let mut d = piece("shelf_lamp", 0.25, 0.25, 0.4);
        d.mount = Some(Mount::OnSurface { class: "shelf".to_owned() });
        d.align.front = Some(crate::descriptor::Face::East);
        d.clearance = vec![crate::descriptor::Clearance { dir: ClearDir::Front, dist: 0.6 }];
        assert_eq!(descriptor_fingerprint(&d), 0x803c_ba5d_56e8_fff0);
    }

    /// Two different mounts are two different fingerprints — the encoding distinguishes what it
    /// claims to.
    #[test]
    fn different_mounts_fingerprint_differently() {
        let mut a = piece("thing", 0.5, 0.5, 0.5);
        a.mount = Some(Mount::OnWall { height: 1.2 });
        let mut b = a.clone();
        b.mount = Some(Mount::OnWall { height: 1.6 });
        let mut c = a.clone();
        c.mount = Some(Mount::OnCeiling);
        let fa = descriptor_fingerprint(&a);
        assert_ne!(fa, descriptor_fingerprint(&b), "height must matter");
        assert_ne!(fa, descriptor_fingerprint(&c), "the variant must matter");
    }

    /// A fingerprint ignores what composition does not depend on. A reworded note is not a change.
    #[test]
    fn a_reworded_note_does_not_move_the_fingerprint() {
        let mut a = piece("desk", 1.0, 1.0, 0.8);
        let before = descriptor_fingerprint(&a);
        a.note = Some("actually the good desk".to_owned());
        a.look = vec!["walnut".to_owned()];
        assert_eq!(descriptor_fingerprint(&a), before);
    }

    /// Resizing a piece does move it — that is the whole point.
    #[test]
    fn resizing_a_piece_moves_the_fingerprint() {
        let a = piece("desk", 1.0, 1.0, 0.8);
        let b = piece("desk", 1.2, 1.0, 0.8);
        assert_ne!(descriptor_fingerprint(&a), descriptor_fingerprint(&b));
    }


    /// **A hand-written group is UNRECORDED, not stale.**
    ///
    /// The distinction cost a shipped-looking bug: with a bare `u64` defaulting to zero, every group
    /// in the hand-authored file read *"STALE — 3 members changed underneath this group"* against
    /// `recorded 0x0000000000000000`, which is a sentence about drift that never happened. A group
    /// nobody has measured has nothing to have drifted from.
    #[test]
    fn a_hand_written_group_reads_unrecorded_rather_than_stale() {
        let (lib, comps) = simple();
        let report = stale_members(&comps[0], &comps, &lib).expect("checks");
        assert_eq!(report.len(), 2);
        assert!(report.iter().all(|s| s.freshness == Freshness::Unrecorded));
        assert!(report.iter().all(|s| s.recorded.is_none()));
    }

    /// Recording turns unrecorded into fresh, and says how many it touched — so a second press can
    /// honestly report nothing to do rather than claiming work.
    #[test]
    fn recording_is_idempotent_and_says_what_it_changed() {
        let (lib, mut comps) = simple();
        let snapshot = comps.clone();
        let first = record_fingerprints(&mut comps[0], &snapshot, &lib).expect("records");
        assert_eq!(first, 2);
        let again = record_fingerprints(&mut comps[0], &snapshot, &lib).expect("records");
        assert_eq!(again, 0, "nothing changed, so nothing to record");
        assert!(stale_members(&comps[0], &comps, &lib).expect("checks").is_empty());
    }

    /// A member whose body changed underneath it is stale, and the report says by how much.
    #[test]
    fn a_member_whose_body_changed_reads_stale() {
        let (lib, mut comps) = simple();
        // Record the truth first, so nothing is stale.
        record_all(&mut comps, &lib);
        assert!(stale_members(&comps[0], &comps, &lib).expect("checks").is_empty());

        let grown = library(vec![piece("desk", 2.0, 1.0, 0.8), piece("chair", 0.5, 0.5, 1.0)]);
        let stale = stale_members(&comps[0], &comps, &grown).expect("checks");
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].member, "desk");
        assert_eq!(stale[0].freshness, Freshness::Stale);
        assert_ne!(stale[0].recorded, Some(stale[0].measured));
    }

    /// **Early cutoff.** An edit that leaves the interface alone leaves every dependent alone.
    ///
    /// The optimisation *Build Systems à la Carte* names as the one that makes incremental work
    /// correct rather than merely fast: invalidate on a fingerprint mismatch, not on any edit.
    #[test]
    fn an_edit_that_does_not_change_the_interface_marks_nothing_stale() {
        let (lib, mut comps) = simple();
        record_all(&mut comps, &lib);
        let mut reworded = lib.descriptors.clone();
        reworded[0].note = Some("the one by the window".to_owned());
        reworded[0].look = vec!["oak".to_owned()];
        let after = library(reworded);
        assert!(
            stale_members(&comps[0], &comps, &after).expect("checks").is_empty(),
            "a note and a look are not something composition depends on"
        );
    }

    // -------------------------------------------------------------------------------------
    // The derived interface
    // -------------------------------------------------------------------------------------

    /// An anchored group claims no tile, so it has no face for anything to abut.
    #[test]
    fn an_anchored_group_has_no_interface() {
        let (lib, comps) = simple();
        assert!(interface(&comps[0], &comps, &lib, 1).expect("derives").is_none());
    }

    /// A bounded group presents what its members present, read off their cells.
    #[test]
    fn a_bounded_groups_face_is_read_off_its_members() {
        let lib = library(vec![tiled("wall", 1.0, 1.0, 1.0, "wall")]);
        let comp = Composition {
            id: "bay".to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 1.0, 1.0) },
            members: vec![member("wall", "wall", (0.0, 0.0))],
            locations: Vec::new(),
            note: None,
        };
        let iface = interface(&comp, &vec![comp.clone()], &lib, 1)
            .expect("derives")
            .expect("bounded");
        assert!(iface.is_clean(), "{:?}", iface.faults);
        // One band, not one per cell: the whole east side says `wall` and there is nothing to break
        // it up, so the face is described by the single rectangle that is the face.
        assert_eq!(iface.faces[E].len(), 1, "east read {:?}", iface.faces[E]);
        assert_eq!(iface.faces[E][0].token.as_deref(), Some("wall"));
        assert_eq!(iface.faces[E][0].y, (0.0, 1.0));
        assert_eq!(iface.faces[E][0].lat, (-0.5, 0.5));
    }

    /// **The bands do not depend on how finely the project divides a tile.**
    ///
    /// This is the whole point of describing a face by its rectangles rather than by its cells. The
    /// same wall in a project that divides a tile once and in one that divides it eight times has to
    /// say the same thing, or "what does this face present" is really "what does this face present,
    /// at this project's settings" — which is what the old cell vector answered, and why its display
    /// line had to quote counts that changed without anything changing.
    #[test]
    fn dividing_a_tile_more_finely_does_not_change_what_a_face_presents() {
        let comp = Composition {
            id: "bay".to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 1.0, 1.0) },
            members: vec![member("wall", "wall", (0.0, 0.0))],
            locations: Vec::new(),
            note: None,
        };
        let comps = vec![comp.clone()];
        let read = |per_tile: u32| {
            let lib = library(vec![tiled_divided("wall", 1.0, 1.0, 1.0, "wall", per_tile)]);
            interface(&comp, &comps, &lib, per_tile).expect("derives").expect("bounded").faces
        };
        assert_eq!(read(1), read(8), "1 division read differently from 8");
    }

    /// **A doorway keeps its opening**, which is why a face is not one token.
    ///
    /// Measured on the shipped kits: `site/wall_doorway`, `site/wall_doorway_wide`,
    /// `site/wall_window` and `site_greybox`'s `wall_doorway_wide` all present `wall` at the jambs and
    /// nothing through the middle. Collapsing a face to a single word would have to fault all four or
    /// pick a winner, and picking is what [`Interface::faults`] exists to avoid.
    #[test]
    fn a_gap_between_two_members_bands_the_face_rather_than_faulting_it() {
        let lib = library(vec![tiled("jamb", 1.0, 1.0, 2.0, "wall")]);
        let comp = Composition {
            id: "doorway".to_owned(),
            // Three tiles wide, with the middle one empty: a doorway, in the smallest form that has
            // one.
            envelope: Envelope::Bounded { size: (1.0, 2.0, 3.0) },
            members: vec![
                member("jamb_north", "jamb", (0.0, -1.0)),
                member("jamb_south", "jamb", (0.0, 1.0)),
            ],
            locations: Vec::new(),
            note: None,
        };
        let iface = interface(&comp, &vec![comp.clone()], &lib, 1)
            .expect("derives")
            .expect("bounded");
        assert!(iface.is_clean(), "a gap is not a disagreement: {:?}", iface.faults);
        let east: Vec<_> = iface.faces[E]
            .iter()
            .map(|b| (b.lat, b.token.as_deref()))
            .collect();
        assert_eq!(
            east,
            vec![
                ((-1.5, -0.5), Some("wall")),
                ((-0.5, 0.5), None),
                ((0.5, 1.5), Some("wall")),
            ],
            "east read {:?}",
            iface.faces[E]
        );
    }

    /// **Vertical variation survives too** — which is why the shipped kits' y-uniformity is not a
    /// property to design around.
    ///
    /// All 192 faces in both libraries read the same at every height, but that is a fact about those
    /// descriptors, not about the format: `interface` skips a member whose height does not reach the
    /// sample, so the moment a group mixes a low piece with a tall one the face has two strips.
    #[test]
    fn a_member_shorter_than_the_envelope_leaves_the_height_above_it_unlabelled() {
        let lib = library(vec![tiled("low", 1.0, 1.0, 1.0, "wall")]);
        let comp = Composition {
            id: "parapet".to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 2.0, 1.0) },
            members: vec![member("low", "low", (0.0, 0.0))],
            locations: Vec::new(),
            note: None,
        };
        let iface = interface(&comp, &vec![comp.clone()], &lib, 1)
            .expect("derives")
            .expect("bounded");
        let east: Vec<_> = iface.faces[E].iter().map(|b| (b.y, b.token.as_deref())).collect();
        assert_eq!(
            east,
            vec![((0.0, 1.0), Some("wall")), ((1.0, 2.0), None)],
            "east read {:?}",
            iface.faces[E]
        );
    }

    /// **A face no member reaches reads `None`** — and `None` is a token in its own right, matching
    /// only `None`. The rule is `adjacency`'s, not a new one invented here.
    #[test]
    fn a_face_no_member_reaches_reads_nothing() {
        let lib = library(vec![tiled("wall", 1.0, 1.0, 1.0, "wall")]);
        let comp = Composition {
            id: "half_bay".to_owned(),
            // Twice as wide as its only member, which sits against the west side.
            envelope: Envelope::Bounded { size: (2.0, 1.0, 1.0) },
            members: vec![member("wall", "wall", (-0.5, 0.0))],
            locations: Vec::new(),
            note: None,
        };
        let iface = interface(&comp, &vec![comp.clone()], &lib, 1)
            .expect("derives")
            .expect("bounded");
        assert_eq!(iface.faces[W].len(), 1, "west read {:?}", iface.faces[W]);
        assert_eq!(iface.faces[W][0].token.as_deref(), Some("wall"));
        assert_eq!(iface.faces[E].len(), 1, "east read {:?}", iface.faces[E]);
        assert_eq!(iface.faces[E][0].token, None, "east read {:?}", iface.faces[E]);
    }

    /// **Two members disagreeing about a face is reported, never resolved by picking one.**
    ///
    /// Silently choosing a winner is how a tool ends up modelling something other than what the author
    /// has in their head — and a group with no single answer for a face cannot constrain a neighbour,
    /// so it is refused as a solver prototype while staying perfectly stampable by hand.
    #[test]
    fn members_disagreeing_about_a_face_produce_a_fault() {
        let lib = library(vec![
            tiled("wall", 1.0, 1.0, 1.0, "wall"),
            tiled("panel", 1.0, 1.0, 1.0, "glass"),
        ]);
        let comp = Composition {
            id: "clash".to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 1.0, 1.0) },
            members: vec![
                member("panel", "panel", (0.0, 0.0)),
                member("wall", "wall", (0.0, 0.0)),
            ],
            locations: Vec::new(),
            note: None,
        };
        let iface = interface(&comp, &vec![comp.clone()], &lib, 1)
            .expect("derives")
            .expect("bounded");
        assert!(!iface.is_clean());
        let f = &iface.faults[0];
        assert!(f.message.contains("glass"), "{}", f.message);
        assert!(f.message.contains("wall"), "{}", f.message);
        assert!(f.message.contains("no single answer"), "{}", f.message);
    }

    /// A nested group flush with the parent's face contributes to it — depth does not hide a token.
    #[test]
    fn a_nested_group_contributes_to_the_parents_face() {
        let lib = library(vec![tiled("wall", 1.0, 1.0, 1.0, "wall")]);
        let inner = Composition {
            id: "inner".to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 1.0, 1.0) },
            members: vec![member("wall", "wall", (0.0, 0.0))],
            locations: Vec::new(),
            note: None,
        };
        let outer = Composition {
            id: "outer".to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 1.0, 1.0) },
            members: vec![Member {
                body: Body::Composition { id: "inner".to_owned() },
                ..member("part", "unused", (0.0, 0.0))
            }],
            locations: Vec::new(),
            note: None,
        };
        let comps = vec![inner, outer.clone()];
        let iface = interface(&outer, &comps, &lib, 1).expect("derives").expect("bounded");
        assert!(iface.is_clean(), "{:?}", iface.faults);
        assert_eq!(iface.faces[N].len(), 1, "north read {:?}", iface.faces[N]);
        assert_eq!(iface.faces[N][0].token.as_deref(), Some("wall"));
    }
}
