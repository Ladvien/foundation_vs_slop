//! **Config-bake machinery: RON splicing and golden re-pinning.**
//!
//! Extracted from `bin/train.rs` (2,569 lines) into the library so it is reusable and testable outside
//! the binary (FVS-N-4). A pure move — no logic changed.
//!
//! Why this is worth its own reviewed unit: `train apply` both **changes the sim** and **moves the
//! ruler** that measures it, and `TESTING.md` is explicit that a tool doing both in one step cannot be
//! reviewed. The safety properties live here — `splice_block` rewrites only the scalars that actually
//! changed (comments, ordering and formatting survive), and `repin_one` refuses to re-pin a golden it
//! cannot uniquely identify, so a bake that would silently move a committed hash stops instead.

/// The shipped config a bake splices into.
pub const CONFIG_PATH: &str = "assets/config/config.ron";
/// The replay test file whose committed golden constants a bake may re-pin.
pub const REPLAY_PATH: &str = "tests/replay.rs";

/// Serialize a config slice to RON in config.ron's anonymous-tuple style (`struct_names: false`).
pub fn ron_slice<T: serde::Serialize>(v: &T) -> Result<String, String> {
    ron::ser::to_string_pretty(v, ron::ser::PrettyConfig::default()).map_err(|e| format!("serialize slice: {e}"))
}

/// A scalar leaf found in RON source text: where it lives in the tree, and the exact byte span of its value
/// token so it can be substituted without disturbing anything around it.
pub struct Leaf {
    /// Dotted field path from the block root, with `[i]` for sequence elements (`room_types[2].weight`).
    pub path: String,
    /// Byte range of the scalar token within the scanned text.
    pub span: std::ops::Range<usize>,
    /// The scalar token's source text (`0x5C09191`, `2`, `1.0`, `"bathroom"`, `true`, `None`).
    pub text: String,
}

/// Scan RON source into its scalar leaves, tracking the path to each and the byte span of its value token.
///
/// This is deliberately a *source* scanner, not a deserializer: it must know where each scalar sits in the
/// original bytes so `splice_block` can substitute one number and leave every comment, alignment, and
/// literal spelling around it untouched. It skips `//`, `/* */`, and string contents — which is also why the
/// old paren-counting block scan was wrong (it counted `(` inside comments and strings).
pub fn scan_ron_leaves(text: &str) -> Result<Vec<Leaf>, String> {
    let b = text.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();

    // Advance past whitespace and comments.
    fn skip(b: &[u8], mut i: usize) -> usize {
        loop {
            while i < b.len() && (b[i] as char).is_whitespace() {
                i += 1;
            }
            if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(b.len());
                continue;
            }
            return i;
        }
    }

    // An identifier (field name, enum variant, `Some`, `None`, `true`).
    fn ident(b: &[u8], mut i: usize) -> (usize, String) {
        let s = i;
        while i < b.len() && ((b[i] as char).is_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        (i, String::from_utf8_lossy(&b[s..i]).into_owned())
    }

    // Recursive-descent over one value. `path` is the path to THIS value.
    #[allow(clippy::too_many_arguments)]
    fn value(
        b: &[u8],
        text: &str,
        mut i: usize,
        path: &str,
        out: &mut Vec<Leaf>,
        depth: usize,
    ) -> Result<usize, String> {
        if depth > 32 {
            return Err("config.ron: value nested deeper than 32 — refusing".to_string());
        }
        i = skip(b, i);
        if i >= b.len() {
            return Err(format!("config.ron: unexpected end of input at `{path}`"));
        }

        // `Some(...)` / `None` — an Option wrapper is transparent to the path.
        if (b[i] as char).is_alphabetic() {
            let (after, id) = ident(b, i);
            let j = skip(b, after);
            if id == "Some" && j < b.len() && b[j] == b'(' {
                i = value(b, text, j + 1, path, out, depth + 1)?;
                i = skip(b, i);
                if i >= b.len() || b[i] != b')' {
                    return Err(format!("config.ron: unclosed `Some(` at `{path}`"));
                }
                return Ok(i + 1);
            }
            // A named struct/enum like `Grid` or `Foo(...)`: if a `(` follows, descend; else it's a scalar.
            if j < b.len() && b[j] == b'(' {
                return strukt(b, text, j + 1, path, out, depth + 1);
            }
            out.push(Leaf { path: path.to_string(), span: i..after, text: id });
            return Ok(after);
        }

        match b[i] {
            b'(' => strukt(b, text, i + 1, path, out, depth + 1),
            b'[' => {
                let mut n = 0usize;
                i += 1;
                loop {
                    i = skip(b, i);
                    if i >= b.len() {
                        return Err(format!("config.ron: unclosed `[` at `{path}`"));
                    }
                    if b[i] == b']' {
                        return Ok(i + 1);
                    }
                    i = value(b, text, i, &format!("{path}[{n}]"), out, depth + 1)?;
                    n += 1;
                    i = skip(b, i);
                    if i < b.len() && b[i] == b',' {
                        i += 1;
                    }
                }
            }
            b'"' => {
                let s = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(b.len());
                out.push(Leaf {
                    path: path.to_string(),
                    span: s..i,
                    text: text[s..i].to_string(),
                });
                Ok(i)
            }
            _ => {
                // A bare scalar: run to the next delimiter. Comments never start mid-token in RON.
                let s = i;
                while i < b.len() && !matches!(b[i], b',' | b')' | b']' | b'\n') {
                    i += 1;
                }
                let raw = text[s..i].trim_end();
                out.push(Leaf {
                    path: path.to_string(),
                    span: s..s + raw.len(),
                    text: raw.to_string(),
                });
                Ok(s + raw.len())
            }
        }
    }

    // A struct body, positioned just after its `(`. Fields are `key: value`; bare values are tuple elements.
    fn strukt(
        b: &[u8],
        text: &str,
        mut i: usize,
        path: &str,
        out: &mut Vec<Leaf>,
        depth: usize,
    ) -> Result<usize, String> {
        let mut tuple_idx = 0usize;
        loop {
            i = skip(b, i);
            if i >= b.len() {
                return Err(format!("config.ron: unclosed `(` at `{path}`"));
            }
            if b[i] == b')' {
                return Ok(i + 1);
            }
            // Field or tuple element? A `key:` has an identifier then a colon.
            let mut child = format!("{path}[{tuple_idx}]");
            if (b[i] as char).is_alphabetic() || b[i] == b'_' {
                let (after, id) = ident(b, i);
                let j = skip(b, after);
                if j < b.len() && b[j] == b':' {
                    child = if path.is_empty() { id } else { format!("{path}.{id}") };
                    i = j + 1;
                } else {
                    tuple_idx += 1;
                }
            } else {
                tuple_idx += 1;
            }
            i = value(b, text, i, &child, out, depth + 1)?;
            i = skip(b, i);
            if i < b.len() && b[i] == b',' {
                i += 1;
            }
        }
    }

    i = skip(b, i);
    if i >= b.len() || b[i] != b'(' {
        return Err("config.ron: block value does not start with `(`".to_string());
    }
    let end = strukt(b, text, i + 1, "", &mut out, 0)?;
    let tail = skip(b, end);
    if tail < b.len() {
        return Err(format!("config.ron: trailing input after block value: {:?}", &text[tail..]));
    }
    Ok(out)
}

/// Do two RON scalar tokens denote the same value? Compares by PARSED value, not by spelling — so the
/// authored `seed: 0x5C09191` and the serializer's `96506257` are equal, and the authored line is left
/// alone (hex spelling, alignment, and its `// nods to SCP-9191` comment all preserved).
pub fn scalar_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (ron::from_str::<ron::Value>(a), ron::from_str::<ron::Value>(b)) {
        (Ok(x), Ok(y)) => x == y,
        // Unparseable on either side: fall back to exact text. Never treat "can't tell" as "equal".
        _ => false,
    }
}

/// Locate the `<name>: ( … )` block in `config.ron`, returning the byte span of its VALUE (the `( … )`).
/// The scan is comment- and string-aware, unlike the old raw `(`/`)` char count.
pub fn find_block_value(text: &str, name: &str) -> Result<std::ops::Range<usize>, String> {
    let header = format!("{name}: (");
    let mut search = 0usize;
    let at = loop {
        let rel = text[search..]
            .find(&header)
            .ok_or_else(|| format!("config.ron: no `{name}:` block header"))?;
        let abs = search + rel;
        // Must be a real field, not a substring of a longer name (`density` inside `foo_density`) and not
        // inside a comment.
        let line_start = text[..abs].rfind('\n').map(|n| n + 1).unwrap_or(0);
        let prefix = &text[line_start..abs];
        let in_comment = prefix.contains("//");
        let boundary = prefix.chars().last().is_none_or(|c| c.is_whitespace());
        if !in_comment && boundary {
            break abs;
        }
        search = abs + header.len();
    };
    let open = at + name.len() + 2; // past `name:`, onto the ` (`
    let open = text[open..].find('(').map(|o| open + o).ok_or("config.ron: malformed header")?;
    // Reuse the scanner to find the matching close: scan the tail and take the end it reports.
    let tail = &text[open..];
    let mut depth = 0usize;
    let b = tail.as_bytes();
    let (mut i, mut in_str, mut in_line_comment, mut in_block_comment) = (0usize, false, false, false);
    while i < b.len() {
        let c = b[i];
        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
        } else if in_block_comment {
            if c == b'*' && i + 1 < b.len() && b[i + 1] == b'/' {
                in_block_comment = false;
                i += 1;
            }
        } else if in_str {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            in_line_comment = true;
            i += 1;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            in_block_comment = true;
            i += 1;
        } else if c == b'"' {
            in_str = true;
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return Ok(open..open + i + 1);
            }
        }
        i += 1;
    }
    Err(format!("config.ron: unbalanced `{name}:` block"))
}

/// Rewrite the `<name>: ( … )` block in `config.ron` to hold `value_ron`, **substituting only the scalars
/// that actually changed** and leaving every other byte — comments, alignment, literal spelling — exactly
/// as authored.
///
/// This replaced a `to_string_pretty` splice that overwrote the whole block with one generated line. That
/// destroyed every comment inside it: on 2026-07-16 a `--dim levels` bake stripped ~279 lines of
/// hand-written rationale from `config.ron` (which carries ~563 comment lines — the reasoning IS the file).
///
/// **It refuses rather than guessing.** If the bake changes the block's *shape* — a dropped/added field, a
/// different sequence length, an `Option` appearing or vanishing — there is no honest edit: the prose around
/// the old shape describes a design the elite no longer has, so preserving it would leave the file
/// confidently lying. `--dim levels` can do exactly this (`level_genome::decode` drops unselected
/// `room_types`), and that bake now errors with the path instead of quietly rewriting the block.
///
/// `before_ron` and `after_ron` are the SAME slice serialized before and after the elite was applied. The
/// diff is taken between those two — never between the authored *text* and the baked value — because the
/// authored text legitimately omits `#[serde(default)]` fields (`room_types[…].expands` is written only on
/// the types that set it). Diffing text against a serializer that always spells every field out would read
/// those omissions as a shape change. Two serializations of one type cannot disagree that way, so what is
/// left is real: a value moved, a sequence grew or shrank, an `Option` flipped.
pub fn splice_block(text: &str, name: &str, before_ron: &str, after_ron: &str) -> Result<String, String> {
    let span = find_block_value(text, name)?;
    let authored = scan_ron_leaves(&text[span.clone()])
        .map_err(|e| format!("{name}: reading the authored block: {e}"))?;
    let before = scan_ron_leaves(before_ron).map_err(|e| format!("{name}: reading the pre-bake value: {e}"))?;
    let after = scan_ron_leaves(after_ron).map_err(|e| format!("{name}: reading the baked value: {e}"))?;

    // Shape check: before vs after. Same Rust type through the same serializer, so any path difference is a
    // genuine structural change (sequence length, `Option` variant), not a defaulted-field omission.
    let (bp, ap): (Vec<&str>, Vec<&str>) = (
        before.iter().map(|l| l.path.as_str()).collect(),
        after.iter().map(|l| l.path.as_str()).collect(),
    );
    if bp != ap {
        let dropped: Vec<&&str> = bp.iter().filter(|p| !ap.contains(p)).take(4).collect();
        let added: Vec<&&str> = ap.iter().filter(|p| !bp.contains(p)).take(4).collect();
        return Err(format!(
            "cannot splice `{name}`: the elite changes the block's SHAPE, not just its values \
             ({} leaves before, {} after).\n  \
               dropped: {dropped:?}\n  \
               added:   {added:?}\n\
             \nRewriting it would mean deleting or inventing lines, and the hand-written rationale around \
             them describes the AUTHORED design — preserving those comments would leave them describing a \
             structure that no longer exists, which is worse than deleting them, because a stale comment \
             reads as authoritative.\n\
             \nThis is expected for `--dim levels` when an elite drops a room type (`level_genome::decode` \
             skips unselected `room_types`). Ship that elite through the runtime overlay \
             (`FVS_LEVELS_ELITE`, see src/elite_overlay.rs), or hand-edit the block and its prose together.",
            bp.len(),
            ap.len(),
        ));
    }

    // Where the authored text actually spells each field out.
    let placed: std::collections::HashMap<&str, &Leaf> =
        authored.iter().map(|l| (l.path.as_str(), l)).collect();

    // The changed leaves, and where each one lives in the authored text.
    let mut edits: Vec<(std::ops::Range<usize>, &str)> = Vec::new();
    let mut unplaceable: Vec<String> = Vec::new();
    for (b, a) in before.iter().zip(&after) {
        if scalar_eq(&b.text, &a.text) {
            continue;
        }
        match placed.get(a.path.as_str()) {
            Some(l) => edits.push((l.span.clone(), a.text.as_str())),
            // The value moved, but the authored file never writes this field — it rides on a serde default.
            // Adding the line means choosing where to put it and what to say about it. Refuse.
            None => unplaceable.push(format!("{} ({} -> {})", a.path, b.text, a.text)),
        }
    }
    if !unplaceable.is_empty() {
        return Err(format!(
            "cannot splice `{name}`: the elite changes {} field(s) the authored block does not spell out \
             (they sit at their `#[serde(default)]` value):\n  {}\n\
             \nWriting them would mean inserting lines into hand-authored prose and deciding, on your \
             behalf, where they go and what they mean. Hand-edit the block, or ship the elite through the \
             runtime overlay (see src/elite_overlay.rs).",
            unplaceable.len(),
            unplaceable.join("\n  "),
        ));
    }

    // Apply right-to-left so earlier spans stay valid.
    // SORT-OK: byte spans in one file, unique by construction — offline tooling, not an ECS query.
    edits.sort_by_key(|(s, _)| std::cmp::Reverse(s.start));
    let mut out = text.to_string();
    for (s, new_text) in &edits {
        out.replace_range(span.start + s.start..span.start + s.end, new_text);
    }
    println!(
        "  {name}: {} value(s) changed, {} unchanged (comments preserved)",
        edits.len(),
        before.len() - edits.len()
    );
    Ok(out)
}

pub const SNAP_MARKER: &str = "const GOLDEN: u64 = ";
pub const FIELD_MARKER: &str = "const GOLDEN_FIELD: u64 = ";

/// The goldens currently committed in `tests/replay.rs`, as `(snapshot, field)`.
pub fn read_committed_goldens() -> Result<(u64, u64), String> {
    let text = std::fs::read_to_string(REPLAY_PATH).map_err(|e| format!("{REPLAY_PATH}: {e}"))?;
    Ok((parse_hex(&extract_hex(&text, SNAP_MARKER)?)?, parse_hex(&extract_hex(&text, FIELD_MARKER)?)?))
}

/// Parse a `0x…` u64 literal, tolerating RON/Rust `_` digit separators (`0xe1ec_dc58_3c8d_bfca`).
pub fn parse_hex(lit: &str) -> Result<u64, String> {
    let cleaned: String = lit.trim().trim_start_matches("0x").chars().filter(|c| *c != '_').collect();
    u64::from_str_radix(&cleaned, 16).map_err(|e| format!("{REPLAY_PATH}: bad golden literal `{lit}`: {e}"))
}

/// Re-pin the replay goldens in `tests/replay.rs`.
///
/// **Scoped to the two `const` declarations, deliberately.** This used to `str::replace` the old hex across
/// the WHOLE file, which had two failure modes: (1) `replay.rs`'s header is an archaeology log that quotes
/// prior hashes in prose on purpose, and any that matched the current value were silently rewritten — the
/// tool ate its own audit trail; (2) it existed only to keep a duplicated literal in
/// `authored_world_config_override_is_a_noop` in step, and that duplicate is now a reference to `GOLDEN`,
/// so there is exactly one declaration site per golden. Rewriting anything beyond these two statements is
/// out of scope by construction.
pub fn repin_replay(snap: u64, field: u64) -> Result<(), String> {
    let text = std::fs::read_to_string(REPLAY_PATH).map_err(|e| format!("{REPLAY_PATH}: {e}"))?;
    let text = repin_one(&text, SNAP_MARKER, snap)?;
    let text = repin_one(&text, FIELD_MARKER, field)?;
    std::fs::write(REPLAY_PATH, text).map_err(|e| format!("{REPLAY_PATH}: write: {e}"))
}

/// Replace the literal in the `<marker><hex>;` declaration for **this** architecture, leaving every
/// other byte untouched.
///
/// # Why two declarations can be one golden
///
/// Goldens are **per-platform** (the 2026-07-27 Director decision: `f32` gameplay math is not
/// identical across instruction sets, so one hash cannot hold on both x86-64 and aarch64). Each
/// golden is therefore written as a `cfg(target_arch)`-selected pair, and the marker literally
/// appears twice — which this function used to reject as ambiguity, failing every
/// `train apply --repin-goldens` at the re-pin step (FVS-J-7's sibling, FVS-J-8).
///
/// **The duplicate check is kept, not deleted.** Two *unconditional* declarations of one golden is
/// still an error; what is now understood is that `cfg`-selected alternatives are one logical
/// declaration with one live arm. A bake measures on the machine it runs on, so the arm re-pinned is
/// the arm that was measured — re-pinning the other would be inventing a number for a platform this
/// process never ran on, which is precisely what the aarch64 arm's deliberate `0` refuses to do.
pub fn repin_one(text: &str, marker: &str, value: u64) -> Result<String, String> {
    let sites: Vec<usize> = text.match_indices(marker).map(|(i, _)| i).collect();
    let at = match sites.len() {
        0 => return Err(format!("{REPLAY_PATH}: no `{marker}`")),
        1 => sites[0],
        _ => {
            // Each site must be cfg-gated, and exactly one gate may be live for this target.
            let live: Vec<usize> =
                sites.iter().copied().filter(|&at| cfg_arm_is_live_here(text, at)).collect();
            match live.len() {
                1 => live[0],
                0 if !sites.iter().any(|&at| is_cfg_gated(text, at)) => {
                    // Plain duplication — the ambiguity the check was built for, and still an error.
                    return Err(format!(
                        "{REPLAY_PATH}: `{marker}` declared more than once — a golden has one home"
                    ));
                }
                0 => {
                    return Err(format!(
                        "{REPLAY_PATH}: `{marker}` is declared {} times, all `cfg`-gated, and NONE \
                         is selected for this target ({}). A declaration this build cannot see is \
                         one no bake can re-pin — measure on that platform and pin it there.",
                        sites.len(),
                        std::env::consts::ARCH,
                    ));
                }
                n => {
                    return Err(format!(
                        "{REPLAY_PATH}: `{marker}` has {n} declarations live at once for this \
                         target ({}) — a golden has one home. Unconditional duplicates are a real \
                         ambiguity; only `cfg(target_arch)`-selected alternatives are one golden.",
                        std::env::consts::ARCH,
                    ));
                }
            }
        }
    };
    let val_start = at + marker.len();
    let end = text[val_start..]
        .find(';')
        .ok_or_else(|| format!("{REPLAY_PATH}: unterminated `{marker}`"))?;
    Ok(format!("{}0x{value:016x}{}", &text[..val_start], &text[val_start + end..]))
}

/// Does the `#[cfg(...)]` attribute immediately above the declaration at `at` select it on the
/// architecture this binary is running on?
///
/// Deliberately narrow: it understands `target_arch` and a single `not(..)` wrapper, which is the
/// whole vocabulary the per-platform goldens use. Anything richer returns `false` and surfaces as the
/// "none is selected" error above — an honest refusal beats a guess about which hash to overwrite.
fn cfg_arm_is_live_here(text: &str, at: usize) -> bool {
    let Some(attr) = preceding_attr(text, at) else {
        return false;
    };
    if !attr.starts_with("#[cfg(") {
        return false;
    }
    let this_arch = format!("target_arch = \"{}\"", std::env::consts::ARCH);
    let mentions_this_arch = attr.contains(&this_arch);
    let negated = attr.contains("not(");
    // `cfg(target_arch = "x86_64")` on x86_64 → live. `cfg(not(target_arch = "x86_64"))` → live only
    // when this is NOT that arch.
    if !attr.contains("target_arch") {
        return false;
    }
    mentions_this_arch != negated
}

/// The nearest non-comment, non-blank line above `at` — where a `#[cfg(..)]` on the declaration sits.
fn preceding_attr(text: &str, at: usize) -> Option<&str> {
    text[..at]
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("///") && !l.starts_with("//"))
}

/// Is this declaration `cfg`-gated at all? Distinguishes "per-platform pair" from plain duplication.
fn is_cfg_gated(text: &str, at: usize) -> bool {
    preceding_attr(text, at).is_some_and(|a| a.starts_with("#[cfg("))
}

/// Extract the hex literal after a `const X: u64 = ` marker, up to the `;`.
pub fn extract_hex(text: &str, marker: &str) -> Result<String, String> {
    let at = text.find(marker).ok_or_else(|| format!("{REPLAY_PATH}: no `{marker}`"))?;
    let rest = &text[at + marker.len()..];
    let end = rest.find(';').ok_or_else(|| format!("{REPLAY_PATH}: unterminated `{marker}`"))?;
    Ok(rest[..end].trim().to_string())
}

/// Pretty RON so an elite is a reviewable diff, not one long line.
pub fn write_ron<T: serde::Serialize>(path: &str, value: &T) -> Result<(), String> {
    let text = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("{path}: serialize: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("{path}: write: {e}"))
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A miniature of `config.ron`'s real shape: prose above a field, an inline trailing comment, a hex
    /// literal, a nested one-line struct, an `Option`, and a sequence of tuple structs.
    const BLOCK: &str = r#"config: (
    other: (
        keep: 1,
    ),
    dungeon: (
        coarse_w: 6,
        corridor_width: 2,          // minimum corridor width (block-centre lane + 1 more)
        seed: 0x5C09191,            // nods to SCP-9191, the slop generator

        // Liminality dial: 1.0 = sparse Backrooms boxes adrift in the void.
        liminality: 1.0,
        notch: Some((
            chance: 0.8,
        )),
        wfc_weights: (
            rock: 3.0, dead_end: 1.2,
        ),
        room_types: [
            ( tag: "bathroom", weight: 0.8 ),
            ( tag: "hall",     weight: 1.0 ),
        ],
    ),
    )
    "#;

    /// The `before` arm: `BLOCK`'s dungeon values as the serializer spells them — every field present,
    /// including the ones the authored text leaves at their serde default.
    const BEFORE: &str = r#"(coarse_w: 6, corridor_width: 2, seed: 96506257, liminality: 1.0,
        notch: Some((chance: 0.8)), wfc_weights: (rock: 3.0, dead_end: 1.2),
        room_types: [(tag: "bathroom", weight: 0.8), (tag: "hall", weight: 1.0)])"#;

    #[test]
    fn splice_preserves_comments_and_touches_only_changed_scalars() {
        // corridor_width 2 -> 3 and rock 3.0 -> 4.5; everything else identical.
        let baked = r#"(coarse_w: 6, corridor_width: 3, seed: 96506257, liminality: 1.0,
            notch: Some((chance: 0.8)), wfc_weights: (rock: 4.5, dead_end: 1.2),
            room_types: [(tag: "bathroom", weight: 0.8), (tag: "hall", weight: 1.0)])"#;
        let out = splice_block(BLOCK, "dungeon", BEFORE, baked).expect("splice");

        // The rationale survives, verbatim.
        assert!(out.contains("// minimum corridor width (block-centre lane + 1 more)"));
        assert!(out.contains("// nods to SCP-9191, the slop generator"));
        assert!(out.contains("// Liminality dial: 1.0 = sparse Backrooms boxes adrift in the void."));
        // The changed scalars moved.
        assert!(out.contains("corridor_width: 3,"), "corridor_width not updated:\n{out}");
        assert!(out.contains("rock: 4.5,"), "nested one-line struct not updated:\n{out}");
        // `seed` is semantically unchanged (0x5C09191 == 96506257), so its line is byte-identical — the hex
        // spelling and its alignment survive. This is the whole point of comparing values, not text.
        assert!(out.contains("seed: 0x5C09191,            // nods to SCP-9191"), "seed line was disturbed:\n{out}");
        // Untouched neighbours stay put.
        assert!(out.contains("coarse_w: 6,"));
        assert!(out.contains("keep: 1,"));
        assert!(out.contains("liminality: 1.0,"));
    }

    #[test]
    fn splice_refuses_a_dropped_sequence_entry() {
        // `--dim levels` dropping a room type: 2 room_types -> 1.
        let baked = r#"(coarse_w: 6, corridor_width: 2, seed: 96506257, liminality: 1.0,
            notch: Some((chance: 0.8)), wfc_weights: (rock: 3.0, dead_end: 1.2),
            room_types: [(tag: "hall", weight: 1.0)])"#;
        let err = splice_block(BLOCK, "dungeon", BEFORE, baked).expect_err("must refuse a shape change");
        assert!(err.contains("SHAPE"), "{err}");
        assert!(err.contains("FVS_LEVELS_ELITE"), "the error must name the one-path alternative: {err}");
    }

    #[test]
    fn splice_refuses_a_vanished_option() {
        // `notch: Some(( … ))` -> `None` removes the `notch.chance` leaf: a shape change, not a value change.
        let baked = r#"(coarse_w: 6, corridor_width: 2, seed: 96506257, liminality: 1.0,
            notch: None, wfc_weights: (rock: 3.0, dead_end: 1.2),
            room_types: [(tag: "bathroom", weight: 0.8), (tag: "hall", weight: 1.0)])"#;
        let err = splice_block(BLOCK, "dungeon", BEFORE, baked).expect_err("must refuse a vanished Option");
        assert!(err.contains("SHAPE"), "{err}");
    }

    /// The old paren scan counted raw `(`/`)` anywhere, including inside comments and strings — so a lone
    /// paren in prose mis-located the block's end and mangled the file.
    #[test]
    fn block_scan_ignores_parens_in_comments_and_strings() {
        let text = r#"root: (
    dungeon: (
        // a smiley :) and an unbalanced ( paren in prose
        tag: "a ) string with ( parens",
        n: 1,
    ),
    after: (
        untouched: 7,
    ),
    )
    "#;
        let before = r#"(tag: "a ) string with ( parens", n: 1)"#;
        let baked = r#"(tag: "a ) string with ( parens", n: 2)"#;
        let out = splice_block(text, "dungeon", before, baked).expect("splice past the decoy parens");
        assert!(out.contains("n: 2,"), "{out}");
        assert!(out.contains("// a smiley :) and an unbalanced ( paren in prose"));
        // The block ended where it should: the sibling below is intact.
        assert!(out.contains("untouched: 7,"), "block end mis-located:\n{out}");
    }

    #[test]
    fn scalar_eq_compares_values_not_spelling() {
        assert!(scalar_eq("0x5C09191", "96506257"), "hex and decimal are the same number");
        assert!(scalar_eq("1.0", "1.0"));
        assert!(!scalar_eq("1.0", "1.5"));
        assert!(!scalar_eq("2", "3"));
    }

    /// `repin_replay` must touch ONLY the const declaration — `replay.rs`'s header quotes prior hashes in
    /// prose deliberately, and the old unbounded `str::replace` rewrote those too.
    #[test]
    fn repin_one_leaves_prose_hashes_alone() {
        let src = "// Was `0xdeadbeefdeadbeef`. See the log.\nconst GOLDEN: u64 = 0xdeadbeefdeadbeef;\n";
        let out = repin_one(src, SNAP_MARKER, 0x1234_5678_9abc_def0).expect("repin");
        assert!(out.contains("// Was `0xdeadbeefdeadbeef`. See the log."), "prose was rewritten:\n{out}");
        assert!(out.contains("const GOLDEN: u64 = 0x123456789abcdef0;"), "{out}");
    }

    #[test]
    fn repin_one_rejects_a_duplicated_golden() {
        let src = "const GOLDEN: u64 = 0x1;\nconst GOLDEN: u64 = 0x2;\n";
        let err = repin_one(src, SNAP_MARKER, 9).expect_err("two homes for one golden must be an error");
        assert!(err.contains("one home"), "{err}");
    }

    /// The per-platform golden shape (FVS-J-8): the marker appears twice, but only one arm is live,
    /// so this is ONE golden and the bake must re-pin the arm it actually measured on.
    #[test]
    fn repin_one_repins_only_the_live_cfg_arm() {
        let this = std::env::consts::ARCH;
        let src = format!(
            "#[cfg(target_arch = \"{this}\")]\nconst GOLDEN: u64 = 0x1111111111111111;\n\n\
             /// Not yet measured on the other architecture.\n\
             #[cfg(not(target_arch = \"{this}\"))]\nconst GOLDEN: u64 = 0;\n"
        );
        let out = repin_one(&src, SNAP_MARKER, 0xabcd_ef01_2345_6789).expect("cfg pair is one golden");
        assert!(out.contains("const GOLDEN: u64 = 0xabcdef0123456789;"), "live arm not re-pinned:\n{out}");
        assert!(
            out.contains("#[cfg(not(target_arch = \"") && out.contains("const GOLDEN: u64 = 0;"),
            "the arm for the OTHER platform must be left alone — this bake never ran there:\n{out}"
        );
    }

    /// The real-file guard: `train apply --repin-goldens` must be able to re-pin the goldens as they
    /// are actually written today. This is the assertion that was red — the tool refused the shipped
    /// file shape — so it is pinned against the file rather than a fixture.
    #[test]
    fn the_shipped_replay_goldens_are_repinnable() {
        let text = std::fs::read_to_string(REPLAY_PATH).expect("read tests/replay.rs");
        for marker in [SNAP_MARKER, FIELD_MARKER] {
            let out = repin_one(&text, marker, 0xdead_beef_dead_beef)
                .unwrap_or_else(|e| panic!("`{marker}` is not re-pinnable in the shipped file: {e}"));
            assert!(
                out.contains(&format!("{marker}0xdeadbeefdeadbeef;")),
                "`{marker}` re-pin did not land"
            );
        }
    }

    #[test]
    fn parse_hex_accepts_ron_digit_separators() {
        assert_eq!(parse_hex("0xe1ec_dc58_3c8d_bfca").expect("sep"), 0xe1ec_dc58_3c8d_bfca);
        assert_eq!(parse_hex("0x38d3c9107d4eed33").expect("plain"), 0x38d3c9107d4eed33);
        assert!(parse_hex("0xnope").is_err());
    }

    /// The real-file guard, and the strongest one: splice each shipped slice with a value decoded FROM the
    /// shipped config. Nothing changed, so `config.ron` must come back BYTE-IDENTICAL — every comment, every
    /// hex literal, every column of alignment. If the scanner mis-parses any real construct in the authored
    /// file, this reds. (The synthetic fixtures above pin the behaviour; this pins it against reality.)
    #[test]
    fn splicing_the_shipped_config_with_its_own_values_is_a_byte_identical_no_op() {
        let gc = crate::config::load_game_config().expect("load the shipped config");
        let text = std::fs::read_to_string(CONFIG_PATH).expect("read config.ron");
        for (name, value) in [
            ("behavior", ron_slice(&gc.behavior).expect("ser behavior")),
            ("sim", ron_slice(&gc.sim).expect("ser sim")),
            ("ai_tuning", ron_slice(&gc.ai_tuning).expect("ser ai_tuning")),
            ("audio", ron_slice(&gc.audio).expect("ser audio")),
            ("dungeon", ron_slice(&gc.dungeon).expect("ser dungeon")),
            ("mycelia", ron_slice(&gc.mycelia).expect("ser mycelia")),
            ("metropolis", ron_slice(&gc.placement.metropolis).expect("ser metropolis")),
            ("density", ron_slice(&gc.placement.density).expect("ser density")),
            ("mold", ron_slice(&gc.mold).expect("ser mold")),
            // These two splice a SUBSET type (the evolvable dials only), so this also pins that a subset
            // whose field names are a flat subset of the authored block round-trips byte-identically.
            (
                "almond_water",
                ron_slice(&crate::almond_water::AlmondWaterDynamics::from_config(
                    &gc.almond_water,
                ))
                .expect("ser almond dynamics"),
            ),
            (
                "lighting",
                ron_slice(&crate::light::LightingDynamics::from_config(&gc.lighting))
                    .expect("ser lighting dynamics"),
            ),
        ] {
            let out = splice_block(&text, name, &value, &value)
                .unwrap_or_else(|e| panic!("splicing `{name}` with its own value must succeed: {e}"));
            assert_eq!(out, text, "splicing `{name}` with its own decoded value changed config.ron");
        }
    }
}
