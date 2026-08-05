//! **A library** — every descriptor a project can place, in one file, validated together.
//!
//! A [`Descriptor`] on its own is a fact about one mesh. Most of the checks worth having are about a
//! *set* of them: a surface class something rests on and nothing offers is only visible when you can
//! see everything at once, and so is a duplicate id. So the unit an editor opens is this.
//!
//! # Why one file and not a directory of them
//!
//! A directory is the obvious shape and it is worse here. Ordering becomes filesystem ordering, which
//! is not stable across machines; a duplicate id becomes two files that both look right in isolation;
//! and the two-sided surface check needs the whole set anyway, so "load them all" is not an
//! optimisation, it is the only mode. One file also means a project's asset set has a single diff.
//!
//! The file is machine-owned — generated from a project's existing manifests by `crate::convert` — so
//! it is written by an ordinary serializer and never text-spliced. Its prose survives because
//! [`Descriptor::note`] is a field; see `crate::map`'s note on why that decision was made.

use serde::{Deserialize, Serialize};

use crate::descriptor::Descriptor;
use crate::vocab::{Masks, Vocabularies};

/// Bumped when the shape below changes. A mismatch is refused, never migrated — `persist.rs`'s rule.
pub const LIBRARY_VERSION: u32 = 1;

/// The most cells one piece's derived lattice may have.
///
/// The old per-descriptor `div` was capped at 9 per axis, which bounded a lattice at 729. Divisions
/// are now **derived** — a piece's span times the project's number — so nothing bounds them from the
/// descriptor's side, and a 10 m corridor at `divisions: 4` would build 80 x 40 x 8. This is the
/// replacement guard: generous enough that a 6 m wall at the shipped setting is nowhere near it, low
/// enough that the editor is never asked to draw a lattice nobody could author.
pub const MAX_LATTICE_CELLS: u32 = 4096;

/// Everything a project can place.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub version: u32,
    /// What this library is and where it came from.
    #[serde(default)]
    pub note: Option<String>,
    pub descriptors: Vec<Descriptor>,
}

impl Library {
    pub fn parse(text: &str) -> Result<Library, String> {
        let lib: Library = ron::from_str(text).map_err(|e| format!("library: {e}"))?;
        lib.validate()?;
        Ok(lib)
    }

    pub fn to_ron(&self) -> Result<String, String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("library: serialize: {e}"))
    }

    /// Structural checks that need no vocabulary: the version, and that ids identify.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != LIBRARY_VERSION {
            return Err(format!(
                "library: version {} but this build reads {LIBRARY_VERSION}. Regenerate it from the \
                 project's manifests rather than hand-editing the number — the fields moved.",
                self.version
            ));
        }
        for (i, d) in self.descriptors.iter().enumerate() {
            if d.id.trim().is_empty() {
                return Err(format!("library: descriptor {i} has no id"));
            }
            // **An id is a name, and names have a shape.** Nothing checked this, so whatever a tool or
            // a hand-edit last wrote is what a map ends up referencing. Segments rather than the whole
            // string, because `/` is the kit namespace — see `naming::is_id`.
            if !crate::naming::is_id(&d.id) {
                return Err(format!(
                    "library: `{}` is not a usable id. An id is snake_case — lowercase letters, \
                     digits and single underscores, starting with a letter — and `/` separates a kit \
                     from a piece, as in `site/wall_corner`.",
                    d.id
                ));
            }
            // The lattice is **not** checked here: a cell is in range or not depending on the
            // project's divisions-per-tile, which lives in `project.ron` and is not knowable from
            // this file alone. `policy::layered_library` runs `validate_lattices` once both layers
            // are parsed — it is the one loader, so there is no path on which the check is skipped.
            if let Some(j) = self.descriptors.iter().position(|o| o.id == d.id) {
                if j != i {
                    return Err(format!(
                        "library: `{}` is declared twice (entries {j} and {i}). A map references \
                         descriptors by id, so a duplicate makes every reference to it ambiguous.",
                        d.id
                    ));
                }
            }
        }
        Ok(())
    }

    /// **Every lattice against the project's divisions.**
    ///
    /// Separate from [`Self::validate`] because it needs a number `library.ron` does not carry: a
    /// cell at `(5, 0, 0)` is in range on a 3 m wall and outside a 0.5 m crate, and which it is
    /// depends on `project.ron`'s `divisions`. Called from `policy::layered_library`, the one place
    /// both layers exist at once.
    ///
    /// Refuses on the same grounds the per-lattice check always did — an out-of-range cell is a
    /// value nothing will ever read, a duplicate is two answers to one question — plus a ceiling on
    /// the derived volume, since divisions are multiplied by a piece's span and a 10 m corridor at a
    /// fine setting would otherwise build a lattice nobody can author or draw.
    pub fn validate_lattices(&self, divisions: u32) -> Result<(), String> {
        for d in &self.descriptors {
            let Some(grid) = &d.subgrid else {
                // Says nothing about its inside, so there is nothing to be wrong about. A piece
                // still gets divisions when something asks; it just has no cells of its own.
                continue;
            };
            let div = crate::descriptor::divisions(&d.extent, divisions, &d.id)?;
            let volume = crate::descriptor::Subgrid::volume(div);
            if volume > MAX_LATTICE_CELLS {
                return Err(format!(
                    "library: `{}` derives a {}x{}x{} lattice of {volume} cells, past the {MAX_LATTICE_CELLS} \
                     a piece may have. Divisions are multiplied by a piece's span, so this is either \
                     an oversized `extent` or a `divisions` set too fine for this kit.",
                    d.id, div.0, div.1, div.2
                ));
            }
            grid.validate(&d.id, div)?;
        }
        Ok(())
    }

    /// The full check: structure, every token, and the two-sided surface pass over the whole set.
    ///
    /// Returns the mask per descriptor, in library order, so a caller that is about to build a
    /// palette does not resolve the same tokens a second time.
    pub fn resolve(&self, vocab: &Vocabularies) -> Result<Vec<Masks>, String> {
        self.validate()?;
        vocab.validate_library(&self.descriptors)
    }

    pub fn get(&self, id: &str) -> Option<&Descriptor> {
        self.descriptors.iter().find(|d| d.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{Descriptor, Mount, Offers};

    fn d(id: &str) -> Descriptor {
        Descriptor {
            id: id.to_owned(),
            ..Descriptor::default()
        }
    }

    fn lib(descriptors: Vec<Descriptor>) -> Library {
        Library {
            version: LIBRARY_VERSION,
            note: None,
            descriptors,
        }
    }

    #[test]
    fn a_library_round_trips() {
        let l = lib(vec![d("a"), d("b")]);
        assert_eq!(
            Library::parse(&l.to_ron().unwrap_or_else(|e| panic!("{e}"))).unwrap_or_else(|e| panic!("{e}")),
            l
        );
    }

    /// A map references descriptors by id, so two entries answering to one name makes every
    /// reference to it ambiguous — and the wrong mesh appearing is a bug nobody traces to the library.
    #[test]
    fn a_duplicate_id_is_refused() {
        let err = lib(vec![d("crate"), d("crate")]).validate().err().unwrap_or_default();
        assert!(err.contains("declared twice"), "{err}");
        assert!(err.contains("crate"), "must name the id: {err}");
    }

    #[test]
    fn another_schema_version_is_refused_not_migrated() {
        let mut l = lib(vec![d("a")]);
        l.version = LIBRARY_VERSION + 1;
        assert!(l.validate().err().unwrap_or_default().contains("Regenerate"));
    }

    /// The check that needs the whole set. Each descriptor is individually perfect.
    #[test]
    fn resolve_catches_a_surface_nobody_offers() {
        let vocab = Vocabularies {
            surfaces: crate::vocab::Vocabulary::of(&[("worktop", "a desk top")]),
            ..Vocabularies::default()
        };
        let mut mug = d("mug");
        mug.mount = Some(Mount::OnSurface {
            class: "worktop".into(),
        });
        let err = lib(vec![mug]).resolve(&vocab).err().unwrap_or_default();
        assert!(err.contains("no descriptor in this library offers"), "{err}");
    }

    #[test]
    fn resolve_returns_masks_in_library_order() {
        let vocab = Vocabularies {
            surfaces: crate::vocab::Vocabulary::of(&[("worktop", "a desk top")]),
            ..Vocabularies::default()
        };
        let mut table = d("table");
        table.offers = Offers {
            surfaces: vec!["worktop".into()],
            ..Offers::default()
        };
        let mut mug = d("mug");
        mug.mount = Some(Mount::OnSurface {
            class: "worktop".into(),
        });
        let masks = lib(vec![table, mug])
            .resolve(&vocab)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(masks.len(), 2);
        assert!(masks[1].rests_on(&masks[0]), "the mug should rest on the table");
    }
}
