//! Hand-rolled computational geometry for the graph dungeon topology (`Topology::Graph`): Poisson-disk
//! site sampling (Bridson 2007, "Fast Poisson Disk Sampling in Arbitrary Dimensions") + an incremental
//! Bowyer–Watson Delaunay triangulation, then a degree cap for the u32-masked graph collapse. Engine-
//! free (no Bevy) and fully deterministic — every random choice flows from the caller's [`DetRng`], so a
//! seed reproduces the same layout. `f64` throughout for a bit-reproducible in-circle predicate.
//!
//! Until the graph front-end wires these in (`dungeon::graph_layout`), the module is consumed only by
//! its own tests, so the public surface is `#![allow(dead_code)]` for now (removed in Step 5).
#![allow(dead_code)]

use crate::rng::DetRng;

/// A 2D point in fine-grid coordinates.
pub type Point = [f64; 2];

/// Bridson Poisson-disk sampling over `[0, w) × [0, h)` with minimum spacing `radius` and `k` candidate
/// attempts per active sample. Deterministic from `rng`. Returns the accepted points — always ≥ 1 for a
/// positive rect and radius (the seed point is placed first).
pub fn poisson_disk(w: f64, h: f64, radius: f64, k: usize, rng: &mut impl DetRng) -> Vec<Point> {
    // Background grid: cell size `radius / √2` so each cell holds at most one sample.
    let cell = radius / std::f64::consts::SQRT_2;
    let gw = ((w / cell).ceil() as usize).max(1);
    let gh = ((h / cell).ceil() as usize).max(1);
    let mut grid: Vec<Option<usize>> = vec![None; gw * gh];
    let mut samples: Vec<Point> = Vec::new();
    let mut active: Vec<usize> = Vec::new();

    let grid_cell = |p: Point| -> (usize, usize) {
        let gx = ((p[0] / cell) as usize).min(gw - 1);
        let gy = ((p[1] / cell) as usize).min(gh - 1);
        (gx, gy)
    };
    let fits = |p: Point, samples: &[Point], grid: &[Option<usize>]| -> bool {
        let (cx, cy) = grid_cell(p);
        // A sample within `radius` lies within ~1.5 cells, so a 2-cell window covers it.
        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                if nx < 0 || ny < 0 || nx >= gw as i32 || ny >= gh as i32 {
                    continue;
                }
                if let Some(si) = grid[ny as usize * gw + nx as usize] {
                    let s = samples[si];
                    if (s[0] - p[0]).powi(2) + (s[1] - p[1]).powi(2) < radius * radius {
                        return false;
                    }
                }
            }
        }
        true
    };

    let first = [rng.unit() * w, rng.unit() * h];
    let (fx, fy) = grid_cell(first);
    grid[fy * gw + fx] = Some(0);
    samples.push(first);
    active.push(0);

    while !active.is_empty() {
        let ai = rng.below(active.len());
        let center = samples[active[ai]];
        let mut placed = false;
        for _ in 0..k {
            // Candidate uniformly in the annulus [radius, 2·radius) around `center`.
            let ang = rng.unit() * std::f64::consts::TAU;
            let rad = radius * (1.0 + rng.unit());
            let cand = [center[0] + rad * ang.cos(), center[1] + rad * ang.sin()];
            if cand[0] < 0.0 || cand[0] >= w || cand[1] < 0.0 || cand[1] >= h {
                continue;
            }
            if fits(cand, &samples, &grid) {
                let idx = samples.len();
                let (cx, cy) = grid_cell(cand);
                grid[cy * gw + cx] = Some(idx);
                samples.push(cand);
                active.push(idx);
                placed = true;
                break;
            }
        }
        if !placed {
            active.swap_remove(ai); // exhausted; deterministic O(1) removal
        }
    }
    samples
}

/// Bowyer–Watson Delaunay triangulation of `points` → the undirected edge set (each edge once, as an
/// ascending `(min, max)` index pair). Deterministic: a fixed insertion order (the caller's Poisson
/// order) with strict-sign in-circle tests (co-circular points leave a locally-non-Delaunay-but-valid
/// edge rather than flip-flopping). `< 2` points → no edges; exactly 2 → the single edge.
pub fn delaunay_edges(points: &[Point]) -> Vec<(usize, usize)> {
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    if n == 2 {
        return vec![(0, 1)];
    }

    // Super-triangle enclosing every point, far enough out that its vertices never fall inside a real
    // circumcircle. Its three vertices get indices n, n+1, n+2 in the working point list.
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in points {
        minx = minx.min(p[0]);
        miny = miny.min(p[1]);
        maxx = maxx.max(p[0]);
        maxy = maxy.max(p[1]);
    }
    let dmax = (maxx - minx).max(maxy - miny).max(1.0);
    let (midx, midy) = ((minx + maxx) / 2.0, (miny + maxy) / 2.0);
    let mut pts: Vec<Point> = points.to_vec();
    pts.push([midx - 20.0 * dmax, midy - dmax]);
    pts.push([midx, midy + 20.0 * dmax]);
    pts.push([midx + 20.0 * dmax, midy - dmax]);

    let mut tris: Vec<[usize; 3]> = vec![[n, n + 1, n + 2]];

    for pi in 0..n {
        let p = pts[pi];
        let mut is_bad = vec![false; tris.len()];
        for (ti, t) in tris.iter().enumerate() {
            if in_circumcircle(pts[t[0]], pts[t[1]], pts[t[2]], p) {
                is_bad[ti] = true;
            }
        }
        // Boundary of the cavity = edges belonging to exactly one bad triangle.
        let mut boundary: Vec<(usize, usize)> = Vec::new();
        for ti in 0..tris.len() {
            if !is_bad[ti] {
                continue;
            }
            let t = tris[ti];
            for &(a, b) in &[(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let shared = (0..tris.len())
                    .any(|oj| oj != ti && is_bad[oj] && triangle_has_edge(tris[oj], a, b));
                if !shared {
                    boundary.push((a, b));
                }
            }
        }
        // Rebuild the triangle list without the bad triangles, then fan the cavity to the new point.
        let mut next: Vec<[usize; 3]> = Vec::with_capacity(tris.len());
        for ti in 0..tris.len() {
            if !is_bad[ti] {
                next.push(tris[ti]);
            }
        }
        for (a, b) in boundary {
            next.push([a, b, pi]);
        }
        tris = next;
    }

    // Edges of every triangle that does NOT touch a super-vertex, deduplicated.
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for t in &tris {
        if t[0] >= n || t[1] >= n || t[2] >= n {
            continue;
        }
        for &(a, b) in &[(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            edges.push((a.min(b), a.max(b)));
        }
    }
    edges.sort_unstable();
    edges.dedup();
    edges
}

/// Prune an undirected graph so every node has degree ≤ `max_degree`, removing the **longest** edges
/// first (tie-break by ascending `(min, max)` for determinism) from any endpoint still over the cap.
/// Keeps the graph symmetric. The largest-component step downstream backstops the rare case where a
/// removal strands a node (removed edges are the longest, so bridges are unlikely to go).
pub fn prune_to_max_degree(
    points: &[Point],
    edges: &[(usize, usize)],
    max_degree: usize,
) -> Vec<(usize, usize)> {
    let len2 = |e: &(usize, usize)| {
        let (a, b) = (points[e.0], points[e.1]);
        (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
    };
    let mut degree = vec![0usize; points.len()];
    for &(a, b) in edges {
        degree[a] += 1;
        degree[b] += 1;
    }
    // Process longest-first; total_cmp gives a total order over the (finite) squared lengths.
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by(|&i, &j| {
        len2(&edges[j])
            .total_cmp(&len2(&edges[i]))
            .then(edges[i].cmp(&edges[j]))
    });
    let mut removed = vec![false; edges.len()];
    for &ei in &order {
        let (a, b) = edges[ei];
        if degree[a] > max_degree || degree[b] > max_degree {
            removed[ei] = true;
            degree[a] -= 1;
            degree[b] -= 1;
        }
    }
    edges
        .iter()
        .enumerate()
        .filter(|(i, _)| !removed[*i])
        .map(|(_, &e)| e)
        .collect()
}

/// Is `p` strictly inside the circumcircle of triangle `abc`? Orientation-normalized determinant test;
/// a degenerate (collinear) triangle reports `false`. Strict sign (no tolerance) keeps the bad-triangle
/// classification internally consistent, so the cavity boundary is always well-formed.
fn in_circumcircle(a: Point, b: Point, c: Point, p: Point) -> bool {
    let orient = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    if orient.abs() < 1e-12 {
        return false; // degenerate triangle — no meaningful circumcircle
    }
    let (ax, ay) = (a[0] - p[0], a[1] - p[1]);
    let (bx, by) = (b[0] - p[0], b[1] - p[1]);
    let (cx, cy) = (c[0] - p[0], c[1] - p[1]);
    let det = (ax * ax + ay * ay) * (bx * cy - cx * by)
        - (bx * bx + by * by) * (ax * cy - cx * ay)
        + (cx * cx + cy * cy) * (ax * by - bx * ay);
    if orient > 0.0 {
        det > 0.0
    } else {
        det < 0.0
    }
}

/// Does triangle `t` contain the undirected edge `(a, b)`?
fn triangle_has_edge(t: [usize; 3], a: usize, b: usize) -> bool {
    let has = |x: usize| t[0] == x || t[1] == x || t[2] == x;
    has(a) && has(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;

    fn dist2(a: Point, b: Point) -> f64 {
        (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
    }

    #[test]
    fn poisson_is_deterministic() {
        let a = poisson_disk(200.0, 200.0, 16.0, 30, &mut seeded(9));
        let b = poisson_disk(200.0, 200.0, 16.0, 30, &mut seeded(9));
        assert_eq!(a, b, "same seed must reproduce the same sites");
    }

    #[test]
    fn poisson_respects_spacing_and_bounds() {
        let radius = 16.0;
        let pts = poisson_disk(200.0, 200.0, radius, 30, &mut seeded(3));
        assert!(pts.len() > 20, "should fill a 200x200 rect at spacing 16, got {}", pts.len());
        for &p in &pts {
            assert!(p[0] >= 0.0 && p[0] < 200.0 && p[1] >= 0.0 && p[1] < 200.0, "point out of rect");
        }
        for (i, &a) in pts.iter().enumerate() {
            for &b in &pts[i + 1..] {
                assert!(dist2(a, b) >= radius * radius * (1.0 - 1e-6), "two sites closer than radius");
            }
        }
    }

    #[test]
    fn delaunay_small_cases() {
        assert_eq!(delaunay_edges(&[[0.0, 0.0]]), Vec::new());
        assert_eq!(delaunay_edges(&[[0.0, 0.0], [1.0, 0.0]]), vec![(0, 1)]);
        // A unit square: 4 hull edges + 1 diagonal = 5 undirected edges.
        let sq = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let e = delaunay_edges(&sq);
        assert_eq!(e.len(), 5, "square triangulation has 5 edges, got {e:?}");
    }

    #[test]
    fn delaunay_every_point_is_a_vertex() {
        let pts = poisson_disk(180.0, 180.0, 20.0, 30, &mut seeded(11));
        let edges = delaunay_edges(&pts);
        let mut seen = vec![false; pts.len()];
        for &(a, b) in &edges {
            seen[a] = true;
            seen[b] = true;
        }
        assert!(seen.iter().all(|&s| s), "every site must be a triangulation vertex");
    }

    #[test]
    fn delaunay_is_deterministic() {
        let pts = poisson_disk(180.0, 180.0, 20.0, 30, &mut seeded(11));
        assert_eq!(delaunay_edges(&pts), delaunay_edges(&pts));
    }

    #[test]
    fn prune_caps_degree_and_is_deterministic() {
        let pts = poisson_disk(180.0, 180.0, 18.0, 30, &mut seeded(5));
        let edges = delaunay_edges(&pts);
        let pruned = prune_to_max_degree(&pts, &edges, 5);
        let mut degree = vec![0usize; pts.len()];
        for &(a, b) in &pruned {
            degree[a] += 1;
            degree[b] += 1;
        }
        assert!(degree.iter().all(|&d| d <= 5), "no node may exceed degree 5 after prune");
        assert!(pruned.len() <= edges.len());
        assert_eq!(pruned, prune_to_max_degree(&pts, &edges, 5), "prune must be deterministic");
    }

    #[test]
    fn prune_leaves_already_bounded_graph_untouched() {
        // A path 0-1-2 (max degree 2) is already under the cap.
        let pts = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
        let edges = vec![(0, 1), (1, 2)];
        assert_eq!(prune_to_max_degree(&pts, &edges, 5), edges);
    }
}
