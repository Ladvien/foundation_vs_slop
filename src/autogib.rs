//! # Autogib — runtime mesh fracture for character gibbing
//!
//! Turns a character's own mesh into flying chunks on death, instead of the pre-authored cube
//! debris `gore` used to fling. The whole thing is one clean path: **pre-fracture the mesh once,
//! cache it, swap the fragments in at death** — the standard both shipped-game–oriented fracture
//! papers endorse for real time:
//!
//! - Müller, Chentanez & Kim, "Real-Time Dynamic Fracture with Volumetric Approximate Convex
//!   Decompositions", ACM TOG 2013 (DOI 10.1145/2461912.2461934). Their §1 documents exactly this
//!   "pre-fracture the asset, replace the intact model at run-time" pipeline as the practical norm.
//!   Their own runtime VACD needs a rigid-body solver + convex decomposition this project has not.
//! - Sellán, Luong, Mattos Da Silva, Ramakrishnan, Yang & Jacobson, "Breaking Good: Fracture Modes
//!   for Realtime Destruction", ACM TOG 2022 (DOI 10.1145/3549540). Most realistic, but the
//!   fragments are precomputed **offline** via tetrahedralization + a conic solve (a Python/libigl
//!   toolchain) — not embeddable in a minimal-dep Rust/Bevy crate. They also note geometric
//!   plane-cut prefracture artifacts are acceptably "hidden behind destruction dust or obscured by
//!   fast explosions" — precisely our fast, bloody, arcade case.
//!
//! So we use the geometric plane-cutter prefracture family Breaking Good compares against
//! (Schvartzman & Otaduy 2014; Museth et al. 2021 — "bumpy planes slicing through the input"):
//! recursively slice the merged character mesh with random planes, **capping each cut watertight**
//! (Sutherland–Hodgman triangle clip → welded boundary-loop assembly → fan-triangulated cap with a
//! planar cross-section UV). Watertight caps + cross-section UVs + runtime plane-slice are the
//! practical parts the graphics literature leaves to engine code — implemented here.
//!
//! The slicer core ([`Soup`] + `split_soup`/`clip_half`/`cap_side`/`fracture`) is pure CPU geometry
//! with no Bevy types, so it unit-tests without an `App`. Only the thin adapters at the edges
//! (`append_mesh` reads a loaded [`Mesh`], `soup_to_mesh` writes one) touch Bevy.
//!
//! **Generality (this figurine is only a greybox — a richer, possibly skinned character replaces it
//! later):** nothing keys off primitive counts or vertex layouts. Body geometry is discovered by
//! walking whatever scene a unit loads (skipping the gun), sub-meshes with missing normals/UVs are
//! synthesized, arbitrary/non-watertight input is welded and any unclosed cap dropped (never a
//! panic), fragment count/size scale off the mesh bounding box, and the cache is keyed by the source
//! asset id so swapping the GLB needs zero code change. The skinned path (snapshot the death pose
//! before fracturing, since bind-pose geometry isn't what the player sees) is not built — our
//! figurine is static.

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use crate::gore::GoreSettings;
use crate::squad::{GunModel, Unit};
use crate::util::hash_f32;

/// Classification tolerance: a vertex within `EPS` of the cut plane is treated as lying *on* it, so
/// slicing near-coincident geometry doesn't spawn zero-area slivers. Positions are in figurine-local
/// units (~1.0 tall), so this is a tight tolerance.
const EPS: f32 = 1.0e-5;
/// Endpoint-weld lattice step for boundary-loop assembly (quantize positions to this grid so cut
/// segments from adjacent triangles share canonical vertex ids even on non-watertight input).
const WELD: f32 = 1.0e-4;

// ---------------------------------------------------------------------------------------------
// Pure CPU triangle soup + slicer (no Bevy asset types — unit-testable without an App).
// ---------------------------------------------------------------------------------------------

/// A vertex sample carried through clipping (interpolated at edge–plane crossings).
#[derive(Clone, Copy)]
struct Vtx {
    pos: Vec3,
    nrm: Vec3,
    uv: Vec2,
}

/// A cut plane: a point on the plane and a unit normal.
struct Plane {
    point: Vec3,
    normal: Vec3,
}

/// CPU triangle soup. Parallel per-vertex arrays plus one triangle per `idx` entry; `tri_interior`
/// tags a triangle as a **cut-cap** face (gets the meat material) vs original **skin** (outfit tint).
/// Every vertex always carries a UV (zero-filled when the source lacked `UV_0`).
#[derive(Default, Clone)]
struct Soup {
    pos: Vec<Vec3>,
    nrm: Vec<Vec3>,
    uv: Vec<Vec2>,
    idx: Vec<[u32; 3]>,
    tri_interior: Vec<bool>,
}

impl Soup {
    fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    fn vtx(&self, i: u32) -> Vtx {
        let i = i as usize;
        Vtx { pos: self.pos[i], nrm: self.nrm[i], uv: self.uv[i] }
    }

    fn push_tri(&mut self, a: Vtx, b: Vtx, c: Vtx, interior: bool) {
        let base = self.pos.len() as u32;
        for v in [a, b, c] {
            self.pos.push(v.pos);
            self.nrm.push(v.nrm);
            self.uv.push(v.uv);
        }
        self.idx.push([base, base + 1, base + 2]);
        self.tri_interior.push(interior);
    }

    /// Axis-aligned bounds over all vertices (min, max). `(ZERO, ZERO)` when empty.
    fn bbox(&self) -> (Vec3, Vec3) {
        let mut mn = Vec3::splat(f32::INFINITY);
        let mut mx = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.pos {
            mn = mn.min(*p);
            mx = mx.max(*p);
        }
        if self.pos.is_empty() {
            (Vec3::ZERO, Vec3::ZERO)
        } else {
            (mn, mx)
        }
    }

    /// Vertex-average center. `ZERO` when empty.
    fn centroid(&self) -> Vec3 {
        if self.pos.is_empty() {
            return Vec3::ZERO;
        }
        self.pos.iter().copied().sum::<Vec3>() / self.pos.len() as f32
    }

    /// Largest bounding half-dimension — the "how big is this piece" measure driving fragment sizing.
    fn extent(&self) -> f32 {
        let (mn, mx) = self.bbox();
        ((mx - mn) * 0.5).max_element()
    }
}

/// Signed distance from `p` to the plane (positive on the `+normal` side).
fn signed_dist(p: Vec3, plane: &Plane) -> f32 {
    (p - plane.point).dot(plane.normal)
}

/// `+1` above / `-1` below / `0` on the plane (within `EPS`).
fn classify(s: f32) -> i32 {
    if s > EPS {
        1
    } else if s < -EPS {
        -1
    } else {
        0
    }
}

/// Vertex interpolated where segment `a→b` crosses the plane at parameter `t`.
fn lerp_vtx(a: Vtx, b: Vtx, t: f32) -> Vtx {
    Vtx {
        pos: a.pos.lerp(b.pos, t),
        nrm: a.nrm.lerp(b.nrm, t).normalize_or_zero(),
        uv: a.uv.lerp(b.uv, t),
    }
}

/// Clip one triangle to the half-space we keep (Sutherland–Hodgman on the 3-gon), fan-triangulate
/// the kept polygon, and append it to `out`. On-plane vertices (`classify == 0`) are kept for *both*
/// half-spaces so the seam geometry is shared. Original `interior` tag is inherited.
fn clip_half(v: [Vtx; 3], s: [f32; 3], keep_above: bool, interior: bool, out: &mut Soup) {
    let mut poly: Vec<Vtx> = Vec::with_capacity(4);
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (ci, cj) = (classify(s[i]), classify(s[j]));
        let keep_i = if keep_above { ci >= 0 } else { ci <= 0 };
        if keep_i {
            poly.push(v[i]);
        }
        // Strict crossing (opposite strict sides) → insert the intersection vertex.
        if ci != 0 && cj != 0 && ci != cj {
            let t = s[i] / (s[i] - s[j]);
            poly.push(lerp_vtx(v[i], v[j], t));
        }
    }
    if poly.len() >= 3 {
        for i in 1..poly.len() - 1 {
            out.push_tri(poly[0], poly[i], poly[i + 1], interior);
        }
    }
}

/// The single cut segment a straddling triangle contributes to the plane (its entry/exit points).
/// `None` when the triangle only touches the plane at a point (no real cut).
fn cut_segment(v: &[Vtx; 3], s: &[f32; 3]) -> Option<[Vec3; 2]> {
    let mut pts: Vec<Vec3> = Vec::new();
    let mut add = |p: Vec3| {
        if !pts.iter().any(|q| q.distance_squared(p) < 1.0e-10) {
            pts.push(p);
        }
    };
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (ci, cj) = (classify(s[i]), classify(s[j]));
        if ci == 0 {
            add(v[i].pos);
        }
        if ci != 0 && cj != 0 && ci != cj {
            let t = s[i] / (s[i] - s[j]);
            add(v[i].pos.lerp(v[j].pos, t));
        }
    }
    if pts.len() == 2 {
        Some([pts[0], pts[1]])
    } else {
        None
    }
}

/// Two orthonormal in-plane axes for a given plane normal (for cross-section UVs).
fn plane_basis(n: Vec3) -> (Vec3, Vec3) {
    let a = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = n.cross(a).normalize_or_zero();
    let v = n.cross(u);
    (u, v)
}

/// Weld a point to a canonical vertex id on the quantized [`WELD`] lattice (robust loop assembly on
/// non-watertight input).
fn weld(verts: &mut Vec<Vec3>, table: &mut HashMap<(i64, i64, i64), u32>, p: Vec3) -> u32 {
    let q = |x: f32| (x / WELD).round() as i64;
    let key = (q(p.x), q(p.y), q(p.z));
    if let Some(&id) = table.get(&key) {
        return id;
    }
    let id = verts.len() as u32;
    verts.push(p);
    table.insert(key, id);
    id
}

/// Chain undirected boundary edges into closed loops. Handles multiple disjoint loops (e.g. a plane
/// through two legs). Open chains (non-watertight input) are `warn!`-dropped, never emitted.
fn assemble_loops(edges: &[(u32, u32)]) -> Vec<Vec<u32>> {
    let mut adj: HashMap<u32, Vec<usize>> = HashMap::new();
    for (ei, &(a, b)) in edges.iter().enumerate() {
        adj.entry(a).or_default().push(ei);
        adj.entry(b).or_default().push(ei);
    }
    let mut used = vec![false; edges.len()];
    let mut loops: Vec<Vec<u32>> = Vec::new();

    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (s0, s1) = edges[start];
        let mut loop_v = vec![s0, s1];
        let (mut prev, mut cur) = (s0, s1);
        let mut closed = false;

        for _ in 0..=edges.len() {
            if cur == s0 {
                closed = true;
                break;
            }
            // Prefer an unused edge that doesn't immediately backtrack; fall back to any unused edge.
            let pick = |avoid_prev: bool| -> Option<(usize, u32)> {
                let eis = adj.get(&cur)?;
                for &ei in eis {
                    if used[ei] {
                        continue;
                    }
                    let (a, b) = edges[ei];
                    let other = if a == cur {
                        b
                    } else if b == cur {
                        a
                    } else {
                        continue;
                    };
                    if avoid_prev && other == prev {
                        continue;
                    }
                    return Some((ei, other));
                }
                None
            };
            match pick(true).or_else(|| pick(false)) {
                Some((ei, other)) => {
                    used[ei] = true;
                    loop_v.push(other);
                    prev = cur;
                    cur = other;
                }
                None => break,
            }
        }

        if closed {
            loops.push(loop_v);
        } else {
            warn!("autogib: dropping unclosed cut boundary ({} verts)", loop_v.len());
        }
    }
    loops
}

/// Fan-triangulate one cap loop around its centroid, giving every cap triangle the `outward` normal
/// (winding fixed to match) and a planar cross-section UV. Tags triangles `interior = true`.
#[allow(clippy::too_many_arguments)]
fn push_cap_tri(out: &mut Soup, c: Vec3, p1: Vec3, p2: Vec3, outward: Vec3, bu: Vec3, bv: Vec3, origin: Vec3) {
    let face = (p1 - c).cross(p2 - c);
    if face.length_squared() < 1.0e-12 {
        return; // skip degenerate fan slice
    }
    let (a, b, d) = if face.dot(outward) >= 0.0 { (c, p1, p2) } else { (c, p2, p1) };
    let uv = |p: Vec3| Vec2::new((p - origin).dot(bu), (p - origin).dot(bv));
    out.push_tri(
        Vtx { pos: a, nrm: outward, uv: uv(a) },
        Vtx { pos: b, nrm: outward, uv: uv(b) },
        Vtx { pos: d, nrm: outward, uv: uv(d) },
        true,
    );
}

/// Close one side of a cut: weld the recorded segments, assemble boundary loops, fan-cap each with
/// the given `outward` normal. Needs at least a triangle's worth of segments.
fn cap_side(segs: &[[Vec3; 2]], plane: &Plane, outward: Vec3, out: &mut Soup) {
    if segs.len() < 3 {
        return;
    }
    let mut verts: Vec<Vec3> = Vec::new();
    let mut table: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for seg in segs {
        let ia = weld(&mut verts, &mut table, seg[0]);
        let ib = weld(&mut verts, &mut table, seg[1]);
        if ia != ib {
            edges.push((ia, ib));
        }
    }
    let (bu, bv) = plane_basis(plane.normal);
    for lp in assemble_loops(&edges) {
        if lp.len() < 3 {
            continue;
        }
        let c: Vec3 = lp.iter().map(|&i| verts[i as usize]).sum::<Vec3>() / lp.len() as f32;
        let n = lp.len();
        for k in 0..n {
            let p1 = verts[lp[k] as usize];
            let p2 = verts[lp[(k + 1) % n] as usize];
            push_cap_tri(out, c, p1, p2, outward, bu, bv, plane.point);
        }
    }
}

/// Split a soup into (above, below) halves by a plane, capping each cut watertight. The cap normals
/// face *out* of each piece: the above piece's cap faces `-normal`, the below piece's faces `+normal`.
fn split_soup(src: &Soup, plane: &Plane) -> (Soup, Soup) {
    let mut above = Soup::default();
    let mut below = Soup::default();
    let mut segs: Vec<[Vec3; 2]> = Vec::new();

    for (t, tri) in src.idx.iter().enumerate() {
        let interior = src.tri_interior[t];
        let v = [src.vtx(tri[0]), src.vtx(tri[1]), src.vtx(tri[2])];
        let s = [
            signed_dist(v[0].pos, plane),
            signed_dist(v[1].pos, plane),
            signed_dist(v[2].pos, plane),
        ];
        clip_half(v, s, true, interior, &mut above);
        clip_half(v, s, false, interior, &mut below);
        if let Some(seg) = cut_segment(&v, &s) {
            segs.push(seg);
        }
    }
    cap_side(&segs, plane, -plane.normal, &mut above);
    cap_side(&segs, plane, plane.normal, &mut below);
    (above, below)
}

/// Random unit vector on the sphere from a hash seed (always exactly unit length — never zero).
fn random_dir(seed: u32) -> Vec3 {
    let h1 = hash_f32(seed.wrapping_add(0x1234_5678));
    let h2 = hash_f32(seed.wrapping_add(0x9E37_79B9));
    let z = 2.0 * h1 - 1.0;
    let r = (1.0 - z * z).max(0.0).sqrt();
    let phi = h2 * TAU;
    Vec3::new(r * phi.cos(), z, r * phi.sin())
}

/// Fracture a soup into up to `target` fragments by repeatedly splitting the current largest piece
/// with a plane through its centroid. `min_extent` stops a piece from being cut below that size.
/// `seed` drives every plane direction deterministically. `impact_dir`, when set, biases the first
/// couple of cuts toward the impact (reserved seam for impact-located fracture, cf. Müller 2013).
fn fracture(src: Soup, target: usize, min_extent: f32, seed: u32, impact_dir: Option<Vec3>) -> Vec<Soup> {
    let mut pieces: Vec<Soup> = vec![src];
    let mut unsplittable: Vec<bool> = vec![false];
    let mut cut_index: u32 = 0;
    let mut iters = 0usize;
    let hard_cap = target.saturating_mul(16).saturating_add(32);

    while pieces.len() < target.max(1) {
        iters += 1;
        if iters > hard_cap {
            break;
        }
        // Largest splittable piece by extent.
        let pick = pieces
            .iter()
            .enumerate()
            .filter(|(i, _)| !unsplittable[*i])
            .max_by(|a, b| a.1.extent().partial_cmp(&b.1.extent()).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i);
        let Some(i) = pick else {
            break; // nothing left worth cutting
        };
        if pieces[i].extent() < min_extent {
            unsplittable[i] = true;
            continue;
        }

        let s = seed
            .wrapping_add(cut_index.wrapping_mul(2_654_435_761))
            .wrapping_add(pieces.len() as u32);
        let base_dir = random_dir(s);
        let normal = match impact_dir {
            Some(d) if cut_index < 2 => {
                let blended = base_dir * 0.5 + d.normalize_or_zero() * 0.5;
                if blended.length_squared() > 1.0e-6 {
                    blended.normalize()
                } else {
                    base_dir
                }
            }
            _ => base_dir,
        };
        let plane = Plane { point: pieces[i].centroid(), normal };

        let piece = std::mem::take(&mut pieces[i]);
        let (a, b) = split_soup(&piece, &plane);
        cut_index = cut_index.wrapping_add(1);
        if a.is_empty() || b.is_empty() {
            pieces[i] = piece; // put it back; this plane didn't separate it
            unsplittable[i] = true;
            continue;
        }
        pieces[i] = a;
        pieces.push(b);
        unsplittable.push(false);
    }
    pieces
}

// ---------------------------------------------------------------------------------------------
// Bevy Mesh <-> Soup adapters (the only Bevy-typed geometry fns).
// ---------------------------------------------------------------------------------------------

/// Decode a mesh's index buffer into a triangle list, handling all encodings: `U16`, `U32`, and
/// non-indexed (consecutive triples). `vertex_count` drives only the non-indexed case, whose
/// triangles are `[0,1,2], [3,4,5], …` over the position array. Callers bounds-check the returned
/// indices against their own vertex data before dereferencing.
fn triangle_indices(mesh: &Mesh, vertex_count: usize) -> Vec<[u32; 3]> {
    let mut tris: Vec<[u32; 3]> = Vec::new();
    match mesh.indices() {
        Some(Indices::U16(v)) => {
            for c in v.chunks_exact(3) {
                tris.push([c[0] as u32, c[1] as u32, c[2] as u32]);
            }
        }
        Some(Indices::U32(v)) => {
            for c in v.chunks_exact(3) {
                tris.push([c[0], c[1], c[2]]);
            }
        }
        None => {
            let n = vertex_count as u32;
            let mut i = 0;
            while i + 3 <= n {
                tris.push([i, i + 1, i + 2]);
                i += 3;
            }
        }
    }
    tris
}

/// Append one loaded mesh's triangles into `soup`, transformed by `xform` (the sub-mesh's transform
/// relative to the character root). Robust to arbitrary layouts: missing `NORMAL` → synthesized flat
/// normals; missing `UV_0` → zero-filled; `U16`/`U32`/non-indexed all handled. Returns `false`
/// (+`warn!`) if the mesh has no `Float32x3` positions or isn't a triangle list.
/// Signed-tetrahedron volume of a triangle mesh (divergence theorem): `Σ a·(b×c) / 6` over every
/// triangle, returned as a magnitude. Used to weigh gib chunks (`weight = density × volume`, see
/// `gore`). `None` if the mesh lacks `Float32x3` positions or isn't a triangle list (one path, no
/// fallback). Assumes a closed, consistently-wound surface — the meat-chunk meshes are.
pub(crate) fn mesh_signed_volume(mesh: &Mesh) -> Option<f32> {
    let VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)? else {
        return None;
    };
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let p: Vec<Vec3> = positions.iter().map(|v| Vec3::from_array(*v)).collect();
    let tris = triangle_indices(mesh, p.len());
    let mut vol = 0.0f32;
    for t in &tris {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        if a >= p.len() || b >= p.len() || c >= p.len() {
            continue;
        }
        vol += p[a].dot(p[b].cross(p[c]));
    }
    Some((vol / 6.0).abs())
}

fn append_mesh(soup: &mut Soup, mesh: &Mesh, xform: Mat4, interior: bool) -> bool {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        warn!("autogib: sub-mesh has no Float32x3 POSITION; skipping it");
        return false;
    };
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        warn!("autogib: sub-mesh is not a TriangleList; skipping it");
        return false;
    }

    // Transform positions into character-local space.
    let tp: Vec<Vec3> = positions.iter().map(|p| xform.transform_point3(Vec3::from_array(*p))).collect();

    // Normals: transform by the inverse-transpose (upper 3x3), or synthesize per-face if absent.
    let normal_mat = Mat3::from_mat4(xform).inverse().transpose();
    let have_normals = matches!(
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
        Some(VertexAttributeValues::Float32x3(n)) if n.len() == positions.len()
    );
    let mut tn: Vec<Vec3> = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(n)) if have_normals => {
            n.iter().map(|v| (normal_mat * Vec3::from_array(*v)).normalize_or_zero()).collect()
        }
        _ => vec![Vec3::ZERO; tp.len()],
    };

    // UVs: keep source or zero-fill.
    let tuv: Vec<Vec2> = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(u)) if u.len() == positions.len() => {
            u.iter().map(|v| Vec2::from_array(*v)).collect()
        }
        _ => vec![Vec2::ZERO; tp.len()],
    };

    // Collect the triangle index list (handling all index encodings).
    let tris = triangle_indices(mesh, tp.len());

    if !have_normals {
        // Area-weighted face normals accumulated onto shared vertices, then renormalized.
        for t in &tris {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            if a >= tp.len() || b >= tp.len() || c >= tp.len() {
                continue;
            }
            let fnrm = (tp[b] - tp[a]).cross(tp[c] - tp[a]);
            tn[a] += fnrm;
            tn[b] += fnrm;
            tn[c] += fnrm;
        }
        for n in &mut tn {
            *n = n.normalize_or_zero();
        }
    }

    let vbase = soup.pos.len() as u32;
    soup.pos.extend_from_slice(&tp);
    soup.nrm.extend_from_slice(&tn);
    soup.uv.extend_from_slice(&tuv);
    for t in &tris {
        // Guard against out-of-range indices from a malformed mesh.
        if (t[0] as usize) < tp.len() && (t[1] as usize) < tp.len() && (t[2] as usize) < tp.len() {
            soup.idx.push([t[0] + vbase, t[1] + vbase, t[2] + vbase]);
            soup.tri_interior.push(interior);
        }
    }
    true
}

/// Build a `Mesh` from the subset of `soup` triangles whose interior flag matches `want_interior`,
/// re-indexed to a compact vertex set and recentered so the origin sits at `recenter` (the fragment
/// centroid → the spawned entity spins about its own center). `None` if the subset is empty.
fn soup_to_mesh(soup: &Soup, want_interior: bool, recenter: Vec3) -> Option<Mesh> {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut nrm: Vec<[f32; 3]> = Vec::new();
    let mut uv: Vec<[f32; 2]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut remap: HashMap<u32, u32> = HashMap::new();

    for (t, tri) in soup.idx.iter().enumerate() {
        if soup.tri_interior[t] != want_interior {
            continue;
        }
        let (pa, pb, pc) = (
            soup.pos[tri[0] as usize],
            soup.pos[tri[1] as usize],
            soup.pos[tri[2] as usize],
        );
        if (pb - pa).cross(pc - pa).length_squared() < 1.0e-12 {
            continue; // drop zero-area triangles
        }
        for &old in tri {
            let nid = if let Some(&n) = remap.get(&old) {
                n
            } else {
                let nid = pos.len() as u32;
                let p = soup.pos[old as usize] - recenter;
                pos.push([p.x, p.y, p.z]);
                let n = soup.nrm[old as usize];
                nrm.push([n.x, n.y, n.z]);
                let u = soup.uv[old as usize];
                uv.push([u.x, u.y]);
                remap.insert(old, nid);
                nid
            };
            idx.push(nid);
        }
    }
    if idx.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(idx));
    Some(mesh)
}

// ---------------------------------------------------------------------------------------------
// Bake-once cache + system.
// ---------------------------------------------------------------------------------------------

/// One baked body fragment, in character-**local** units (render scale is applied at spawn). Both
/// meshes are recentered to `center_local` (their shared bounding-box center), so a physics body
/// placed at `origin + center_local*scale` with a `half_extents*scale` box collider lines up exactly
/// with the rendered chunk. Either mesh may be `None` (a fragment with no cut faces has no cap; a
/// pure-cap sliver has no outer skin).
pub struct Fragment {
    pub outer_mesh: Option<Handle<Mesh>>,
    pub cap_mesh: Option<Handle<Mesh>>,
    pub center_local: Vec3,
    /// Half the bounding box per axis (local units) → sizes the chunk's box collider.
    pub half_extents: Vec3,
}

/// The carried weapon, flung intact as a single tumbling chunk (kept its own GLTF material). Baked in
/// the same character-local space as body fragments.
pub struct GunChunk {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub center_local: Vec3,
    pub half_extents: Vec3,
}

/// Baked fracture data, keyed by the character's source scene asset id so multiple distinct
/// characters (this greybox figurine now, a richer/animated mesh later) each get their own bake and
/// swapping the GLB needs zero code change.
#[derive(Resource, Default)]
pub struct AutogibCache {
    body: HashMap<AssetId<WorldAsset>, Vec<Fragment>>,
    guns: HashMap<AssetId<WorldAsset>, GunChunk>,
    baked: HashSet<AssetId<WorldAsset>>,
}

impl AutogibCache {
    /// Baked body fragments for a source, or `None` if that source hasn't been baked.
    pub fn fragments(&self, source: AssetId<WorldAsset>) -> Option<&[Fragment]> {
        self.body.get(&source).map(|v| v.as_slice())
    }

    /// Baked gun chunk for a source, if any.
    pub fn gun(&self, source: AssetId<WorldAsset>) -> Option<&GunChunk> {
        self.guns.get(&source)
    }
}

/// Turn a fragment soup into cached meshes recentered to its bounding-box center (so a box collider
/// centered on the spawned body matches the geometry). `None` if it has no drawable triangles.
fn build_fragment(soup: &Soup, meshes: &mut Assets<Mesh>) -> Option<Fragment> {
    if soup.is_empty() {
        return None;
    }
    let (mn, mx) = soup.bbox();
    let center = (mn + mx) * 0.5;
    let half_extents = ((mx - mn) * 0.5).max(Vec3::splat(0.01));
    let outer = soup_to_mesh(soup, false, center).map(|m| meshes.add(m));
    let cap = soup_to_mesh(soup, true, center).map(|m| meshes.add(m));
    if outer.is_none() && cap.is_none() {
        return None;
    }
    Some(Fragment { outer_mesh: outer, cap_mesh: cap, center_local: center, half_extents })
}

/// Bake the carried weapon into a single intact chunk (no fracture), keeping its own material.
/// `None` if the gun soup is empty or had no material.
fn bake_gun(gun: &Soup, material: Option<Handle<StandardMaterial>>, meshes: &mut Assets<Mesh>) -> Option<GunChunk> {
    let frag = build_fragment(gun, meshes)?;
    let material = material?;
    let mesh = frag.outer_mesh.or(frag.cap_mesh)?;
    Some(GunChunk { mesh, material, center_local: frag.center_local, half_extents: frag.half_extents })
}

/// Derive a stable per-source fracture seed (deterministic within a run).
fn seed_from(id: AssetId<WorldAsset>) -> u32 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut h);
    h.finish() as u32
}

/// Once a unit's whole body scene has streamed in, bake its fracture set (and its gun chunk) exactly
/// once per source. Mirrors `recolor_units`' Children DFS: walk the unit's descendants, prune the
/// `GunModel` subtree into a separate gun chunk, merge the rest into one soup in character-local
/// space, and fracture. Self-gates on all sub-meshes being present in `Assets<Mesh>`; combat can't
/// start before scenes load, so the bake is a completed prerequisite of any death.
#[allow(clippy::too_many_arguments)]
fn bake_autogib(
    mut cache: ResMut<AutogibCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    settings: Res<GoreSettings>,
    units: Query<(&WorldAssetRoot, &Children), With<Unit>>,
    children_q: Query<&Children>,
    transforms: Query<&Transform>,
    mesh_q: Query<&Mesh3d>,
    mat_q: Query<&MeshMaterial3d<StandardMaterial>>,
    is_gun: Query<(), With<GunModel>>,
) {
    for (root, children) in &units {
        let source = root.0.id();
        if cache.baked.contains(&source) {
            continue;
        }

        let mut body = Soup::default();
        let mut gun = Soup::default();
        let mut gun_material: Option<Handle<StandardMaterial>> = None;
        let mut all_loaded = true;

        // DFS stack of (entity, transform-relative-to-unit-root, inside-gun-subtree).
        let mut stack: Vec<(Entity, Mat4, bool)> = Vec::new();
        for child in children.iter() {
            let m = transforms.get(child).map(|t| t.to_matrix()).unwrap_or(Mat4::IDENTITY);
            stack.push((child, m, is_gun.get(child).is_ok()));
        }
        while let Some((e, mat, in_gun)) = stack.pop() {
            if let Ok(mesh3d) = mesh_q.get(e) {
                match meshes.get(&mesh3d.0) {
                    Some(m) => {
                        if in_gun {
                            append_mesh(&mut gun, m, mat, false);
                            if gun_material.is_none() {
                                gun_material = mat_q.get(e).ok().map(|mm| mm.0.clone());
                            }
                        } else {
                            append_mesh(&mut body, m, mat, false);
                        }
                    }
                    None => all_loaded = false, // sub-mesh still streaming
                }
            }
            if let Ok(ch) = children_q.get(e) {
                for child in ch.iter() {
                    let ct = transforms.get(child).map(|t| t.to_matrix()).unwrap_or(Mat4::IDENTITY);
                    let child_gun = in_gun || is_gun.get(child).is_ok();
                    stack.push((child, mat * ct, child_gun));
                }
            }
        }

        // Wait until the async scene has actually instantiated its body meshes AND they're loaded
        // into `Assets<Mesh>`. Before the GLTF scene spawns its descendants there are simply no body
        // `Mesh3d` entities to find, so an empty body here means "still streaming", not "no geometry"
        // — retry next frame (same as `recolor_units`), rather than caching an empty fracture set.
        if !all_loaded || body.is_empty() {
            continue;
        }

        let ext = body.extent();
        if ext <= 1.0e-5 {
            warn!("autogib: source body is degenerate (zero extent); marking baked with no fragments");
            cache.body.insert(source, Vec::new());
            cache.baked.insert(source);
            continue;
        }

        // Bounding-box-driven sizing: bigger/denser meshes yield more, appropriately-sized pieces.
        let ref_ext = settings.autogib_ref_extent.max(1.0e-4);
        let raw = (settings.autogib_pieces_base as f32 * (ext / ref_ext)).round() as i32;
        let target = raw.clamp(settings.autogib_min_pieces, settings.autogib_max_pieces).max(1) as usize;
        let min_extent = ext * settings.autogib_min_fraction;

        let soups = fracture(body, target, min_extent, seed_from(source), None);
        let frags: Vec<Fragment> = soups.iter().filter_map(|s| build_fragment(s, &mut meshes)).collect();
        info!("autogib: baked {} fragments for a character source", frags.len());
        cache.body.insert(source, frags);

        // Gun chunk (single intact piece, keeps its own material).
        if let Some(chunk) = bake_gun(&gun, gun_material, &mut meshes) {
            cache.guns.insert(source, chunk);
        }

        cache.baked.insert(source);
    }
}

/// Registers the autogib fracture cache and its one-shot bake system.
pub struct AutogibPlugin;

impl Plugin for AutogibPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AutogibCache>().add_systems(Update, bake_autogib);
    }
}

// ---------------------------------------------------------------------------------------------
// Tests — pure geometry, no App required.
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_soup() -> Soup {
        let mut s = Soup::default();
        assert!(append_mesh(&mut s, &Mesh::from(Cuboid::new(1.0, 1.0, 1.0)), Mat4::IDENTITY, false));
        s
    }

    fn all_finite(s: &Soup) -> bool {
        s.pos.iter().all(|p| p.is_finite()) && s.nrm.iter().all(|n| n.is_finite()) && s.uv.iter().all(|u| u.is_finite())
    }

    fn interior_area(s: &Soup) -> f32 {
        s.idx
            .iter()
            .enumerate()
            .filter(|(t, _)| s.tri_interior[*t])
            .map(|(_, tri)| {
                let (a, b, c) = (s.pos[tri[0] as usize], s.pos[tri[1] as usize], s.pos[tri[2] as usize]);
                0.5 * (b - a).cross(c - a).length()
            })
            .sum()
    }

    #[test]
    fn slice_cube_axis_plane() {
        let s = cube_soup();
        let (above, below) = split_soup(&s, &Plane { point: Vec3::ZERO, normal: Vec3::X });
        assert!(!above.is_empty() && !below.is_empty());
        assert!(above.pos.iter().all(|p| p.x >= -1.0e-3), "above stays on +X side");
        assert!(below.pos.iter().all(|p| p.x <= 1.0e-3), "below stays on -X side");
        assert!(above.tri_interior.iter().any(|&i| i), "above has a cap");
        assert!(below.tri_interior.iter().any(|&i| i), "below has a cap");
        assert!(all_finite(&above) && all_finite(&below));
    }

    #[test]
    fn cap_is_unit_square_area() {
        let s = cube_soup();
        let (above, _) = split_soup(&s, &Plane { point: Vec3::ZERO, normal: Vec3::Y });
        // A mid-slice of the unit cube leaves a 1x1 cross-section.
        assert!((interior_area(&above) - 1.0).abs() < 0.05, "cap area ~1.0, got {}", interior_area(&above));
    }

    #[test]
    fn fracture_reaches_target_and_is_deterministic() {
        let a = fracture(cube_soup(), 8, 0.05, 0xABCD_1234, None);
        let b = fracture(cube_soup(), 8, 0.05, 0xABCD_1234, None);
        assert_eq!(a.len(), b.len());
        assert!(a.len() >= 2 && a.len() <= 8, "reached a sane fragment count: {}", a.len());
        assert!(a.iter().all(|s| !s.is_empty()));
        assert!(a[0].centroid().distance(b[0].centroid()) < 1.0e-6, "deterministic per seed");
    }

    #[test]
    fn missing_uv_is_zero_filled() {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3]);
        m.insert_indices(Indices::U32(vec![0, 1, 2]));
        let mut s = Soup::default();
        assert!(append_mesh(&mut s, &m, Mat4::IDENTITY, false));
        assert_eq!(s.uv.len(), s.pos.len());
        assert!(s.uv.iter().all(|u| *u == Vec2::ZERO));
    }

    #[test]
    fn missing_normals_are_synthesized() {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        m.insert_indices(Indices::U32(vec![0, 1, 2]));
        let mut s = Soup::default();
        assert!(append_mesh(&mut s, &m, Mat4::IDENTITY, false));
        // Flat triangle in the XY plane → +Z normals.
        assert!(s.nrm.iter().all(|n| n.z.abs() > 0.99));
    }

    #[test]
    fn open_boundary_is_dropped() {
        // Path a-b-c-d (open, never returns to a) → no cap emitted, no panic.
        let (a, b, c, d) = (Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        let segs = vec![[a, b], [b, c], [c, d]];
        let mut out = Soup::default();
        cap_side(&segs, &Plane { point: Vec3::ZERO, normal: Vec3::Z }, Vec3::Z, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn degenerate_plane_leaves_piece_whole() {
        let s = cube_soup();
        // Plane far outside the cube (all vertices on one side).
        let (above, below) = split_soup(&s, &Plane { point: Vec3::splat(5.0), normal: Vec3::X });
        assert!(above.is_empty(), "nothing above a plane past the cube");
        assert!(!below.is_empty());
        // And the fracture driver must not spin on such a piece.
        let out = fracture(cube_soup(), 4, 0.6, 42, None);
        assert!(!out.is_empty());
    }
}
