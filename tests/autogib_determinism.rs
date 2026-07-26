//! **The fracture bake must be reproducible** (feature `test-harness`).
//!
//! This is FVS-N-8's regression gate, and it is its own fast target on purpose: it costs ~1.5 s and
//! needs no CPU load, whereas the bug it guards took two sessions to find and its previous reproducer
//! was an `#[ignore]`d, load-dependent, five-unit-death scenario.
//!
//! **What the bug was.** `autogib::seed_from` hashed the `AssetId` of the character GLB. An `AssetId`
//! is a slot index in the asset arena, assigned by **async load order**, so the same mesh got a
//! different id run to run, hashed to a different fracture seed, and `fracture` sliced the body along
//! completely different planes. Every downstream symptom followed from that: chunk positions differing
//! by tens of ULPs, the cascade into `crab::assign_meat_targets`, and the load-dependence that made it
//! look like a race. The seed now comes from the asset **path**, which is authored rather than
//! allocated.
//!
//! **Why assert the bake and not just the gibs.** A gib-level assertion needs a death, which needs the
//! squad, the fracture, the drain and the physics — so it fails for a dozen reasons and localises none
//! of them. This asserts the one property directly: same seed in, same fragments out.
#![cfg(feature = "test-harness")]

use bevy::prelude::With;
use foundation_vs_slop::autogib::AutogibCache;
use foundation_vs_slop::sim_harness::{
    build_headless_app, serial_guard, step_until_autogib_ready, SimConfig,
};
use foundation_vs_slop::squad::{FigurineSource, Unit};

/// Every baked fragment's centroid and half-extents, as raw bits, in a canonical source order.
///
/// **Half-extents as well as centroids, deliberately.** A centroid-only fingerprint would pass if the
/// fracture partitioned the mesh differently but happened to land the centres nearby. The original bug
/// moved both — 23 of 23 fragments differed in both — so both are pinned.
fn bake_fingerprint(cfg: &SimConfig) -> Vec<[u32; 6]> {
    let mut app = build_headless_app(cfg);
    step_until_autogib_ready(&mut app, cfg, 4000).expect("the figurine bake must complete");
    let world = app.world_mut();

    let mut sources: Vec<_> = {
        let mut q = world.query_filtered::<&FigurineSource, With<Unit>>();
        q.iter(world).map(|f| f.0.id()).collect()
    };
    // SORT-OK: `AssetId` is `Ord` and unique per asset. Sorting here is about the FINGERPRINT's own
    // stability, not the sim's: the query yields sources in an order that is not stable across `App`
    // instances, and an unsorted fingerprint would then differ for a reason that has nothing to do
    // with the bake — a false red that would send the next person hunting the wrong thing.
    sources.sort_unstable();
    sources.dedup();

    let cache = world.resource::<AutogibCache>();
    let mut rows = Vec::new();
    for s in &sources {
        let Some(frags) = cache.fragments(*s) else { continue };
        for f in frags {
            rows.push([
                f.center_local.x.to_bits(),
                f.center_local.y.to_bits(),
                f.center_local.z.to_bits(),
                f.half_extents.x.to_bits(),
                f.half_extents.y.to_bits(),
                f.half_extents.z.to_bits(),
            ]);
        }
    }
    rows
}

#[test]
fn the_fracture_bake_is_bit_identical_across_builds() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();

    let a = bake_fingerprint(&cfg);
    let b = bake_fingerprint(&cfg);

    assert!(!a.is_empty(), "precondition: the bake produced fragments at all");
    let differing = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
    assert_eq!(
        differing,
        0,
        "the fracture bake is not reproducible: {differing} of {} fragments differ. \
         The seed must derive from the asset PATH, never from an `AssetId` (a load-order-dependent \
         arena slot). See FVS-N-8.",
        a.len()
    );
    assert_eq!(a.len(), b.len(), "the bake produced a different fragment COUNT between builds");
}
