//! **The project's layer over the library** — where one game's architecture goes.
//!
//! A descriptor is a **measurement**. `extent.height` is 0.796 m because somebody measured the mesh;
//! `align.front` is 90° because the artist modelled it facing that way; `align.scale` corrects an
//! export. None of that is an opinion, and all of it is the same in every game that loads the asset.
//!
//! `align.stretch_y` is not like that. *"A 2.0 m wall mesh made to reach a 2.4 m wall height is a
//! statement about one game's architecture."* It sat on the descriptor because that is where the Site
//! kit kept it — as a derived `y_scale` — and it is the reason the plan left open question 3 open:
//! bake a project's wall height into a shared library and you get one game's ceiling in another's
//! rooms, silently, with the library file as the only evidence.
//!
//! # The patch model already had the answer
//!
//! [`Descriptor`] is a patch over defaults — every field optional, absence means inherit. So the
//! answer is not a new mechanism, it is **one more layer of the mechanism that already exists**:
//!
//! ```text
//! library.ron   the measurements          — shared, portable, true anywhere
//! project.ron   this game's policy        — stretch_y, and anything else that is an opinion
//! map.ron       this instance's overrides — Placed::patch
//! ```
//!
//! Each layer is a `Descriptor` and each is applied with [`Descriptor::patched_with`], which is the
//! same function all the way down. Nothing here is a second way to express a scale.
//!
//! # Matching by tag, not only by id
//!
//! A policy that had to name all 41 descriptors would be a policy nobody keeps current — the 42nd
//! piece is imported and quietly does not get the rule. So a patch names either **one id** or **one
//! `kind` token**, and a rule reads as *"every door in this game is stretched to 2.4 m."*
//!
//! Order is file order and later wins, because that is the only rule an author can hold in their head
//! while reading a file top to bottom. Specificity ordering — "the id beats the tag no matter where
//! you put it" — sounds better and then requires you to know the whole file to predict one line.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::descriptor::Descriptor;
use crate::library::Library;

/// Bumped whenever the shape below changes. A mismatch is refused, never migrated.
pub const POLICY_VERSION: u32 = 1;

/// The library file, inside a project directory.
pub const LIBRARY_FILE: &str = "library.ron";
/// The policy file, inside a project directory.
pub const POLICY_FILE: &str = "project.ron";

/// **The** way a library is loaded: measurements, then this project's policy over them.
///
/// One function because there are two readers — the editor and the game — and a library layered one
/// way in the editor and another in the game is a preview that lies, which is the failure this whole
/// crate keeps being written to avoid.
///
/// Both files are **required**. A missing `project.ron` could reasonably mean "no rules", and that is
/// exactly the reasoning that grows a second code path: the absent-file branch and the empty-file
/// branch would then be two ways to say the same thing, and only one of them would get tested. A
/// project states its policy, even when its policy is nothing.
pub fn layered_library(dir: &Path) -> Result<Layered, String> {
    let read = |name: &str| -> Result<String, String> {
        let path = dir.join(name);
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let measured = Library::parse(&read(LIBRARY_FILE)?)
        .map_err(|e| format!("{}: {e}", dir.join(LIBRARY_FILE).display()))?;
    let policy = Policy::parse(&read(POLICY_FILE)?)
        .map_err(|e| format!("{}: {e}", dir.join(POLICY_FILE).display()))?;
    let library = policy
        .apply(&measured)
        .map_err(|e| format!("{}: {e}", dir.join(POLICY_FILE).display()))?;
    // The lattice check lives here rather than in `Library::validate` because it needs both files: a
    // cell is in range or not depending on the piece's size and this project's `divisions`. Run on
    // the **layered** library, since that is what the game reads and a patch may change an extent —
    // which is exactly the case where a cell authored against the measurement falls outside.
    library
        .validate_lattices(policy.face_bands)
        .map_err(|e| format!("{}: {e}", dir.join(LIBRARY_FILE).display()))?;
    // **Compositions are optional and their absence is not a degraded mode.** A project that stamps
    // nothing has no file, and that is the same state as a file holding no compositions — so this is
    // one path with one meaning, not a fallback. A file that *exists* and cannot be read is fatal,
    // exactly as the other two are: an unreadable palette that opens empty looks like a project with
    // no assets.
    let comp_path = dir.join(crate::composition::Compositions::FILE);
    let compositions = if comp_path.exists() {
        let text = std::fs::read_to_string(&comp_path)
            .map_err(|e| format!("{}: {e}", comp_path.display()))?;
        crate::composition::Compositions::parse(&text)
            .map_err(|e| format!("{}: {e}", comp_path.display()))?
    } else {
        crate::composition::Compositions {
            version: crate::composition::COMPOSITIONS_VERSION,
            ..Default::default()
        }
    };
    crate::composition::validate(&compositions.compositions, &library)
        .map_err(|e| format!("{}: {e}", comp_path.display()))?;

    Ok(Layered {
        measured,
        library,
        policy,
        compositions,
    })
}

/// **All three layers of an opened project**, because two callers need more than the top one.
///
/// The game reads [`Self::library`] and nothing else. An editor needs the other two: it writes
/// [`Self::measured`] — the measurements file, *without* this game's architecture baked into it —
/// and it reads [`Policy::face_bands`] to know how finely a face is read.
///
/// One struct from the one loader rather than a second parse in the editor, for the reason
/// [`layered_library`] exists at all: a library layered one way in the editor and another in the
/// game is a preview that lies.
pub struct Layered {
    /// Every composition the project can stamp, validated against the layered library.
    ///
    /// Empty when the project has no `compositions.ron`, which means exactly what a file with no
    /// compositions in it means.
    pub compositions: crate::composition::Compositions,
    /// `library.ron` exactly as parsed — the measurements, portable to any game.
    ///
    /// **What an editor writes back.** Serializing the layered library over this file bakes one
    /// game's wall heights into a kit meant to be shared, and the next load applies the patches
    /// again on top of them.
    pub measured: Library,
    /// The measurements with this project's policy applied. What the game places.
    pub library: Library,
    /// The policy itself, for the fields that are not patches — see [`Policy::face_bands`].
    pub policy: Policy,
}

/// What a patch applies to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Match {
    /// Exactly this descriptor.
    Id(String),
    /// Every descriptor carrying this `kind` token — *"every door"*.
    Kind(String),
}

/// One rule: what it applies to, why, and the fields it layers on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    #[serde(rename = "match")]
    pub matches: Match,
    /// **Why this game does this.** Required, and not decoration: a policy layer is precisely the
    /// place where a number nobody can justify goes unnoticed for a year, because the file it changes
    /// is not the file it lives in. `Placed::owned_because` makes the same demand for the same reason.
    pub because: String,
    /// The fields to layer on. A `Descriptor` because that is what the layer below is, so the merge is
    /// [`Descriptor::patched_with`] rather than a second, subtly different set of rules.
    pub patch: Descriptor,
}

/// A game's policy over a shared library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u32,
    /// What this project's architecture is, in a sentence.
    #[serde(default)]
    pub note: Option<String>,
    /// **How finely a piece's subgrid of EDGE TOKENS is indexed** — the lattice a face is read on.
    ///
    /// A band is `grid::SNAP / face_bands` on every axis, and a piece spanning N cells gets
    /// `N * face_bands` of them. See [`crate::descriptor::divisions`] for the derivation and
    /// [`crate::descriptor::Subgrid`] for why this is a project number rather than a per-piece one.
    ///
    /// # This was `divisions`, and the rename is the point
    ///
    /// One number used to serve two jobs: indexing edge tokens *and* deciding how finely the Compose
    /// tab seats a member. They belong to different objects. **Edge tokens belong to the face** — a
    /// 2-D component, where a token should be one word per face however finely the interior is cut;
    /// `summarise_face`'s ten-cells-saying-the-same-word complaint was what it looked like when they
    /// were the same number. **Seating belongs to the volume**, and is [`Policy::seating_divisions`].
    ///
    /// Splitting them also keeps a deferred migration deferred: edge-token indexing is still blocked
    /// on the edge-versus-corner question, so raising this to seat a sconce would re-author every
    /// token in the kit on a format that may change again. Merrell names the other half of the price
    /// — *"small objects require closely spaced planes while large objects require large volumes,
    /// which together means that many planes must be created"* — a finer face vocabulary buys the
    /// adjacency problem nothing.
    ///
    /// **It belongs here, not in `library.ron`.** How finely to divide is a statement about how much
    /// detail *this game's* generator needs, exactly like `stretch_y` is a statement about its
    /// ceiling height — and the same argument applies: bake it into a shared library and one game's
    /// resolution silently governs another's.
    ///
    /// **1 by default**, so a band is `grid::SNAP` itself — the half-metre grid the kits are
    /// already authored on, on which a 3 m wall is 6 bands and a 2.4 m one is 5 layers.
    #[serde(default = "one")]
    pub face_bands: u32,
    /// **How finely a tile subdivides for SEATING** — the step the Compose tab moves a member by.
    ///
    /// A seat step is `grid::SNAP / seating_divisions` metres, and seats are the multiples of it
    /// measured from the envelope's **centre** in X/Z and its **floor** in Y. Multiples of a unit
    /// from the centre always include the centre, so nudging a piece out and back returns it exactly
    /// where it started — which dividing the tile into cells and seating at cell centres would not
    /// (at 4 those are 0.125 / 0.375 / 0.625 / 0.875, with no seat in the middle).
    ///
    /// **Seating precision does not become token precision.** Two members at different seats can
    /// project onto the same face band, because bands are indexed by [`Policy::face_bands`]. That is
    /// the two axes being independent, working as intended — it is not a rounding bug.
    ///
    /// **4 by default**: 125 mm, fine enough to place a sconce and coarse enough to be a lattice
    /// rather than free movement. It does not make the flush verb redundant — `site/wall` is 0.1 m
    /// thick and sits flush at −0.45, which is not a multiple of 0.125 either, because art is
    /// authored to look right rather than to tile.
    #[serde(default = "four")]
    pub seating_divisions: u32,
    #[serde(default)]
    pub patches: Vec<Patch>,
}

/// The default for [`Policy::face_bands`]. A free function because `serde(default = ..)` needs a path.
fn one() -> u32 {
    1
}

/// The default for [`Policy::seating_divisions`].
fn four() -> u32 {
    4
}

/// The most a project may divide one tile.
///
/// Not a number anyone should need: at 8 a subunit is 62 mm, finer than the meshes it describes, and
/// a 3 m wall carries 48 x 40 x 8 cells. The ceiling exists because divisions are derived and
/// multiplied by a piece's span, so a typo here is not one absurd tile but every tile at once.
pub const MAX_DIVISIONS: u32 = 8;

impl Default for Policy {
    fn default() -> Self {
        Policy {
            version: POLICY_VERSION,
            note: None,
            face_bands: one(),
            seating_divisions: four(),
            patches: Vec::new(),
        }
    }
}

impl Policy {
    pub fn parse(text: &str) -> Result<Policy, String> {
        let p: Policy = ron::from_str(text).map_err(|e| format!("policy: {e}"))?;
        p.validate()?;
        Ok(p)
    }

    pub fn to_ron(&self) -> Result<String, String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("policy: {e}"))
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != POLICY_VERSION {
            return Err(format!(
                "policy: version {} but this build reads {POLICY_VERSION} — refusing to load rather \
                 than guess at a migration",
                self.version
            ));
        }
        // Refused at the project boundary rather than per lattice: every piece derives from this one
        // number, so a zero here would make every tile in the project cell-less at once.
        if self.seating_divisions == 0 || self.seating_divisions > MAX_DIVISIONS {
            return Err(format!(
                "policy: `seating_divisions` is {}; a tile seats between 1 and {MAX_DIVISIONS} ways.",
                self.seating_divisions
            ));
        }
        if self.face_bands == 0 || self.face_bands > MAX_DIVISIONS {
            return Err(format!(
                "policy: `face_bands` is {}; a face reads between 1 and {MAX_DIVISIONS} ways. Zero \
                 leaves every piece without cells, and past {MAX_DIVISIONS} the lattice is finer \
                 than the meshes it describes.",
                self.face_bands
            ));
        }
        for p in &self.patches {
            if p.because.trim().is_empty() {
                return Err(format!(
                    "policy: the patch for {:?} says nothing about why. A policy layer is where an \
                     unjustifiable number hides longest, because the file it changes is not the file \
                     it lives in.",
                    p.matches
                ));
            }
            // A patch that renames what it patches is not a patch, it is a second descriptor wearing
            // one — and the id is what every map placement references.
            if !p.patch.id.is_empty() {
                return Err(format!(
                    "policy: the patch for {:?} sets `id`. A policy layers fields onto a descriptor; \
                     renaming it would break every placement that names it.",
                    p.matches
                ));
            }
        }
        Ok(())
    }

    /// Layer this policy over a library, returning the library the game actually uses.
    ///
    /// Every matching patch is applied in file order, so two rules touching one descriptor compose
    /// and the last word wins. A patch that matches nothing is an **error**: it is either a typo or a
    /// rule about a piece somebody deleted, and both of those are things an author wants to hear
    /// about at load rather than infer from a room that looks wrong.
    pub fn apply(&self, library: &Library) -> Result<Library, String> {
        let mut out = library.clone();
        for rule in &self.patches {
            let mut hits = 0usize;
            for d in &mut out.descriptors {
                let applies = match &rule.matches {
                    Match::Id(id) => &d.id == id,
                    Match::Kind(token) => d.kind.iter().any(|k| k == token),
                };
                if applies {
                    *d = d.patched_with(&rule.patch);
                    hits += 1;
                }
            }
            if hits == 0 {
                return Err(match &rule.matches {
                    Match::Id(id) => format!(
                        "policy: the patch for `{id}` matches no descriptor in this library. Either \
                         the id is misspelled or the piece is gone; a rule that silently applies to \
                         nothing is how a policy rots."
                    ),
                    Match::Kind(token) => format!(
                        "policy: the patch for kind `{token}` matches no descriptor in this library. \
                         Either the token is misspelled or nothing carries it yet."
                    ),
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{Align, Extent};
    use crate::library::LIBRARY_VERSION;

    fn descriptor(id: &str, kind: &[&str], height: f32) -> Descriptor {
        Descriptor {
            id: id.to_owned(),
            mesh: Some(format!("{id}.glb")),
            extent: Extent {
                footprint: Some((1.0, 1.0)),
                height: Some(height),
            },
            kind: kind.iter().map(|k| (*k).to_owned()).collect(),
            ..Descriptor::default()
        }
    }

    fn library() -> Library {
        Library {
            version: LIBRARY_VERSION,
            note: None,
            descriptors: vec![
                descriptor("door_single", &["door"], 2.0),
                descriptor("door_double", &["door"], 2.0),
                descriptor("crate", &["container"], 0.6),
            ],
        }
    }

    fn stretch(by: f32) -> Descriptor {
        Descriptor {
            align: Align {
                stretch_y: Some(by),
                ..Align::default()
            },
            ..Descriptor::default()
        }
    }

    fn rule(matches: Match, patch: Descriptor) -> Patch {
        Patch {
            matches,
            because: "this game's doorways are 2.4 m".into(),
            patch,
        }
    }

    /// **The point of the layer.** A rule reads as "every door in this game", so the 42nd door
    /// imported next month gets it without anyone remembering to say so.
    #[test]
    fn a_rule_about_a_kind_reaches_every_piece_that_carries_it() {
        let policy = Policy {
            patches: vec![rule(Match::Kind("door".into()), stretch(1.2))],
            ..Policy::default()
        };
        let out = policy.apply(&library()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.descriptors[0].align.stretch_y, Some(1.2));
        assert_eq!(out.descriptors[1].align.stretch_y, Some(1.2));
        // And nothing else is touched — a policy is a statement about some pieces, not all of them.
        assert_eq!(out.descriptors[2].align.stretch_y, None);
    }

    /// The library underneath is unchanged: the same measurements can be layered by two games.
    #[test]
    fn the_measurements_underneath_are_left_alone() {
        let base = library();
        let policy = Policy {
            patches: vec![rule(Match::Kind("door".into()), stretch(1.2))],
            ..Policy::default()
        };
        let _ = policy.apply(&base).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(base.descriptors[0].align.stretch_y, None);

        // Two projects, one library, two architectures — which is the whole argument for the split.
        let tall = Policy {
            patches: vec![rule(Match::Kind("door".into()), stretch(1.5))],
            ..Policy::default()
        };
        let a = policy.apply(&base).unwrap_or_else(|e| panic!("{e}"));
        let b = tall.apply(&base).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(a.descriptors[0].align.stretch_y, Some(1.2));
        assert_eq!(b.descriptors[0].align.stretch_y, Some(1.5));
    }

    /// Rules compose in file order and the last word wins — the rule an author can hold in their head
    /// while reading top to bottom.
    #[test]
    fn a_later_rule_wins_over_an_earlier_one() {
        let policy = Policy {
            patches: vec![
                rule(Match::Kind("door".into()), stretch(1.2)),
                rule(Match::Id("door_double".into()), stretch(1.4)),
            ],
            ..Policy::default()
        };
        let out = policy.apply(&library()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.descriptors[0].align.stretch_y, Some(1.2));
        assert_eq!(out.descriptors[1].align.stretch_y, Some(1.4));
    }

    /// A rule that matches nothing is a typo or a rule about a deleted piece. Both are worth hearing
    /// at load rather than inferring from a room that looks wrong.
    #[test]
    fn a_rule_that_matches_nothing_is_refused() {
        let policy = Policy {
            patches: vec![rule(Match::Id("dooor_single".into()), stretch(1.2))],
            ..Policy::default()
        };
        let err = policy.apply(&library()).err().unwrap_or_default();
        assert!(err.contains("misspelled"), "{err}");

        let policy = Policy {
            patches: vec![rule(Match::Kind("hatch".into()), stretch(1.2))],
            ..Policy::default()
        };
        let err = policy.apply(&library()).err().unwrap_or_default();
        assert!(err.contains("hatch"), "{err}");
    }

    /// A number nobody can justify hides longest in a policy layer, because the file it changes is not
    /// the file it lives in.
    #[test]
    fn a_patch_has_to_say_why() {
        let text = Policy {
            patches: vec![Patch {
                matches: Match::Kind("door".into()),
                because: "   ".into(),
                patch: stretch(1.2),
            }],
            ..Policy::default()
        }
        .to_ron()
        .unwrap_or_else(|e| panic!("{e}"));
        let err = Policy::parse(&text).err().unwrap_or_default();
        assert!(err.contains("says nothing about why"), "{err}");
    }

    /// A patch may not rename what it patches — the id is what every placement references.
    #[test]
    fn a_patch_may_not_rename_its_target() {
        let text = Policy {
            patches: vec![Patch {
                matches: Match::Kind("door".into()),
                because: "because".into(),
                patch: Descriptor {
                    id: "something_else".into(),
                    ..Descriptor::default()
                },
            }],
            ..Policy::default()
        }
        .to_ron()
        .unwrap_or_else(|e| panic!("{e}"));
        let err = Policy::parse(&text).err().unwrap_or_default();
        assert!(err.contains("renaming it"), "{err}");
    }

    /// Round-trips through RON, so the file an author writes is the file the game reads.
    #[test]
    fn a_policy_round_trips() {
        let policy = Policy {
            version: POLICY_VERSION,
            seating_divisions: 4,
            note: Some("this facility has 2.4 m ceilings".into()),
            face_bands: 2,
            patches: vec![rule(Match::Kind("door".into()), stretch(1.2))],
        };
        let text = policy.to_ron().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Policy::parse(&text).unwrap_or_else(|e| panic!("{e}")), policy);
    }

    /// A policy written before divisions existed still parses, and gets the half-metre subunit the
    /// kits are already authored on.
    #[test]
    fn a_policy_written_before_face_bands_defaults_to_one() {
        let p = Policy::parse("(version: 1)").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.face_bands, 1);
    }

    /// Refused at the project boundary, because every piece in the project derives from these.
    ///
    /// **Both numbers, separately.** They were one field until the Compose tab needed a seating step
    /// finer than the face lattice, and a range check that only policed one of them would let the
    /// other be zero — which is every piece with no cells, or every member unable to move.
    #[test]
    fn a_division_count_outside_the_range_is_refused() {
        for bad in [0, MAX_DIVISIONS + 1] {
            let err = Policy::parse(&format!("(version: 1, face_bands: {bad})"))
                .err()
                .unwrap_or_default();
            assert!(err.contains("a face reads between 1 and"), "face_bands {bad}: {err}");
            let err = Policy::parse(&format!("(version: 1, seating_divisions: {bad})"))
                .err()
                .unwrap_or_default();
            assert!(err.contains("a tile seats between 1 and"), "seating {bad}: {err}");
        }
        assert!(Policy::parse("(version: 1, face_bands: 1)").is_ok());
        assert!(Policy::parse(&format!("(version: 1, face_bands: {MAX_DIVISIONS})")).is_ok());
        assert!(Policy::parse(&format!("(version: 1, seating_divisions: {MAX_DIVISIONS})")).is_ok());
    }

    /// **The two numbers are independent**, and a project that sets only one gets the default for the
    /// other. The whole reason they were split is that a face lattice and a seating lattice answer
    /// different questions; a project raising one must not silently raise the other.
    #[test]
    fn the_two_lattices_default_independently() {
        let p = Policy::parse("(version: 1, face_bands: 2)").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.face_bands, 2);
        assert_eq!(p.seating_divisions, 4, "seating keeps its own default");
        let p = Policy::parse("(version: 1, seating_divisions: 8)").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.face_bands, 1, "faces keep the half-metre grid the kits are authored on");
        assert_eq!(p.seating_divisions, 8);
    }

    /// Version mismatch is refused rather than migrated, the same rule the map and library hold.
    #[test]
    fn a_policy_from_another_schema_is_refused() {
        let text = Policy {
            version: POLICY_VERSION + 1,
            ..Policy::default()
        }
        .to_ron()
        .unwrap_or_else(|e| panic!("{e}"));
        let err = Policy::parse(&text).err().unwrap_or_default();
        assert!(err.contains("refusing to load"), "{err}");
    }
}
