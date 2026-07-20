//! `MetropolisSolver` — the Soft + Relational placement backend for `Freestanding` furniture.
//!
//! Adapts the optimize-a-density-function-via-Metropolis–Hastings method of
//! Merrell, Schkufza, Li, Agrawala & Koltun, "Interactive Furniture Layout Using Interior Design
//! Guidelines" (ACM TOG / SIGGRAPH 2011, DOI 10.1145/2010324.1964982). Each candidate is an oriented
//! footprint; a cost (density) function scores a layout by summing interior-design terms — non-overlap,
//! in-bounds, back-to-wall alignment, plus any explicit pairwise constraints — and Metropolis–Hastings
//! samples low-cost layouts. Per-region, seeded, and reproducible.
//!
//! **Deliberate departures from the paper** (this makes a game layout, not an interactive design tool):
//! - **Single-chain simulated annealing** (geometric cooling `temp_start`→`temp_end`), not the paper's
//!   fixed-temperature **parallel tempering** — that technique exists to *parallelize* the sampler, which
//!   the one-shot offline layout here does not need.
//! - **Two proposal moves** (a discrete 90° yaw snap + a uniform translation), not the paper's three
//!   (Gaussian translate, Gaussian rotate, and a two-item **swap**). The swap — the paper's engine for
//!   global rearrangement — is omitted; with only a few sparse pieces per room, annealing plus local
//!   moves explores adequately, and discrete 90° orientation suits axis-aligned Backrooms furniture.
//! - The back-to-wall reward is a **custom 1-fold** term (see [`MetropolisWeights::w_wall_angle`]), not the
//!   paper's 4-fold `m_wa`; and `w_wall` adds a **wall-proximity pull** (no analogue in the paper, which in
//!   fact critiques wall-hugging for large items) — both are deliberate Backrooms choices, not Merrell terms.
//!
//! Weights live in the `placement.metropolis` slice of `assets/config/config.ron` so layout is re-tunable with no code change
//! (Merrell 2011 reports robustness to ~2× weight perturbation).

use std::f32::consts::{FRAC_PI_2, TAU};

use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::dungeon::WALL_THICKNESS;
use crate::placement::ir::{
    Candidate, Capabilities, Constraint, Hardness, Locality, Modality, Outcome, Placement,
    PlacementProblem, Predicate, Region, Role, Scope, SolveError,
};
use crate::placement::solver::Solver;
use crate::rng::DetRng;

/// Wall inset so a footprint edge stops short of the wall slab. Tracks `dungeon::WALL_THICKNESS`
/// directly (one source of truth) so a change to the wall slab can't silently desync the layout inset.
const WALL_INSET: f32 = WALL_THICKNESS;

/// Tunable cost weights + MH schedule, loaded from RON (Merrell 2011 density terms).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetropolisWeights {
    pub iterations: u32,
    pub temp_start: f64,
    pub temp_end: f64,
    pub translate_sigma: f32,
    pub rotate_prob: f64,
    pub w_overlap: f64,
    pub w_bounds: f64,
    pub w_wall: f64,
    pub w_min_distance: f64,
    pub w_facing: f64,
    pub w_clearance: f64,
    /// Multiplier applied to a **Hard** constraint's cost so the sampler drives it to satisfaction
    /// rather than treating it as one soft term among many (e.g. a fixture's back-to-wall rule).
    pub w_hard: f64,
    /// Angular wall-snap strength for Hard `AgainstWall` fixtures: rewards a wall-backed piece for
    /// orienting so its **back** faces the nearest wall (front into the room). This is a custom **1-fold**
    /// back-to-wall term, NOT Merrell 2011's `m_wa` (which is `-cos(4Δθ)`, 4-fold — it rewards *any*
    /// axis-aligned orientation equally, with no front/back preference). The game wants backs-to-wall.
    pub w_wall_angle: f64,
    /// Grouping strength for the `Near` band that draws same-group pieces together (toilet + sink).
    pub w_group: f64,
    /// How coherently related furniture is arranged, in [0, 1]. Scales the `Facing` relation (a seat
    /// facing its screen): 0 = pieces ignore each other (sparse, backrooms-random), 1 = fully arranged
    /// (sofa firmly faces the TV). Tunable independently of the raw `w_facing` strength.
    pub coherence: f64,
}

pub struct MetropolisSolver {
    weights: MetropolisWeights,
}

impl MetropolisSolver {
    pub fn new(weights: MetropolisWeights) -> Self {
        Self { weights }
    }
}

/// One object's live state during optimization: centre (x, z) in world/tile coords + yaw about +Y.
#[derive(Clone, Copy)]
struct Obj {
    x: f32,
    z: f32,
    yaw: f32,
    // Native footprint half-extents (before yaw); AABB half-extents are swapped at 90°/270°.
    hw: f32,
    hd: f32,
}

impl Obj {
    /// Axis-aligned half-extents given the current (quarter-turn) yaw.
    fn half_extents(&self) -> (f32, f32) {
        // Quarter-turn furniture: at 90°/270° width and depth swap.
        let quarter = (self.yaw / FRAC_PI_2).round() as i32 & 3;
        if quarter % 2 == 1 {
            (self.hd, self.hw)
        } else {
            (self.hw, self.hd)
        }
    }
}

/// Interior bounds (min/max object-centre coords) after wall inset — computed per footprint at eval.
struct Bounds {
    xmin: f32,
    xmax: f32,
    zmin: f32,
    zmax: f32,
}

impl Solver for MetropolisSolver {
    fn name(&self) -> &str {
        "metropolis"
    }

    fn handles(&self, role: &Role) -> bool {
        matches!(role, Role::Freestanding)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // Both: soft interior-design terms PLUS hard per-piece rules the annealer drives to
            // satisfaction via `w_hard` (e.g. a plumbing fixture's back-to-wall association). A layout
            // that still violates a hard term after the budget degrades to `Outcome::Partial`.
            hardness: Hardness::Both,
            locality: Locality::Relational,
            cardinality: false,
            deterministic: true,
            needs_training_data: false,
        }
    }

    fn solve(
        &self,
        problem: &PlacementProblem,
        rng: &mut ChaCha8Rng,
    ) -> Result<Outcome, SolveError> {
        let objs_meta: Vec<&Candidate> = problem.candidates.iter().collect();
        let n = objs_meta.len();
        if n == 0 {
            return Ok(Outcome::Assignment(Vec::new()));
        }

        // Room interior in world/tile coords: cell centres span [min, max-1]; walls sit half a tile
        // beyond, so an object centre is free within [min-0.5+inset+half, max-0.5-inset-half].
        let r = problem.region;
        let (rx0, rx1, rz0, rz1) = room_world_bounds(r);

        // Initial layout: each object dropped at a random in-bounds cell, random quarter-turn yaw.
        let mut cur: Vec<Obj> = objs_meta
            .iter()
            .map(|c| {
                let hw = c.footprint[0] * 0.5;
                let hd = c.footprint[1] * 0.5;
                let yaw = (rng.below(4) as f32) * FRAC_PI_2;
                let mut o = Obj {
                    x: 0.0,
                    z: 0.0,
                    yaw,
                    hw,
                    hd,
                };
                let b = obj_bounds(&o, rx0, rx1, rz0, rz1);
                o.x = rand_range(rng, b.xmin, b.xmax);
                o.z = rand_range(rng, b.zmin, b.zmax);
                o
            })
            .collect();

        let mut cur_cost = self.cost(&cur, problem, rx0, rx1, rz0, rz1);
        let mut best = cur.clone();
        let mut best_cost = cur_cost;

        let iters = self.weights.iterations.max(1);
        let (t0, t1) = (
            self.weights.temp_start.max(1e-6),
            self.weights.temp_end.max(1e-6),
        );
        for step in 0..iters {
            // Geometric simulated-annealing cooling t0 → t1 (single chain — see the module doc on why not
            // the paper's parallel tempering). One local move per step below: a discrete 90° yaw snap or a
            // uniform translate — the paper's Gaussian perturbations and two-item swap are adapted away.
            let frac = step as f64 / iters as f64;
            let temp = t0 * (t1 / t0).powf(frac);

            let i = rng.below(n);
            let saved = cur[i];
            if rng.unit() < self.weights.rotate_prob {
                cur[i].yaw = (cur[i].yaw + FRAC_PI_2) % TAU;
            } else {
                let s = self.weights.translate_sigma;
                cur[i].x += rand_range(rng, -s, s);
                cur[i].z += rand_range(rng, -s, s);
            }
            // Clamp into this object's in-bounds window so proposals stay legal.
            let b = obj_bounds(&cur[i], rx0, rx1, rz0, rz1);
            cur[i].x = cur[i].x.clamp(b.xmin, b.xmax);
            cur[i].z = cur[i].z.clamp(b.zmin, b.zmax);

            let cand_cost = self.cost(&cur, problem, rx0, rx1, rz0, rz1);
            let accept =
                cand_cost <= cur_cost || rng.unit() < (-(cand_cost - cur_cost) / temp).exp();
            if accept {
                cur_cost = cand_cost;
                if cur_cost < best_cost {
                    best_cost = cur_cost;
                    best = cur.clone();
                }
            } else {
                cur[i] = saved; // reject: restore
            }
        }

        let placed: Vec<Placement> = best
            .iter()
            .enumerate()
            .map(|(i, o)| Placement {
                candidate: i,
                pos: [o.x, 0.0, o.z],
                yaw: o.yaw,
            })
            .collect();

        // If explicit HARD constraints remain violated in the best layout, report them (graceful
        // degradation, risk R3) — otherwise the best-effort soft layout is the ranked result.
        let unsatisfied: Vec<_> = problem
            .constraints
            .iter()
            .filter(|c| matches!(c.modality, Modality::Hard))
            .filter(|c| self.constraint_cost(c, &best, (rx0, rx1, rz0, rz1)) > 1e-3)
            .map(|c| c.id)
            .collect();
        if unsatisfied.is_empty() {
            Ok(Outcome::Ranked(vec![(best_cost, placed)]))
        } else {
            Ok(Outcome::Partial {
                placed,
                unsatisfied,
            })
        }
    }
}

impl MetropolisSolver {
    /// Total layout cost (Merrell 2011 density function): the weighted sum of interior-design terms.
    fn cost(
        &self,
        objs: &[Obj],
        problem: &PlacementProblem,
        rx0: f32,
        rx1: f32,
        rz0: f32,
        rz1: f32,
    ) -> f64 {
        let w = &self.weights;
        let mut overlap = 0.0;
        let mut bounds = 0.0;

        for (i, a) in objs.iter().enumerate() {
            let (ahw, ahd) = a.half_extents();
            // In-bounds: quadratic penalty for any part of the footprint past a wall.
            let (bx0, bx1, bz0, bz1) = (rx0 + ahw, rx1 - ahw, rz0 + ahd, rz1 - ahd);
            bounds += over(bx0 - a.x) + over(a.x - bx1) + over(bz0 - a.z) + over(a.z - bz1);
            // Back-to-wall attraction is NOT applied unconditionally here: it is emitted per piece as
            // an explicit `AgainstWall` constraint (see `freestanding_constraints`) and scored once in
            // `constraint_cost`. Scoring it here too would double-count `w_wall` (~2× the tuned pull).

            for b in objs.iter().skip(i + 1) {
                overlap += aabb_overlap_area(a, b) as f64;
            }
        }

        let mut constraint_cost = 0.0;
        for c in &problem.constraints {
            let weight = match c.modality {
                Modality::Soft(wt) => wt,
                // Hard terms are scaled by `w_hard` so the annealer drives them to satisfaction rather
                // than trading them off against the soft terms one-for-one.
                Modality::Hard => w.w_hard,
            };
            constraint_cost += weight * self.constraint_cost(c, objs, (rx0, rx1, rz0, rz1));
        }

        w.w_overlap * overlap + w.w_bounds * bounds + constraint_cost
    }

    /// Cost contribution of one explicit constraint (0 = satisfied). `bounds` is the room interior
    /// (rx0, rx1, rz0, rz1) so wall-relative predicates can measure against the walls.
    fn constraint_cost(&self, c: &Constraint, objs: &[Obj], bounds: (f32, f32, f32, f32)) -> f64 {
        let w = &self.weights;
        match (&c.scope, &c.predicate) {
            (Scope::Pair(i, j), Predicate::MinDistance(min)) => {
                if let (Some(a), Some(b)) = (objs.get(*i), objs.get(*j)) {
                    let d = ((a.x - b.x).powi(2) + (a.z - b.z).powi(2)).sqrt();
                    w.w_min_distance * over(*min - d) as f64
                } else {
                    0.0
                }
            }
            (Scope::Object(i), Predicate::Facing(j)) => {
                if let (Some(a), Some(b)) = (objs.get(*i), objs.get(*j)) {
                    // Reward `a`'s facing direction pointing toward `b`: cost = 1 - cos(angle).
                    let (fx, fz) = (a.yaw.sin(), a.yaw.cos());
                    let (dx, dz) = (b.x - a.x, b.z - a.z);
                    let len = (dx * dx + dz * dz).sqrt().max(1e-4);
                    let dot = (fx * dx + fz * dz) / len;
                    // Scaled by `coherence` so the seat→screen arrangement is tunable from
                    // backrooms-random (0) to living-room-coherent (1) without retuning `w_facing`.
                    w.w_facing * w.coherence * (1.0 - dot as f64)
                } else {
                    0.0
                }
            }
            (Scope::Object(i), Predicate::Clearance(gap)) => {
                // Penalize other objects intruding within `gap` of object i's footprint.
                let Some(a) = objs.get(*i) else { return 0.0 };
                let (ahw, ahd) = a.half_extents();
                let mut pen = 0.0;
                for (k, b) in objs.iter().enumerate() {
                    if k == *i {
                        continue;
                    }
                    let (bhw, bhd) = b.half_extents();
                    let dx = (a.x - b.x).abs() - (ahw + bhw);
                    let dz = (a.z - b.z).abs() - (ahd + bhd);
                    let sep = dx.max(dz); // separation between footprints (negative = overlap)
                    pen += over(gap - sep) as f64;
                }
                w.w_clearance * pen
            }
            (Scope::Object(i), Predicate::Aligned(_feature)) => {
                // Domain-swap predicate (e.g. `aligned(a, "road")`): align the object's facing to the
                // road axis (running along X). Penalty = |sin(yaw)|, zero when yaw ∈ {0, π}. Adding a
                // new domain adds a predicate like this, not a new engine (vetting §3.3).
                let Some(a) = objs.get(*i) else { return 0.0 };
                w.w_facing * a.yaw.sin().abs() as f64
            }
            (Scope::Object(i), Predicate::AgainstWall) => {
                // Distance from object i's footprint to each of the four walls (≥0 inside the room);
                // minimizing the nearest one seats the back against a wall.
                let Some(a) = objs.get(*i) else { return 0.0 };
                let (ahw, ahd) = a.half_extents();
                let (rx0, rx1, rz0, rz1) = bounds;
                let dw = ((a.x - ahw) - rx0).max(0.0); // to the west wall (inward normal +X)
                let de = (rx1 - (a.x + ahw)).max(0.0); // to the east wall (inward normal -X)
                let dn = ((a.z - ahd) - rz0).max(0.0); // to the north wall (inward normal +Z)
                let ds = (rz1 - (a.z + ahd)).max(0.0); // to the south wall (inward normal -Z)
                let d = dw.min(de).min(dn).min(ds);
                let mut cost = w.w_wall * d as f64;
                // Angular wall-snap (Merrell 2011 m_wa), applied to HARD fixtures (toilet/sink/fridge):
                // reward the piece for facing INTO the room along the nearest wall's inward normal, so
                // its back — not its side — sits against the wall. `forward = (sin yaw, cos yaw)` matches
                // the convention the `Facing` term and the wall-light yaw (`furnish.rs`) already use.
                if matches!(c.modality, Modality::Hard) {
                    let (nx, nz) = if dw <= de && dw <= dn && dw <= ds {
                        (1.0, 0.0)
                    } else if de <= dn && de <= ds {
                        (-1.0, 0.0)
                    } else if dn <= ds {
                        (0.0, 1.0)
                    } else {
                        (0.0, -1.0)
                    };
                    let (fx, fz) = (a.yaw.sin(), a.yaw.cos());
                    let align = 1.0 - (fx * nx + fz * nz); // 0 when the back faces the nearest wall
                    cost += w.w_wall_angle * align as f64;
                }
                cost
            }
            (Scope::Pair(i, j), Predicate::Near(max)) => {
                // Grouping band: draw the pair within `max` metres of each other (toilet + sink on the
                // same wall). Zero once they're close; grows with the excess distance. Overlap is
                // handled separately by the layout's overlap term, so this only needs the near side.
                if let (Some(a), Some(b)) = (objs.get(*i), objs.get(*j)) {
                    let d = ((a.x - b.x).powi(2) + (a.z - b.z).powi(2)).sqrt();
                    w.w_group * over(d - max) as f64
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }
}

/// A one-sided ("hinge") penalty: `x` if positive, else 0.
#[inline]
fn over(x: f32) -> f64 {
    x.max(0.0) as f64
}

#[inline]
fn rand_range(rng: &mut ChaCha8Rng, lo: f32, hi: f32) -> f32 {
    if hi <= lo {
        return lo;
    }
    lo + (rng.unit() as f32) * (hi - lo)
}

/// AABB overlap area of two oriented (quarter-turn) footprints.
fn aabb_overlap_area(a: &Obj, b: &Obj) -> f32 {
    let (ahw, ahd) = a.half_extents();
    let (bhw, bhd) = b.half_extents();
    let ox = (ahw + bhw) - (a.x - b.x).abs();
    let oz = (ahd + bhd) - (a.z - b.z).abs();
    if ox > 0.0 && oz > 0.0 {
        ox * oz
    } else {
        0.0
    }
}

/// Room interior extents in world/tile coords, inset for the wall slab. Object centres live inside.
fn room_world_bounds(r: &Region) -> (f32, f32, f32, f32) {
    // Cell centres span [min, max-1]; the wall is half a tile beyond the outermost cell centre.
    let rx0 = r.rect.min[0] as f32 - 0.5 + WALL_INSET;
    let rx1 = (r.rect.max[0] - 1) as f32 + 0.5 - WALL_INSET;
    let rz0 = r.rect.min[1] as f32 - 0.5 + WALL_INSET;
    let rz1 = (r.rect.max[1] - 1) as f32 + 0.5 - WALL_INSET;
    (rx0, rx1, rz0, rz1)
}

/// The in-bounds window for a specific object's centre (room bounds shrunk by its half-extents).
fn obj_bounds(o: &Obj, rx0: f32, rx1: f32, rz0: f32, rz1: f32) -> Bounds {
    let (hw, hd) = o.half_extents();
    Bounds {
        xmin: rx0 + hw,
        xmax: (rx1 - hw).max(rx0 + hw),
        zmin: rz0 + hd,
        zmax: (rz1 - hd).max(rz0 + hd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::ir::{Dof, PropertyBag, Rect2, Role};
    use crate::rng::seeded;

    fn weights() -> MetropolisWeights {
        MetropolisWeights {
            iterations: 3000,
            temp_start: 1.0,
            temp_end: 0.02,
            translate_sigma: 0.6,
            rotate_prob: 0.35,
            w_overlap: 10.0,
            w_bounds: 25.0,
            w_wall: 1.2,
            w_min_distance: 2.0,
            w_facing: 1.5,
            w_clearance: 2.0,
            w_hard: 12.0,
            w_wall_angle: 1.0,
            w_group: 1.5,
            coherence: 0.3,
        }
    }
    fn region() -> Region {
        Region {
            id: 0,
            rect: Rect2 {
                min: [0, 0],
                max: [5, 5],
            }, // 5×5 room
            openings: Vec::new(),
            adjacency: Vec::new(),
            props: PropertyBag::default(),
        }
    }
    fn item(w: f32, d: f32) -> Candidate {
        Candidate {
            asset: "x".into(),
            role: Role::Freestanding,
            footprint: [w, d],
            dof: Dof {
                translate: true,
                rotate_quarter: true,
                rotate_free: false,
            },
            affordances: Vec::new(),
        }
    }

    #[test]
    fn keeps_objects_inside_and_non_overlapping() {
        let r = region();
        let problem = PlacementProblem {
            region: &r,
            candidates: vec![item(1.6, 0.7), item(2.0, 0.9), item(0.9, 0.6)].into(),
            constraints: Vec::new(),
        };
        let mut rng = seeded(3);
        let solver = MetropolisSolver::new(weights());
        let out = solver.solve(&problem, &mut rng).expect("solve");
        let placed = match out {
            Outcome::Ranked(mut v) => v.remove(0).1,
            Outcome::Assignment(p) => p,
            Outcome::Partial { placed, .. } => placed,
        };
        assert_eq!(placed.len(), 3);
        let (rx0, rx1, rz0, rz1) = room_world_bounds(&r);
        // Reconstruct Objs to check bounds/overlap on the returned layout.
        let objs: Vec<Obj> = placed
            .iter()
            .map(|p| {
                let c = &problem.candidates[p.candidate];
                Obj {
                    x: p.pos[0],
                    z: p.pos[2],
                    yaw: p.yaw,
                    hw: c.footprint[0] * 0.5,
                    hd: c.footprint[1] * 0.5,
                }
            })
            .collect();
        for o in &objs {
            let (hw, hd) = o.half_extents();
            assert!(
                o.x - hw >= rx0 - 0.05 && o.x + hw <= rx1 + 0.05,
                "x out of bounds"
            );
            assert!(
                o.z - hd >= rz0 - 0.05 && o.z + hd <= rz1 + 0.05,
                "z out of bounds"
            );
        }
        // Overlap should be driven near zero by the optimizer.
        let mut total = 0.0;
        for (i, a) in objs.iter().enumerate() {
            for b in objs.iter().skip(i + 1) {
                total += aabb_overlap_area(a, b);
            }
        }
        assert!(total < 0.25, "residual overlap too high: {total}");
    }

    #[test]
    fn hard_against_wall_seats_a_fixture_flush() {
        // README ISSUE 3: a HARD AgainstWall fixture is driven to sit flush against a wall (not merely
        // pulled toward it as a soft preference), with its back — the -forward side — to that wall.
        let r = region(); // 5×5
        let problem = PlacementProblem {
            region: &r,
            candidates: vec![item(0.4, 0.6)].into(), // toilet-sized
            constraints: vec![Constraint {
                id: 0,
                scope: Scope::Object(0),
                predicate: Predicate::AgainstWall,
                modality: Modality::Hard,
                guard: None,
            }],
        };
        let solver = MetropolisSolver::new(weights());
        // Deterministic: a handful of seeds must all seat the fixture flush.
        for seed in [1u64, 7, 42] {
            let mut rng = seeded(seed);
            let placed = match solver.solve(&problem, &mut rng).expect("solve") {
                Outcome::Ranked(mut v) => v.remove(0).1,
                Outcome::Assignment(p) => p,
                Outcome::Partial { placed, .. } => placed,
            };
            let p = placed[0];
            let o = Obj {
                x: p.pos[0],
                z: p.pos[2],
                yaw: p.yaw,
                hw: 0.2,
                hd: 0.3,
            };
            let (hw, hd) = o.half_extents();
            let (rx0, rx1, rz0, rz1) = room_world_bounds(&r);
            let d = ((o.x - hw) - rx0)
                .min(rx1 - (o.x + hw))
                .min((o.z - hd) - rz0)
                .min(rz1 - (o.z + hd))
                .max(0.0);
            assert!(
                d < 0.12,
                "seed {seed}: fixture should sit flush to a wall, got d={d}"
            );
        }
    }

    #[test]
    fn deterministic_under_seed() {
        let r = region();
        let problem = PlacementProblem {
            region: &r,
            candidates: vec![item(1.6, 0.7), item(0.9, 0.6)].into(),
            constraints: Vec::new(),
        };
        let solver = MetropolisSolver::new(weights());
        let run = || {
            let mut rng = seeded(11);
            match solver.solve(&problem, &mut rng).expect("solve") {
                Outcome::Ranked(v) => v[0]
                    .1
                    .iter()
                    .map(|p| (p.pos[0].to_bits(), p.pos[2].to_bits()))
                    .collect::<Vec<_>>(),
                _ => panic!("expected ranked"),
            }
        };
        assert_eq!(run(), run());
    }
}
