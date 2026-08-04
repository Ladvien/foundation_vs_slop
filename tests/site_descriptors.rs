//! **The Site kit and its descriptors say the same thing.**
//!
//! `src/site/descriptors.rs` translates `SiteKit` into a descriptor library plus a policy. A
//! translation nobody checks is a second description that drifts, and the drift would surface as the
//! hub looking subtly wrong in one of the two tools that draw it.
//!
//! So this is a **semantic** pin, not only a byte one: every field, for every piece, compared against
//! the accessor the game actually calls today. Byte-pinning the file alone would prove the generator
//! is stable and prove nothing about whether it is right.
//!
//! Regenerate the committed pair with:
//!
//! ```text
//! EMERGE_WRITE_SITE=1 cargo test --test site_descriptors
//! ```

use std::path::Path;

use emerge_core::descriptor::Mount;
use emerge_core::library::Library;
use emerge_core::policy::Policy;
use foundation_vs_slop::site::descriptors::{self, id_of, GREYBOX_PROJECT_DIR, SITE_PROJECT_DIR};
use foundation_vs_slop::site::kit::{SiteKit, GREYBOX_KIT_PATH, SITE_KIT_PATH};
use foundation_vs_slop::site::pieces::SitePiece;

fn kit() -> SiteKit {
    kit_at(SITE_KIT_PATH)
}

fn kit_at(path: &str) -> SiteKit {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    ron::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Every shipped kit, and where its converted pair lives.
fn kits() -> [(&'static str, &'static str); 2] {
    [
        (SITE_KIT_PATH, SITE_PROJECT_DIR),
        (GREYBOX_KIT_PATH, GREYBOX_PROJECT_DIR),
    ]
}

/// **Every measurement crosses unchanged.** This is the plan's Stage 1 gate — *"converted descriptors
/// reproduce today's semantics exactly"* — checked against the accessors the game calls rather than
/// against a copy of the file.
#[test]
fn every_piece_converts_to_a_descriptor_that_says_the_same_thing() {
    let kit = kit();
    let library = descriptors::library(&kit);

    assert_eq!(
        library.descriptors.len(),
        SitePiece::ALL.len(),
        "the library must hold every piece the kit defines"
    );

    for piece in SitePiece::ALL {
        let id = id_of(*piece);
        let d = library
            .get(&id)
            .unwrap_or_else(|| panic!("{id} is missing from the converted library"));
        let p = kit.piece(*piece);

        assert_eq!(d.mesh.as_deref(), Some(p.glb.as_str()), "{id}: mesh");
        assert_eq!(d.extent.height, Some(p.height), "{id}: height");
        assert_eq!(d.extent.footprint, Some(p.footprint), "{id}: footprint");
        assert_eq!(d.align.front, p.front, "{id}: front");
        assert_eq!(d.offers.surfaces, p.surfaces, "{id}: surfaces");

        // Absence means "no correction", so the two spellings have to be compared through the
        // accessor rather than field to field.
        assert_eq!(
            d.align.scale.unwrap_or(1.0),
            kit.scale(*piece),
            "{id}: scale"
        );
        assert_eq!(
            d.align.y_offset.unwrap_or(0.0),
            kit.y_offset(*piece),
            "{id}: y_offset"
        );

        // The layer. `rests_on` is the Site's word for `OnSurface`, and they use one vocabulary.
        match (&d.mount, kit.rests_on(*piece)) {
            (Some(Mount::OnSurface { class }), Some(want)) => {
                assert_eq!(class, want, "{id}: rests_on");
            }
            (Some(Mount::OnSurface { class }), None) => {
                panic!("{id}: converted to OnSurface({class}) but the kit says it stands on the floor")
            }
            (_, Some(want)) => panic!("{id}: the kit rests it on `{want}` and the descriptor does not"),
            _ => {}
        }
    }

    // The doorways are the shape the old manifest could not express, and they cross exactly.
    for (piece, door) in [
        (SitePiece::WallDoorway, &kit.wall_doorway),
        (SitePiece::WallDoorwayWide, &kit.wall_doorway_wide),
    ] {
        let id = id_of(piece);
        let d = library.get(&id).unwrap_or_else(|| panic!("{id} missing"));
        assert_eq!(
            d.mount,
            Some(Mount::InOpening {
                clear: Some(door.opening)
            }),
            "{id}: clear opening"
        );
    }
}

/// **The policy reproduces `y_scale` exactly, for every piece.**
///
/// This is the whole argument for the layer: the measurement is art and the target is this facility,
/// and putting them in separate files must not change the number the game ends up drawing with. A
/// piece with no target height must come out at 1.0 — absent, not `Some(1.0)` — because the kit's own
/// `y_scale` returns 1.0 for it.
#[test]
fn the_policy_reproduces_the_kits_y_scale() {
    let kit = kit();
    let layered = descriptors::policy(&kit)
        .apply(&descriptors::library(&kit))
        .unwrap_or_else(|e| panic!("{e}"));

    let mut stretched = 0usize;
    for piece in SitePiece::ALL {
        let id = id_of(*piece);
        let d = layered.get(&id).unwrap_or_else(|| panic!("{id} missing"));
        let want = kit.y_scale(*piece);
        assert_eq!(
            d.align.stretch_y.unwrap_or(1.0),
            want,
            "{id}: y_scale — the layered library must draw exactly what the kit draws"
        );
        if d.align.stretch_y.is_some() {
            stretched += 1;
        }
    }

    // The architecture, and nothing else. If this ever covers the whole kit, a policy rule has
    // escaped onto the furniture and every chair is being resized to a wall height.
    assert!(
        (1..SitePiece::ALL.len() / 2).contains(&stretched),
        "{stretched} of {} pieces carry a stretch — the structural pieces should, the furniture \
         should not",
        SitePiece::ALL.len()
    );

    // **And the policy is doing real work.** Most of the Ozea kit is already authored at this
    // facility's heights, so most ratios are 1.0 — but `WallLow` is a 2 m wall mesh squashed to a
    // 0.9 m counter, and that number exists nowhere but here. A policy that had quietly become all
    // 1.0 would pass every assertion above while describing nothing.
    let counter = layered
        .get(&id_of(SitePiece::WallLow))
        .unwrap_or_else(|| panic!("wall_low missing"));
    assert_eq!(counter.align.stretch_y, Some(kit.y_scale(SitePiece::WallLow)));
    assert!(
        counter.align.stretch_y.is_some_and(|s| (s - 1.0).abs() > 0.1),
        "the counter's stretch is the clearest evidence the layer carries policy rather than \
         restating measurements: {:?}",
        counter.align.stretch_y
    );
}

/// The converted library stands on its own: it parses, it has no duplicate ids, and every surface
/// class something needs is a class something offers.
#[test]
fn the_converted_library_validates() {
    let kit = kit();
    let text = descriptors::library(&kit)
        .to_ron()
        .unwrap_or_else(|e| panic!("{e}"));
    let parsed = Library::parse(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(parsed.descriptors.len(), SitePiece::ALL.len());

    let policy_text = descriptors::policy(&kit)
        .to_ron()
        .unwrap_or_else(|e| panic!("{e}"));
    let policy = Policy::parse(&policy_text).unwrap_or_else(|e| panic!("{e}"));
    // Every rule finds its target — `apply` refuses a rule that matches nothing, which is what makes
    // a renamed piece a load error rather than a silently unstretched wall.
    policy.apply(&parsed).unwrap_or_else(|e| panic!("{e}"));
}

/// **The committed pair is what the kit converts to**, and it opens as a project directory.
///
/// Same discipline as `the_committed_libraries_match_the_manifests`: the files are committed, the
/// test regenerates and compares, and the error says how to update them. A generated file that drifts
/// from its source is a description of a kit that no longer exists.
#[test]
fn the_committed_site_project_matches_the_kit() {
    let write = std::env::var("EMERGE_WRITE_SITE").as_deref() == Ok("1");
    for (kit_path, project_dir) in kits() {
    let kit = kit_at(kit_path);
    let dir = Path::new(project_dir);

    let files = [
        ("library.ron", descriptors::library(&kit).to_ron()),
        ("project.ron", descriptors::policy(&kit).to_ron()),
    ];

    for (name, built) in files {
        let path = dir.join(name);
        let text = built.unwrap_or_else(|e| panic!("{name}: {e}"));

        if write {
            std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            emerge_core::ron_surgery::save_atomic(&path, &text).unwrap_or_else(|e| panic!("{e}"));
            println!("wrote {}", path.display());
            continue;
        }

        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}\n\nGenerate it with:\n  EMERGE_WRITE_SITE=1 cargo test --test \
                 site_descriptors",
                path.display()
            )
        });
        assert_eq!(
            committed,
            text,
            "{} is out of date with the Site kit. Regenerate with:\n  EMERGE_WRITE_SITE=1 cargo \
             test --test site_descriptors",
            path.display()
        );
    }

    if !write {
        // And the pair opens the way any project does — the layer, through its real entry point.
        let layered = emerge_core::policy::layered_library(dir).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(layered.descriptors.len(), SitePiece::ALL.len());
    }
    }
}

/// **Two kits, two architectures, one set of ids.** The greybox kit is a 1 m module set, so its
/// policy stretches a wall by 2.4 while the Ozea kit's leaves it alone — the same descriptor ids, the
/// same field, wildly different numbers.
///
/// This is the argument for the split made concrete: if the stretch lived in the library, swapping
/// kits would mean swapping measurements *and* architecture together, and there would be no file that
/// said which was which.
#[test]
fn the_two_kits_state_the_same_architecture_with_different_numbers() {
    let ozea = kit_at(SITE_KIT_PATH);
    let greybox = kit_at(GREYBOX_KIT_PATH);

    let wall = id_of(SitePiece::Wall);
    let stretch = |kit: &SiteKit| -> f32 {
        descriptors::policy(kit)
            .apply(&descriptors::library(kit))
            .unwrap_or_else(|e| panic!("{e}"))
            .get(&wall)
            .and_then(|d| d.align.stretch_y)
            .unwrap_or(1.0)
    };

    let (a, b) = (stretch(&ozea), stretch(&greybox));
    assert_ne!(
        a, b,
        "both kits stretch a wall by {a} — one of them is not being read"
    );
    // Each reproduces its own kit exactly, which is the only claim that matters.
    assert_eq!(a, ozea.y_scale(SitePiece::Wall));
    assert_eq!(b, greybox.y_scale(SitePiece::Wall));
}
