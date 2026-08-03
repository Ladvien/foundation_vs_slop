//! **The document being edited** — the working layout, its source text, the undo stack, and the live
//! fault list.
//!
//! Every mutation the editor can perform goes through [`EditorDoc`], and each one does three things
//! together or not at all: update the parsed [`SiteLayout`], rewrite the matching source line, and
//! re-run the placement rules. Keeping them in one place is what stops the three from drifting — a
//! layout that says one thing, a file that says another, and a fault list describing neither.
//!
//! # Why the checker runs on every edit
//!
//! `site::layout::prop_placement_report` is a pure function over the layout and the kit, and at this
//! size (86 props) it is far too cheap to be worth deferring. Running it per-edit is the whole point
//! of the tool: the same six rules that used to be a wall of text at startup become a marker on the
//! prop you are dragging, which is the real-time evaluation loop Liapis, Yannakakis & Togelius
//! describe for *Sentient Sketchbook* (FDG 2013).
//!
//! # Undo is symmetric, not a special case
//!
//! Each [`EditOp`] is the *inverse* of an applied change, and applying an inverse returns the op that
//! would redo it. So undo and redo are the same function pointed at different stacks, and there is no
//! second code path that reconstructs a record — a restored prop gets its original bytes back,
//! trailing comment included.

use crate::site::kit::SiteKit;
use crate::site::layout::{prop_placement_report, PlacementFault, PropPlacement, SiteLayout};

use super::source_map::{Removed, SourceMap};

/// How many edits are undoable. A bound rather than unbounded growth; the oldest is dropped.
const UNDO_DEPTH: usize = 256;

/// What an edit did to the `props` list, so the caller can bring the spawned bodies into line.
///
/// [`Change::Added`] and [`Change::Removed`] shift every later record's index, so the caller must
/// renumber the `PropIndex` of every affected body — not just touch the one that changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Moved(usize),
    Added(usize),
    Removed(usize),
}

/// The inverse of an applied edit — what it would take to put things back.
#[derive(Debug, Clone)]
enum EditOp {
    /// Restore this record's prior value and its prior source line.
    Restore {
        index: usize,
        prop: PropPlacement,
        line: String,
    },
    /// Delete the record at this index.
    Drop { index: usize },
    /// Put this record back at this index.
    ///
    /// `line` is a [`Removed`] rather than a bare `String` because restoring the bytes is not enough:
    /// the record has to land back under the comment block it was under. See `ron_surgery::Removed`.
    Reinsert {
        index: usize,
        prop: PropPlacement,
        line: Removed,
    },
}

/// The layout being edited, its text, and everything derived from them.
pub struct EditorDoc {
    /// The working layout. Diverges from `SiteLayoutRes` as soon as the first edit lands; the spawned
    /// bodies are kept in step by the caller reading [`Change`].
    pub layout: SiteLayout,
    map: SourceMap,
    undo: Vec<EditOp>,
    redo: Vec<EditOp>,
    /// Every broken placement rule, refreshed after each edit. Each entry names the record it is
    /// about, so the overlay can mark the offending prop rather than only listing a message.
    pub faults: Vec<PlacementFault>,
    /// Whether there are edits not yet written to disk.
    pub dirty: bool,
}

impl EditorDoc {
    /// Open the shipped layout for editing.
    ///
    /// `spawned` is the layout the Site's bodies were built from. The file is re-read rather than
    /// cloned from it, because the source **text** is what this editor writes and only the file has
    /// that — but the two must describe the same props, or the `PropIndex` carried by every body
    /// points at the wrong record. A mismatch is refused rather than reconciled: the file changed
    /// under a running game, and the honest fix is to relaunch.
    pub fn open(spawned: &SiteLayout, kit: &SiteKit) -> Result<Self, String> {
        let path = crate::site::layout::SITE_LAYOUT_PATH;
        let text = std::fs::read_to_string(path).map_err(|e| format!("site editor: {path}: {e}"))?;
        let layout: SiteLayout =
            ron::from_str(&text).map_err(|e| format!("site editor: {path}: {e}"))?;

        if layout.props.len() != spawned.props.len() {
            return Err(format!(
                "site editor: {path} has {} prop(s) but the running Site was built with {}. \
                 The file changed since launch — relaunch to edit it.",
                layout.props.len(),
                spawned.props.len()
            ));
        }

        let map = SourceMap::parse(&text, &layout)?;
        let faults = prop_placement_report(&layout, kit).faults;
        Ok(EditorDoc {
            layout,
            map,
            undo: Vec::new(),
            redo: Vec::new(),
            faults,
            dirty: false,
        })
    }

    /// The source line a prop was authored on — used to warn before a delete destroys its comment.
    pub fn prop_line(&self, index: usize) -> Result<&str, String> {
        self.map.prop_line(index)
    }

    /// The document as it would be written. What [`Self::save`] puts on disk, without putting it
    /// there — so a test can assert on the bytes an edit produces without touching the shipped file.
    pub fn text(&self) -> String {
        self.map.render()
    }

    /// Move and/or rotate a prop.
    ///
    /// Each field is written only if it actually changed, so rotating a chair leaves its `pos:` bytes
    /// untouched and the diff is one field wide.
    pub fn move_prop(
        &mut self,
        index: usize,
        pos: (f32, f32),
        yaw_deg: f32,
        kit: &SiteKit,
    ) -> Result<Change, String> {
        let before = self.snapshot(index)?;
        let current = self
            .layout
            .props
            .get(index)
            .ok_or_else(|| format!("site editor: no prop at index {index}"))?;

        let (moved, turned) = (current.pos != pos, current.yaw != yaw_deg);
        if moved {
            self.map.set_prop_pos(index, pos)?;
        }
        if turned {
            self.map.set_prop_yaw(index, yaw_deg)?;
        }
        if !moved && !turned {
            return Ok(Change::Moved(index));
        }

        let prop = self
            .layout
            .props
            .get_mut(index)
            .ok_or_else(|| format!("site editor: no prop at index {index}"))?;
        prop.pos = pos;
        prop.yaw = yaw_deg;

        self.push_undo(before);
        self.settle(kit);
        Ok(Change::Moved(index))
    }

    /// Append a prop, returning its index.
    pub fn insert_prop(&mut self, prop: PropPlacement, kit: &SiteKit) -> Result<Change, String> {
        let index = self.map.insert_prop(&prop)?;
        self.layout.props.push(prop);
        self.push_undo(EditOp::Drop { index });
        self.settle(kit);
        Ok(Change::Added(index))
    }

    /// Delete a prop. Its source line is kept verbatim so undo restores the bytes, comment included.
    pub fn delete_prop(&mut self, index: usize, kit: &SiteKit) -> Result<Change, String> {
        if index >= self.layout.props.len() {
            return Err(format!("site editor: no prop at index {index}"));
        }
        let line = self.map.remove_prop(index)?;
        let prop = self.layout.props.remove(index);
        self.push_undo(EditOp::Reinsert { index, prop, line });
        self.settle(kit);
        Ok(Change::Removed(index))
    }

    /// Undo the most recent edit. `None` when there is nothing to undo.
    pub fn undo(&mut self, kit: &SiteKit) -> Option<Result<Change, String>> {
        let op = self.undo.pop()?;
        Some(match self.apply_inverse(op) {
            Ok((redo_op, change)) => {
                self.redo.push(redo_op);
                self.settle(kit);
                Ok(change)
            }
            Err(e) => Err(e),
        })
    }

    /// Redo the most recently undone edit. `None` when there is nothing to redo.
    pub fn redo(&mut self, kit: &SiteKit) -> Option<Result<Change, String>> {
        let op = self.redo.pop()?;
        Some(match self.apply_inverse(op) {
            Ok((undo_op, change)) => {
                self.undo.push(undo_op);
                self.settle(kit);
                Ok(change)
            }
            Err(e) => Err(e),
        })
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Write the document back to `site67.ron`, then prove the round trip by reloading it.
    ///
    /// The reload is the point: a file that will not parse or will not validate is caught here, at the
    /// moment of writing, rather than at the next launch with no clue which edit did it.
    pub fn save(&mut self) -> Result<(), String> {
        let path = std::path::Path::new(crate::site::layout::SITE_LAYOUT_PATH);
        let text = self.map.render();
        super::source_map::save_atomic(path, &text)?;

        let written =
            std::fs::read_to_string(path).map_err(|e| format!("site editor: {}: {e}", path.display()))?;
        let reloaded: SiteLayout = ron::from_str(&written)
            .map_err(|e| format!("site editor: wrote a file that will not parse — {e}"))?;
        reloaded
            .validate()
            .map_err(|e| format!("site editor: wrote a file that will not validate — {e}"))?;

        self.dirty = false;
        Ok(())
    }

    /// Apply an inverse op and return the op that would undo *this* change. Undo and redo are the
    /// same operation pointed at different stacks, which is why there is only one of these.
    fn apply_inverse(&mut self, op: EditOp) -> Result<(EditOp, Change), String> {
        match op {
            EditOp::Restore { index, prop, line } => {
                let counter = self.snapshot(index)?;
                self.map.set_prop_line(index, line)?;
                let slot = self
                    .layout
                    .props
                    .get_mut(index)
                    .ok_or_else(|| format!("site editor: no prop at index {index}"))?;
                *slot = prop;
                Ok((counter, Change::Moved(index)))
            }
            EditOp::Drop { index } => {
                let line = self.map.remove_prop(index)?;
                if index >= self.layout.props.len() {
                    return Err(format!("site editor: no prop at index {index}"));
                }
                let prop = self.layout.props.remove(index);
                Ok((
                    EditOp::Reinsert { index, prop, line },
                    Change::Removed(index),
                ))
            }
            EditOp::Reinsert { index, prop, line } => {
                self.map.restore_prop(index, line)?;
                if index > self.layout.props.len() {
                    return Err(format!(
                        "site editor: cannot restore prop at index {index}; only {} exist",
                        self.layout.props.len()
                    ));
                }
                self.layout.props.insert(index, prop);
                Ok((EditOp::Drop { index }, Change::Added(index)))
            }
        }
    }

    /// Capture what it would take to put record `index` back the way it is right now.
    fn snapshot(&self, index: usize) -> Result<EditOp, String> {
        let prop = self
            .layout
            .props
            .get(index)
            .ok_or_else(|| format!("site editor: no prop at index {index}"))?
            .clone();
        let line = self.map.prop_line(index)?.to_owned();
        Ok(EditOp::Restore { index, prop, line })
    }

    /// Record an undoable step. A fresh edit invalidates the redo stack, as in any editor.
    fn push_undo(&mut self, op: EditOp) {
        self.undo.push(op);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Re-run the placement rules and mark the document unsaved. Called after every mutation, so the
    /// fault list can never describe a layout that is no longer on screen.
    fn settle(&mut self, kit: &SiteKit) {
        self.faults = prop_placement_report(&self.layout, kit).faults;
        self.dirty = true;
    }
}
