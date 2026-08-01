//! **Asset contract: a manifest footprint may not understate the mesh it reserves room for.**
//!
//! GPU-free, no `App` — this runs in the `cargo test` hard gate.
//!
//! # Why this exists
//!
//! `ManifestItem::footprint` is what the placement solvers reserve: the freestanding layout keeps
//! pieces from overlapping by it, and `placement::scatter` decides whether a prop fits on a support's
//! top by it. Nothing anywhere compared it to the **mesh**, so a footprint could be authored smaller
//! than the model and every consumer would agree the piece fit — while the player watched it hang
//! through a wall.
//!
//! That is not hypothetical. `tv` shipped as `(0.88, 0.30)` against a mesh measuring 0.88 x **0.80**,
//! with a comment explaining the shrink as "a shallow contact area so it seats on a ~0.6 m-deep desk
//! without overhanging". The reasoning is inverted: shrinking the reservation does not shrink the
//! chassis, it only stops the solver reserving room for it. 0.25 m hung off each side, and because
//! `MetropolisSolver` backs supports to walls, the rear 0.25 m went through the wall — which is
//! exactly what the 2026-08-01 player capture shows.
//!
//! Auditing all 41 rows found a second, unreported instance the same day: `lamp_tall` at `(0.35, 0.35)`
//! against a 0.61 x 0.61 mesh. One bug is a mistake; two is a class, and a class wants a test.
//!
//! # What it measures, and what it refuses to guess
//!
//! Extents come from the POSITION accessors' `min`/`max` — the same source `tests/ozea_asset.rs` uses,
//! and for the same reason: it is the file's own declaration rather than a number re-derived from a
//! resolved world transform. That is only equal to the world extent when no node carries a transform,
//! so an item whose GLB has a node `scale` or `matrix` is **reported and skipped** rather than measured
//! wrongly. A silently mismeasured pass would be worse than no pass. Today that set is empty; the
//! counter is printed so it cannot quietly grow.
//!
//! Height is deliberately not checked. `ManifestItem::height` is a *seat* height for stacking, not a
//! bounding extent, so comparing it to the mesh would be comparing two different claims.

mod common;

use common::Glb;
use foundation_vs_slop::config::load_game_config;

/// A footprint must be at least this fraction of the mesh's extent on both axes.
///
/// Not 1.0: a footprint is a reservation, and a little slack is legitimate — a chair's splayed legs or
/// a lamp's shade may overhang a base without ever reaching a wall. It is high enough to catch the two
/// real offenders (0.38 and 0.57) and loose enough that the 39 honest rows pass untouched, which is the
/// only calibration a threshold like this can honestly claim.
const MIN_RATIO: f32 = 0.85;

/// XZ extents of every POSITION accessor in the file, or `None` when a node transform means the raw
/// accessor bounds are not the world extents.
fn mesh_extents_xz(glb: &Glb) -> Option<(f32, f32)> {
    for node in glb.json["nodes"].as_array().into_iter().flatten() {
        if node.get("matrix").is_some() {
            return None;
        }
        if let Some(scale) = node.get("scale").and_then(|s| s.as_array())
            && scale.iter().any(|v| (v.as_f64().unwrap_or(1.0) - 1.0).abs() > 1e-4)
        {
            return None;
        }
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut seen = false;
    for mesh in glb.json["meshes"].as_array().into_iter().flatten() {
        for prim in mesh["primitives"].as_array().into_iter().flatten() {
            let Some(ix) = prim["attributes"]["POSITION"].as_u64() else { continue };
            let acc = &glb.json["accessors"][ix as usize];
            let (Some(min), Some(max)) = (acc["min"].as_array(), acc["max"].as_array()) else {
                continue;
            };
            for axis in 0..3 {
                lo[axis] = lo[axis].min(min[axis].as_f64().unwrap_or(0.0) as f32);
                hi[axis] = hi[axis].max(max[axis].as_f64().unwrap_or(0.0) as f32);
            }
            seen = true;
        }
    }
    seen.then(|| (hi[0] - lo[0], hi[2] - lo[2]))
}

#[test]
fn no_manifest_footprint_understates_its_mesh() {
    let config = load_game_config().unwrap_or_else(|e| panic!("config: {e}"));
    let items = config.placement.furniture.by_role(|_| true);
    assert!(!items.is_empty(), "the shipped manifest is empty — this test would pass vacuously");

    let mut offenders: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for item in &items {
        // `#` separates a sub-scene label (`foo.glb#Scene0`) from the file path.
        let file = item.glb.split('#').next().unwrap_or(&item.glb);
        let path = format!("assets/{file}");
        if !std::path::Path::new(&path).exists() {
            // A missing asset is a different contract, already covered elsewhere; do not double-report.
            continue;
        }
        let glb = Glb::load(&path);
        let Some((mw, md)) = mesh_extents_xz(&glb) else {
            skipped.push(format!("{} ({file}) — node transform, extents not readable", item.key));
            continue;
        };
        checked += 1;
        let (fw, fd) = item.footprint;
        let rw: f32 = if mw > 1e-6 { fw / mw } else { 1.0 };
        let rd: f32 = if md > 1e-6 { fd / md } else { 1.0 };
        if rw.min(rd) < MIN_RATIO {
            offenders.push(format!(
                "  {:<18} declared {:.2} x {:.2}   mesh {:.2} x {:.2}   ({:.0}% of the mesh)",
                item.key,
                fw,
                fd,
                mw,
                md,
                rw.min(rd) * 100.0
            ));
        }
    }

    // Printed, not asserted away: if node transforms start appearing this pass silently covers less,
    // and a shrinking denominator is exactly how a green test stops meaning anything.
    println!("footprint contract: {checked} measured, {} unreadable", skipped.len());
    for s in &skipped {
        println!("  skipped: {s}");
    }

    assert!(
        offenders.is_empty(),
        "{} manifest item(s) reserve less floor than their mesh occupies:\n{}\n\n\
         The solvers reserve `footprint`, so a piece authored smaller than its model is placed as if \
         it fit and then renders through whatever it was placed against — `MetropolisSolver` backs \
         supports to walls, so the overhang lands in a wall. Set the footprint to the MEASURED extent. \
         If the piece then no longer fits its intended support, that is the honest answer: give it a \
         deeper support or a different role, rather than shrinking the number until the solver stops \
         objecting.",
        offenders.len(),
        offenders.join("\n")
    );
}
