//! **Editing RON without destroying it** — the machinery both of this repo's writers were built on.
//!
//! Two tools here write RON that a human also writes: the config bake (`bake.rs`, which splices an
//! evolved elite into `assets/config/config.ron`) and the Site editor (`site_editor::source_map`,
//! which moves a prop in `assets/site/site67.ron`). Neither may re-serialise, for the same reason
//! stated twice: **the comments are the content.** `config.ron` carries ~563 comment lines;
//! `site67.ron` is 1401 lines of which 217 are comments, and its `props` list runs 105 comment lines
//! against 86 records — more prose than data. A `to_string_pretty` round-trip deletes all of it, and
//! on 2026-07-16 one did: a `--dim levels` bake stripped ~279 lines of hand-written rationale.
//!
//! So both tools hold the file as **text** and rewrite only the bytes that changed. They arrived at
//! that independently and grew two implementations of it; this module is the one they share. The
//! standalone editor needs the same guarantee for maps it did not author, which is what finally made
//! the duplication worth paying to remove.
//!
//! # Two mechanisms, not one — and that is deliberate
//!
//! They are not competing paths to one result. They address different file shapes:
//!
//! - [`scan_ron_leaves`] / [`find_block_value`] are a **byte-span** scanner over arbitrarily nested
//!   values. `config.ron`'s blocks are deep and multi-line, so an edit there is "substitute the token
//!   at bytes 4012..4016". Comment- and string-aware, because the paren-counting scan it replaced
//!   counted `(` inside comments.
//! - [`LineDoc`] is a **line** rewriter over flat lists of one-line records. Every record in
//!   `site67.ron` occupies exactly one line, trailing comment included (measured: 1070 of them), so
//!   "record *i*" is "line `records[i]`" and an edit is a substring replacement inside it. That in
//!   turn is what makes an *undo* able to restore the original bytes — a span splicer has nothing to
//!   put back.
//!
//! A file that is deep uses the first; a file that is flat and heavily annotated uses the second.
//! Using the span splicer on `site67.ron` would work and would lose the ability to say "this move
//! changed exactly one line", which is the property that makes the editor's diffs reviewable.
//!
//! # What is *not* here
//!
//! Policy. `bake::splice_block` decides when an edit is dishonest — a changed block *shape*, or a
//! moved field the authored file never spells out — and refuses with prose about that specific
//! situation. `site_editor::source_map` knows what a prop record looks like. Those stay with their
//! callers; this module only offers the primitives they refuse *with*.

use std::path::Path;

// ---------------------------------------------------------------------------------------------
// Span surgery — for nested values
// ---------------------------------------------------------------------------------------------

/// A scalar leaf found in RON source text: where it lives in the tree, and the exact byte span of its
/// value token so it can be substituted without disturbing anything around it.
pub struct Leaf {
    /// Dotted field path from the block root, with `[i]` for sequence elements (`room_types[2].weight`).
    pub path: String,
    /// Byte range of the scalar token within the scanned text.
    pub span: std::ops::Range<usize>,
    /// The scalar token's source text (`0x5C09191`, `2`, `1.0`, `"bathroom"`, `true`, `None`).
    pub text: String,
}

/// Scan RON source into its scalar leaves, tracking the path to each and the byte span of its value
/// token.
///
/// This is deliberately a *source* scanner, not a deserializer: it must know where each scalar sits in
/// the original bytes so a caller can substitute one number and leave every comment, alignment, and
/// literal spelling around it untouched. It skips `//`, `/* */`, and string contents — which is also
/// why the paren-counting block scan it replaced was wrong (it counted `(` inside comments and
/// strings).
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
    fn value(
        b: &[u8],
        text: &str,
        mut i: usize,
        path: &str,
        out: &mut Vec<Leaf>,
        depth: usize,
    ) -> Result<usize, String> {
        if depth > 32 {
            return Err("ron: value nested deeper than 32 — refusing".to_string());
        }
        i = skip(b, i);
        if i >= b.len() {
            return Err(format!("ron: unexpected end of input at `{path}`"));
        }

        // `Some(...)` / `None` — an Option wrapper is transparent to the path.
        if (b[i] as char).is_alphabetic() {
            let (after, id) = ident(b, i);
            let j = skip(b, after);
            if id == "Some" && j < b.len() && b[j] == b'(' {
                i = value(b, text, j + 1, path, out, depth + 1)?;
                i = skip(b, i);
                if i >= b.len() || b[i] != b')' {
                    return Err(format!("ron: unclosed `Some(` at `{path}`"));
                }
                return Ok(i + 1);
            }
            // A named struct/enum like `Grid` or `Foo(...)`: if a `(` follows, descend; else it's a
            // scalar.
            if j < b.len() && b[j] == b'(' {
                return strukt(b, text, j + 1, path, out, depth + 1);
            }
            out.push(Leaf {
                path: path.to_string(),
                span: i..after,
                text: id,
            });
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
                        return Err(format!("ron: unclosed `[` at `{path}`"));
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

    // A struct body, positioned just after its `(`. Fields are `key: value`; bare values are tuple
    // elements.
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
                return Err(format!("ron: unclosed `(` at `{path}`"));
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
                    child = if path.is_empty() {
                        id
                    } else {
                        format!("{path}.{id}")
                    };
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
        return Err("ron: block value does not start with `(`".to_string());
    }
    let end = strukt(b, text, i + 1, "", &mut out, 0)?;
    let tail = skip(b, end);
    if tail < b.len() {
        return Err(format!(
            "ron: trailing input after block value: {:?}",
            &text[tail..]
        ));
    }
    Ok(out)
}

/// Do two RON scalar tokens denote the same value? Compares by PARSED value, not by spelling — so the
/// authored `seed: 0x5C09191` and the serializer's `96506257` are equal, and the authored line is left
/// alone (hex spelling, alignment, and its `// nods to SCP-9191` comment all preserved).
///
/// # `Value::Unit` is where the parsed comparison stops being trustworthy
///
/// `ron::Value` cannot represent a unit variant's *name*: measured against `ron 0.12.2`, both `Grid`
/// and `Hex` parse to `Ok(Value::Unit)`. Comparing those parsed would call every enum variant equal to
/// every other, which for a splicer means an elite that flips `Grid` to `Hex` reports "0 values
/// changed" and writes nothing — the exact silent-wrong-answer this whole module exists to avoid.
///
/// So a token that parses to `Unit` falls back to exact text, which for two different identifiers is
/// `false`. That is the honest answer: this function can prove two numbers equal, and it cannot prove
/// two identifiers equal beyond them being the same identifier.
pub fn scalar_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (ron::from_str::<ron::Value>(a), ron::from_str::<ron::Value>(b)) {
        (Ok(ron::Value::Unit), _) | (_, Ok(ron::Value::Unit)) => false,
        (Ok(x), Ok(y)) => x == y,
        // Unparseable on either side: fall back to exact text. Never treat "can't tell" as "equal".
        _ => false,
    }
}

/// Locate the `<name>: ( … )` block in a RON document, returning the byte span of its VALUE (the
/// `( … )`). The scan is comment- and string-aware, unlike a raw `(`/`)` char count.
pub fn find_block_value(text: &str, name: &str) -> Result<std::ops::Range<usize>, String> {
    let header = format!("{name}: (");
    let mut search = 0usize;
    let at = loop {
        let rel = text[search..]
            .find(&header)
            .ok_or_else(|| format!("ron: no `{name}:` block header"))?;
        let abs = search + rel;
        // Must be a real field, not a substring of a longer name (`density` inside `foo_density`) and
        // not inside a comment.
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
    let open = text[open..]
        .find('(')
        .map(|o| open + o)
        .ok_or("ron: malformed block header")?;
    let tail = &text[open..];
    let mut depth = 0usize;
    let b = tail.as_bytes();
    let (mut i, mut in_str, mut in_line_comment, mut in_block_comment) =
        (0usize, false, false, false);
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
    Err(format!("ron: unbalanced `{name}:` block"))
}

// ---------------------------------------------------------------------------------------------
// Line surgery — for flat lists of one-line records
// ---------------------------------------------------------------------------------------------

/// A RON document held as lines, with the line index of every record in the lists a caller declared
/// it owns.
///
/// Indices are kept correct across inserts and deletes by [`Self::shift_after`]. They are meaningless
/// against a different file, so a `LineDoc` is only ever valid alongside the parse of the same text —
/// callers are expected to cross-check record counts against their own parser, because a scan that
/// finds a different number of records than the parser did means the file's shape has broken and the
/// only safe response is to refuse. Editing the wrong line looks exactly like a successful save.
pub struct LineDoc {
    lines: Vec<String>,
    lists: Vec<ListSpan>,
    /// Whether the source ended with a newline, so [`Self::render`] reproduces it exactly.
    trailing_newline: bool,
}

/// One declared list: its RON key, the source line of each record in it, and the lines that open and
/// close it.
struct ListSpan {
    key: String,
    /// The `key: [` line. Only [`LineDoc::restore`] needs it, and it needs it badly — see [`Removed`].
    open: usize,
    /// `records[i]` is the line the *i*th record of this list was parsed from.
    records: Vec<usize>,
    /// The `],` line closing the list — where an appended record goes.
    end: usize,
}

/// A record taken out of a [`LineDoc`], carrying what [`LineDoc::restore`] needs to put it back where
/// it was.
///
/// # Why this is not just the line
///
/// It was, and that was wrong. Restoring at "wherever record *i* starts now" puts the record *below*
/// any comment lines that were sitting above it, because after the removal those lines belong to the
/// record that took its index. `site67.ron` interleaves 102 comment lines through 86 prop records, so
/// this is the shipped shape, not a corner case: delete the last pipe above
/// `// Threshold pads: the door's mouth…` and undo, and the pipe comes back underneath a comment
/// about floor buttons.
///
/// [`gap`](Self::gap) is therefore stored as a *relative* offset — how many non-record lines sat
/// between the previous record and this one — rather than an absolute line number, so it survives
/// unrelated edits made between the delete and the undo.
#[derive(Clone, Debug)]
pub struct Removed {
    /// The record's exact bytes, trailing comment included.
    pub line: String,
    /// Lines between the end of the previous record (or the `key: [` line) and this record.
    gap: usize,
}

impl LineDoc {
    /// Scan `text` and locate every record line in each of `list_keys`.
    ///
    /// A key that is not found, or a list that is never closed, is an error: this type exists to be
    /// certain which line it is writing.
    pub fn parse(text: &str, list_keys: &[&str]) -> Result<LineDoc, String> {
        let trailing_newline = text.ends_with('\n');
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let mut lists = Vec::with_capacity(list_keys.len());
        for key in list_keys {
            let (open, records, end) = scan_list(&lines, key)?;
            lists.push(ListSpan {
                key: (*key).to_owned(),
                open,
                records,
                end,
            });
        }
        Ok(LineDoc {
            lines,
            lists,
            trailing_newline,
        })
    }

    /// The document as text, byte-identical to the input when nothing has been edited.
    pub fn render(&self) -> String {
        let mut out = self.lines.join("\n");
        if self.trailing_newline {
            out.push('\n');
        }
        out
    }

    /// How many records the named list holds.
    pub fn len(&self, key: &str) -> Result<usize, String> {
        Ok(self.list(key)?.records.len())
    }

    /// Whether the named list is empty.
    pub fn is_empty(&self, key: &str) -> Result<bool, String> {
        Ok(self.len(key)? == 0)
    }

    /// The source line a record was parsed from — the unit of undo.
    pub fn line(&self, key: &str, i: usize) -> Result<&str, String> {
        let at = self.record_line(key, i)?;
        self.lines
            .get(at)
            .map(String::as_str)
            .ok_or_else(|| format!("ron: `{key}` record {i} points past the end of the file"))
    }

    /// Overwrite a record's source line verbatim.
    ///
    /// The undo path uses this to put back the exact bytes [`Self::line`] handed out, so a restored
    /// record keeps its original formatting and trailing comment rather than being re-emitted in some
    /// canonical style.
    pub fn set_line(&mut self, key: &str, i: usize, line: String) -> Result<(), String> {
        let at = self.record_line(key, i)?;
        let slot = self
            .lines
            .get_mut(at)
            .ok_or_else(|| format!("ron: `{key}` record {i} points past the end of the file"))?;
        *slot = line;
        Ok(())
    }

    /// Rewrite one `field: <value>` inside a record's line and nothing else. See [`replace_field`].
    pub fn edit_field(
        &mut self,
        key: &str,
        i: usize,
        field: &str,
        value: &str,
    ) -> Result<(), String> {
        let at = self.record_line(key, i)?;
        let line = self
            .lines
            .get(at)
            .ok_or_else(|| format!("ron: `{key}` record {i} points past the end of the file"))?;
        let edited = replace_field(line, field, value)?;
        self.lines[at] = edited;
        Ok(())
    }

    /// Delete a record, returning it verbatim — comment included — along with where it sat relative
    /// to the record above it, so [`Self::restore`] can put it back exactly there.
    pub fn remove(&mut self, key: &str, i: usize) -> Result<Removed, String> {
        let at = self.record_line(key, i)?;
        if at >= self.lines.len() {
            return Err(format!(
                "ron: `{key}` record {i} points past the end of the file"
            ));
        }
        let ix = self.list_ix(key)?;
        let gap = at.saturating_sub(self.anchor(ix, i));
        let line = self.lines.remove(at);
        self.lists[ix].records.remove(i);
        self.shift_after(at, -1);
        Ok(Removed { line, gap })
    }

    /// Put a previously removed record back where it was, with its original bytes. The inverse of
    /// [`Self::remove`], and the reason undo restores comments rather than dropping them.
    pub fn restore(&mut self, key: &str, i: usize, removed: Removed) -> Result<(), String> {
        let ix = self.list_ix(key)?;
        if i > self.lists[ix].records.len() {
            return Err(format!(
                "ron: cannot restore `{key}` record {i}; only {} exist",
                self.lists[ix].records.len()
            ));
        }
        // Land it `gap` lines below the record above it, so the comment block it sat under stays above
        // it. Clamped to the list's closing bracket, because a stale gap must not push a record out of
        // its own list.
        let at = self
            .anchor(ix, i)
            .saturating_add(removed.gap)
            .min(self.lists[ix].end);
        self.lines.insert(at, removed.line);
        self.shift_after(at, 1);
        self.lists[ix].records.insert(i, at);
        Ok(())
    }

    /// The first line record `i` of list `ix` could occupy: just past the record above it, or just
    /// past the `key: [` line when it is the first.
    fn anchor(&self, ix: usize, i: usize) -> usize {
        let list = &self.lists[ix];
        match i.checked_sub(1).and_then(|p| list.records.get(p)) {
            Some(prev) => prev + 1,
            None => list.open + 1,
        }
    }

    /// Append a record line to the end of a list, returning its new index.
    pub fn append(&mut self, key: &str, line: String) -> Result<usize, String> {
        let ix = self.list_ix(key)?;
        let at = self.lists[ix].end;
        self.lines.insert(at, line);
        self.shift_after(at, 1);
        self.lists[ix].records.push(at);
        Ok(self.lists[ix].records.len() - 1)
    }

    fn list_ix(&self, key: &str) -> Result<usize, String> {
        self.lists
            .iter()
            .position(|l| l.key == key)
            .ok_or_else(|| format!("ron: `{key}` is not a list this document was parsed with"))
    }

    fn list(&self, key: &str) -> Result<&ListSpan, String> {
        self.lists
            .iter()
            .find(|l| l.key == key)
            .ok_or_else(|| format!("ron: `{key}` is not a list this document was parsed with"))
    }

    fn record_line(&self, key: &str, i: usize) -> Result<usize, String> {
        self.list(key)?
            .records
            .get(i)
            .copied()
            .ok_or_else(|| format!("ron: no `{key}` record at index {i}"))
    }

    /// Fix up every stored line index after an insertion (`delta = 1`) or deletion (`delta = -1`) at
    /// `at`. Missing one of these is how a writer starts editing the wrong line, so **every** list is
    /// updated here — not only the one that changed.
    fn shift_after(&mut self, at: usize, delta: isize) {
        for list in &mut self.lists {
            for ix in &mut list.records {
                if *ix >= at {
                    *ix = ix.saturating_add_signed(delta);
                }
            }
            if list.end >= at {
                list.end = list.end.saturating_add_signed(delta);
            }
            if list.open >= at {
                list.open = list.open.saturating_add_signed(delta);
            }
        }
    }
}

/// Find `key: [` and return its line, the line index of every record inside it, and the index of the
/// `],` that closes it.
///
/// Records are lines whose first non-space character is `(`. Comment lines and blank lines inside the
/// list are skipped and left untouched — preserving them is the whole point of this module.
fn scan_list(lines: &[String], key: &str) -> Result<(usize, Vec<usize>, usize), String> {
    let header = format!("{key}: [");
    let start = lines
        .iter()
        .position(|l| l.trim_start().starts_with(&header))
        .ok_or_else(|| format!("ron: `{key}` list not found"))?;

    let mut records = Vec::new();
    for (offset, line) in lines.iter().enumerate().skip(start + 1) {
        let t = line.trim_start();
        if t.starts_with("],") || t == "]" {
            return Ok((start, records, offset));
        }
        if t.starts_with('(') {
            records.push(offset);
        }
    }
    Err(format!("ron: `{key}` list is never closed"))
}

/// Replace the value of `key: <value>` inside one record line, preserving everything else — leading
/// indentation, sibling fields, inter-field padding, and any trailing `// comment`.
///
/// The value runs from just after `key:` to the first `,` or `)` encountered at paren-depth zero, so a
/// tuple value like `(41.5, 26.5)` is matched whole rather than being cut at its inner comma. Padding
/// immediately after the colon is preserved and padding before the terminator is dropped, which keeps
/// a same-width replacement byte-stable.
pub fn replace_field(line: &str, key: &str, value: &str) -> Result<String, String> {
    let needle = format!("{key}:");
    // Search only the code part, so a `//` comment mentioning "yaw:" cannot be mistaken for a field.
    let code_len = comment_split(line);
    let code = &line[..code_len];
    let key_at = code
        .find(&needle)
        .ok_or_else(|| format!("ron: no `{key}:` field in `{}`", line.trim()))?;

    let after_key = key_at + needle.len();
    // Keep the author's padding between the colon and the value.
    let pad_len = code[after_key..]
        .len()
        .saturating_sub(code[after_key..].trim_start_matches(' ').len());
    let value_start = after_key + pad_len;

    let mut depth = 0usize;
    let mut value_end = None;
    for (ix, ch) in code[value_start..].char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' if depth > 0 => depth -= 1,
            ')' | ']' if depth == 0 => {
                value_end = Some(value_start + ix);
                break;
            }
            ',' if depth == 0 => {
                value_end = Some(value_start + ix);
                break;
            }
            _ => {}
        }
    }
    let value_end =
        value_end.ok_or_else(|| format!("ron: `{key}:` value is not terminated in `{}`", line.trim()))?;

    // Whitespace between the value and its terminator is the author's column alignment, not part of
    // the value — `yaw:  90.0 ),` must become `yaw:  0.0 ),` and not `yaw:  0.0),`. Trimming it out of
    // the replaced span is what keeps a same-width edit byte-stable.
    let span = &code[value_start..value_end];
    let value_end = value_end - (span.len() - span.trim_end().len());

    let mut out = String::with_capacity(line.len() + value.len());
    out.push_str(&line[..value_start]);
    out.push_str(value);
    out.push_str(&line[value_end..]);
    Ok(out)
}

/// Byte offset where a line's trailing `//` comment begins, or the line's length when it has none.
/// Quote-aware, because `label: "RESEARCH // wing"` is data, not a comment.
fn comment_split(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut in_quotes = false;
    let mut ix = 0;
    while ix < bytes.len() {
        match bytes[ix] {
            b'\\' if in_quotes => ix += 1,
            b'"' => in_quotes = !in_quotes,
            b'/' if !in_quotes && bytes.get(ix + 1) == Some(&b'/') => return ix,
            _ => {}
        }
        ix += 1;
    }
    line.len()
}

/// The trailing `// comment` on a record line, if it has one.
///
/// A tool should surface this before deleting the record: a comment is somebody's recorded reasoning,
/// and destroying it should be a decision rather than a side effect.
pub fn trailing_comment(line: &str) -> Option<&str> {
    let at = comment_split(line);
    (at < line.len()).then(|| line[at..].trim_end())
}

/// Format an `f32` at the fewest decimal places that still round-trip to the same value.
///
/// Round-tripping through `parse` is what decides, rather than a tolerance, so a written file always
/// reloads to the same `f32` the writer was holding. One decimal is exact for anything snapped to a
/// half-metre or a whole degree; the wider forms are only reached by a value that came from elsewhere.
pub fn fmt_f32(v: f32) -> String {
    for places in 1..=6 {
        let s = format!("{v:.places$}");
        if s.parse::<f32>() == Ok(v) {
            return s;
        }
    }
    format!("{v}")
}

/// Write `text` to `path` atomically — tmp file then rename, so a crash mid-write cannot leave a
/// half-written document on disk.
///
/// The caller is expected to re-load and re-validate afterwards; this only guarantees the bytes
/// landed.
pub fn save_atomic(path: &Path, text: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("ron: {} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("ron: {}: {e}", parent.display()))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, text).map_err(|e| format!("ron: {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("ron: {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A miniature of a heavily annotated flat list, in the shape `site67.ron`'s `props` has: prose
    /// above the list, a blank line inside it, and a trailing comment on a record.
    const DOC: &str = "layout: (\n\
        \x20   // The props the editor owns.\n\
        \x20   props: [\n\
        \x20       ( piece: Crate,  pos: (41.5, 26.5), yaw:  0.0 ),   // by the door\n\
        \x20\n\
        \x20       ( piece: Stool,  pos: ( 1.5,  2.5), yaw: 90.0 ),\n\
        \x20   ],\n\
        \x20   spawns: [\n\
        \x20       ( at: (0.0, 0.0) ),\n\
        \x20   ],\n\
        )\n";

    #[test]
    fn a_no_op_parse_and_render_is_byte_identical() {
        let doc = LineDoc::parse(DOC, &["props", "spawns"]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.render(), DOC);
    }

    #[test]
    fn comment_lines_and_blanks_are_not_records() {
        let doc = LineDoc::parse(DOC, &["props", "spawns"]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.len("props"), Ok(2));
        assert_eq!(doc.len("spawns"), Ok(1));
    }

    /// The property the Site editor's diffs rest on: moving one record rewrites one line, keeps the
    /// author's column alignment, and leaves the trailing comment alone.
    #[test]
    fn editing_a_field_changes_exactly_one_line() {
        let mut doc = LineDoc::parse(DOC, &["props", "spawns"]).unwrap_or_else(|e| panic!("{e}"));
        doc.edit_field("props", 0, "pos", "(10.0, 11.0)")
            .unwrap_or_else(|e| panic!("{e}"));
        let out = doc.render();
        let changed: Vec<_> = DOC
            .lines()
            .zip(out.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(changed.len(), 1, "expected one changed line, got {changed:?}");
        assert!(
            out.contains("( piece: Crate,  pos: (10.0, 11.0), yaw:  0.0 ),   // by the door"),
            "alignment or comment was disturbed:\n{out}"
        );
    }

    /// A yaw sitting in a padded span is the case that once produced `yaw: 0.0)` — the padding before
    /// the `)` was eaten along with the value.
    #[test]
    fn padding_before_the_terminator_survives_a_same_width_edit() {
        let line = "( piece: Stool,  pos: ( 1.5,  2.5), yaw: 90.0 ),";
        let out = replace_field(line, "yaw", "45.0").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out, "( piece: Stool,  pos: ( 1.5,  2.5), yaw: 45.0 ),");
    }

    /// A `//` inside a string is data. A field named in a real comment is not a field.
    #[test]
    fn the_comment_split_is_quote_aware() {
        let line = r#"( label: "RESEARCH // wing", yaw: 0.0 ), // set yaw: later"#;
        assert_eq!(trailing_comment(line), Some("// set yaw: later"));
        let out = replace_field(line, "yaw", "90.0").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            out,
            r#"( label: "RESEARCH // wing", yaw: 90.0 ), // set yaw: later"#
        );
    }

    /// Delete then restore must put the *bytes* back, comment included — the whole reason undo works
    /// on lines rather than on a re-serialised record.
    #[test]
    fn remove_then_restore_is_byte_identical() {
        let mut doc = LineDoc::parse(DOC, &["props", "spawns"]).unwrap_or_else(|e| panic!("{e}"));
        let removed = doc.remove("props", 0).unwrap_or_else(|e| panic!("{e}"));
        assert!(removed.line.contains("// by the door"));
        assert_eq!(doc.len("props"), Ok(1));
        doc.restore("props", 0, removed)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.render(), DOC);
    }

    /// **The bug [`Removed`] exists for.** Restoring at "wherever record *i* starts now" puts the
    /// record below the comment block that came to belong to its successor. Here the crate sits
    /// directly above a comment introducing the stool; delete it, undo, and a naive restore leaves the
    /// crate underneath a note about stools.
    ///
    /// This is `site67.ron`'s shipped shape — 102 comment lines interleaved through 86 prop records —
    /// and it shipped undetected because the editor's own test happens to pick a record that sits
    /// *after* a comment block, where the naive placement is accidentally right.
    #[test]
    fn restoring_a_record_that_sat_above_a_comment_block_stays_above_it() {
        let doc_text = "props: [\n\
            \x20   ( piece: Crate, pos: (1.0, 1.0), yaw: 0.0 ),\n\
            \x20   // The stools, which are a different matter entirely.\n\
            \x20   ( piece: Stool, pos: (2.0, 2.0), yaw: 0.0 ),\n\
            ],\n";
        let mut doc = LineDoc::parse(doc_text, &["props"]).unwrap_or_else(|e| panic!("{e}"));
        let removed = doc.remove("props", 0).unwrap_or_else(|e| panic!("{e}"));
        doc.restore("props", 0, removed)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.render(), doc_text);
    }

    /// The same shape at the end of a list: the last record sits below a comment block and must come
    /// back below it, not above.
    #[test]
    fn restoring_the_last_record_lands_below_its_own_comment() {
        let doc_text = "props: [\n\
            \x20   ( piece: Crate, pos: (1.0, 1.0), yaw: 0.0 ),\n\
            \x20   // And one stool.\n\
            \x20   ( piece: Stool, pos: (2.0, 2.0), yaw: 0.0 ),\n\
            ],\n";
        let mut doc = LineDoc::parse(doc_text, &["props"]).unwrap_or_else(|e| panic!("{e}"));
        let removed = doc.remove("props", 1).unwrap_or_else(|e| panic!("{e}"));
        doc.restore("props", 1, removed)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.render(), doc_text);
    }

    /// Every list's indices must survive an edit to another list, which is the bug a single-list
    /// `shift_after` would hide until someone edited a spawn after deleting a prop.
    #[test]
    fn deleting_from_one_list_keeps_the_others_pointing_at_the_right_lines() {
        let mut doc = LineDoc::parse(DOC, &["props", "spawns"]).unwrap_or_else(|e| panic!("{e}"));
        let before = doc.line("spawns", 0).map(str::to_owned);
        doc.remove("props", 1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.line("spawns", 0).map(str::to_owned), before);
    }

    #[test]
    fn appending_lands_inside_the_list_and_renders() {
        let mut doc = LineDoc::parse(DOC, &["props", "spawns"]).unwrap_or_else(|e| panic!("{e}"));
        let i = doc
            .append("props", "        ( piece: Bench, pos: (0.0, 0.0), yaw: 0.0 ),".to_owned())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(i, 2);
        assert_eq!(doc.len("props"), Ok(3));
        // It must sit before the `],`, and the spawns list must still be findable afterwards.
        let out = doc.render();
        let bench = out.find("Bench").unwrap_or_else(|| panic!("appended line missing"));
        let spawns = out.find("spawns: [").unwrap_or_else(|| panic!("spawns list missing"));
        assert!(bench < spawns, "appended record escaped the props list:\n{out}");
        assert_eq!(doc.line("spawns", 0).map(|l| l.contains("at:")), Ok(true));
    }

    #[test]
    fn an_unclosed_list_is_refused_rather_than_guessed() {
        let bad = "props: [\n    ( piece: Crate ),\n";
        let err = LineDoc::parse(bad, &["props"]).err().unwrap_or_default();
        assert!(err.contains("never closed"), "unhelpful error: {err}");
    }

    #[test]
    fn a_missing_list_names_itself() {
        let err = LineDoc::parse(DOC, &["cells"]).err().unwrap_or_default();
        assert!(err.contains("cells"), "unhelpful error: {err}");
    }

    #[test]
    fn floats_are_written_at_the_shortest_round_tripping_width() {
        assert_eq!(fmt_f32(41.5), "41.5");
        assert_eq!(fmt_f32(0.0), "0.0");
        assert_eq!(fmt_f32(-1.25), "-1.25");
        // Whatever it writes must read back as the same f32 — that is the actual contract.
        for v in [0.1f32, 1.0 / 3.0, 1e-6, 12345.678] {
            assert_eq!(fmt_f32(v).parse::<f32>(), Ok(v), "{v} did not round-trip");
        }
    }

    /// The span scanner's reason for existing: an authored hex literal and the serializer's decimal
    /// are the same value, so a bake leaves the authored spelling — and its comment — alone.
    #[test]
    fn scalars_compare_by_value_not_by_spelling() {
        assert!(scalar_eq("0x5C09191", "96506257"));
        assert!(scalar_eq("1.0", "1.0"));
        assert!(!scalar_eq("1.0", "2.0"));
    }

    /// **Two different enum variants are not the same value.** `ron 0.12.2` parses every bare
    /// identifier to `Value::Unit`, so a parsed comparison calls `Grid` and `Hex` equal — and a
    /// splicer built on that reports "0 values changed" while the elite's variant never reaches the
    /// file. Measured, not assumed: the probe printed `Grid -> Ok(Unit)` and `Hex -> Ok(Unit)`.
    #[test]
    fn two_different_enum_variants_are_never_equal() {
        assert!(!scalar_eq("Grid", "Hex"));
        assert!(!scalar_eq("Chase", "Patrol"));
        assert!(scalar_eq("Grid", "Grid"), "a variant still equals itself");
        // And a variant is not equal to whatever else happens to parse oddly.
        assert!(!scalar_eq("None", "Grid"));
    }

    #[test]
    fn the_leaf_scanner_paths_nested_values_and_skips_comments() {
        let text = "( a: 1, // a: 99 is a lie\n  b: ( c: 2.5 ), d: [ ( e: \"x\" ) ] )";
        let leaves = scan_ron_leaves(text).unwrap_or_else(|e| panic!("{e}"));
        let paths: Vec<&str> = leaves.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, ["a", "b.c", "d[0].e"]);
        // And the span must point at the token, so a substitution touches nothing else.
        let a = &leaves[0];
        assert_eq!(&text[a.span.clone()], "1");
    }

    #[test]
    fn a_block_header_inside_a_comment_is_not_the_block() {
        let text = "root: (\n    // dungeon: ( not this one )\n    dungeon: ( w: 6 ),\n)";
        let span = find_block_value(text, "dungeon").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(&text[span], "( w: 6 )");
    }
}
