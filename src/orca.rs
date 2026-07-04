//! Hand-rolled **ORCA** (Optimal Reciprocal Collision Avoidance) — the local, unit-vs-unit
//! avoidance layer beneath the flow-field global navigator (see `flowfield`). Given each unit's
//! *preferred* velocity (from the flow field), ORCA returns the collision-free velocity **closest**
//! to it, assuming every neighbor does the same reasoning and each side takes half the avoidance
//! (reciprocity). That reciprocity is what removes the oscillation and the mutual-cancel freeze of
//! the old summed-force separation: two units on a head-on course each step aside by half instead of
//! shoving equally and netting zero.
//!
//! References:
//! - van den Berg, Lin & Manocha, "Reciprocal Velocity Obstacles for Real-Time Multi-Agent
//!   Navigation", ICRA 2008, DOI 10.1109/ROBOT.2008.4543489 (reciprocity removes oscillation).
//! - van den Berg, Guy, Lin & Manocha, "Reciprocal n-Body Collision Avoidance", 2011,
//!   DOI 10.1109/TRO.2011.2120810 (the ORCA half-plane construction + 2-D linear program below).
//!
//! Only unit↔unit avoidance lives here; static walls are the flow field's concern (it never routes
//! through rock) and the collision resolver's authority (`dungeon::resolve_move`), so no static
//! obstacle lines are fed in — the LP's obstacle-line count is always zero.

use bevy::prelude::*;

/// Guard against division by near-zero / parallel-line degeneracies (RVO2's `RVO_EPSILON`).
const EPSILON: f32 = 1.0e-5;

/// A disc agent in the xz ground plane (y dropped): position, current velocity, radius.
///
/// `avoids` is whether this agent is *also* running avoidance this step. When a neighbor avoids
/// too (another commanded unit), the pair splits the avoidance 50/50 (reciprocity). When it does
/// not (an idle unit holding ground), the moving agent takes the **full** avoidance so the idle
/// unit isn't assumed to step aside and get walked through.
#[derive(Clone, Copy)]
pub struct Agent {
    pub pos: Vec2,
    pub vel: Vec2,
    pub radius: f32,
    pub avoids: bool,
}

/// A half-plane constraint `{ v : det(direction, v - point) >= 0 }` on the new velocity.
#[derive(Clone, Copy)]
struct Line {
    point: Vec2,
    direction: Vec2,
}

/// 2-D cross product (signed area); `> 0` means `b` is left of `a`.
#[inline]
fn det(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Compute the collision-avoiding velocity for `agent` nearest to `pref_vel`, given `neighbors`.
///
/// `time_horizon` is how many seconds ahead agent-agent collisions are anticipated (larger = more
/// cautious, earlier avoidance); `dt` is the simulation step (used for the in-contact case); result
/// magnitude is clamped to `max_speed`. Pure math — no panics, no allocation beyond the line list.
pub fn new_velocity(
    agent: &Agent,
    pref_vel: Vec2,
    neighbors: &[Agent],
    walls: &[(Vec2, f32)],
    time_horizon: f32,
    dt: f32,
    max_speed: f32,
) -> Vec2 {
    let inv_time_horizon = 1.0 / time_horizon;
    let inv_dt = 1.0 / dt;

    let mut lines: Vec<Line> = Vec::with_capacity(walls.len() + neighbors.len());

    // Wall half-planes come first as *hard* obstacle constraints (index 0..num_obstacle). Each is
    // `(b, approach)`: `b` is a unit vector from the agent toward a solid cell and `approach` is the
    // max speed at which the agent may still close on that wall this step (its remaining gap ÷ dt).
    // The constraint `v · b <= approach` lets the agent slide along or move away freely and coast up
    // to contact, but never accelerate *through* the wall — so ORCA won't dodge a neighbor by
    // steering into a wall and stalling. Actual contact is still resolved by `dungeon::resolve_move`.
    for &(b, approach) in walls {
        lines.push(Line {
            point: approach * b,
            direction: Vec2::new(-b.y, b.x),
        });
    }
    let num_obstacle = lines.len();

    for other in neighbors {
        let relative_position = other.pos - agent.pos;
        let relative_velocity = agent.vel - other.vel;
        let dist_sq = relative_position.length_squared();
        let combined_radius = agent.radius + other.radius;
        let combined_radius_sq = combined_radius * combined_radius;

        let u: Vec2;
        let direction: Vec2;

        if dist_sq > combined_radius_sq {
            // No collision yet. `w` is the vector from the cutoff-circle center to the relative velocity.
            let w = relative_velocity - inv_time_horizon * relative_position;
            let w_length_sq = w.length_squared();
            let dot1 = w.dot(relative_position);

            if dot1 < 0.0 && dot1 * dot1 > combined_radius_sq * w_length_sq {
                // Project on the cutoff circle.
                let w_length = w_length_sq.sqrt();
                let unit_w = if w_length > EPSILON { w / w_length } else { Vec2::ZERO };
                direction = Vec2::new(unit_w.y, -unit_w.x);
                u = (combined_radius * inv_time_horizon - w_length) * unit_w;
            } else {
                // Project on one of the velocity-obstacle legs.
                let leg = (dist_sq - combined_radius_sq).max(0.0).sqrt();
                if det(relative_position, w) > 0.0 {
                    // Left leg.
                    direction = Vec2::new(
                        relative_position.x * leg - relative_position.y * combined_radius,
                        relative_position.x * combined_radius + relative_position.y * leg,
                    ) / dist_sq;
                } else {
                    // Right leg.
                    direction = -Vec2::new(
                        relative_position.x * leg + relative_position.y * combined_radius,
                        -relative_position.x * combined_radius + relative_position.y * leg,
                    ) / dist_sq;
                }
                let dot2 = relative_velocity.dot(direction);
                u = dot2 * direction - relative_velocity;
            }
        } else {
            // Already overlapping. Push apart along the cutoff circle of the current time step.
            let w = relative_velocity - inv_dt * relative_position;
            let w_length = w.length();
            let unit_w = if w_length > EPSILON { w / w_length } else { Vec2::ZERO };
            direction = Vec2::new(unit_w.y, -unit_w.x);
            u = (combined_radius * inv_dt - w_length) * unit_w;
        }

        // Reciprocity: split the change with neighbors that also avoid; take it all against ones
        // that don't (idle units holding ground).
        let responsibility = if other.avoids { 0.5 } else { 1.0 };
        lines.push(Line {
            point: agent.vel + responsibility * u,
            direction,
        });
    }

    let mut result = pref_vel;
    let fail = linear_program2(&lines, max_speed, pref_vel, false, &mut result);
    if fail < lines.len() {
        linear_program3(&lines, num_obstacle, fail, max_speed, &mut result);
    }
    result
}

/// Optimize `result` along a single line `lines[line_no]` subject to the speed disc and the earlier
/// half-planes. Returns `false` if the constraints are infeasible for this line.
fn linear_program1(
    lines: &[Line],
    line_no: usize,
    radius: f32,
    opt_velocity: Vec2,
    direction_opt: bool,
    result: &mut Vec2,
) -> bool {
    let line = lines[line_no];
    let dot_product = line.point.dot(line.direction);
    let discriminant = dot_product * dot_product + radius * radius - line.point.length_squared();

    if discriminant < 0.0 {
        // The max-speed disc doesn't intersect this line at all — infeasible.
        return false;
    }

    let sqrt_discriminant = discriminant.sqrt();
    let mut t_left = -dot_product - sqrt_discriminant;
    let mut t_right = -dot_product + sqrt_discriminant;

    for i in 0..line_no {
        let denominator = det(line.direction, lines[i].direction);
        let numerator = det(lines[i].direction, line.point - lines[i].point);

        if denominator.abs() <= EPSILON {
            // Lines are (almost) parallel.
            if numerator < 0.0 {
                return false;
            }
            continue;
        }

        let t = numerator / denominator;
        if denominator >= 0.0 {
            t_right = t_right.min(t);
        } else {
            t_left = t_left.max(t);
        }
        if t_left > t_right {
            return false;
        }
    }

    if direction_opt {
        // Optimize direction: take the extreme feasible point in the preferred heading.
        if opt_velocity.dot(line.direction) > 0.0 {
            *result = line.point + t_right * line.direction;
        } else {
            *result = line.point + t_left * line.direction;
        }
    } else {
        // Optimize closest to `opt_velocity`.
        let t = line.direction.dot(opt_velocity - line.point);
        if t < t_left {
            *result = line.point + t_left * line.direction;
        } else if t > t_right {
            *result = line.point + t_right * line.direction;
        } else {
            *result = line.point + t * line.direction;
        }
    }
    true
}

/// Find the velocity within the speed disc closest to `opt_velocity` satisfying all `lines`.
/// Returns `lines.len()` on success, or the index of the first infeasible line.
fn linear_program2(
    lines: &[Line],
    radius: f32,
    opt_velocity: Vec2,
    direction_opt: bool,
    result: &mut Vec2,
) -> usize {
    if direction_opt {
        *result = opt_velocity * radius;
    } else if opt_velocity.length_squared() > radius * radius {
        *result = opt_velocity.normalize_or_zero() * radius;
    } else {
        *result = opt_velocity;
    }

    for i in 0..lines.len() {
        if det(lines[i].direction, lines[i].point - *result) > 0.0 {
            // `result` violates constraint i; re-optimize on that line.
            let temp = *result;
            if !linear_program1(lines, i, radius, opt_velocity, direction_opt, result) {
                *result = temp;
                return i;
            }
        }
    }
    lines.len()
}

/// Dense/infeasible fallback: when no velocity satisfies every agent half-plane, minimize the maximum
/// agent-constraint violation (the "safest" velocity). This is ORCA's defined behavior for
/// over-constrained crowds, not a feature fallback. The first `num_obstacle` lines are hard wall
/// constraints kept in every projection and never relaxed (an agent yields into a neighbor before it
/// walks through a wall). `begin_line` is the first line that failed in `linear_program2`.
fn linear_program3(lines: &[Line], num_obstacle: usize, begin_line: usize, radius: f32, result: &mut Vec2) {
    let mut distance = 0.0_f32;

    for i in begin_line.max(num_obstacle)..lines.len() {
        if det(lines[i].direction, lines[i].point - *result) > distance {
            // `result` violates line i by more than any seen so far; project the earlier lines onto
            // it and re-solve for the velocity that pushes constraint i's violation to a minimum. The
            // hard wall lines (0..num_obstacle) are always carried into the projection set.
            let mut proj_lines: Vec<Line> = lines[0..num_obstacle].to_vec();
            for j in num_obstacle..i {
                let determinant = det(lines[i].direction, lines[j].direction);
                let point = if determinant.abs() <= EPSILON {
                    if lines[i].direction.dot(lines[j].direction) > 0.0 {
                        // Same direction — no new constraint.
                        continue;
                    }
                    0.5 * (lines[i].point + lines[j].point)
                } else {
                    lines[i].point
                        + (det(lines[j].direction, lines[i].point - lines[j].point) / determinant)
                            * lines[i].direction
                };
                let direction = (lines[j].direction - lines[i].direction).normalize_or_zero();
                proj_lines.push(Line { point, direction });
            }

            let temp = *result;
            let opt = Vec2::new(-lines[i].direction.y, lines[i].direction.x);
            if linear_program2(&proj_lines, radius, opt, true, result) < proj_lines.len() {
                // Should not happen except through floating-point error; keep the safer prior result.
                *result = temp;
            }
            distance = det(lines[i].direction, lines[i].point - *result);
        }
    }
}
