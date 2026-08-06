//! **Editing one rig inside `rigs.ron` without destroying the rest of it.**
//!
//! `Rigs::to_ron` would round-trip the *values* and delete every comment — and this manifest's
//! comments are load-bearing prose (the Valkyrie's LEFTWARD note among them), which is exactly the
//! failure [`crate::ron_surgery`]'s header records. So the bench's write-back goes through here:
//! surgical text edits on the one rig's block, everything outside it byte-identical by construction.
//!
//! This is the rigs-shaped policy layer over `ron_surgery`'s primitives, kept out of that module on
//! its own charter ("the primitives stay policy-free"). Two of its gaps are solved here rather than
//! there: `scan_list` finds only the *first* `slots: [` in a document and this file has sixteen, so
//! [`RigDoc`] scans only the one rig's byte span; and a rig's block header is `"name": (` with the
//! quotes, so the lookup key is quoted before `find_block_value` sees it.

use std::ops::Range;

use crate::rigs::{Provenance, Rigs};
use crate::ron_surgery;

/// One rig's block, held as lines inside the whole file's text.
pub struct RigDoc {
    /// The complete document.
    full: String,
    /// The byte span of the rig's value block — `( ... )` — within `full`.
    span: Range<usize>,
    /// The block's text, split into lines. Rejoined with `\n` on render.
    lines: Vec<String>,
    /// Line index (into `lines`) of each slot record, in slot order.
    records: Vec<usize>,
    /// Line index of the `mesh:` line — the anchor a new rig-level field is inserted after.
    mesh_line: usize,
    /// Line indices bounding the `slots: [ ... ]` list, exclusive of the records.
    slots_open: usize,
}

impl RigDoc {
    /// Open `rig_name`'s block inside the manifest text.
    ///
    /// Refuses a document that does not parse and validate — editing a broken file surgically
    /// produces a precisely edited broken file — and refuses when its own line scan disagrees with
    /// the parser about how many slots the rig has, which is the cross-check that keeps "a record
    /// is a line starting with `(`" honest.
    pub fn open(text: &str, rig_name: &str) -> Result<RigDoc, String> {
        let parsed = Rigs::parse(text)?;
        let rig = parsed
            .get(rig_name)
            .ok_or_else(|| format!("rigs.ron has no rig named `{rig_name}`"))?;
        // The block header in the file is `"valkyrie": (` — the key is quoted.
        let span = ron_surgery::find_block_value(text, &format!("\"{rig_name}\""))?;
        let lines: Vec<String> = text[span.clone()].split('\n').map(str::to_owned).collect();

        let mesh_line = lines
            .iter()
            .position(|l| l.trim_start().starts_with("mesh:"))
            .ok_or_else(|| format!("rig `{rig_name}` has no mesh: line"))?;
        let slots_open = lines
            .iter()
            .position(|l| l.trim_start().starts_with("slots: ["))
            .ok_or_else(|| format!("rig `{rig_name}` has no slots: list"))?;
        let mut records = Vec::new();
        for (ix, line) in lines.iter().enumerate().skip(slots_open + 1) {
            let t = line.trim_start();
            if t.starts_with(']') {
                break;
            }
            if t.starts_with('(') {
                records.push(ix);
            }
        }
        if records.len() != rig.slots.len() {
            return Err(format!(
                "rig `{rig_name}`: the line scan sees {} slot record(s) but the parser sees {} — \
                 a slot spanning multiple lines cannot be line-edited; NOT touching this file",
                records.len(),
                rig.slots.len()
            ));
        }
        Ok(RigDoc {
            full: text.to_owned(),
            span,
            lines,
            records,
            mesh_line,
            slots_open,
        })
    }

    /// Rewrite one field of one slot record — `duration`, `phase_offset`, `cycle_distance` — via
    /// [`ron_surgery::replace_field`], which preserves indentation, sibling fields, alignment
    /// padding and any trailing comment. The `Gait(...)` parens around the field are handled by
    /// its depth counting.
    pub fn edit_slot_field(&mut self, slot: usize, field: &str, value: &str) -> Result<(), String> {
        let at = *self
            .records
            .get(slot)
            .ok_or_else(|| format!("no slot {slot} in this rig"))?;
        self.lines[at] = ron_surgery::replace_field(&self.lines[at], field, value)?;
        Ok(())
    }

    /// Replace-or-insert a one-line rig-level field — the provenance stamp's door.
    ///
    /// A field that already exists (outside the slots list) is rewritten in place; a new one is
    /// inserted directly after the `mesh:` line with the same indentation, which keeps the block's
    /// shape stable across repeated adopts.
    pub fn set_rig_field(&mut self, field: &str, value: &str) -> Result<(), String> {
        let needle = format!("{field}:");
        let existing = self
            .lines
            .iter()
            .enumerate()
            .filter(|(ix, _)| *ix <= self.slots_open)
            .find(|(_, l)| l.trim_start().starts_with(&needle))
            .map(|(ix, _)| ix);
        match existing {
            Some(ix) => {
                self.lines[ix] = ron_surgery::replace_field(&self.lines[ix], field, value)?;
            }
            None => {
                let indent: String = self.lines[self.mesh_line]
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .collect();
                self.lines
                    .insert(self.mesh_line + 1, format!("{indent}{field}: {value},"));
                // Every bookmark at or past the insertion point slides down one line.
                if self.slots_open > self.mesh_line {
                    self.slots_open += 1;
                }
                for r in &mut self.records {
                    if *r > self.mesh_line {
                        *r += 1;
                    }
                }
            }
        }
        Ok(())
    }

    /// The whole document with the edited block spliced back in. Byte-identical outside the rig's
    /// block by construction, and byte-identical inside it when nothing was edited.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(self.full.len() + 128);
        out.push_str(&self.full[..self.span.start]);
        out.push_str(&self.lines.join("\n"));
        out.push_str(&self.full[self.span.end..]);
        out
    }
}

/// A [`Provenance`] as the one-line RON value `set_rig_field` writes:
/// `Some((glb_fnv1a: "0x...", clips: 20, clip_names: ["idle", ...], tool: 1, date: "2026-08-06"))`.
/// Hand-formatted rather than serialized so the on-disk spelling is stable; pinned by a
/// parse-back test below.
pub fn provenance_value(p: &Provenance) -> String {
    let names = p
        .clip_names
        .iter()
        .map(|n| ron_str(n))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Some((glb_fnv1a: {}, clips: {}, clip_names: [{names}], tool: {}, date: {}))",
        ron_str(&p.glb_fnv1a),
        p.clips,
        p.tool,
        ron_str(&p.date),
    )
}

/// A RON string literal, escaped. Clip names come from an exporter, not from us.
fn ron_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rigs::{BENCH_TOOL_VERSION, fingerprint_string};

    /// A two-rig miniature in the real manifest's shape: comments above and inside, a note that
    /// contains a field-name-looking string, and one-line slot records.
    const DOC: &str = r#"// Header prose that must survive.
(
    version: 2,
    rigs: {
        // The first rig.
        "alpha": (
            mesh: "a/a.glb",
            scale: 1.0,
            slots: [
                (clip: 0, playback: Free(speed: 1.0), note: Some("idle")),
            ],
        ),
        "beta": (
            mesh: "b/b.glb",
            scale: 2.0,
            slots: [
                (clip: 0, playback: Gait(duration: 1.417, phase_offset: 0.0, cycle_distance: 1.388), note: Some("walk — the reference")), // trailing
                (clip: 1, playback: Gait(duration: 0.75, phase_offset: -0.016, cycle_distance: 2.135), note: Some("run")),
            ],
        ),
    },
)
"#;

    #[test]
    fn a_no_op_open_and_render_is_byte_identical() {
        let doc = RigDoc::open(DOC, "beta").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.render(), DOC);
    }

    #[test]
    fn the_quoted_key_finds_the_second_rig_not_the_first() {
        let mut doc = RigDoc::open(DOC, "beta").unwrap_or_else(|e| panic!("{e}"));
        doc.edit_slot_field(0, "duration", "1.402").unwrap_or_else(|e| panic!("{e}"));
        let out = doc.render();
        assert!(out.contains("duration: 1.402"), "{out}");
        // Alpha's block and every comment are untouched.
        assert!(out.contains("mesh: \"a/a.glb\""));
        assert!(out.contains("// The first rig."));
        assert!(out.contains("// Header prose that must survive."));
        assert!(out.contains("// trailing"), "the record's trailing comment survives");
        assert!(out.contains("walk — the reference"), "the note survives");
    }

    #[test]
    fn editing_one_field_changes_exactly_one_line() {
        let mut doc = RigDoc::open(DOC, "beta").unwrap_or_else(|e| panic!("{e}"));
        doc.edit_slot_field(1, "cycle_distance", "2.106").unwrap_or_else(|e| panic!("{e}"));
        let before: Vec<&str> = DOC.lines().collect();
        let out = doc.render();
        let after: Vec<&str> = out.lines().collect();
        assert_eq!(before.len(), after.len());
        let changed: Vec<usize> = (0..before.len()).filter(|&i| before[i] != after[i]).collect();
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert!(after[changed[0]].contains("cycle_distance: 2.106"));
    }

    #[test]
    fn a_provenance_stamp_inserts_once_then_rewrites_in_place() {
        let stamp = Provenance {
            glb_fnv1a: fingerprint_string(0x1234_5678_9abc_def0),
            clips: 2,
            clip_names: vec!["walk".to_owned(), String::new()],
            tool: BENCH_TOOL_VERSION,
            date: "2026-08-06".to_owned(),
        };
        let mut doc = RigDoc::open(DOC, "beta").unwrap_or_else(|e| panic!("{e}"));
        doc.set_rig_field("provenance", &provenance_value(&stamp)).unwrap_or_else(|e| panic!("{e}"));
        let once = doc.render();
        assert_eq!(
            once.lines().count(),
            DOC.lines().count() + 1,
            "the first stamp is one inserted line"
        );
        // It parses back to the same value — the spelling pin.
        let parsed = Rigs::parse(&once).unwrap_or_else(|e| panic!("{e}"));
        let read = parsed.get("beta").and_then(|r| r.provenance.as_ref());
        assert_eq!(read, Some(&stamp));

        // A second adopt rewrites the same line rather than stacking stamps, and slot edits still
        // land after the insertion shifted the bookmarks.
        let mut doc = RigDoc::open(&once, "beta").unwrap_or_else(|e| panic!("{e}"));
        let mut newer = stamp.clone();
        newer.date = "2026-08-07".to_owned();
        doc.set_rig_field("provenance", &provenance_value(&newer)).unwrap_or_else(|e| panic!("{e}"));
        doc.edit_slot_field(0, "duration", "1.4").unwrap_or_else(|e| panic!("{e}"));
        let twice = doc.render();
        assert_eq!(twice.lines().count(), once.lines().count());
        assert!(twice.contains("2026-08-07") && !twice.contains("2026-08-06"));
        assert!(twice.contains("duration: 1.4,"), "{twice}");
    }

    #[test]
    fn the_shipped_manifest_opens_and_renders_byte_identical_for_every_rig() {
        // The strongest comment-preservation assertion available, and it is read-only: every rig's
        // block round-trips the REAL file untouched.
        let text = std::fs::read_to_string("../../assets/emerge/rigs.ron")
            .unwrap_or_else(|e| panic!("rigs.ron: {e}"));
        let rigs = Rigs::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        for name in rigs.rigs.keys() {
            let doc = RigDoc::open(&text, name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(doc.render(), text, "rig `{name}` did not round-trip");
        }
    }

    #[test]
    fn a_document_that_does_not_parse_is_refused_before_any_edit() {
        let e = RigDoc::open("(version: 2, rigs: {)", "alpha").err();
        assert!(e.is_some());
    }
}
