//! **The preview and the commit ask one question, in one place.**
//!
//! `editor.rs` opens with the rule: *"a preview drawn somewhere the piece will NOT end up is worse
//! than no preview, because it is a promise the game then breaks."* It was broken anyway, and the
//! shape of the break is why this is a test rather than a comment.
//!
//! `a10cadf` — the commit that made a tile land filling a cell — changed *where a piece lands* by
//! threading the piece's real footprint into `map_at`. It updated four call sites: `drive_place`,
//! both branches of `drive_move`, and `drive_stamp_ghost`. It missed the fifth. `drive_ghost` kept
//! passing a hardcoded `(0.0, 0.0)` span, and a zero span makes `grid::snap_corner` degenerate from
//! corner-snapping to rounding the **centre** — so the preview and the click sat on lattices half a
//! tile apart for any piece an odd number of cells across.
//!
//! Nothing caught it. The only test named for this property,
//! `editor.rs`'s `the_ghost_stands_where_the_drop_lands`, is about the **clone** ghost.
//!
//! # Why a source scan and not a behavioural test
//!
//! Both functions are Bevy systems needing a camera, a viewport and a cursor ray — `drive_ghost`'s
//! own sibling records that it *"cannot be driven headless: it needs `cursor_ground`, which needs a
//! [viewport]"*. So the arithmetic is unit-tested where it lives (`editor.rs`'s `snap_tests`), and
//! what no unit test can see is the thing that actually broke: **two call sites that must agree,
//! updated by hand.**
//!
//! The fix is `editor::brush_at` — one expression both callers ask. This test is what keeps it one.

use std::path::Path;

/// The body of a column-zero `fn <name>(`, as `(1-based line, text)`.
///
/// Column-zero open, column-zero `}` close — the same two-line rule `compose_is_read_only.rs` uses
/// for `#[cfg(test)]` blocks, and it holds for the same reason: these are top-level items.
///
/// Returns `None` when the function is absent, which the callers treat as a failure rather than a
/// pass. A scan that quietly finds nothing is the defect that file's doc comment records: *"a ratchet
/// that cannot fail is worse than no ratchet, because it reads as a guarantee."*
fn body_of<'a>(src: &'a str, name: &str) -> Option<Vec<(usize, &'a str)>> {
    let open = format!("fn {name}(");
    let pub_open = format!("pub {open}");
    let lines: Vec<&str> = src.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.starts_with(&open) || l.starts_with(&pub_open))?;
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if *line == "}" {
            return Some(out);
        }
        out.push((i + 1, *line));
    }
    None
}

/// Code with any trailing line comment removed, so a `//` mentioning a call is not a call.
fn code(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

fn editor_src() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/editor.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// **Neither the preview nor the commit rolls its own brush landing.**
///
/// Stated on `brush_span` rather than on `map_at`, and the distinction is the whole precision of this
/// test. `map_at` has several legitimate callers inside these very functions — `drive_place`'s first
/// branch snaps an armed **stamp** through `stamp_snap`, which is a different question with a
/// different answer. Banning `map_at` outright fails on that line, which is correct code.
///
/// `brush_span` is the narrower thing: it is *the piece's own footprint*, and asking for it here is
/// the first half of deciding where a brush lands. The second half is `map_at`. Doing both locally is
/// precisely what `drive_ghost` and `drive_place` each did, and how they drifted apart.
#[test]
fn the_brush_preview_and_the_brush_commit_ask_the_same_question() {
    let src = editor_src();

    for f in ["drive_ghost", "drive_place"] {
        let body = body_of(&src, f).unwrap_or_else(|| {
            panic!(
                "`{f}` is gone from editor.rs — this test names a function that no longer exists, \
                 so it is guarding nothing. Repoint it or delete it deliberately."
            )
        });

        let rolled: Vec<String> = body
            .iter()
            .filter(|(_, l)| code(l).contains("brush_span("))
            .map(|(n, l)| format!("  src/editor.rs:{n}  {}", l.trim()))
            .collect();
        assert!(
            rolled.is_empty(),
            "`{f}` measures the brush itself instead of asking `brush_at`. That is how the preview \
             and the click ended up on lattices half a tile apart: two call sites that must agree, \
             updated by hand, and `a10cadf` updated four of five. If this genuinely needs a \
             different landing from the other, that is a design change and this test is where to \
             argue it:\n{}",
            rolled.join("\n")
        );

        assert!(
            body.iter().any(|(_, l)| code(l).contains("brush_at(")),
            "`{f}` no longer asks `brush_at`. Both the preview and the commit must, or there is \
             nothing keeping them on one lattice."
        );
    }
}

/// **And the scan can see a `map_at` call at all** — without this the test above is a guarantee
/// about a function that returns nothing.
///
/// The same argument `compose_is_read_only.rs` makes for scanning `editor.rs` after `compose.rs`:
/// asserting only that two functions are silent would also pass if `map_at` had been renamed, or if
/// `body_of` had quietly stopped finding bodies.
#[test]
fn the_scan_can_see_a_map_at_call_where_one_belongs() {
    let src = editor_src();
    let body = body_of(&src, "brush_at")
        .expect("`brush_at` is gone — it is the one expression the preview and the commit share");

    assert!(
        body.iter().any(|(_, l)| code(l).contains("map_at(")),
        "`brush_at` does not call `map_at`, so the scan above is looking for something that never \
         appears and would pass over any code at all"
    );
    assert!(
        body.iter().any(|(_, l)| code(l).contains("brush_span(")),
        "`brush_at` does not snap by the piece's own footprint — a zero span rounds the CENTRE, \
         which is the defect this whole file exists for"
    );
}

/// **A system that repaints something it does not own must use `try_insert`.**
///
/// `brighten_held` and `fade_ghost` both walk a hierarchy they did not spawn — the staged tile and
/// the ghosts — and hand each mesh a fresh material. Both of those hierarchies are despawned and
/// respawned **wholesale** by the systems that do own them, on every `Build` change, which is what
/// an arrow key is. So an entity read this frame can be gone before the command applies, and in
/// Bevy 0.19 `EntityCommands::insert` on a despawned entity is a **hard panic**, not a no-op:
///
/// ```text
/// Encountered an error in command `insert<(MeshMaterial3d<StandardMaterial>, Brightened)>`:
/// Entity despawned: The entity with ID 800v16 is invalid; its index now has generation 17.
/// ```
///
/// That killed the editor mid-session on 2026-08-15, from nothing more exotic than nudging a held
/// piece. `fade_ghost` had the identical shape and had simply not lost the race yet.
///
/// # Why a source scan and not a behavioural test
///
/// The same reason the scan above exists, and it was measured rather than assumed: a headless test
/// that drops a piece and nudges it six times **passes with the bug put back**. `MinimalPlugins`
/// has no renderer, so the GLB scene children these systems walk are never spawned, `painted` never
/// matches, and no command is ever queued. A test that cannot fail reads as a guarantee, so it was
/// written, checked against the reverted fix, and deleted.
///
/// What is left is the rule itself: these two systems queue their paint with `try_insert`.
#[test]
fn the_systems_that_repaint_borrowed_entities_tolerate_a_despawn() {
    let src = editor_src();

    for f in ["brighten_held", "fade_ghost"] {
        let body = body_of(&src, f).unwrap_or_else(|| {
            panic!(
                "`{f}` is gone from editor.rs — this test names a function that no longer exists, \
                 so it is guarding nothing. Repoint it or delete it deliberately."
            )
        });

        let bare: Vec<String> = body
            .iter()
            .filter(|(_, l)| {
                let c = code(l);
                // `.insert(` that is not `.try_insert(` — the panicking form.
                c.contains(".insert(") && !c.contains(".try_insert(")
            })
            .map(|(n, l)| format!("  src/editor.rs:{n}  {}", l.trim()))
            .collect();
        assert!(
            bare.is_empty(),
            "`{f}` inserts into an entity it does not own with the panicking form. That hierarchy \
             is rebuilt wholesale while this system is mid-flight, so the entity can be gone before \
             the command applies — which is a crash, not a dropped paint. Use `try_insert`:\n{}",
            bare.join("\n")
        );

        assert!(
            body.iter().any(|(_, l)| code(l).contains(".try_insert(")),
            "`{f}` no longer paints anything at all — if that is deliberate, delete its arm here."
        );
    }
}
