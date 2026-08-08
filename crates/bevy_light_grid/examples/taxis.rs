//! **The three light responses, side by side.**
//!
//! Same field, three creatures. `light_push_at` returns a world-space nudge along the illuminance
//! gradient: pass a *negative* gain to flee the light (photophobic), a *positive* one to seek it
//! (photophilic). Zero where the field is flat — deep dark, or the middle of a uniform pool — so a
//! creature with nothing to go on is simply unbiased rather than shoved somewhere arbitrary.
//!
//! `phototropic_scale` is the odd one out: not steering at all, but a body that swells toward light,
//! rate-limited so the change stays sub-perceptual.
//!
//! Run: `cargo run -p bevy_light_grid --example taxis`

use bevy_light_grid::{light_push_at, phototropic_scale, LightGrid};
use bevy_math::IVec2;

const W: usize = 40;
const H: usize = 13;

fn all_floor() -> impl Iterator<Item = IVec2> {
    (0..H as i32).flat_map(|y| (0..W as i32).map(move |x| IVec2::new(x, y)))
}

/// No walls in this scene — the point here is the gradient, not occlusion.
fn unobstructed(_a: IVec2, _b: IVec2) -> bool {
    true
}

fn main() {
    let mut field = LightGrid::new(W, H, all_floor());
    // One lamp, left of centre, so there is a clean gradient to walk.
    field.bake(&[(IVec2::new(12, 6), 1.0, 18.0)], unobstructed);
    field.compose(&[], unobstructed);

    let peak = field.peak().max(1.0e-6);

    println!("lamp at (12,6), peak illuminance {peak:.3}\n");
    println!("  cell      light   light01 | photophobic push   photophilic push");
    println!("  ---------------------------------------------------------------");
    for &(x, y) in &[(12, 6), (16, 6), (22, 6), (30, 6), (38, 6), (12, 1)] {
        let c = IVec2::new(x, y);
        let light = field.sample_cell(c);
        let flee = light_push_at(&field, c, -1.0);
        let seek = light_push_at(&field, c, 1.0);
        println!(
            "  ({x:>2},{y:>2})   {light:.4}   {:.3}  | ({:>6.3},{:>6.3})     ({:>6.3},{:>6.3})",
            light / peak,
            flee.x,
            flee.z,
            seek.x,
            seek.z,
        );
    }
    println!(
        "\n  The two pushes are exact negatives — one taxis, one sign. And note the far-right cell:\n\
         out past the lamp's reach the gradient is flat, so both pushes go to zero."
    );

    // A photophobic walker, released in the light, stepping down the gradient each tick.
    println!("\nA photophobic creature released at (14,6), 24 steps of gradient descent:");
    let mut pos = IVec2::new(14, 6);
    for step in 0..24 {
        let push = light_push_at(&field, pos, -1.0);
        // Step one cell along whichever axis the push is strongest on.
        if push.length_squared() > 1.0e-9 {
            if push.x.abs() >= push.z.abs() {
                pos.x = (pos.x + push.x.signum() as i32).clamp(0, W as i32 - 1);
            } else {
                pos.y = (pos.y + push.z.signum() as i32).clamp(0, H as i32 - 1);
            }
        }
        if step % 6 == 0 || step == 23 {
            println!(
                "  step {step:>2}  at ({:>2},{:>2})  light {:.4}",
                pos.x,
                pos.y,
                field.sample_cell(pos)
            );
        }
    }

    // Phototropic: a body easing toward `base * (1 + bonus * light01)`, never jumping there.
    println!("\nA phototropic body (base 1.0, bonus 0.6) carried from dark into the lamp:");
    let mut scale = 1.0f32;
    for (tick, x) in (12..=30).rev().step_by(2).enumerate() {
        let light01 = field.sample_cell(IVec2::new(x, 6)) / peak;
        scale = phototropic_scale(1.0, scale, light01, 0.6, 0.02);
        if tick % 2 == 0 {
            println!("  at x={x:>2}  light01 {light01:.3}  scale {scale:.4}");
        }
    }
    println!(
        "\n  Capped at 0.02 per tick, so it grows visibly but never pops — the rate limit is the\n\
         difference between a creature reacting to light and a creature flickering in it."
    );
}
