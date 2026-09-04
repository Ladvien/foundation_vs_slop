//! **How far below the skin a point on a cut face is**, and the vertex attribute that carries it.
//!
//! A cut face belongs to a convex cell whose *supplied* faces are the subject's own surface — the
//! skin. For a point inside a convex polytope the distance to its boundary is exactly the minimum
//! over its faces of the signed distance to that face's plane, so depth-below-skin is a handful of
//! dot products and needs no mesh query at all. That is the whole reason this crate can run at bake
//! time on the CPU and hash its output.
//!
//! The depth lands in `ATTRIBUTE_UV_1` as `(depth / span, along / tile)`: a material that samples the
//! region's strip texture through `UvChannel::Uv1` then paints the bands, and `UV_0` — the planar
//! cross-section coordinates every cap already carries — is untouched, so nothing that hashed a cap
//! before this crate existed moves.

use bevy::math::{Vec2, Vec3};
use bevy::mesh::{Mesh, VertexAttributeValues};

use crate::layers::Layers;

/// A skin plane: a point on the surface and the **outward** unit normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinPlane {
    /// Any point on the plane.
    pub point: Vec3,
    /// Outward unit normal.
    pub normal: Vec3,
}

/// **Depth of `p` below the nearest skin plane**, in the planes' own units. Negative outside.
///
/// Exact for a point inside a convex cell whose supplied faces are `planes`; no planes at all means
/// no skin to measure from, and the answer is zero rather than infinity so a cell with no supplied
/// face (an interior fragment) reads as skin rather than as marrow.
pub fn depth_below_skin(p: Vec3, planes: &[SkinPlane]) -> f32 {
    let mut best = f32::INFINITY;
    for pl in planes {
        let d = (pl.point - p).dot(pl.normal);
        if d < best {
            best = d;
        }
    }
    if best.is_finite() { best } else { 0.0 }
}

/// How the depth attribute is scaled into the strip's UV space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// Millimetres per mesh unit. `1000.0` for a mesh authored in metres.
    pub mm_per_unit: f32,
    /// Mesh units one repeat of the strip's along-axis covers. `0.05` (5 cm) at metres.
    pub tile_units: f32,
}

impl Default for Scale {
    fn default() -> Self {
        Self { mm_per_unit: 1000.0, tile_units: 0.05 }
    }
}

/// **Write `ATTRIBUTE_UV_1` on a cap mesh** from its positions and the cell's skin planes.
///
/// `offset` is added to every position before measuring — a cap mesh recentred on its fragment's
/// own origin hands in that centre here, so the planes and the positions share a frame. Along the
/// strip the coordinate is `UV_0.x` when the mesh has it (the planar cross-section coordinate, which
/// is continuous over the face) and otherwise the position's own `x`.
///
/// Returns the number of vertices written; zero, and nothing inserted, when the mesh has no
/// `Float32x3` positions.
pub fn annotate_cap(mesh: &mut Mesh, planes: &[SkinPlane], offset: Vec3, layers: &Layers, scale: &Scale) -> usize {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        return 0;
    };
    let along: Vec<f32> = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(uv)) if uv.len() == pos.len() => uv.iter().map(|u| u[0]).collect(),
        _ => pos.iter().map(|p| p[0]).collect(),
    };
    let span = layers.span_mm().max(1.0e-3);
    let tile = if scale.tile_units.is_finite() && scale.tile_units > 0.0 { scale.tile_units } else { 1.0 };
    let uv1: Vec<[f32; 2]> = pos
        .iter()
        .zip(along.iter())
        .map(|(p, a)| {
            let world = Vec3::from_array(*p) + offset;
            let depth_mm = depth_below_skin(world, planes).max(0.0) * scale.mm_per_unit;
            [(depth_mm / span).clamp(0.0, 1.0), a / tile]
        })
        .collect();
    let n = uv1.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
    n
}

/// The `UV_1` a single point would receive — the per-vertex rule of [`annotate_cap`], exposed so a
/// caller building its own geometry (a wound bed, a flayed patch) can parameterise it the same way.
pub fn uv1_at(p: Vec3, along: f32, planes: &[SkinPlane], layers: &Layers, scale: &Scale) -> Vec2 {
    let span = layers.span_mm().max(1.0e-3);
    let tile = if scale.tile_units.is_finite() && scale.tile_units > 0.0 { scale.tile_units } else { 1.0 };
    let depth_mm = depth_below_skin(p, planes).max(0.0) * scale.mm_per_unit;
    Vec2::new((depth_mm / span).clamp(0.0, 1.0), along / tile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::Region;
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::PrimitiveTopology;

    fn slab() -> Vec<SkinPlane> {
        // A slab 0.1 units thick between y = ±0.05, skin on both faces.
        vec![
            SkinPlane { point: Vec3::new(0.0, 0.05, 0.0), normal: Vec3::Y },
            SkinPlane { point: Vec3::new(0.0, -0.05, 0.0), normal: -Vec3::Y },
        ]
    }

    #[test]
    fn depth_is_the_nearest_skin() {
        let s = slab();
        assert!((depth_below_skin(Vec3::ZERO, &s) - 0.05).abs() < 1.0e-6);
        assert!((depth_below_skin(Vec3::new(0.0, 0.04, 0.0), &s) - 0.01).abs() < 1.0e-6);
        assert!(depth_below_skin(Vec3::new(0.0, 0.06, 0.0), &s) < 0.0, "outside is negative");
        assert_eq!(depth_below_skin(Vec3::ONE, &[]), 0.0, "no skin, no depth");
    }

    #[test]
    fn annotate_writes_uv1_and_leaves_uv0_alone() {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.05, 0.0], [0.0, 0.0, 0.0], [0.02, -0.05, 0.0]],
        );
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0], [0.5, 0.0], [1.0, 0.0]]);
        let layers = Layers::for_region(Region::Limb);
        let n = annotate_cap(&mut m, &slab(), Vec3::ZERO, &layers, &Scale::default());
        assert_eq!(n, 3);
        let Some(VertexAttributeValues::Float32x2(uv1)) = m.attribute(Mesh::ATTRIBUTE_UV_1) else {
            panic!("UV_1 missing");
        };
        assert_eq!(uv1[0][0], 0.0, "a vertex on the skin is at depth zero");
        // 50 mm below the skin, over a 40.8 mm limb span: clamped to the end of the strip.
        assert_eq!(uv1[1][0], 1.0);
        assert_eq!(uv1[2][0], 0.0);
        // Along the strip is UV_0.x over the tile length.
        assert!((uv1[1][1] - 0.5 / 0.05).abs() < 1.0e-5);
        let Some(VertexAttributeValues::Float32x2(uv0)) = m.attribute(Mesh::ATTRIBUTE_UV_0) else {
            panic!("UV_0 lost");
        };
        assert_eq!(uv0[1], [0.5, 0.0]);
    }

    #[test]
    fn a_mesh_without_positions_is_refused() {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        assert_eq!(annotate_cap(&mut m, &slab(), Vec3::ZERO, &Layers::for_region(Region::Head), &Scale::default()), 0);
        assert!(m.attribute(Mesh::ATTRIBUTE_UV_1).is_none());
    }
}
