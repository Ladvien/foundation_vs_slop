//! **No transitions: the entire locomotion system is a weight vector.**
//!
//! Every clip in a rig's blend set stays resident on the `AnimationPlayer` forever and is never
//! restarted. Each frame only two things move — the eased clip *weights*, and one shared gait phase.
//! This example prints the weights across the whole speed × heading domain, with no engine, no assets
//! and no `App`, because that is genuinely all there is to it.
//!
//! Why it matters: a transition would *restart* a clip, and a restarted clip slides the foot that was
//! planted. Crossfading by weight alone is what keeps contact through a walk→run change.
//!
//! Run: `cargo run -p emerge-anim --example blend_weights`

use emerge_anim::blend::{
    dir_weights, locomotion_weights, tier_weights, travel_angle, LOCO_SLOTS, MOVE_BAND, RUN_BAND,
};
use emerge_anim::wrap01;

const SLOTS: [&str; LOCO_SLOTS] =
    ["idle", "idle_alert", "walk", "run", "walk_bk", "run_bk", "strafe_L", "strafe_R"];

fn main() {
    println!("Bands: moving ramps over {MOVE_BAND:?} m/s, run ramps over {RUN_BAND:?} m/s.\n");

    println!("Walking forward, accelerating from a standstill (theta = 0, not aiming):");
    print!("  speed |");
    for s in SLOTS {
        print!(" {s:>9}");
    }
    println!("  |   sum");
    for step in 0..=14 {
        let speed = step as f32 * 0.25;
        let w = locomotion_weights(speed, 0.0, false);
        print!("  {speed:>5.2} |");
        for v in w {
            print!(" {v:>9.3}");
        }
        println!("  | {:>5.3}", w.iter().sum::<f32>());
    }
    println!(
        "\n  Watch idle → walk → run hand off smoothly, and the sum stay at exactly 1.000 the whole\n\
         way. No clip was ever started or stopped to make that happen."
    );

    println!("\nHeading sweep at a constant 1.8 m/s — the four direction weights:");
    println!("  heading |       fwd     right      back      left |  strafe_L  strafe_R");
    for deg in (0..360).step_by(30) {
        let theta = (deg as f32).to_radians();
        let d = dir_weights(theta);
        let w = locomotion_weights(1.8, theta, false);
        println!(
            "  {deg:>5}°  | {:>9.3} {:>9.3} {:>9.3} {:>9.3} | {:>9.3} {:>9.3}",
            d[0], d[1], d[2], d[3], w[6], w[7]
        );
    }

    println!("\n`aiming` only picks which idle the character settles into — the moving clips are untouched,");
    println!("because aiming while moving is layered onto the upper body instead:");
    for (speed, aiming) in [(0.0, false), (0.0, true), (1.8, false), (1.8, true)] {
        let w = locomotion_weights(speed, 0.0, aiming);
        println!(
            "  speed {speed:>4.1}  aiming {:<5}  idle {:.3}  idle_alert {:.3}  walk {:.3}  run {:.3}",
            aiming, w[0], w[1], w[2], w[3]
        );
    }

    println!("\ntier_weights(speed) → (moving, fast), the two smoothsteps everything else is built from:");
    for speed in [0.0f32, 0.1, 0.3, 0.5, 1.0, 1.3, 1.85, 2.4, 3.0] {
        let (moving, fast) = tier_weights(speed);
        println!("  {speed:>4.2} m/s → moving {moving:.3}  fast {fast:.3}");
    }

    println!("\ntravel_angle maps a local movement direction to the heading the blend uses:");
    for (name, v) in [
        ("forward", bevy::math::Vec2::new(0.0, 1.0)),
        ("right", bevy::math::Vec2::new(1.0, 0.0)),
        ("back", bevy::math::Vec2::new(0.0, -1.0)),
        ("left", bevy::math::Vec2::new(-1.0, 0.0)),
        ("fwd-right", bevy::math::Vec2::new(1.0, 1.0)),
    ] {
        let theta = travel_angle(v);
        println!("  {name:<10} ({:>5.2},{:>5.2}) → {:>7.3} rad ({:>6.1}°)", v.x, v.y, theta, theta.to_degrees());
    }

    // The shared gait phase is the other moving part: one normalised cursor every clip reads, wrapped
    // rather than reset, so nothing ever jumps back to frame zero.
    println!("\nThe one shared gait phase, advanced past 1.0 — wrapped, never reset:");
    let mut phase = 0.0f32;
    for tick in 0..8 {
        phase = wrap01(phase + 0.17);
        println!("  tick {tick}: phase {phase:.4}");
    }

    println!(
        "\nNever add `AnimationTransitions` to anything this drives — its PostUpdate pass would stomp\n\
         every weight printed above."
    );
}
