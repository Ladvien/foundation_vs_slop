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

use std::path::Path;

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
    ///
    /// A plain `String` rather than an `Option`, so the file reads `note: "..."` instead of
    /// `note: Some("...")`. Empty is the absence, and it should be rare: a token nobody can describe
    /// in a sentence is a token whose meaning has not been decided yet, which is exactly how
    /// `sleep`/`store`/`decor`/`hygiene` shipped with no reader.
    #[serde(default)]
    pub note: String,
    /// **Other tokens this one brings with it**, on the axis the implication targets.
    ///
    /// Declared here because it is a fact about the WORD, not about any one asset: a bed restores
    /// stamina because this game says beds do, and a light draws power because it is a light. No
    /// render can show either, so no labeller can be asked for them and no author should have to
    /// remember them.
    ///
    /// It lived as a hardcoded five-row table in `emerge-mapper`'s labeller until 2026-08-19, which
    /// put game-design semantics inside the editor and left every other consumer — the game
    /// included — unaware of it. A descriptor written by hand, or by any tool that is not this
    /// editor, simply did not get the effect. Moving it here makes it one fact, read by
    /// [`Vocabularies::masks`], which is the pass both the game and the editor already go through.
    ///
    /// Only `kind` uses this today. It is on [`Token`] rather than on a `kind`-specific type
    /// because the relation is "token implies token" and nothing about it is special to that axis.
    #[serde(default)]
    pub implies: Vec<String>,
}

impl Vocabulary {
    /// Build from `(name, note)` pairs. For tables written in Rust rather than loaded from RON.
    pub fn of(pairs: &[(&str, &str)]) -> Vocabulary {
        Vocabulary {
            tokens: pairs
                .iter()
                .map(|(n, d)| Token {
                    name: (*n).to_owned(),
                    note: (*d).to_owned(),
                    // Rust-side tables state no implications; those are authored in `vocab.ron`,
                    // which is where a fact about the game's words belongs.
                    implies: Vec::new(),
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
    /// What an **actor** can do: `"eat"`, `"cook"`, `"guard"`. The only axis about people rather than
    /// props, and the one a [`crate::map::RoleSlot`] matches on.
    ///
    /// Separate from `effects` on purpose, even though both are "what something does". `effects` is a
    /// property of a thing standing in a room; this is a property of somebody who could walk into it.
    /// Sharing one table would let a chair be authored as able to cook, and the error would surface as
    /// a scene that silently never starts.
    #[serde(default)]
    pub capabilities: Vocabulary,
    /// What a lattice cell **presents to whatever abuts it** — `"wall"`, `"door"`, `"open"`.
    ///
    /// Closed for the reason every axis here is closed, but the cost of leaving it open is higher than
    /// elsewhere: `crate::adjacency` matches these by **equality**, so a typo does not read as a wrong
    /// token, it reads as a token that matches nothing. The seam simply reports a fault the author
    /// cannot account for — and once these feed a solver, as an unexplained contradiction.
    ///
    /// Empty means a project authors no edge tokens. That is not permissive: a cell carrying one is
    /// then refused, naming the empty axis, which is the honest reading of "this project has not
    /// decided what its tiles present".
    #[serde(default)]
    pub edge: Vocabulary,
    /// **What may fill a hole in a tile** — `"wall-fixture"`, `"floor-decal"`.
    ///
    /// The token a `composition::Body::Slot` accepts. Distinct from `capabilities`, which is about
    /// people, and from `surfaces`, which is about what holds what up. The axis exists so that the
    /// first one has to be declared rather than invented at a keyboard.
    ///
    /// **It was `anchor`**, a role an item could occupy on a *mesh's* lattice — authored, validated,
    /// drawn, saved and read by nothing, with an empty axis and 289 `None`s in the shipped kits. A
    /// hole belongs to a tile rather than to a mesh: it can sit in open air, and two tiles sharing a
    /// wall can differ about whether one has a socket in it. The axis is inherited rather than
    /// replaced, because it is the same question asked of the right object.
    #[serde(default)]
    pub slot: Vocabulary,
}

/// A descriptor's tokens, resolved to masks. Cheap to compare, which is the point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Masks {
    pub kind: u64,
    pub effects: u64,
    pub look: u64,
    /// Surface classes this piece **offers** a top for.
    pub provides: u64,
    /// Classes this piece **presents as a vertical face** — the standing half of [`Self::provides`].
    pub presents: u64,
    /// The surface class this piece **needs** to rest on, if it rests on one at all.
    pub requires: u64,
    /// Every distinct `edge` token anywhere in this piece's lattice, OR'd together.
    pub edges: u64,
}

/// What one actor can do, as a bitmask over the `capabilities` axis.
///
/// Game AI Pro 4 ch.4 on smart-object matching: *"a simple bit-mask can be used to represent the
/// requirements for the link and the capabilities of the agent. Comparing these bitmasks is a very
/// efficient way to filter out invalid links."*
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Can(pub u64);

impl Can {
    /// Whether this actor meets **every** requirement in `needs`. All of them, not any: a role that
    /// wants a cook who can also carry wants both, and `!= 0` would take somebody who can only carry.
    pub fn meets(self, needs: u64) -> bool {
        (self.0 & needs) == needs
    }
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
    /// **Every effect token the given kinds bring with them**, in vocabulary order.
    ///
    /// Refused rather than ignored when an implication names a token the effects axis does not
    /// hold: a vocabulary that promises an effect nobody defines is a promise every consumer would
    /// silently drop, and this crate's rule is that an invented token is refused at the door.
    pub fn implied_effects(&self, kinds: &[String]) -> Result<Vec<String>, String> {
        let mut out: Vec<String> = Vec::new();
        for token in &self.kind.tokens {
            if !kinds.iter().any(|k| k == &token.name) {
                continue;
            }
            for implied in &token.implies {
                if !self.effects.contains(implied) {
                    return Err(format!(
                        "vocabulary: `kind` token `{}` implies `{implied}`, which is not an \
                         `effects` token. The axis holds: {}.",
                        token.name,
                        self.effects.names().collect::<Vec<_>>().join(", ")
                    ));
                }
                if !out.contains(implied) {
                    out.push(implied.clone());
                }
            }
        }
        // Vocabulary order, so a resolved set is the same list however the kinds were written.
        out.sort_by_key(|t| self.effects.names().position(|n| n == t).unwrap_or(usize::MAX));
        Ok(out)
    }

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

        // **Both mounts that name a class, against the one axis.** A top and a face are read
        // differently — `OnSurface` takes the host's height, `OnFace` a distance up it — but the
        // token means the same thing either way, so a second axis would be a second place for
        // `worktop` to be spelled.
        let requires = match &d.mount {
            Some(Mount::OnSurface { class }) | Some(Mount::OnFace { class, .. }) => {
                let how = if matches!(d.mount, Some(Mount::OnFace { .. })) {
                    "is fixed to face class"
                } else {
                    "mounts on surface class"
                };
                self.surfaces.bit(class).ok_or_else(|| {
                    let hint = nearest(&self.surfaces, class)
                        .map(|n| format!(" Did you mean `{n}`?"))
                        .unwrap_or_default();
                    format!(
                        "descriptor `{}`: {how} `{class}`, which is not a `surfaces` token.{hint} \
                         The axis holds: {}.",
                        d.id,
                        self.surfaces.names().collect::<Vec<_>>().join(", ")
                    )
                })?
            }
            _ => 0,
        };

        // **The lattice's own axis.** Gathered from the cells rather than a list on the descriptor,
        // because that is where they are authored — one token per cell, many cells. The mask is the
        // OR of them, which is the useful question ("does this piece say anything about `door`") and
        // the only one that fits a `u64`.
        //
        // One axis, not two, since `SubCell::anchor` retired: a hole belongs to a tile and is checked
        // by `Vocabularies::check_slots`, which asks the compositions rather than the library.
        let mut cell_tokens: Vec<String> = Vec::new();
        if let Some(g) = &d.subgrid {
            for c in &g.cells {
                if let Some(e) = &c.edge
                    && !cell_tokens.contains(e)
                {
                    cell_tokens.push(e.clone());
                }
            }
        }

        // **The effects a kind implies are resolved here, with the authored ones.**
        //
        // One place, so every consumer agrees without remembering to. This was a table in
        // `emerge-mapper`'s labeller that only its apply path consulted — so a bed labelled by the
        // editor gained `stamina-recharge` and the identical bed written by hand did not, and the
        // game, which reads these masks, could not tell the two apart. Deriving it at resolve time
        // means the stored list is an author's *statement* and the mask is the *truth*, which is
        // the only arrangement in which the two cannot drift.
        let implied = self.implied_effects(&d.kind)?;
        let mut effects = d.effects.clone();
        for token in implied {
            if !effects.contains(&token) {
                effects.push(token);
            }
        }

        Ok(Masks {
            kind: axis(&self.kind, "kind", &d.kind)?,
            effects: axis(&self.effects, "effects", &effects)?,
            look: axis(&self.look, "look", &d.look)?,
            provides: axis(&self.surfaces, "surfaces", &d.offers.surfaces)?,
            // Faces share the axis for the reason `requires` gives above.
            presents: axis(&self.surfaces, "surfaces", &d.offers.faces)?,
            requires,
            edges: axis(&self.edge, "edge", &cell_tokens)?,
        })
    }

    /// **Every hole in every tile accepts something this project has declared.**
    ///
    /// Asked of the compositions rather than of the library, because a hole belongs to a *tile*: the
    /// same wall mesh appears in a tile that has a socket in it and one that does not, so there is
    /// nothing on the descriptor to check. That is the whole difference from the `anchor` axis this one
    /// inherited — see [`Vocabularies::slot`].
    ///
    /// Not folded into `composition::validate`: that runs in the project loader, which reads the
    /// library and the policy and never opens the vocabulary. Each check stays where its data already
    /// is rather than threading a file through a function that does not otherwise want it.
    ///
    /// Names the composition **and** the member **and** the axis, for the reason the module docs give:
    /// a bare *"`wall-fixtre` is not a slot"* in a kit with forty tiles is a search rather than an
    /// answer.
    pub fn check_slots(&self, compositions: &[crate::composition::Composition]) -> Result<(), String> {
        for c in compositions {
            for m in &c.members {
                let crate::composition::Body::Slot { accepts } = &m.body else {
                    continue;
                };
                if self.slot.contains(accepts) {
                    continue;
                }
                let axis: Vec<&str> = self.slot.names().collect();
                let holds = if axis.is_empty() {
                    "the axis is empty — declare the first one in `vocab.ron` rather than at a \
                     keyboard"
                        .to_owned()
                } else {
                    format!("the axis holds: {}", axis.join(", "))
                };
                return Err(format!(
                    "composition `{}` slot `{}` accepts `{accepts}`, which is not a `slot` token. \
                     {holds}.",
                    c.id, m.id
                ));
            }
        }
        Ok(())
    }

    /// The capability mask one role demands, or the reason its tokens are not in the axis.
    ///
    /// `where_` names the site — `"galley_table_1/eat"` — because a bare *"`chef` is not a
    /// capability"* in a map with nine locations is a search rather than an answer.
    pub fn role_mask(&self, role: &crate::map::RoleSlot, where_: &str) -> Result<u64, String> {
        self.capabilities.mask(&role.requires).map_err(|_| {
            let bad = role
                .requires
                .iter()
                .find(|t| !self.capabilities.contains(t))
                .map_or_else(String::new, |t| t.clone());
            let hint = nearest(&self.capabilities, &bad)
                .map(|n| format!(" Did you mean `{n}`?"))
                .unwrap_or_default();
            format!(
                "{where_}: role `{}` requires `{bad}`, which is not a `capabilities` token.{hint} \
                 The axis holds: {}.",
                role.name,
                self.capabilities.names().collect::<Vec<_>>().join(", ")
            )
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

/// **Append one token to an axis, preserving every comment in the file.**
///
/// A token's bit is its position (`1 << i`), so this appends and never inserts, reorders, renames or
/// removes — any of those silently re-point every mask already computed. Serializing `Vocabularies`
/// over the file is not an option either: a `to_string_pretty` round-trip deletes all comments, and
/// this file is mostly comments.
///
/// The four axes this accepts are the ones the editor's tag block draws. `capabilities`, `edge` and
/// `slot` are drawn by no UI and stay hand-authored.
pub fn append_token(path: &Path, axis: &str, name: &str, note: &str) -> Result<(), String> {
    const AXES: [&str; 4] = ["kind", "effects", "look", "surfaces"];
    if !AXES.contains(&axis) {
        return Err(format!(
            "vocab: `{axis}` is not an axis the editor draws. The tag block draws {}; \
             `capabilities`, `edge` and `slot` are hand-authored.",
            AXES.join(", ")
        ));
    }
    // Shipped tokens are lowercase with hyphens (`uses-electricity`, `blocks-sight`) — not
    // snake_case, and not anything with an uppercase letter. The rule is `[a-z][a-z0-9-]*`.
    let mut chars = name.chars();
    let ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        return Err(format!(
            "vocab: `{name}` is not a valid token name. Tokens are lowercase letters, digits and \
             hyphens, starting with a letter — like `uses-electricity`."
        ));
    }
    if note.is_empty() {
        return Err("vocab: a token needs a note — a token nobody can describe in a sentence is a \
                     token whose meaning has not been decided yet"
            .to_owned());
    }
    if note.contains('"') || note.contains('\\') || note.chars().any(|c| c.is_control()) {
        return Err(
            "vocab: the note must not contain a quote, a backslash or a control character — those \
             are RON escapes and would break the parse or silently alter the note"
                .to_owned(),
        );
    }

    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let vocab = Vocabularies::parse(&text)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let table = match axis {
        "kind" => &vocab.kind,
        "effects" => &vocab.effects,
        "look" => &vocab.look,
        "surfaces" => &vocab.surfaces,
        _ => unreachable!("checked above"),
    };
    if table.contains(name) {
        return Err(format!(
            "vocab: `{name}` is already a `{axis}` token. The axis holds: {}.",
            table.names().collect::<Vec<_>>().join(", ")
        ));
    }
    if table.tokens.len() >= MAX_TOKENS {
        return Err(format!(
            "vocab: `{axis}` already holds {} tokens, the most the mask can pack. Widen the mask \
             deliberately or split the axis — do not let the 65th token silently alias the 1st.",
            table.tokens.len()
        ));
    }

    // **Locate THAT axis's `tokens: [` block** — not the first one in the file. `find_block_value`
    // finds the axis's `( ... )` by name, and `LineDoc` then scans the `tokens: [` list inside it,
    // so `look` splices into `look` even though `kind`'s list appears first.
    let span = crate::ron_surgery::find_block_value(&text, axis)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut doc = crate::ron_surgery::LineDoc::parse(&text[span.start..span.end], &["tokens"])
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let record = format!("            ( name: \"{name}\",  note: \"{note}\" ),");
    doc.append("tokens", record)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let out = format!("{}{}{}", &text[..span.start], doc.render(), &text[span.end..]);

    // **Validate-then-write**, the order `bind_kit` uses: a splice that produced something the
    // project could not read is an error here rather than a broken vocabulary on disk.
    Vocabularies::parse(&out).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::ron_surgery::save_atomic(path, &out)
}

/// The closest token by edit distance, when it is close enough to be worth suggesting.
///
/// Bounded at a third of the token's length: suggesting `light` for `worktop` would be noise, and a
/// wrong hint is worse than none because it sends the reader to the wrong table.
/// Public because the editor's VLM labeler reuses the same did-you-mean in its early gate — one
/// spelling of the hint, whether the bad token came from a hand or a model.
pub fn nearest<'a>(v: &'a Vocabulary, bad: &str) -> Option<&'a str> {
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
            edge: Vocabulary::of(&[
                ("wall", "a solid run-face"),
                ("door", "an opening that must stay clear"),
            ]),
            slot: Vocabulary::of(&[("shelf-item", "something small standing on a shelf")]),
            surfaces: Vocabulary::of(&[
                ("support", "any support top"),
                ("worktop", "a desk or table top"),
            ]),
            capabilities: Vocabulary::of(&[
                ("eat", "can take a meal"),
                ("cook", "can prepare food"),
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
                    note: String::new(),
                    implies: Vec::new(),
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

    /// **A hole accepts something the project declared**, refused at open and naming all three of
    /// the composition, the member and the axis — because a bare *"`wall-fixtre` is not a slot"* in a
    /// kit of forty tiles is a search rather than an answer.
    #[test]
    fn a_slot_accepting_an_undeclared_token_is_refused_by_name() {
        use crate::composition::{Body, Composition, Envelope, Member};
        let tile = |accepts: &str| Composition {
            id: "site/tile_wall_n".to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 1.0, 1.0) },
            members: vec![Member {
                id: "fixture".to_owned(),
                body: Body::Slot { accepts: accepts.to_owned() },
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
        let mut v = vocabs();
        v.slot = Vocabulary::of(&[("wall-fixture", "something bolted to a wall")]);

        v.check_slots(&[tile("wall-fixture")]).expect("a declared token passes");

        let err = v.check_slots(&[tile("wall-fixtre")]).err().unwrap_or_default();
        assert!(err.contains("site/tile_wall_n"), "{err}");
        assert!(err.contains("fixture"), "{err}");
        assert!(err.contains("wall-fixture"), "the message must list the axis: {err}");
    }

    /// A temp file for the splice tests, unique per test name and process.
    fn temp_vocab(name: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("emerge-vocab-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("cannot make {dir:?}: {e}"));
        let path = dir.join("vocab.ron");
        std::fs::write(&path, text).unwrap_or_else(|e| panic!("cannot write {path:?}: {e}"));
        path
    }

    /// A vocabulary with a `//` comment above a token in EVERY axis, so a scanner that naively
    /// finds the first `tokens: [` passes on `kind` and splices into the wrong list for `look`.
    const COMMENTED: &str = r#"(
    kind: (
        // the kind comment
        tokens: [
            ( name: "seating",  note: "a thing to sit on" ),
        ],
    ),
    effects: (
        // the effects comment
        tokens: [
            ( name: "emit",  note: "casts light" ),
        ],
    ),
    look: (
        // the look comment
        tokens: [
            ( name: "brown",  note: "" ),
        ],
    ),
    surfaces: (
        // the surfaces comment
        tokens: [
            ( name: "support",  note: "any support top" ),
        ],
    ),
)
"#;

    /// **Comments survive a token append, on every axis.** Parameterising over all four is the
    /// point: a scanner that naively finds the first `tokens: [` passes on `kind` and splices into
    /// the wrong list for `look`.
    #[test]
    fn appending_a_token_keeps_every_comment_on_every_axis() {
        for (axis, token, note) in [
            ("kind", "table", "a thing with a top"),
            ("effects", "uses-electricity", "stops working when the power does"),
            ("look", "metal", "bare metal"),
            ("surfaces", "worktop", "a desk or table top"),
        ] {
            let path = temp_vocab(axis, COMMENTED);
            append_token(&path, axis, token, note).unwrap_or_else(|e| panic!("{axis}: {e}"));
            let out = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));

            // The comment above the token in THIS axis survives.
            let comment = format!("// the {axis} comment");
            assert!(out.contains(&comment), "{axis}: comment lost:\n{out}");

            // The new record is the LAST in this axis, and no other axis gained a record.
            let v = Vocabularies::parse(&out).unwrap_or_else(|e| panic!("{axis}: {e}"));
            let table = match axis {
                "kind" => &v.kind,
                "effects" => &v.effects,
                "look" => &v.look,
                "surfaces" => &v.surfaces,
                _ => unreachable!(),
            };
            assert_eq!(
                table.tokens.last().map(|t| t.name.as_str()),
                Some(token),
                "{axis}: new token must be last"
            );
            assert_eq!(table.tokens.len(), 2, "{axis}: exactly one record added");
            for (other, other_table) in [
                ("kind", &v.kind),
                ("effects", &v.effects),
                ("look", &v.look),
                ("surfaces", &v.surfaces),
            ] {
                if other != axis {
                    assert_eq!(
                        other_table.tokens.len(),
                        1,
                        "{axis}: `{other}` must not gain a record:\n{out}"
                    );
                }
            }
        }
    }

    /// **Append-only is enforced.** Every refusal leaves the file byte-identical.
    #[test]
    fn a_refused_append_leaves_the_file_untouched() {
        let cases: Vec<(&str, &str, &str)> = vec![
            // Duplicate name.
            ("kind", "seating", "a duplicate"),
            // Empty note.
            ("kind", "table", ""),
            // A quote is a RON escape.
            ("kind", "table", "a \"quoted\" note"),
            // A backslash is a RON escape.
            ("kind", "table", "a \\ note"),
            // A newline is a control character.
            ("kind", "table", "two\nlines"),
            // An axis no UI draws.
            ("capabilities", "eat", "can take a meal"),
            // An uppercase letter.
            ("kind", "Table", "a thing with a top"),
        ];
        for (axis, name, note) in cases {
            let path = temp_vocab(&format!("{axis}-{name}"), COMMENTED);
            let before = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
            let err = append_token(&path, axis, name, note)
                .err()
                .unwrap_or_else(|| panic!("`{name}` on `{axis}` should have been refused"));
            assert!(!err.is_empty(), "a refusal must say something");
            let after = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(before, after, "`{name}` on `{axis}` must not write: {err}");
        }
    }
}
