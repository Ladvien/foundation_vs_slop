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
//! # Versioning: refuse, never migrate
//!
//! `persist.rs` states the rule this follows and the reason it has no `#[serde(default)]` anywhere:
//! defaulting a missing field is an unreachable compatibility branch wearing a rationale, because the
//! version check already refused the file. A map from another schema is a loud error.

use serde::{Deserialize, Serialize};

use crate::descriptor::Descriptor;
use crate::placement::ir::Guard;

/// Bumped whenever the shape below changes. A mismatch is refused, never migrated.
pub const MAP_VERSION: u32 = 1;

/// One authored world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Map {
    pub version: u32,
    /// World-space offset of the map's own origin.
    pub origin: (f32, f32, f32),
    pub placements: Vec<Placed>,
    #[serde(default)]
    pub locations: Vec<Location>,
    /// What this map is and why it is laid out this way — the header prose, as data. See the module
    /// docs on why this is a field rather than a comment.
    #[serde(default)]
    pub note: Option<String>,
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
    /// Position in map space, metres.
    pub at: (f32, f32),
    pub yaw: f32,
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

    pub fn validate(&self) -> Result<(), String> {
        if self.version != MAP_VERSION {
            return Err(format!(
                "map: version {} but this build reads {MAP_VERSION} — refusing to load rather than \
                 guess at a migration",
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_map() -> Map {
        Map {
            version: MAP_VERSION,
            origin: (0.0, 0.0, 0.0),
            placements: vec![
                Placed {
                    id: "table_1".into(),
                    descriptor: "mess_table".into(),
                    at: (4.0, 4.0),
                    yaw: 0.0,
                    owned: false,
                    owned_because: None,
                    patch: None,
                    note: None,
                },
                Placed {
                    id: "stool_1".into(),
                    descriptor: "stool".into(),
                    at: (4.0, 3.0),
                    yaw: 180.0,
                    owned: false,
                    owned_because: None,
                    patch: None,
                    note: None,
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

    #[test]
    fn a_map_from_another_schema_is_refused_not_migrated() {
        let mut m = table_map();
        m.version = MAP_VERSION + 1;
        let err = m.validate().expect_err("must refuse");
        assert!(err.contains("refusing to load"), "{err}");
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
