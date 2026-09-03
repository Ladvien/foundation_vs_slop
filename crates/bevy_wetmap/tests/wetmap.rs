//! **The observable contracts, one test each.**
//!
//! The headline claim is that this wetmap is reproducible, so the first two tests are the claim and its
//! negation: the same scripted hits give the same digest, and **one hit moved by a single texel** gives
//! a different one. A digest test that only proved equality would pass on a function that returned a
//! constant.
//!
//! Everything else defends a rule the crate states in prose somewhere: mass is conserved, dry paint
//! does not move, a mesh without UVs is refused rather than silently painted, a hit lands where the
//! mesh's UVs say it should, the upload budget is a budget, and the plugin's system survives a missing
//! resource — which in Bevy 0.19 is a panic rather than a skip unless you ask for an `Option`.

use bevy::app::{App, TaskPoolPlugin};
use bevy::asset::{AssetPlugin, Assets};
use bevy::ecs::component::Component;
use bevy::image::{Image, ImagePlugin};
use bevy::math::primitives::Sphere;
use bevy::math::{Vec2, Vec3};
use bevy::mesh::{Mesh, Meshable, PrimitiveTopology};
use bevy::transform::components::GlobalTransform;
use bevy_wetmap::{StainShape, WetCanvas, WetSettings, WetmapPlugin};

/// A round stain of a given size in metres, with no spines, so a test's expectations are about the
/// wetmap and not about `bloodstain`'s silhouette.
fn blob(major: f32) -> StainShape {
    StainShape { major, minor: major, spines: 0, satellites: 0, direction: [1.0, 0.0], seed: 11 }
}

fn scratch(size: u32) -> (Assets<Image>, WetCanvas) {
    let mut images = Assets::<Image>::default();
    let canvas = WetCanvas::new(&mut images, size, [0.78, 0.66, 0.60], 0.55);
    (images, canvas)
}

/// Total coverage on the canvas, exactly — the integer quantity `wetted_area` scales.
fn coverage(canvas: &WetCanvas) -> u32 {
    let mut sum = 0;
    for y in 0..canvas.size() {
        for x in 0..canvas.size() {
            sum += canvas.amount_at(x, y) as u32;
        }
    }
    sum
}

/// The scripted sequence both digest tests run: three hits at different ticks, ticked in between, so
/// the drip, spread, age and shade passes have all run more than once by the end.
fn scripted_run(nudge_texels: i32) -> u64 {
    let size = 64u32;
    let (_images, mut canvas) = scratch(size);
    let s = WetSettings::default();
    let nudge = nudge_texels as f32 / size as f32;
    let hits: [(f32, f32, u32); 3] =
        [(0.30 + nudge, 0.20, 0), (0.55, 0.35, 6), (0.32, 0.50, 14)];
    let mut next = 0;
    for tick in 0..30u32 {
        while next < hits.len() && hits[next].2 == tick {
            canvas.paint_uv(Vec2::new(hits[next].0, hits[next].1), &blob(0.09), tick);
            next += 1;
        }
        canvas.tick(tick, Vec2::new(0.0, 1.0), &s);
    }
    canvas.digest()
}

#[test]
fn two_identical_runs_agree_to_the_bit() {
    assert_eq!(scripted_run(0), scripted_run(0));
}

#[test]
fn one_hit_moved_by_a_single_texel_changes_the_digest() {
    let baseline = scripted_run(0);
    let nudged = scripted_run(1);
    assert_ne!(
        baseline, nudged,
        "moving a hit by one texel left the digest alone — the fold is not reading the buffer"
    );
}

#[test]
fn a_tick_conserves_every_byte_of_coverage() {
    // `absorbency = 0` isolates movement from the substrate's cut: the drip and spread passes are
    // each mass-conserving on their own, and this is what says so through the public surface.
    let s = WetSettings { absorbency: 0.0, ..Default::default() };
    let (_images, mut canvas) = scratch(64);
    canvas.paint_uv(Vec2::new(0.5, 0.25), &blob(0.12), 0);
    let before = coverage(&canvas);
    assert!(before > 0, "the stamp painted nothing");

    canvas.tick(0, Vec2::new(0.0, 1.0), &s);
    assert_eq!(before, coverage(&canvas), "one tick lost or invented coverage");

    // Five more, and the run still has not reached a border, so it is still exact.
    for tick in 1..6u32 {
        canvas.tick(tick, Vec2::new(0.0, 1.0), &s);
    }
    assert_eq!(before, coverage(&canvas), "a run of ticks leaked coverage");
}

#[test]
fn dry_paint_does_not_move() {
    // `dry_ticks = 1` means the paint is set after its first tick, so everything from tick 1 onwards
    // must be a fixed point.
    let s = WetSettings { dry_ticks: 1, ..Default::default() };
    let (_images, mut canvas) = scratch(32);
    canvas.paint_uv(Vec2::new(0.5, 0.3), &blob(0.15), 0);
    canvas.tick(0, Vec2::new(0.0, 1.0), &s);
    let settled = canvas.digest();
    let settled_coverage = coverage(&canvas);
    assert!(settled_coverage > 0, "nothing was left to be dry");

    for tick in 1..50u32 {
        canvas.tick(tick, Vec2::new(0.0, 1.0), &s);
    }
    assert_eq!(canvas.digest(), settled, "dry paint moved");
    assert_eq!(coverage(&canvas), settled_coverage, "dry paint soaked away");
}

#[test]
fn a_mesh_with_no_uvs_is_refused_and_a_miss_is_refused_too() {
    let (_images, mut canvas) = scratch(32);
    let xf = GlobalTransform::default();

    let mut bare = Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::MAIN_WORLD);
    bare.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    );
    bare.insert_indices(bevy::mesh::Indices::U32(vec![0, 1, 2]));
    assert!(
        !canvas.paint_world(
            &bare,
            &xf,
            Vec3::new(0.2, 0.2, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
            &blob(0.1),
            0
        ),
        "a mesh with no UVs must be refused, not silently painted onto"
    );
    assert_eq!(coverage(&canvas), 0, "a refused mesh still painted something");

    let sphere = Sphere::new(0.5).mesh().uv(32, 18);
    assert!(
        !canvas.paint_world(
            &sphere,
            &xf,
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            &blob(0.1),
            0
        ),
        "a ray pointing away from the mesh must miss"
    );
    assert_eq!(coverage(&canvas), 0, "a miss painted something");
}

#[test]
fn a_hit_on_a_uv_sphere_lands_where_the_spheres_uvs_say_it_should() {
    // Bevy's UV sphere puts its pole on +Z and its seam on +X: `u = atan2(y, x) / 2pi` and
    // `v = 0.5 - asin(z / r) / pi` (`bevy_mesh-0.19.0/src/primitives/dim3/sphere.rs:187-200`). So the
    // analytic answer is computable, and the assertion is against the geometry rather than against a
    // previously recorded number.
    //
    // The aim is deliberately off every axis: +X is exactly on the u seam and the equator is exactly
    // on a vertex ring, and a test that fired at either would be asserting which side of a shared
    // edge Moller-Trumbore happened to pick.
    let size = 128u32;
    let (_images, mut canvas) = scratch(size);
    let sphere = Sphere::new(0.5).mesh().uv(32, 18);
    let xf = GlobalTransform::default();

    let aim = Vec3::new(0.6, 0.7, 0.35).normalize();
    let expected_u = aim.y.atan2(aim.x) / std::f32::consts::TAU;
    let expected_v = 0.5 - aim.z.asin() / std::f32::consts::PI;

    // One texel of paint, so the wet centroid IS the stamp centre.
    assert!(canvas.paint_world(
        &sphere,
        &xf,
        aim * 5.0,
        -aim,
        &blob(1.0 / size as f32),
        0
    ));

    let mut weight = 0f32;
    let mut cx = 0f32;
    let mut cy = 0f32;
    for y in 0..size {
        for x in 0..size {
            let a = canvas.amount_at(x, y) as f32;
            if a > 0.0 {
                weight += a;
                cx += a * x as f32;
                cy += a * y as f32;
            }
        }
    }
    assert!(weight > 0.0, "the hit reported success but painted nothing");
    let (cx, cy) = (cx / weight, cy / weight);
    let (want_x, want_y) = (expected_u * size as f32, expected_v * size as f32);
    // Three texels of tolerance: the mesh is a 32x18 facetted approximation of the sphere, so the hit
    // point is a chord rather than the analytic surface and its UV is off by up to half a facet.
    assert!(
        (cx - want_x).abs() <= 3.0 && (cy - want_y).abs() <= 3.0,
        "paint landed at ({cx:.1}, {cy:.1}) texels but the sphere's UVs put that hit at \
         ({want_x:.1}, {want_y:.1})"
    );
}

#[test]
fn wetted_area_rises_with_paint_and_falls_as_the_substrate_takes_its_cut() {
    // Gravity zero, so nothing runs off a border and the only thing changing the total is absorption.
    let s = WetSettings { dry_ticks: 120, ..Default::default() };
    let (_images, mut canvas) = scratch(32);
    assert_eq!(canvas.wetted_area(), 0.0, "a blank canvas is not dry");

    canvas.paint_uv(Vec2::new(0.5, 0.5), &blob(0.4), 0);
    let fresh = canvas.wetted_area();
    assert!(fresh > 0.0, "paint did not wet anything");

    for tick in 0..s.dry_ticks {
        canvas.tick(tick, Vec2::ZERO, &s);
    }
    let dried = canvas.wetted_area();
    assert!(dried < fresh, "the substrate absorbed nothing: {fresh} -> {dried}");
    assert!(dried > 0.0, "the whole stain soaked away — absorbency is being read per tick");
}

#[test]
fn flush_uploads_only_when_dirty() {
    let s = WetSettings::default();
    let (mut images, mut canvas) = scratch(16);
    assert!(!canvas.is_dirty(), "a fresh canvas claims to be out of date");
    assert!(!canvas.flush(&mut images), "a clean canvas uploaded anyway");

    canvas.paint_uv(Vec2::new(0.5, 0.5), &blob(0.2), 3);
    canvas.tick(3, Vec2::new(0.0, 1.0), &s);
    assert_eq!(canvas.dirty_since(), Some(3), "the dirty tick is not the tick it was painted on");
    assert!(canvas.flush(&mut images), "a painted canvas refused to upload");
    assert!(!canvas.is_dirty(), "flush did not clear the dirty flag");
    assert!(!canvas.flush(&mut images), "a freshly flushed canvas uploaded twice");
}

#[test]
fn what_reaches_the_gpu_is_blood_over_the_base_surface() {
    // **A test may read `Assets<Image>`; the library may not.** This is the only place in the crate
    // that looks at an uploaded pixel, and it exists because "the canvas is composited on the CPU so
    // there is nothing to blend in WGSL" is a claim about what a renderer will actually sample. A
    // green build proves the handles exist; this proves they carry blood.
    let s = WetSettings::default();
    let size = 32u32;
    let mut images = Assets::<Image>::default();
    let mut canvas = WetCanvas::new(&mut images, size, [0.78, 0.66, 0.60], 0.55);
    let albedo = canvas.albedo();
    let roughness = canvas.roughness();

    let at = |image: Option<&Image>, x: u32, y: u32| -> [u8; 4] {
        let data = image.and_then(|i| i.data.as_ref()).expect("the canvas built its images");
        let i = ((y * size + x) * 4) as usize;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    };

    let base_albedo = at(images.get(&albedo), size / 2, size / 2);
    let base_rough = at(images.get(&roughness), size / 2, size / 2);
    assert_eq!(base_albedo, [199, 168, 153, 255], "the base surface is not the sRGB it was given");
    // G is roughness, B is metallic (`bevy_pbr-0.19.0/src/pbr_material.rs:153-154`), and blood is a
    // dielectric, so B stays 0.
    assert_eq!(base_rough, [0, 140, 0, 255], "the base roughness is not in the green channel");

    canvas.paint_uv(Vec2::new(0.5, 0.5), &blob(0.25), 0);
    canvas.tick(0, Vec2::new(0.0, 1.0), &s);
    assert!(canvas.flush(&mut images));

    let wet_albedo = at(images.get(&albedo), size / 2, size / 2);
    let wet_rough = at(images.get(&roughness), size / 2, size / 2);
    let base_chroma = base_albedo[0] as i32 - base_albedo[1] as i32;
    let wet_chroma = wet_albedo[0] as i32 - wet_albedo[1] as i32;
    assert!(
        wet_chroma > base_chroma,
        "the uploaded albedo did not get redder: {base_albedo:?} -> {wet_albedo:?}"
    );
    assert!(
        wet_rough[1] < base_rough[1],
        "fresh blood must be glossier than the dry surface: {base_rough:?} -> {wet_rough:?}"
    );
    assert_eq!(wet_rough[2], 0, "blood stopped being a dielectric");
}

/// Which canvas is which, for the budget test.
#[derive(Component)]
struct Slot(u32);

fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        ImagePlugin::default(),
        WetmapPlugin,
    ));
    app
}

#[test]
fn the_plugin_uploads_at_most_the_budget_oldest_dirty_first() {
    let mut app = headless_app();
    let s = WetSettings::default();
    let budget = s.max_canvas_updates_per_tick;
    let total = budget + 2;

    app.world_mut().resource_scope(|world, mut images: bevy::ecs::change_detection::Mut<Assets<Image>>| {
        for slot in 0..total {
            let mut canvas = WetCanvas::new(&mut images, 16, [0.8, 0.7, 0.6], 0.5);
            // Painted on tick `slot`, so `dirty_since` orders them and the sort has real work to do.
            canvas.paint_uv(Vec2::new(0.5, 0.5), &blob(0.2), slot);
            canvas.tick(slot, Vec2::new(0.0, 1.0), &s);
            assert_eq!(canvas.dirty_since(), Some(slot));
            world.spawn((canvas, Slot(slot)));
        }
    });

    app.update();

    let mut still_dirty: Vec<u32> = app
        .world_mut()
        .query::<(&WetCanvas, &Slot)>()
        .iter(app.world())
        .filter(|(canvas, _)| canvas.is_dirty())
        .map(|(_, slot)| slot.0)
        .collect();
    still_dirty.sort_unstable();
    assert_eq!(
        still_dirty,
        (budget..total).collect::<Vec<_>>(),
        "the budget uploaded the wrong canvases — oldest dirty must go first"
    );

    // A second frame clears the remainder, and a third has nothing to do.
    app.update();
    let dirty_after = app
        .world_mut()
        .query::<&WetCanvas>()
        .iter(app.world())
        .filter(|canvas| canvas.is_dirty())
        .count();
    assert_eq!(dirty_after, 0, "the backlog never drained");
}

#[test]
fn the_upload_system_survives_a_missing_settings_resource() {
    // Bevy 0.19 PANICS a system with a missing `Res<T>` rather than skipping it, so the system takes
    // an `Option` even though the plugin inits the resource. This is that guard, checked.
    let mut app = headless_app();
    assert!(app.world().get_resource::<WetSettings>().is_some(), "the plugin did not init its dials");
    app.world_mut().remove_resource::<WetSettings>();
    app.update();
    app.update();
}

#[test]
fn the_shipped_dials_are_the_contract() {
    let s = WetSettings::default();
    assert_eq!(s.drip_rate, 0.35);
    assert_eq!(s.spread_rate, 0.08);
    assert_eq!(s.dry_ticks, 1800);
    assert_eq!(s.absorbency, 0.15);
    assert_eq!(s.max_canvas_updates_per_tick, 4);
    assert_eq!(s.humidity, 0.4);
}
