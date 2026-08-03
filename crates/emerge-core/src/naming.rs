//! **Names that are also filenames** — one spelling, forced at the point of entry.
//!
//! A map carries a name, and that name becomes a file on disk, a key in a manifest, and something a
//! person types into a terminal. Those have different tolerances: a filesystem will happily accept
//! `My Map (final) v2.ron`, a shell will not without quoting, and two of them differing only by case
//! are the same file on one platform and two on another.
//!
//! So there is one spelling and it is **snake_case**: lowercase ASCII letters, digits, and single
//! underscores, starting with a letter. Everything the project already names this way agrees —
//! `wall_doorway_wide`, `mess_table`, `furniture_kenney` — so this is the existing convention being
//! enforced rather than a new one being imposed.
//!
//! # Forced, not merely validated
//!
//! [`to_snake_case`] is the important half. Rejecting a bad name tells an author they typed something
//! wrong and leaves them to work out what would be right; transforming it as they type means the
//! illegal state is never reachable. The editor filters keystrokes through this, so "Site 67" becomes
//! `site_67` while it is being typed rather than being refused when it is finished.
//!
//! [`is_snake_case`] stays because a file on disk was not necessarily typed here.

/// Is this already the one spelling?
pub fn is_snake_case(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.contains("__")
        && !s.ends_with('_')
}

/// Force a string into the one spelling.
///
/// Case folds, word boundaries become single underscores, and anything that is not a letter or digit
/// is dropped. `camelCase` and `PascalCase` split on the case change, so `SiteSixtySeven` becomes
/// `site_sixty_seven` rather than `sitesixtyseven` — the boundary is real information and throwing it
/// away makes the result unreadable.
///
/// Returns an empty string when nothing survives, which callers must treat as "no name yet" rather
/// than substituting one. A default name would be a second thing called `untitled` the first time two
/// people used it.
pub fn to_snake_case(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    let mut pending_break = false;

    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_alphanumeric() {
            // A lower→upper transition is a word boundary an author can see, so keep it. `HTTPServer`
            // splits before the last capital of a run, giving `http_server`, which is what a reader
            // expects even though it is the awkward case.
            let starts_word = c.is_ascii_uppercase()
                && i > 0
                && (chars[i - 1].is_ascii_lowercase()
                    || chars[i - 1].is_ascii_digit()
                    || (chars[i - 1].is_ascii_uppercase()
                        && chars.get(i + 1).is_some_and(char::is_ascii_lowercase)));
            if (pending_break || starts_word) && !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            pending_break = false;
        } else {
            // Any run of separators or junk collapses to one boundary, and a boundary before the
            // first character is nothing at all.
            pending_break = !out.is_empty();
        }
    }

    // A name must start with a letter, so lead digits are dropped rather than prefixed — `2nd_floor`
    // silently becoming `n_2nd_floor` would be a name nobody typed.
    let out = out.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_');
    out.trim_end_matches('_').to_owned()
}

/// The file a named map is written to.
pub fn map_file_name(name: &str) -> String {
    format!("{name}.map.ron")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_snake_is_left_alone() {
        for s in ["site_67", "a", "wall_doorway_wide", "kit_b2"] {
            assert!(is_snake_case(s), "{s} should be valid");
            assert_eq!(to_snake_case(s), s);
        }
    }

    #[test]
    fn the_invalid_shapes_are_named() {
        for s in ["", "Site", "site 67", "site-67", "_site", "site_", "site__67", "67"] {
            assert!(!is_snake_case(s), "{s} should be invalid");
        }
    }

    #[test]
    fn spaces_and_punctuation_become_one_underscore() {
        assert_eq!(to_snake_case("Site 67"), "site_67");
        assert_eq!(to_snake_case("my map (final) v2"), "my_map_final_v2");
        assert_eq!(to_snake_case("  lots   of   space  "), "lots_of_space");
        assert_eq!(to_snake_case("dash-and_underscore"), "dash_and_underscore");
    }

    /// A case boundary is information a reader uses, so splitting on it beats folding it away.
    #[test]
    fn case_boundaries_are_word_boundaries() {
        assert_eq!(to_snake_case("SiteSixtySeven"), "site_sixty_seven");
        assert_eq!(to_snake_case("camelCase"), "camel_case");
        assert_eq!(to_snake_case("HTTPServer"), "http_server");
    }

    /// A name has to start with a letter. Leading digits are dropped rather than prefixed: inventing
    /// a character produces a name the author never typed.
    #[test]
    fn leading_digits_are_dropped_not_prefixed() {
        assert_eq!(to_snake_case("2nd floor"), "nd_floor");
        assert_eq!(to_snake_case("67"), "");
    }

    /// Whatever comes out must be valid, or forcing the spelling has not forced anything.
    #[test]
    fn anything_non_empty_out_is_valid_in() {
        for s in [
            "Site 67",
            "---",
            "a",
            "ALLCAPS",
            "trailing___",
            "2nd floor",
            "m i x e d 99",
            "café",
        ] {
            let out = to_snake_case(s);
            if !out.is_empty() {
                assert!(is_snake_case(&out), "{s:?} produced invalid {out:?}");
            }
        }
    }

    /// Nothing survivable means no name — never a substituted one, because two maps both called
    /// `untitled` is the collision the naming rule exists to prevent.
    #[test]
    fn nothing_survivable_yields_no_name() {
        assert_eq!(to_snake_case(""), "");
        assert_eq!(to_snake_case("!!! ???"), "");
    }

    #[test]
    fn the_file_name_is_derived_from_the_name() {
        assert_eq!(map_file_name("site_67"), "site_67.map.ron");
    }
}
