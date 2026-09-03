//! **Every question this editor asks is asked the same way.**
//!
//! Three prompts grew independently and each invented its own vocabulary. Measured 2026-08-19,
//! before `crate::confirm`:
//!
//! | asked by | agree | refuse | third answer |
//! |---|---|---|---|
//! | `chooser` delete / quit | `Y` | `Esc` | — |
//! | `editor` leaving a dirty map | `S` | `Esc` | `D` discards |
//! | `labels` re-label judged pieces | `Enter` | `Esc` | — |
//!
//! Each was defensible where it was written. Together they were unlearnable — `Enter` meant *yes*
//! in one and was deliberately refused in another, on the same screen, minutes apart.
//!
//! **This file is the audit the fix asked for.** It is a source scan rather than a behavioural test
//! because what must not come back is a *shape*: a feature reading answer keys out of its own
//! `ButtonInput` instead of asking `confirm`. That is invisible to any test that only drives the
//! prompts that exist today, and it is exactly how the second vocabulary appeared the first time.
//!
//! The same argument `the_way_back_actually_goes_back.rs` makes for `Screen::Menu`, and
//! `compose_is_read_only.rs` for writes.

use std::path::Path;

fn read(rel: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

/// **The features that ask questions go through `confirm` and hold no prompt state of their own.**
///
/// `Chooser::ask` and `LabelQueue::ask` survive on purpose — they carry the *subject* (which map is
/// pending, which two target sets the walk would take), which the modal deliberately does not know.
/// What must not survive is a second place that decides which key means yes.
#[test]
fn every_prompt_is_raised_through_confirm() {
    for (file, asked) in [
        ("src/chooser.rs", "Asked::DeleteEntry"),
        ("src/chooser.rs", "Asked::QuitApp"),
        ("src/editor.rs", "Asked::LeaveMap"),
        ("src/labels.rs", "Asked::RelabelJudged"),
    ] {
        let src = read(file);
        assert!(
            src.contains(asked),
            "{file} no longer raises `{asked}`. Every question goes through `confirm` — if this \
             prompt was removed, take its arm out of `confirm::Asked` too, so the census stays a \
             census."
        );
    }
}

/// **`confirm` is the only place that reads an answer key.**
///
/// The keys are `Y`, `N` and `Esc`-as-`N`. `KeyY` outside `confirm.rs` means some feature has begun
/// answering its own question again; `KeyD`/`KeyS` as an *answer* is the specific shape that was
/// removed from the leaving prompt.
///
/// `Escape` is not policed here and cannot be: it is this editor's universal back-out and appears
/// in every peel, every field and every tool. What the modal owns is `Escape` **while a question is
/// up**, which `Confirm::is_open` gates rather than any spelling of the key.
#[test]
fn only_confirm_reads_the_answer_keys() {
    // `chooser.rs` is exempt for `KeyY` only in its own tests, which drive the old hint strings.
    for file in ["src/chooser.rs", "src/editor.rs", "src/labels.rs", "src/tiles.rs"] {
        let src = read(file);
        let code = src
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(&src)
            .to_owned();
        for needle in ["KeyCode::KeyY", "just_pressed(KeyCode::KeyD)"] {
            assert!(
                !code.contains(needle),
                "{file} reads `{needle}` outside a test. Answering a question is \
                 `confirm::Confirm`'s job — a second feature deciding what yes looks like is how \
                 this editor came to have three agree keys at once. Ask through `Confirm::ask` and \
                 read the result with `Confirm::answer`."
            );
        }
    }
}

/// **The prompt is a modal, not a line in the status bar.**
///
/// The old questions rendered where this editor puts commentary — bottom-left, same colour as
/// `baked 82 palette thumbnails`. A question that blocks progress has to be the thing on screen, so
/// the panel is centred over a scrim and the scrim eats the click.
///
/// **Where those facts live moved on 2026-09-03, and this test moved with them.** Three modals
/// shipped three contracts — different z, different padding, different scrim, one of them written
/// inline as `srgba(0, 0, 0, 0.72)` beside a constant whose own doc says it exists so a second modal
/// cannot dim by a different amount. They share `chrome::modal_card` now, so the scrim, the
/// centring and the z-index are *its* properties and asserting them against `confirm.rs`'s source
/// text would fail on a change that improved exactly the thing this test is about.
///
/// So each half is checked where it is: confirm goes through the shell, and the shell has the
/// properties. What stays in `confirm.rs` is what is genuinely confirm's — that both answers are
/// clickable, and that the question is not *also* written into the band it was moved off.
#[test]
fn the_prompt_is_centred_and_blocks_what_is_behind_it() {
    let src = read("src/confirm.rs");
    for (needle, why) in [
        (
            "modal_card",
            "one shell for every question, so a second modal cannot dim, pad or stack differently",
        ),
        (
            "ConfirmButton",
            "both answers are clickable; keyboard-first is not keyboard-only",
        ),
    ] {
        assert!(src.contains(needle), "confirm.rs lost `{needle}` — {why}");
    }

    let shell = read("src/chrome.rs");
    let card = shell
        .split_once("pub fn modal_card")
        .map(|(_, rest)| rest)
        .unwrap_or_else(|| panic!("`chrome::modal_card` is the shell every question is asked in"));
    for (needle, why) in [
        ("SCRIM", "the backdrop dims the application, so the question is the only lit thing"),
        (
            "justify_content: JustifyContent::Center",
            "centred — the whole point of the move off the status band",
        ),
        (
            "GlobalZIndex(MODAL_Z)",
            "above every panel, or a dock draws over the question — and named, so two overlays \
             cannot disagree about which is in front",
        ),
    ] {
        assert!(card.contains(needle), "`chrome::modal_card` lost `{needle}` — {why}");
    }

    assert!(
        !src.contains("status.note") && !src.contains("status.problem"),
        "the prompt must not write itself into the status line as well; that is the band it was \
         moved off, and two copies of one question is worse than the one it replaced"
    );
}
