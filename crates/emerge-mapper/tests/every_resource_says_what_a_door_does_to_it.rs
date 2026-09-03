//! **Every resource the editor registers has to say what a door change does to it.**
//!
//! `screen.rs` claims a door change is a full teardown, and defends it: *"A reload cannot be wrong;
//! a partial teardown can be, silently, and the bug lands weeks later looking like something
//! else."* Measured on 2026-08-17, that is not what the code does — entities are swept by
//! reachability, four resources are named, and the rest are not touched. Every `OnEnter(Editor)`
//! system is a spawn, so nothing resets them on the way in either. The door change is **already**
//! the partial teardown the comment is spent avoiding, and the bug class it warns about is already
//! open: edit a tile in kit A, leave, open kit B, and A's undo stack is there to be replayed into B.
//!
//! So this is the list, and this is what keeps it honest. It is
//! `docs/2026-08-17-one-application.md` §6 step 1 — no behaviour change, and worth building even if
//! the rest of that design is dropped, because the fifty-odd survivors are live today and were
//! unclassified.
//!
//! **It boots the editor and asks the World**, rather than reading source for `init_resource`. A
//! source scan would miss a resource inserted by a nested plugin, or one added by a `Commands` call,
//! which is exactly the kind nobody remembers to classify.

mod fixtures;

use emerge_mapper::harness;
use emerge_mapper::screen::{ownership, OWNERSHIP};
use fixtures::Fixture;

/// The editor's own resources, as the World reports them after a real boot.
///
/// **The caller names the fixture**, because these tests run in parallel and `Fixture::new` builds a
/// directory from the name it is given — three tests sharing one name is three processes writing one
/// temp project, which fails as a resource that is briefly missing rather than as anything that
/// names the collision. Caught here by exactly that: one run of three passed, the next failed one.
fn editors_resources(fixture: &str) -> Vec<String> {
    let root = Fixture::new(fixture)
        .descriptor("wall", "alpha")
        .place("wall", (0.0, 0.0))
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();
    let mut names: Vec<String> = app
        .world()
        .iter_resources()
        .map(|(info, _)| info.name().to_string())
        // Bevy's own resources are Bevy's business; a door change is this crate's question.
        .filter(|n| n.starts_with("emerge_mapper::"))
        .collect();
    names.sort();
    names
}

/// **Nothing arrives unanswered.**
#[test]
fn every_resource_is_classified() {
    let live = editors_resources("resources-classified");
    assert!(
        live.len() > 40,
        "expected the editor's resources after a boot; found {}. If this ever reads near zero the \
         assertion below passes for the wrong reason.",
        live.len()
    );
    let unanswered: Vec<&String> = live.iter().filter(|n| ownership(n).is_none()).collect();
    assert!(
        unanswered.is_empty(),
        "these resources do not say what a door change does to them. Add each to \
         `screen::OWNERSHIP` — `Project` if it is read off disk, `Door` if it is this door's \
         working state, `Session` if it is true for as long as the app runs. When unsure, `Door`: \
         it is what a full teardown would do, so a wrong guess is conservative rather than a stale \
         value nobody looks for.\n{unanswered:#?}"
    );
}

/// **And nothing lingers in the list after the resource is gone.**
///
/// The other half, and the one that rots quietly: a classification naming a type nobody registers
/// any more reads as coverage. `census_is_the_one_counter.rs` and `leaf.rs` both carry this pair for
/// the same reason.
#[test]
fn the_classification_names_nothing_that_is_gone() {
    let live = editors_resources("resources-not-stale");
    let stale: Vec<&str> = OWNERSHIP
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !live.iter().any(|l| l == name))
        .collect();
    assert!(
        stale.is_empty(),
        "`screen::OWNERSHIP` names resources the editor no longer registers. Drop them — a list \
         that answers for types nobody has reads as coverage it does not have:\n{stale:#?}"
    );
}

/// **The door's own state is the largest class, and that is the finding rather than an accident.**
///
/// Fifty-six resources survived a door change unreset when this was measured. If that number ever
/// collapses it is because somebody reclassified working state as `Session` to make a reset
/// cheaper, which is the bug this file is about, written down as a decision.
#[test]
fn the_working_state_is_named_as_such() {
    use emerge_mapper::screen::Ownership;
    let doors = OWNERSHIP
        .iter()
        .filter(|(_, c)| *c == Ownership::Door)
        .count();
    assert!(
        doors >= 30,
        "only {doors} resources are classified as the door's own working state. That was 40+ when \
         the list was written; a drop means working state has been reclassified as `Session`, and \
         `Session` means it survives into the next kit."
    );
}

/// **And the classification is now executed, not merely declared.**
///
/// This file's own header records the finding it was written for: *"The door change is **already**
/// the partial teardown the comment is spent avoiding, and the bug class it warns about is already
/// open: edit a tile in kit A, leave, open kit B, and A's undo stack is there to be replayed into
/// B."* It stayed open, and on 2026-09-03 it was reproduced on camera as something louder than an
/// undo stack: opening a second kit showed the **first** kit's meshes, counts, tile list, cursors
/// and staged piece, under a chrome bar correctly naming the second.
///
/// `screen::reset_door_state` closes it, and this is what stops the two halves drifting. A
/// hand-written list of forty types is exactly the thing that goes stale — which is the failure this
/// whole file is about — so the list and the table are compared in both directions.
#[test]
fn the_door_resets_what_it_says_it_owns() {
    use emerge_mapper::screen::{door_state_type_paths, Ownership};

    // `Frame` is the one deliberate absence. It holds entity ids from the screen that spawned them,
    // so re-initialising it would be meaningless — `chrome::spawn_frame` replaces it wholesale on
    // every entry, which is a stronger guarantee than a reset. The table says the same thing.
    const REPLACED_ON_ENTRY: &[&str] = &["emerge_mapper::chrome::Frame"];

    let declared: Vec<&str> = OWNERSHIP
        .iter()
        .filter(|(_, c)| *c == Ownership::Door)
        .map(|(n, _)| *n)
        .filter(|n| !REPLACED_ON_ENTRY.contains(n))
        .collect();
    let reset = door_state_type_paths();

    let unreset: Vec<&&str> = declared.iter().filter(|n| !reset.contains(n)).collect();
    assert!(
        unreset.is_empty(),
        "classified as the door's own working state and NOT reset when the door closes — each of \
         these survives into the next kit, which is the bug this file was opened for:\n{unreset:#?}"
    );

    let unclassified: Vec<&&str> = reset
        .iter()
        .filter(|n| !declared.contains(n) && !REPLACED_ON_ENTRY.contains(n))
        .collect();
    assert!(
        unclassified.is_empty(),
        "reset when the door closes but not classified `Ownership::Door` — the table is what a \
         reader consults, so a reset it does not mention is a reset nobody can find:\n{unclassified:#?}"
    );
}
