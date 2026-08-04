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
pub fn layered_library(dir: &Path) -> Result<Library, String> {
    let read = |name: &str| -> Result<String, String> {
        let path = dir.join(name);
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let library = Library::parse(&read(LIBRARY_FILE)?)
        .map_err(|e| format!("{}: {e}", dir.join(LIBRARY_FILE).display()))?;
    let policy = Policy::parse(&read(POLICY_FILE)?)
        .map_err(|e| format!("{}: {e}", dir.join(POLICY_FILE).display()))?;
    policy
        .apply(&library)
        .map_err(|e| format!("{}: {e}", dir.join(POLICY_FILE).display()))
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
    #[serde(default)]
    pub patches: Vec<Patch>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            version: POLICY_VERSION,
            note: None,
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
            note: Some("this facility has 2.4 m ceilings".into()),
            patches: vec![rule(Match::Kind("door".into()), stretch(1.2))],
        };
        let text = policy.to_ron().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Policy::parse(&text).unwrap_or_else(|e| panic!("{e}")), policy);
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
