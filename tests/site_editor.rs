//! **The Site editor's writer contract** — that editing `site67.ron` in-game cannot damage it.
//!
//! GPU-free and `App`-free, so these run in the `cargo test` hard gate on every push.
//!
//! The thing being defended is concrete: the shipped layout is 1401 lines of which **217 are
//! comments**, and the `props` list this editor touches most carries *more* comment lines than
//! records. A serializer-based writer would delete all of it on the first save and no test would
//! notice, because the file would still parse. So the contract is stated in bytes, not in "it still
//! loads": a no-op save is byte-identical, and a one-prop move changes exactly one line.

use foundation_vs_slop::site::kit::{load_site_kit, SITE_KIT_PATH};
use foundation_vs_slop::site::layout::{
    check_prop_placements, PropPlacement, SiteLayout, SITE_LAYOUT_PATH,
};
use foundation_vs_slop::site::pieces::SitePiece;
use foundation_vs_slop::site_editor::edit::EditorDoc;
use foundation_vs_slop::site_editor::source_map::{replace_field, trailing_comment, SourceMap};

/// The shipped layout as (text, parsed). Every test starts here so they all speak about the real
/// file rather than a fixture that could drift away from it.
fn shipped() -> (String, SiteLayout) {
    let text = std::fs::read_to_string(SITE_LAYOUT_PATH)
        .unwrap_or_else(|e| panic!("{SITE_LAYOUT_PATH}: {e}"));
    let layout: SiteLayout =
        ron::from_str(&text).unwrap_or_else(|e| panic!("{SITE_LAYOUT_PATH}: {e}"));
    (text, layout)
}

fn map_of(text: &str, layout: &SiteLayout) -> SourceMap {
    SourceMap::parse(text, layout).unwrap_or_else(|e| panic!("source map: {e}"))
}

/// The load-bearing one. If a no-op save is not byte-identical, every save silently reformats the
/// file and the 217 comment lines are living on borrowed time.
#[test]
fn a_save_that_changes_nothing_rewrites_nothing() {
    let (text, layout) = shipped();
    let map = map_of(&text, &layout);
    assert_eq!(
        map.render(),
        text,
        "an unedited source map must render the input byte for byte"
    );
}

/// The scan has to account for every record the parser found, in every list the editor owns.
/// A silent miscount is how an editor starts writing to the wrong line.
#[test]
fn the_scan_accounts_for_every_owned_record() {
    let (text, layout) = shipped();
    let map = map_of(&text, &layout);
    // `parse` already cross-checks the counts and errors out; reaching here means it agreed. Assert
    // the shipped file is actually non-trivial, so this test cannot pass vacuously on an empty list.
    assert!(
        layout.props.len() >= 50,
        "expected the shipped layout to carry real dressing, found {} props",
        layout.props.len()
    );
    assert!(!layout.cells.is_empty() && !layout.spawns.is_empty());
    let _ = map;
}

/// Moving one prop must change one line — not reflow the block around it.
#[test]
fn moving_a_prop_changes_exactly_one_line() {
    let (text, layout) = shipped();
    let mut map = map_of(&text, &layout);

    map.set_prop_pos(0, (12.5, 34.5))
        .unwrap_or_else(|e| panic!("set_prop_pos: {e}"));
    let after = map.render();

    let before_lines: Vec<&str> = text.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    assert_eq!(
        before_lines.len(),
        after_lines.len(),
        "a move must not add or remove lines"
    );
    let changed: Vec<usize> = (0..before_lines.len())
        .filter(|&i| before_lines[i] != after_lines[i])
        .collect();
    assert_eq!(
        changed.len(),
        1,
        "expected exactly one changed line, got {changed:?}"
    );
    assert!(
        after_lines[changed[0]].contains("pos: (12.5, 34.5)"),
        "the new position should be written verbatim: {}",
        after_lines[changed[0]]
    );

    // And the result must still be a valid layout.
    let reparsed: SiteLayout = ron::from_str(&after).unwrap_or_else(|e| panic!("reparse: {e}"));
    assert_eq!(reparsed.props[0].pos, (12.5, 34.5));
    assert_eq!(
        reparsed.props.len(),
        layout.props.len(),
        "a move must not change the record count"
    );
}

/// A rotation must not reformat the position, so "I turned this chair" is a one-field diff.
#[test]
fn rotating_a_prop_leaves_its_position_bytes_alone() {
    let (text, layout) = shipped();
    let mut map = map_of(&text, &layout);
    let before = map.prop_line(0).map(str::to_owned).expect("prop 0");

    map.set_prop_yaw(0, 45.0)
        .unwrap_or_else(|e| panic!("set_prop_yaw: {e}"));
    let after = map.prop_line(0).expect("prop 0");

    // The `pos: (...)` field, closing paren included, must be untouched byte for byte.
    let pos_span = |l: &str| {
        let at = l.find("pos:").expect("pos field");
        let end = l[at..].find(')').expect("pos field end") + at + 1;
        l[at..end].to_owned()
    };
    assert_eq!(
        pos_span(&before),
        pos_span(after),
        "rotating must not touch the pos field"
    );

    // Assert the value through the parser rather than through spacing: how many spaces pad `yaw:`
    // is the author's business and this test must not freeze it.
    let reparsed: SiteLayout =
        ron::from_str(&map.render()).unwrap_or_else(|e| panic!("reparse: {e}"));
    assert_eq!(reparsed.props[0].yaw, 45.0);
    assert_eq!(
        reparsed.props[0].pos, layout.props[0].pos,
        "rotating must not move the prop"
    );
}

/// Undo must restore the bytes, comment included. This is why `remove_prop` hands back the whole
/// source line rather than a reconstructed record.
#[test]
fn deleting_a_prop_and_undoing_restores_the_file_byte_for_byte() {
    let (text, layout) = shipped();
    let mut map = map_of(&text, &layout);

    // Pick a prop that actually carries a trailing comment — those are the ones with something to
    // lose, and the shipped file has several (`// records desk`, `// requisition counter`).
    let commented = (0..layout.props.len())
        .find(|&i| map.prop_line(i).ok().and_then(trailing_comment).is_some())
        .expect("the shipped layout should have at least one commented prop record");

    let removed = map
        .remove_prop(commented)
        .unwrap_or_else(|e| panic!("remove_prop: {e}"));
    assert!(
        trailing_comment(&removed).is_some(),
        "the removed line should have carried the comment: {removed}"
    );
    assert_ne!(map.render(), text, "the delete should have changed the file");

    map.restore_prop(commented, removed)
        .unwrap_or_else(|e| panic!("restore_prop: {e}"));
    assert_eq!(
        map.render(),
        text,
        "undoing a delete must restore the file byte for byte, comment included"
    );
}

/// An inserted record has to parse, and the layout the editor believed it had must be the layout that
/// comes back off disk.
#[test]
fn an_inserted_prop_parses_and_lands_where_the_editor_put_it() {
    let (text, layout) = shipped();
    let mut map = map_of(&text, &layout);

    let want = PropPlacement {
        piece: SitePiece::Crate,
        pos: (41.5, 29.5),
        yaw: 15.0,
        waive: None,
    };
    let ix = map
        .insert_prop(&want)
        .unwrap_or_else(|e| panic!("insert_prop: {e}"));
    assert_eq!(ix, layout.props.len(), "an insert appends");

    let after = map.render();
    let reparsed: SiteLayout = ron::from_str(&after).unwrap_or_else(|e| panic!("reparse: {e}"));
    assert_eq!(reparsed.props.len(), layout.props.len() + 1);
    let got = &reparsed.props[ix];
    assert_eq!(got.piece, want.piece);
    assert_eq!(got.pos, want.pos);
    assert_eq!(got.yaw, want.yaw);

    // And the source map must still agree with the document it just produced, which is the invariant
    // that keeps a second edit from writing to the wrong line.
    SourceMap::parse(&after, &reparsed)
        .unwrap_or_else(|e| panic!("the map must re-parse its own output: {e}"));
}

/// A waiver is a reason string that has to survive the round trip, including quoting.
#[test]
fn an_inserted_waiver_round_trips_with_its_reason() {
    let (text, layout) = shipped();
    let mut map = map_of(&text, &layout);
    let reason = r#"deliberately overhangs the "counter""#;
    let ix = map
        .insert_prop(&PropPlacement {
            piece: SitePiece::Mug,
            pos: (9.0, 27.5),
            yaw: 0.0,
            waive: Some(reason.to_owned()),
        })
        .unwrap_or_else(|e| panic!("insert_prop: {e}"));

    let reparsed: SiteLayout =
        ron::from_str(&map.render()).unwrap_or_else(|e| panic!("reparse: {e}"));
    assert_eq!(reparsed.props[ix].waive.as_deref(), Some(reason));
}

/// The checker the editor will run per-edit has to actually catch a bad edit. This is the oracle the
/// live fault overlay reads, so if it stays silent here the overlay would too.
#[test]
fn a_move_that_overlaps_another_prop_is_caught_by_the_placement_checker() {
    let (_, layout) = shipped();
    let kit = load_site_kit(SITE_KIT_PATH).unwrap_or_else(|e| panic!("site kit: {e}"));

    check_prop_placements(&layout, &kit)
        .unwrap_or_else(|e| panic!("the shipped layout should be clean, but: {e}"));

    // Drop prop 1 exactly on top of prop 0. Both are solid dressing, so the overlap rule must fire.
    let mut broken = layout.clone();
    let onto = broken.props[0].pos;
    broken.props[1].pos = onto;
    broken.props[1].waive = None;

    let faults = check_prop_placements(&broken, &kit)
        .expect_err("two props in the same spot must be a fault");
    assert!(
        faults.contains("overlap") || faults.contains("Overlap"),
        "expected an overlap fault, got: {faults}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// `EditorDoc` — the layout, its text and the undo stack moving together.
//
// None of these call `EditorDoc::save`, deliberately: that writes to the shipped `site67.ron` by
// design (one path, no injectable location), and a test suite that could overwrite the real level
// data is a worse hazard than the coverage is worth. `EditorDoc::text` gives the same bytes without
// the write.
// ─────────────────────────────────────────────────────────────────────────────────────────────────

fn doc() -> (String, EditorDoc) {
    let (text, layout) = shipped();
    let kit = load_site_kit(SITE_KIT_PATH).unwrap_or_else(|e| panic!("site kit: {e}"));
    let doc = EditorDoc::open(&layout, &kit).unwrap_or_else(|e| panic!("open: {e}"));
    (text, doc)
}

fn kit() -> foundation_vs_slop::site::kit::SiteKit {
    load_site_kit(SITE_KIT_PATH).unwrap_or_else(|e| panic!("site kit: {e}"))
}

#[test]
fn a_freshly_opened_document_is_clean_and_matches_the_file() {
    let (text, doc) = doc();
    assert!(!doc.dirty, "opening is not an edit");
    assert!(!doc.can_undo() && !doc.can_redo());
    assert_eq!(doc.text(), text);
    assert!(
        doc.faults.is_empty(),
        "the shipped layout should have no placement faults, found: {:?}",
        doc.faults
    );
}

#[test]
fn undoing_a_move_restores_both_the_record_and_the_bytes() {
    let (text, mut doc) = doc();
    let kit = kit();
    let before = doc.layout.props[3].clone();

    doc.move_prop(3, (20.0, 20.0), 45.0, &kit)
        .unwrap_or_else(|e| panic!("move: {e}"));
    assert!(doc.dirty);
    assert_ne!(doc.text(), text);
    assert_eq!(doc.layout.props[3].pos, (20.0, 20.0));

    doc.undo(&kit).expect("something to undo").expect("undo");
    assert_eq!(doc.layout.props[3].pos, before.pos);
    assert_eq!(doc.layout.props[3].yaw, before.yaw);
    assert_eq!(doc.text(), text, "undo must restore the bytes");
}

#[test]
fn redo_replays_what_undo_took_back() {
    let kit = kit();
    let (_, mut doc) = doc();

    doc.move_prop(3, (20.0, 20.0), 45.0, &kit)
        .unwrap_or_else(|e| panic!("move: {e}"));
    let moved = doc.text();

    doc.undo(&kit).expect("something to undo").expect("undo");
    assert!(doc.can_redo());
    doc.redo(&kit).expect("something to redo").expect("redo");

    assert_eq!(doc.text(), moved, "redo must reproduce the edit exactly");
    assert_eq!(doc.layout.props[3].pos, (20.0, 20.0));
}

/// The index-shifting case — the one that would silently write to the wrong line if the source map
/// and the layout disagreed about which record is which.
#[test]
fn deleting_a_middle_record_keeps_the_map_and_the_layout_in_step() {
    let kit = kit();
    let (text, mut doc) = doc();
    let after_target = doc.layout.props[5].clone();

    doc.delete_prop(4, &kit)
        .unwrap_or_else(|e| panic!("delete: {e}"));
    // What was record 5 is now record 4, in the layout AND in the text.
    assert_eq!(doc.layout.props[4].pos, after_target.pos);
    assert_eq!(doc.layout.props[4].piece, after_target.piece);
    let line = doc.prop_line(4).expect("prop 4");
    assert!(
        line.contains(&format!("{:?}", after_target.piece)),
        "the map should now point at the record that moved up: {line}"
    );

    // And the document still describes itself: re-parsing its own output must agree.
    let reparsed: SiteLayout = ron::from_str(&doc.text()).unwrap_or_else(|e| panic!("reparse: {e}"));
    assert_eq!(reparsed.props.len(), doc.layout.props.len());
    assert_eq!(reparsed.props[4].pos, after_target.pos);

    doc.undo(&kit).expect("something to undo").expect("undo");
    assert_eq!(doc.text(), text);
}

#[test]
fn a_fresh_edit_clears_the_redo_stack() {
    let kit = kit();
    let (_, mut doc) = doc();
    doc.move_prop(3, (20.0, 20.0), 45.0, &kit).expect("move");
    doc.undo(&kit).expect("undo available").expect("undo");
    assert!(doc.can_redo());

    doc.move_prop(4, (21.0, 21.0), 0.0, &kit).expect("move");
    assert!(
        !doc.can_redo(),
        "a new edit must invalidate the redo stack, as in any editor"
    );
}

/// The live fault list is what the overlay marks props from, so it has to name the right record —
/// not just report that *something* is wrong.
#[test]
fn an_illegal_move_names_the_record_that_broke_the_rule() {
    let kit = kit();
    let (_, mut doc) = doc();
    assert!(doc.faults.is_empty());

    let onto = doc.layout.props[0].pos;
    doc.move_prop(1, onto, doc.layout.props[1].yaw, &kit)
        .unwrap_or_else(|e| panic!("move: {e}"));

    assert!(
        !doc.faults.is_empty(),
        "dropping a prop onto another must raise a fault"
    );
    let overlap = doc
        .faults
        .iter()
        .find(|f| f.message.contains("overlaps"))
        .expect("an overlap fault");
    let named = [Some(overlap.prop), overlap.other]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert!(
        named.contains(&0) && named.contains(&1),
        "an overlap must name BOTH records — either moving would fix it. Got {named:?}"
    );

    // And it clears when the edit is taken back.
    doc.undo(&kit).expect("undo available").expect("undo");
    assert!(doc.faults.is_empty(), "undo must clear the fault it caused");
}

/// `replace_field` is the whole reason a save is non-destructive, so pin its edge cases directly
/// rather than only through the file.
#[test]
fn field_replacement_preserves_padding_and_trailing_comments() {
    let line = "        ( piece: WallLow,    pos: ( 8.5, 27.5), yaw:  90.0 ),   // records desk";

    let moved = replace_field(line, "pos", "(9.0, 28.0)").expect("replace pos");
    assert_eq!(
        moved,
        "        ( piece: WallLow,    pos: (9.0, 28.0), yaw:  90.0 ),   // records desk"
    );
    assert_eq!(trailing_comment(&moved), Some("// records desk"));

    // Padding after the colon belongs to the author and stays.
    let turned = replace_field(line, "yaw", "0.0").expect("replace yaw");
    assert!(turned.contains("yaw:  0.0 )"), "{turned}");

    // A `//` inside a string is data, not a comment — the field scanner must not cut there.
    let labelled = r#"        ( cell: ( 30, 35), yaw:  90.0, clearance: None, label: "A // B" ),"#;
    let out = replace_field(labelled, "yaw", "0.0").expect("replace yaw beside a quoted slash");
    assert!(out.contains(r#"label: "A // B""#), "{out}");
    assert!(out.contains("yaw:  0.0,"), "{out}");

    // A missing field is a loud error, never a silent no-op that would look like a successful save.
    assert!(replace_field(line, "nope", "1.0").is_err());
}
