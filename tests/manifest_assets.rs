//! **The shipped furniture manifest points at files that exist.**
//!
//! Lifted out of `forge-core`'s `placement::manifest` when the workspace split landed (Stage 0b of
//! `docs/2026-08-03-forge-plan.md`). It is a fact about *this game's* assets and *this game's*
//! `config.ron`, not about the manifest schema, so it belongs on the game side of the boundary — the
//! crate must not know how a particular game loads its config.
//!
//! GPU-free and `App`-free, so it stays in the `cargo test` hard gate where it was.

use foundation_vs_slop::config::load_game_config;

/// Every `glb` path in the SHIPPED manifest resolves to a real file under `assets/`.
///
/// The failure this catches is silent. A mistyped path is a perfectly valid manifest — it parses,
/// validates, and the solver happily places the item; Bevy then fails the asset load and the prop
/// simply never appears. Nothing asserts, and a room that is one chair short looks exactly like a
/// room the layout put one chair in. With ~130 hand-written paths that is a matter of time.
///
/// Runs against `assets/config/config.ron` rather than a fixture, because the point is to check the
/// paths that actually ship.
#[test]
fn every_shipped_manifest_glb_exists_on_disk() {
    let cfg = load_game_config().expect("shipped game config must load");
    let assets = std::path::Path::new("assets");
    let missing: Vec<&str> = cfg
        .placement
        .furniture
        .items
        .iter()
        .map(|i| i.glb.as_str())
        .filter(|glb| !assets.join(glb).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "manifest rows point at {} file(s) that do not exist under assets/ — each would be placed \
         by the solver and then silently fail to load: {missing:#?}",
        missing.len()
    );
}
