//! **Closed token vocabularies** — what a descriptor's `kind`, `effects` and `look` are allowed to say.
//!
//! `docs/2026-08-03-asset-schema-audit.md` §2 is the reason this exists. The shipped asset schema has
//! exactly **one** closed vocabulary (`placement::surfaces`, two tokens, validated from both sides);
//! `affordances`, `tags` and `group` are unvalidated free text. The result is on the record: of eight
//! affordance tokens shipped, **four are never read anywhere** — `sleep`, `store`, `decor`, `hygiene`
//! — and nothing ever said so. A typo and a genuine feature request look identical in a free-text
//! field, and both fail silently.
//!
//! So every axis here is a table. A token not in the table is an error naming the descriptor *and* the
//! token, at load, before anything spawns.
//!
//! # Provided and required — the check that catches the real mistake
//!
//! Rejecting typos is the cheap half. The valuable half is the **two-sided** check
//! `placement::surfaces` already performs: a class that something *requires* and nothing *provides* is
//! an error, because it describes a relationship that can never happen.
//!
//! Osborn, Wardrip-Fruin & Mateas name this pattern in *Refining Operational Logics* (FDG 2017,
//! `10.1145/3102071.3102107`): catalog entries carry **required and provided concepts**, *"a kind of
//! ontological free variable, a placeholder for terms which can't be defined within the logic itself
//! or which could be converted into the native terms of other logics."* A support surface class is
//! exactly that — meaningless to the geometry, meaningful only where a provider and a requirer meet.
//!
//! # Why these three axes, and why they are all immutable
//!
//! Kapadia et al.'s smart-object formalism separates a smart object's **attributes** from its
//! **state**: attributes *"identify immutable properties of a smart object such as its role (e.g., a
//! button or a person) which never changes"*, as against dynamic properties like `IsPressed` or
//! `IsStanding` which change during play (`papers/et/eth-cgl-ar_games-Zun16b.pdf` §2).
//!
//! Everything here is the immutable half. `kind` is what a thing *is*, `effects` is what it *does to
//! the world*, `look` is what it *looks like*. A chair is always a chair; whether anyone is sitting on
//! it is a fact about the entity at runtime and has no business in an asset descriptor. That line is
//! what keeps this a vocabulary rather than a savegame.
//!
//! # Bits, because matching is the point
//!
//! Game AI Pro 4 ch.4, on smart-object links: *"the types of links can often be reduced to a
//! reasonable set of core capabilities, in which case a simple bit-mask can be used to represent the
//! requirements for the link and the capabilities of the agent. Comparing these bitmasks is a very
//! efficient way to filter out invalid links."* Each token gets a bit from its **position in the
//! table**, so a set of tokens is a `u64` and "does this offer what that needs" is one `&`.
//!
//! Position-derived bits mean **appending is safe and reordering is not**. That is deliberate: a
//! vocabulary is a schema, and shuffling one would silently re-point every mask already computed.
//! [`Vocabulary::validate`] refuses more than 64 tokens on an axis rather than wrapping.

use serde::{Deserialize, Serialize};

use crate::descriptor::{Descriptor, Mount};

/// The most tokens one axis can hold, set by the width of the mask they pack into.
pub const MAX_TOKENS: usize = 64;

/// One axis's closed token table.
///
/// Order is significant — see the module docs. Serialized as a plain list so a project's vocabulary
/// file reads as what it is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    /// Tokens, in bit order. `tokens[i]` owns bit `1 << i`.
    pub tokens: Vec<Token>,
}

/// One token, and the note that says what it means.
///
/// The note is not decoration. Four shipped affordance tokens turned out to have no reader, and the
/// only way anyone found out was an audit that read the whole tree. A token that cannot be described
/// in a sentence is a token nobody has decided the meaning of yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Token {
    pub name: String,
    /// What it means, and — where it has one — what reads it.
    #[serde(default)]
    pub note: Option<String>,
}

impl Vocabulary {
    /// Build from `(name, note)` pairs. For tables written in Rust rather than loaded from RON.
    pub fn of(pairs: &[(&str, &str)]) -> Vocabulary {
        Vocabulary {
            tokens: pairs
                .iter()
                .map(|(n, d)| Token {
                    name: (*n).to_owned(),
                    note: Some((*d).to_owned()),
                })
                .collect(),
        }
    }

    /// The bit for one token, or `None` if the axis does not have it.
    ///
    /// `None` rather than `0`: `placement::surfaces::surface_bits` returns `0` for an unknown token
    /// and relies on a *separate* validator to have rejected it first, which works only because that
    /// validator exists. Here the absence is in the type, so a caller cannot forget.
    pub fn bit(&self, token: &str) -> Option<u64> {
        self.tokens
            .iter()
            .position(|t| t.name == token)
            .map(|i| 1u64 << i)
    }

    /// The OR of the bits for `tokens`. Errors — naming the offender — on anything not in the table.
    pub fn mask(&self, tokens: &[String]) -> Result<u64, String> {
        let mut m = 0u64;
        for t in tokens {
            m |= self
                .bit(t)
                .ok_or_else(|| format!("`{t}` is not in this vocabulary"))?;
        }
        Ok(m)
    }

    pub fn contains(&self, token: &str) -> bool {
        self.tokens.iter().any(|t| t.name == token)
    }

    /// Every token, in bit order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tokens.iter().map(|t| t.name.as_str())
    }

    /// The table itself is well-formed: no duplicates, and few enough tokens to pack.
    fn validate(&self, axis: &str) -> Result<(), String> {
        if self.tokens.len() > MAX_TOKENS {
            return Err(format!(
                "vocab: axis `{axis}` declares {} tokens; the mask holds {MAX_TOKENS}. Widen the mask \
                 deliberately or split the axis — do not let the 65th token silently alias the 1st.",
                self.tokens.len()
            ));
        }
        for (i, t) in self.tokens.iter().enumerate() {
            if t.name.trim().is_empty() {
                return Err(format!("vocab: axis `{axis}` has an empty token at index {i}"));
            }
            if let Some(j) = self.tokens.iter().position(|o| o.name == t.name) {
                if j != i {
                    return Err(format!(
                        "vocab: axis `{axis}` declares `{}` twice (indices {j} and {i}). Tokens take \
                         their bit from their position, so a duplicate is two different bits meaning \
                         one thing.",
                        t.name
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Every axis a descriptor is validated against.
///
/// One file per project rather than a table in this crate, for the same reason `stretch_y` is a
/// project patch: a library shared between games must not carry one game's idea of what a prop can be.
/// The *mechanism* — closed tables, position bits, two-sided checking — is what is reusable.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabularies {
    /// What a thing **is**. Immutable identity: `"seating"`, `"light"`, `"food"`.
    pub kind: Vocabulary,
    /// What it **does to the world**, and what it needs in order to: `"emit"`,
    /// `"uses-electricity"`, `"stamina-recharge"`. This is the axis gameplay matches on.
    pub effects: Vocabulary,
    /// What it **looks like**: `"brown"`, `"rusted"`, `"metal"`. Never matched by gameplay; present so
    /// an author can search the library by eye.
    pub look: Vocabulary,
    /// Support-surface classes — what a piece offers a top for, and what a prop needs to rest on.
    /// The two-sided axis; see [`Vocabularies::validate_library`].
    pub surfaces: Vocabulary,
}

/// A descriptor's tokens, resolved to masks. Cheap to compare, which is the point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Masks {
    pub kind: u64,
    pub effects: u64,
    pub look: u64,
    /// Surface classes this piece **offers** a top for.
    pub provides: u64,
    /// The surface class this piece **needs** to rest on, if it rests on one at all.
    pub requires: u64,
}

impl Masks {
    /// Can a prop needing `self.requires` rest on a piece offering `host.provides`?
    ///
    /// The bitmask test from Game AI Pro 4 ch.4, and the same rule `placement::surfaces` documents: a
    /// prop rests only where `provides & requires != 0`. A prop that requires nothing rests on
    /// nothing — `0 & anything` is `0` — which is the honest answer for a freestanding piece.
    pub fn rests_on(&self, host: &Masks) -> bool {
        self.requires != 0 && (host.provides & self.requires) != 0
    }
}

impl Vocabularies {
    /// Parse a vocabulary file.
    pub fn parse(text: &str) -> Result<Vocabularies, String> {
        let v: Vocabularies =
            ron::from_str(text).map_err(|e| format!("vocab: does not parse: {e}"))?;
        v.validate_tables()?;
        Ok(v)
    }

    /// Every table is well-formed. Called by [`Self::parse`]; call it directly for tables built in
    /// Rust.
    pub fn validate_tables(&self) -> Result<(), String> {
        self.kind.validate("kind")?;
        self.effects.validate("effects")?;
        self.look.validate("look")?;
        self.surfaces.validate("surfaces")?;
        Ok(())
    }

    /// Resolve one descriptor's tokens to masks, or say which token is wrong.
    ///
    /// Errors name the descriptor, the axis, the token, and — because the overwhelmingly common cause
    /// is a typo rather than a missing feature — the nearest token that *is* in the table.
    pub fn masks(&self, d: &Descriptor) -> Result<Masks, String> {
        let axis = |v: &Vocabulary, name: &str, tokens: &[String]| -> Result<u64, String> {
            v.mask(tokens).map_err(|_| {
                let bad = tokens
                    .iter()
                    .find(|t| !v.contains(t))
                    .map_or_else(String::new, |t| t.clone());
                let hint = nearest(v, &bad)
                    .map(|n| format!(" Did you mean `{n}`?"))
                    .unwrap_or_default();
                format!(
                    "descriptor `{}`: `{bad}` is not a `{name}` token.{hint} The axis holds: {}. \
                     Growing a vocabulary is one row in the table — never a second list.",
                    d.id,
                    v.names().collect::<Vec<_>>().join(", ")
                )
            })
        };

        let requires = match &d.mount {
            Some(Mount::OnSurface { class }) => self.surfaces.bit(class).ok_or_else(|| {
                let hint = nearest(&self.surfaces, class)
                    .map(|n| format!(" Did you mean `{n}`?"))
                    .unwrap_or_default();
                format!(
                    "descriptor `{}`: mounts on surface class `{class}`, which is not a `surfaces` \
                     token.{hint} The axis holds: {}.",
                    d.id,
                    self.surfaces.names().collect::<Vec<_>>().join(", ")
                )
            })?,
            _ => 0,
        };

        Ok(Masks {
            kind: axis(&self.kind, "kind", &d.kind)?,
            effects: axis(&self.effects, "effects", &d.effects)?,
            look: axis(&self.look, "look", &d.look)?,
            provides: axis(&self.surfaces, "surfaces", &d.offers.surfaces)?,
            requires,
        })
    }

    /// **The two-sided check.** Validate a whole library at once, and refuse a surface class that
    /// something needs and nothing offers.
    ///
    /// A prop mounted on `"worktop"` in a library where no piece offers a worktop is not a typo the
    /// per-descriptor check can see — every token is spelled correctly. It is a prop that will never
    /// be placed, and the only moment anyone would otherwise notice is an empty room.
    pub fn validate_library(&self, library: &[Descriptor]) -> Result<Vec<Masks>, String> {
        let masks: Vec<Masks> = library
            .iter()
            .map(|d| self.masks(d))
            .collect::<Result<_, _>>()?;

        let offered = masks.iter().fold(0u64, |acc, m| acc | m.provides);
        for (d, m) in library.iter().zip(&masks) {
            if m.requires != 0 && (offered & m.requires) == 0 {
                let class = match &d.mount {
                    Some(Mount::OnSurface { class }) => class.clone(),
                    _ => String::new(),
                };
                return Err(format!(
                    "descriptor `{}` rests on surface class `{class}`, which no descriptor in this \
                     library offers. Nothing would ever place it, and an empty shelf looks exactly \
                     like a shelf nobody authored. Either give a piece `offers.surfaces` containing \
                     `{class}`, or change this prop's mount.",
                    d.id
                ));
            }
        }
        Ok(masks)
    }
}

/// The closest token by edit distance, when it is close enough to be worth suggesting.
///
/// Bounded at a third of the token's length: suggesting `light` for `worktop` would be noise, and a
/// wrong hint is worse than none because it sends the reader to the wrong table.
fn nearest<'a>(v: &'a Vocabulary, bad: &str) -> Option<&'a str> {
    if bad.is_empty() {
        return None;
    }
    let budget = (bad.len() / 3).max(1);
    v.names()
        .map(|n| (edit_distance(n, bad), n))
        .filter(|(d, _)| *d <= budget)
        .min_by_key(|(d, n)| (*d, n.len()))
        .map(|(_, n)| n)
}

/// Levenshtein distance, two rows at a time.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::Offers;

    fn vocabs() -> Vocabularies {
        Vocabularies {
            kind: Vocabulary::of(&[
                ("seating", "a thing to sit on"),
                ("table", "a thing with a top"),
                ("light", "a thing that lights the room"),
                ("food", "a thing that can be eaten"),
            ]),
            effects: Vocabulary::of(&[
                ("emit", "casts light; read by light::LightEmitter"),
                ("uses-electricity", "stops working when the power does"),
                ("stamina-recharge", "restores stamina when used"),
            ]),
            look: Vocabulary::of(&[("brown", "brown"), ("metal", "bare metal")]),
            surfaces: Vocabulary::of(&[
                ("support", "any support top"),
                ("worktop", "a desk or table top"),
            ]),
        }
    }

    fn desc(id: &str) -> Descriptor {
        Descriptor {
            id: id.to_owned(),
            ..Descriptor::default()
        }
    }

    #[test]
    fn a_token_takes_its_bit_from_its_position() {
        let v = vocabs();
        assert_eq!(v.kind.bit("seating"), Some(1));
        assert_eq!(v.kind.bit("table"), Some(2));
        assert_eq!(v.kind.bit("light"), Some(4));
        assert_eq!(v.kind.bit("chair"), None);
    }

    #[test]
    fn masks_or_together() {
        let v = vocabs();
        let m = v
            .kind
            .mask(&["seating".into(), "light".into()])
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m, 1 | 4);
    }

    /// The failure the audit found: four affordance tokens shipped that nothing read, and a typo
    /// would have looked identical. The error has to name the descriptor and the token.
    #[test]
    fn an_unknown_token_names_the_descriptor_and_the_token() {
        let v = vocabs();
        let mut d = desc("ozea/chair");
        d.kind = vec!["seeting".into()];
        let err = v.masks(&d).err().unwrap_or_default();
        assert!(err.contains("ozea/chair"), "must name the descriptor: {err}");
        assert!(err.contains("seeting"), "must name the token: {err}");
        assert!(err.contains("kind"), "must name the axis: {err}");
    }

    /// A near-miss is overwhelmingly a typo, so say so — but only when the guess is actually close.
    #[test]
    fn a_near_miss_suggests_the_token_that_was_meant() {
        let v = vocabs();
        let mut d = desc("x");
        d.kind = vec!["seeting".into()];
        assert!(
            v.masks(&d).err().unwrap_or_default().contains("Did you mean `seating`?"),
            "a one-character slip should be caught"
        );

        let mut far = desc("x");
        far.kind = vec!["helicopter".into()];
        let err = far_err(&v, &far);
        assert!(
            !err.contains("Did you mean"),
            "a wrong guess sends the reader to the wrong table: {err}"
        );
    }

    fn far_err(v: &Vocabularies, d: &Descriptor) -> String {
        v.masks(d).err().unwrap_or_default()
    }

    #[test]
    fn the_mount_class_is_validated_too() {
        let v = vocabs();
        let mut d = desc("ozea/mug");
        d.mount = Some(Mount::OnSurface {
            class: "worktopp".into(),
        });
        let err = v.masks(&d).err().unwrap_or_default();
        assert!(err.contains("worktopp") && err.contains("Did you mean `worktop`?"), "{err}");
    }

    /// The valuable half of the check, and the one a per-descriptor pass cannot make: every token is
    /// spelled right, and the prop will still never be placed.
    #[test]
    fn a_surface_nothing_offers_is_refused() {
        let v = vocabs();
        let mut mug = desc("ozea/mug");
        mug.mount = Some(Mount::OnSurface {
            class: "worktop".into(),
        });
        let err = v.validate_library(&[mug]).err().unwrap_or_default();
        assert!(err.contains("no descriptor in this library offers"), "{err}");
        assert!(err.contains("ozea/mug"), "must name the prop: {err}");
    }

    #[test]
    fn a_surface_something_offers_is_accepted_and_the_pair_matches() {
        let v = vocabs();
        let mut mug = desc("ozea/mug");
        mug.mount = Some(Mount::OnSurface {
            class: "worktop".into(),
        });
        let mut table = desc("ozea/mess_table");
        table.offers = Offers {
            surfaces: vec!["support".into(), "worktop".into()],
            ..Offers::default()
        };

        let masks = v
            .validate_library(&[table, mug])
            .unwrap_or_else(|e| panic!("{e}"));
        let (t, m) = (masks[0], masks[1]);
        assert!(m.rests_on(&t), "the mug should rest on the table");
        assert!(!t.rests_on(&m), "the table should not rest on the mug");
    }

    /// A freestanding piece requires nothing, and "nothing" must not match everything — `0 & x == 0`
    /// is the honest answer, and getting it backwards would let a wardrobe sit on a mug.
    #[test]
    fn a_piece_that_requires_nothing_rests_on_nothing() {
        let free = Masks::default();
        let table = Masks {
            provides: 0b11,
            ..Masks::default()
        };
        assert!(!free.rests_on(&table));
    }

    #[test]
    fn a_duplicate_token_is_refused_because_bits_come_from_position() {
        let v = Vocabulary::of(&[("a", ""), ("b", ""), ("a", "")]);
        let err = v.validate("kind").err().unwrap_or_default();
        assert!(err.contains("twice"), "{err}");
    }

    #[test]
    fn an_axis_wider_than_the_mask_is_refused_rather_than_wrapped() {
        let names: Vec<String> = (0..=MAX_TOKENS).map(|i| format!("t{i}")).collect();
        let v = Vocabulary {
            tokens: names
                .iter()
                .map(|n| Token {
                    name: n.clone(),
                    note: None,
                })
                .collect(),
        };
        let err = v.validate("kind").err().unwrap_or_default();
        assert!(err.contains("65") || err.contains("holds"), "{err}");
    }

    #[test]
    fn a_vocabulary_file_round_trips() {
        let v = vocabs();
        let text =
            ron::ser::to_string_pretty(&v, ron::ser::PrettyConfig::default()).expect("serializes");
        assert_eq!(Vocabularies::parse(&text).expect("parses"), v);
    }

    /// An unknown field in a vocabulary file is a mistake, not an extension — `deny_unknown_fields`
    /// everywhere, as `persist.rs` requires.
    #[test]
    fn an_unknown_axis_is_refused() {
        let err = Vocabularies::parse(
            "( kind: ( tokens: [] ), effects: ( tokens: [] ), look: ( tokens: [] ), \
             surfaces: ( tokens: [] ), mood: ( tokens: [] ) )",
        )
        .err()
        .unwrap_or_default();
        assert!(err.contains("does not parse"), "{err}");
    }
}
