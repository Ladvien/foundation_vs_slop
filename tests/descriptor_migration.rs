//! **The migration gate: a diff, not a judgement.**
//!
//! Stage 1 of `docs/2026-08-03-emerge-mapper-plan.md` replaces two asset schemas with one. The plan's
//! gate for that is deliberately mechanical — *"converted descriptors reproduce today's semantics
//! exactly"* — because "I read all 41 rows and they look right" is not a thing anyone can check, and
//! the failure it would miss is silent: a dropped field means a prop that is placed and never
//! appears, or a room one chair short, which looks exactly like a room the layout put one chair in.
//!
//! So this converts the **shipped** manifests to descriptors, converts them back, and compares. A
//! field lost in either direction fails here rather than in a screenshot three weeks from now.
//!
//! It also runs every converted descriptor through `assets/emerge/vocab.ron`, which is the first time
//! anything in this project has validated a prop's tags. `docs/2026-08-03-asset-schema-audit.md` §2:
//! of eight shipped affordance tokens, four have no reader anywhere and nothing ever said so.
//!
//! GPU-free and `App`-free, so it belongs in the `cargo test` hard gate.

use std::path::Path;

use emerge_core::convert::{
    descriptor_from_manifest, manifest_from_descriptor, without_dropped_affordances, Policy,
};
use emerge_core::placement::manifest::{load_manifest, FurnitureManifest};
use emerge_core::vocab::Vocabularies;
use foundation_vs_slop::config::load_game_config;

/// This project's answer to the one question the assets cannot answer for themselves. Matches
/// `placement::furnish::WALL_LIGHT_HEIGHT`, which is where the sconce row gets its height today.
const POLICY: Policy = Policy {
    wall_mount_height: 1.8,
};

fn vocab() -> Vocabularies {
    let text = std::fs::read_to_string("assets/emerge/vocab.ron")
        .unwrap_or_else(|e| panic!("assets/emerge/vocab.ron: {e}"));
    Vocabularies::parse(&text).unwrap_or_else(|e| panic!("{e}"))
}

/// Both shipped manifests: the one `config.ron` embeds, and the standalone asset-swap kit that
/// `placement::acceptance_tests` uses to prove the solver is kit-agnostic. Converting only the first
/// would leave the second free to drift into a shape the converter cannot read.
fn manifests() -> Vec<(&'static str, FurnitureManifest)> {
    let cfg = load_game_config().expect("shipped game config must load");
    let kenney = load_manifest("assets/config/furniture_kenney.ron").expect("kit B must parse");
    vec![
        ("config.ron:placement.furniture", cfg.placement.furniture),
        ("furniture_kenney.ron", kenney),
    ]
}

/// **The gate.** Every shipped row survives the round trip, field for field.
#[test]
fn every_shipped_manifest_row_round_trips_through_a_descriptor() {
    let mut checked = 0usize;
    for (source, manifest) in manifests() {
        for item in &manifest.items {
            let d = descriptor_from_manifest(item, POLICY)
                .unwrap_or_else(|e| panic!("{source}: converting `{}`: {e}", item.key));
            let back = manifest_from_descriptor(&d)
                .unwrap_or_else(|e| panic!("{source}: converting `{}` back: {e}", item.key));
            let want = without_dropped_affordances(item);
            assert_eq!(
                back, want,
                "{source}: `{}` does not survive the round trip. The descriptor schema is missing \
                 something this row says, and shipping the migration would lose it silently.",
                item.key
            );
            checked += 1;
        }
    }
    // A shrinking denominator is how a green test stops meaning anything.
    assert!(
        checked >= 40,
        "expected to convert the whole shipped furniture set, managed only {checked}"
    );
    println!("round-tripped {checked} manifest rows");
}

/// **Every tag a shipped row carries is a token the vocabulary knows.**
///
/// This is the check that has never existed. `affordances`, `tags` and `group` were unvalidated free
/// text, so `sleep`/`store`/`decor`/`hygiene` could ship with no reader and a typo would look
/// identical to a feature request.
#[test]
fn every_converted_descriptor_validates_against_the_shipped_vocabulary() {
    let v = vocab();
    let mut library = Vec::new();
    for (source, manifest) in manifests() {
        for item in &manifest.items {
            let d = descriptor_from_manifest(item, POLICY)
                .unwrap_or_else(|e| panic!("{source}: {e}"));
            v.masks(&d)
                .unwrap_or_else(|e| panic!("{source}: {e}"));
            library.push(d);
        }
    }

    // And the two-sided pass over the whole library at once: a surface class something rests on and
    // nothing offers is a prop that will never be placed.
    v.validate_library(&library)
        .unwrap_or_else(|e| panic!("the shipped furniture set does not close over its surfaces: {e}"));
}

/// **Room hints are a preference, not a constraint** — so what is worth pinning is that the
/// preference can actually fire.
///
/// The first version of this test asserted that every `rooms` token names a declared room type, and
/// it failed on eleven shipped rows (`decor`, `wall`, `dining`). That was the test being wrong, not
/// the data: `furnish::room_profile` says so in its own comment — *"a kit that tags differently (or a
/// room whose type has no kit match) still furnishes via the top-up pass below. (The base `room` tag
/// matches nothing in the kit — harmless.)"* An unmatched tag costs preference, not placement.
///
/// What WOULD be a silent failure is every tag missing, because then theming does nothing at all and
/// every room furnishes from the top-up pass. That is the invariant with teeth.
#[test]
fn enough_room_hints_match_a_declared_room_type_for_theming_to_do_anything() {
    let cfg = load_game_config().expect("shipped game config must load");
    let known: Vec<&str> = cfg
        .dungeon
        .room_types
        .iter()
        .map(|r| r.tag.as_str())
        .collect();

    let matching = cfg
        .placement
        .furniture
        .items
        .iter()
        .filter(|item| {
            let d = descriptor_from_manifest(item, POLICY).unwrap_or_else(|e| panic!("{e}"));
            d.placement.rooms.iter().any(|r| known.contains(&r.as_str()))
        })
        .count();

    assert!(
        matching >= 5,
        "only {matching} shipped item(s) carry a room hint matching a declared type {known:?} — \
         below that, `room_profile`'s preferred pass has nothing to prefer and every room is \
         furnished by the top-up scan, which is themed rooms silently not happening"
    );
    println!("{matching} items carry a room hint that can actually fire");
}

/// Wall height is project policy, and the conversion must take it from the caller rather than from
/// the asset — otherwise a library shared between games carries one game's architecture.
#[test]
fn wall_mount_height_comes_from_policy_not_from_the_asset() {
    let cfg = load_game_config().expect("shipped game config must load");
    let anchored: Vec<_> = cfg
        .placement
        .furniture
        .items
        .iter()
        .filter(|i| {
            matches!(
                i.role,
                emerge_core::placement::ir::Role::Anchor {
                    host: emerge_core::placement::ir::Host::Wall
                }
            )
        })
        .collect();
    assert!(
        !anchored.is_empty(),
        "the shipped manifest should carry at least one wall anchor (the sconces) — if it no longer \
         does, this test is measuring nothing"
    );

    for item in anchored {
        let a = descriptor_from_manifest(item, POLICY).unwrap_or_else(|e| panic!("{e}"));
        let b = descriptor_from_manifest(
            item,
            Policy {
                wall_mount_height: 2.2,
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_ne!(
            a.mount, b.mount,
            "`{}`: the mount must follow the policy it was given",
            item.key
        );
    }
}

/// The vocabulary file ships and is well-formed. Cheap, and it is the file every other check here
/// leans on.
#[test]
fn the_shipped_vocabulary_parses_and_is_well_formed() {
    assert!(
        Path::new("assets/emerge/vocab.ron").is_file(),
        "assets/emerge/vocab.ron must ship"
    );
    let v = vocab();
    v.validate_tables().unwrap_or_else(|e| panic!("{e}"));
    // The surface axis is the one vocabulary this project already had; losing its tokens would
    // silently unground every `rests_on` in the kit.
    for token in ["support", "worktop"] {
        assert!(
            v.surfaces.contains(token),
            "the surfaces axis must keep `{token}` — `placement::surfaces` has called its table THE \
             single source of truth since before this file existed"
        );
    }
}

/// **The committed libraries are what the manifests convert to — one per kit.**
///
/// `assets/emerge/library*.ron` is what `emerge-mapper` opens, and it is generated rather than
/// authored, so the risk is not that someone edits one badly — it is that a manifest moves and the
/// library quietly does not. This pins them together the way `bake::repin_replay` pins a golden: the
/// files are committed, the test regenerates and compares, and the error says how to update them.
///
/// **One file per kit, not one merged file.** The first version concatenated both manifests and
/// `Library::validate` caught it immediately: `wall_light` is declared by both. They are not two
/// halves of a set, they are *alternatives* — `placement::acceptance_tests` uses the second to prove
/// the solver is kit-agnostic by swapping it in. Merging them would have made every reference to a
/// shared id ambiguous, which is exactly the failure that validator exists to name.
///
/// Regenerate with `EMERGE_WRITE_LIBRARY=1 cargo test --test descriptor_migration`, and commit the
/// result *in the same change* as whatever moved the manifests — a library that drifts from its
/// source is a palette offering pieces the game does not have.
#[test]
fn the_committed_libraries_match_the_manifests() {
    let write = std::env::var("EMERGE_WRITE_LIBRARY").as_deref() == Ok("1");
    let v = vocab();

    for (source, manifest) in manifests() {
        let file = if source.starts_with("config.ron") {
            "library_from_manifests.ron"
        } else {
            "library_kenney_from_manifests.ron"
        };
        let path = Path::new("assets/emerge").join(file);

        let descriptors = manifest
            .items
            .iter()
            .map(|item| {
                descriptor_from_manifest(item, POLICY).unwrap_or_else(|e| panic!("{source}: {e}"))
            })
            .collect::<Vec<_>>();
        let built = emerge_core::library::Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: Some(format!(
                "GENERATED from {source} by tests/descriptor_migration.rs. Do not hand-edit — \
                 regenerate with EMERGE_WRITE_LIBRARY=1 cargo test --test descriptor_migration."
            )),
            descriptors,
        };
        let text = built.to_ron().unwrap_or_else(|e| panic!("{e}"));

        if write {
            emerge_core::ron_surgery::save_atomic(&path, &text).unwrap_or_else(|e| panic!("{e}"));
            println!(
                "wrote {} ({} descriptors from {source})",
                path.display(),
                built.descriptors.len()
            );
            continue;
        }

        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}\n\nGenerate it with:\n  EMERGE_WRITE_LIBRARY=1 cargo test --test \
                 descriptor_migration",
                path.display()
            )
        });
        assert_eq!(
            committed.trim_end(),
            text.trim_end(),
            "{} no longer matches what {source} converts to. Regenerate with:\n  \
             EMERGE_WRITE_LIBRARY=1 cargo test --test descriptor_migration",
            path.display()
        );

        // And it must load on its own terms, which is what emerge-mapper actually does.
        let lib = emerge_core::library::Library::parse(&committed).unwrap_or_else(|e| panic!("{e}"));
        lib.resolve(&v).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}
