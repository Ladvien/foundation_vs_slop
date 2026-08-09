//! **Light your AI can read — including the shadow behind a pillar.**
//!
//! Two passes, split on cost. `bake` is expensive and event-driven: it recomputes the static baseline
//! from fixed lamps. `compose` is cheap and runs every tick: it re-adds only the moving cones on top
//! of that cached base. A moving light can never be dirty-gated, which is why the split exists.
//!
//! Occlusion is **the caller's**. Both passes take a line-of-sight closure, so this crate never learns
//! what a wall is — here it is a Bresenham walk over the example's own wall set.
//!
//! Run: `cargo run -p bevy_light_grid --example shadow`

use bevy_light_grid::{FlashlightCone, LightGrid};
use bevy_math::{IVec2, Vec2};

const W: usize = 56;
const H: usize = 19;

/// Two pillars and a spur wall — enough to cast shadows a creature could hide in.
fn is_wall(c: IVec2) -> bool {
    let (x, y) = (c.x, c.y);
    // Border.
    if x == 0 || y == 0 || x == W as i32 - 1 || y == H as i32 - 1 {
        return true;
    }
    // Two square pillars.
    if (14..=16).contains(&x) && (6..=8).contains(&y) {
        return true;
    }
    if (30..=32).contains(&x) && (10..=12).contains(&y) {
        return true;
    }
    // A spur jutting down from the top.
    if x == 42 && (1..=9).contains(&y) {
        return true;
    }
    false
}

fn floor_cells() -> impl Iterator<Item = IVec2> {
    (0..H as i32)
        .flat_map(|y| (0..W as i32).map(move |x| IVec2::new(x, y)))
        .filter(|c| !is_wall(*c))
}

/// Bresenham line-of-sight: blocked if any cell strictly between `a` and `b` is a wall.
///
/// This is exactly the shape the crate asks for — `impl Fn(IVec2, IVec2) -> bool`, monomorphised at
/// the call site and returning a plain `bool`, so it cannot perturb a float in the light sum.
fn line_of_sight(a: IVec2, b: IVec2) -> bool {
    let (mut x, mut y) = (a.x, a.y);
    let (dx, dy) = ((b.x - a.x).abs(), -(b.y - a.y).abs());
    let (sx, sy) = (if a.x < b.x { 1 } else { -1 }, if a.y < b.y { 1 } else { -1 });
    let mut err = dx + dy;
    loop {
        if (x, y) == (b.x, b.y) {
            return true;
        }
        if (x, y) != (a.x, a.y) && is_wall(IVec2::new(x, y)) {
            return false;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn plot(field: &LightGrid, title: &str, lamps: &[IVec2]) {
    const RAMP: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];
    let peak = field.peak().max(1.0e-6);
    println!("\n{title}   (peak {:.3})", field.peak());
    for y in 0..H as i32 {
        let row: String = (0..W as i32)
            .map(|x| {
                let c = IVec2::new(x, y);
                if is_wall(c) {
                    '█'
                } else if lamps.contains(&c) {
                    'o'
                } else {
                    let t = (field.sample_cell(c) / peak).clamp(0.0, 1.0);
                    RAMP[((t * 9.0).round() as usize).min(9)]
                }
            })
            .collect();
        println!("  {row}");
    }
}

fn main() {
    let mut field = LightGrid::new(W, H, floor_cells());

    // Static fixtures: (cell, intensity, range in cells). Order is stable because the per-cell sum is
    // a non-associative float add — the crate documents that requirement and means it.
    let lamps = [IVec2::new(10, 4), IVec2::new(36, 14)];
    let fixtures = [(lamps[0], 1.0, 14.0), (lamps[1], 0.8, 12.0)];

    field.bake(&fixtures, line_of_sight);
    field.compose(&[], line_of_sight);
    plot(&field, "STATIC BAKE — pillars cast real shadow", &lamps);

    // Now a flashlight sweeping across, re-added on top of the cached base every tick.
    for (i, deg) in [200.0f32, 245.0, 290.0].iter().enumerate() {
        let a = deg.to_radians();
        let cone = FlashlightCone {
            source: IVec2::new(46, 9),
            forward: Vec2::new(a.cos(), a.sin()),
            intensity: 1.6,
            range: 22.0,
            // cos of the half-angle: ~30° cone.
            cone_cos: 30.0f32.to_radians().cos(),
            edge_softness: 0.15,
        };
        field.compose(&[cone], line_of_sight);
        plot(&field, &format!("DYNAMIC PASS {} — flashlight at {deg:.0}°", i + 1), &lamps);
    }

    // What an agent actually asks the field.
    println!("\nWhat a creature reads (illuminance, and normalised to peak):");
    for &(x, y, what) in &[
        (10, 4, "at the lamp"),
        (18, 7, "shadow behind the left pillar"),
        (28, 9, "open floor between them"),
        (44, 5, "behind the spur wall"),
    ] {
        let v = field.sample_cell(IVec2::new(x, y));
        println!("  ({x:>2},{y:>2}) {what:<32} {v:.4}   {:.3} of peak", v / field.peak().max(1e-6));
    }

    println!(
        "\nNone of this is a render. The GPU already knows what colour each pixel is — but a creature\n\
         deciding whether to scuttle into shadow cannot read a framebuffer."
    );
}
