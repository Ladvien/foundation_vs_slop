//! **Measuring a mesh** — the numbers a descriptor records, taken from the file rather than by hand.
//!
//! `docs/2026-08-03-asset-schema-audit.md` §5 lists four fields nothing in this project validates:
//! `footprint` against the mesh, `scale`, `DoorPiece::opening`, and `front` — whose derivation method
//! is *written down* in `site::kit` and **implemented nowhere**, having been measured once by hand for
//! two chairs. This module is where those stop being hand-work.
//!
//! # Why the reader is hand-rolled
//!
//! `tests/common/mod.rs` states the reason and it survives the promotion into a library: *"pulling a
//! glTF crate in as a dev-dependency to read a header would be a second, differently-behaved reader of
//! an asset the engine already parses its own way."* This reads exactly the parts a measurement needs
//! and refuses anything else rather than misreading it.
//!
//! # The difference from the test copy
//!
//! The test reader panics with a good message, which is right for a test. This one returns `Result`
//! everywhere: it is library code, and an importer looking at a stranger's mesh must be able to say
//! "this file is not something I can measure" without taking the editor down with it.

use serde_json::Value;

/// A parsed binary glTF container: the JSON chunk and the BIN chunk.
pub struct Glb {
    pub json: Value,
    pub bin: Vec<u8>,
}

/// What a mesh measures, in its own file units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measured {
    /// Axis-aligned bounds over every `POSITION` accessor: (min, max).
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    /// (width, depth) on XZ — what a descriptor's `extent.footprint` wants.
    pub footprint: (f32, f32),
    /// Y extent — `extent.height`.
    pub height: f32,
    /// XZ offset of the bounding-box centre from the file's origin — `align.pivot`.
    pub pivot: (f32, f32),
    /// How far the base sits above (or below) y = 0. A descriptor's `align.y_offset` is the negation:
    /// what must be added to seat the base on the ground plane.
    pub base_y: f32,
    /// True when the largest extent is implausibly big for a metre-authored asset.
    ///
    /// The FBX importer represents centimetre authoring as a node `scale: 0.01` over 100× vertex data
    /// — valid glTF that renders correctly, but anything reading accessor bounds directly sees
    /// centimetres. Measured once on `SM_DoorFrame_Double`: bounds of 200.3 units for a 2.003 m door.
    pub suspect_centimetres: bool,
}

/// Where the file's origin sits relative to its geometry, in the vocabulary the asset library's own
/// per-pack READMEs already use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginAlignment {
    /// Base on y = 0 and centred on XZ — what this project's kit requires.
    BaseAtOriginCentred,
    /// Centred on all three axes.
    Centred,
    /// Neither.
    Offset,
}

/// Tolerance for "centred" / "on the ground", metres. `tests/ozea_asset.rs` asserts the shipped kit to
/// 5 mm and notes that is "comfortably tighter than the 15 mm the source packs actually drift by".
pub const ORIGIN_TOL: f32 = 0.005;

/// Below this asymmetry (metres), [`Glb::derive_front`] reports no front — saying a symmetric mesh has
/// one would be inventing a fact about the art.
///
/// **Measured, not guessed.** Against the shipped Ozea kit:
///
/// | mesh | asymmetry | kit records |
/// |---|---|---|
/// | `chair` | 166 mm | `Some(90.0)` |
/// | `command_chair` | 83 mm | `Some(90.0)` |
/// | `stool` | **12 mm** | `None` |
/// | `bench` | 1.9 mm | `None` |
/// | `mess_table` | 2.0 mm | `None` |
///
/// The gap between "has a back" and "does not" is wide — 12 mm to 83 mm — and 50 mm sits in the
/// middle of it.
///
/// Note the stool, because `site::kit`'s prose says a stool and a bench "measure symmetric to within a
/// centimetre" and the stool is 12 mm. That sentence is an approximation, not a spec; a threshold read
/// off it rejects the stool. Its asymmetry is a modelling detail, not a backrest.
///
/// A threshold over five samples is a suggestion, which is why [`Glb::front_detail`] exists and the
/// importer should show the number rather than only the verdict.
pub const FRONT_MIN_OFFSET: f32 = 0.05;

/// The slice of the mesh whose centroid is compared against the whole. From `site::kit`'s account of
/// how `front` was derived: the top of a chair is its back, and that is what breaks the symmetry.
pub const FRONT_UPPER_FRACTION: f32 = 0.45;

impl Glb {
    /// Parse a `.glb` from bytes.
    pub fn parse(bytes: &[u8]) -> Result<Glb, String> {
        if bytes.len() < 20 {
            return Err("glb: too short to be a container".to_owned());
        }
        if &bytes[0..4] != b"glTF" {
            return Err("glb: missing `glTF` magic — not a binary glTF".to_owned());
        }
        let json_len =
            u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        if &bytes[16..20] != b"JSON" {
            return Err("glb: first chunk is not JSON".to_owned());
        }
        let json_end = 20usize
            .checked_add(json_len)
            .ok_or_else(|| "glb: JSON chunk length overflows".to_owned())?;
        if json_end > bytes.len() {
            return Err("glb: JSON chunk runs past the end of the file".to_owned());
        }
        let json: Value = serde_json::from_slice(&bytes[20..json_end])
            .map_err(|e| format!("glb: JSON chunk does not parse: {e}"))?;

        // A container with no BIN chunk is legal glTF but has no vertex data, so nothing here can
        // measure it. Report that rather than reading zeros.
        let bin = if json_end + 8 <= bytes.len() && &bytes[json_end + 4..json_end + 8] == b"BIN\0" {
            let bin_len = u32::from_le_bytes([
                bytes[json_end],
                bytes[json_end + 1],
                bytes[json_end + 2],
                bytes[json_end + 3],
            ]) as usize;
            let start = json_end + 8;
            let end = start
                .checked_add(bin_len)
                .ok_or_else(|| "glb: BIN chunk length overflows".to_owned())?;
            if end > bytes.len() {
                return Err("glb: BIN chunk runs past the end of the file".to_owned());
            }
            bytes[start..end].to_vec()
        } else {
            Vec::new()
        };
        Ok(Glb { json, bin })
    }

    /// Read a `.glb` from disk.
    pub fn open(path: &std::path::Path) -> Result<Glb, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("glb: {}: {e}", path.display()))?;
        Glb::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// **Does any node carry a transform that makes accessor bounds a lie?**
    ///
    /// Raw accessor bounds equal world extents only when no node scales or matrices the geometry.
    /// `tests/prop_footprint_contract.rs` reports and skips such meshes rather than misreading them,
    /// on the grounds that "a silently mismeasured pass would be worse than no pass"; a measurement
    /// library must be able to say the same.
    pub fn has_node_transform(&self) -> bool {
        let Some(nodes) = self.json["nodes"].as_array() else {
            return false;
        };
        nodes.iter().any(|n| {
            if n.get("matrix").is_some() {
                return true;
            }
            n["scale"]
                .as_array()
                .is_some_and(|s| s.iter().any(|v| (v.as_f64().unwrap_or(1.0) - 1.0).abs() > 1e-3))
        })
    }

    /// Axis-aligned bounds over every mesh primitive's `POSITION` accessor, from the accessors' own
    /// declared `min`/`max` — no vertex decoding needed.
    pub fn bounds(&self) -> Result<([f32; 3], [f32; 3]), String> {
        let accessors = self.json["accessors"]
            .as_array()
            .ok_or_else(|| "glb: no accessors".to_owned())?;
        let meshes = self.json["meshes"]
            .as_array()
            .ok_or_else(|| "glb: no meshes".to_owned())?;

        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        let mut seen = false;
        for mesh in meshes {
            let Some(prims) = mesh["primitives"].as_array() else {
                continue;
            };
            for prim in prims {
                let Some(ix) = prim["attributes"]["POSITION"].as_u64() else {
                    continue;
                };
                let Some(acc) = accessors.get(ix as usize) else {
                    continue;
                };
                let (Some(mn), Some(mx)) = (acc["min"].as_array(), acc["max"].as_array()) else {
                    continue;
                };
                if mn.len() < 3 || mx.len() < 3 {
                    continue;
                }
                for c in 0..3 {
                    lo[c] = lo[c].min(mn[c].as_f64().unwrap_or(0.0) as f32);
                    hi[c] = hi[c].max(mx[c].as_f64().unwrap_or(0.0) as f32);
                }
                seen = true;
            }
        }
        if !seen {
            return Err("glb: no POSITION accessor declared min/max bounds".to_owned());
        }
        Ok((lo, hi))
    }

    /// Measure everything a descriptor's `extent` and `align` want.
    pub fn measure(&self) -> Result<Measured, String> {
        let (lo, hi) = self.bounds()?;
        let (w, h, d) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        Ok(Measured {
            lo,
            hi,
            footprint: (w, d),
            height: h,
            pivot: ((lo[0] + hi[0]) * 0.5, (lo[2] + hi[2]) * 0.5),
            base_y: lo[1],
            // 12 m is `fbx_to_glb.py`'s own "unusually large" threshold; a real prop at 100× lands
            // far past it.
            suspect_centimetres: w.max(h).max(d) > 12.0,
        })
    }

    /// Classify where the origin sits, in the asset library's own vocabulary.
    pub fn origin_alignment(&self) -> Result<OriginAlignment, String> {
        let m = self.measure()?;
        let centred_xz = m.pivot.0.abs() <= ORIGIN_TOL && m.pivot.1.abs() <= ORIGIN_TOL;
        let centred_y = ((m.lo[1] + m.hi[1]) * 0.5).abs() <= ORIGIN_TOL;
        Ok(match (centred_xz, m.base_y.abs() <= ORIGIN_TOL, centred_y) {
            (true, true, _) => OriginAlignment::BaseAtOriginCentred,
            (true, false, true) => OriginAlignment::Centred,
            _ => OriginAlignment::Offset,
        })
    }

    /// Decode a `VEC3`/`FLOAT` accessor. Anything else is refused rather than misread.
    pub fn read_vec3(&self, index: usize) -> Result<Vec<[f32; 3]>, String> {
        let acc = &self.json["accessors"][index];
        if acc["type"].as_str() != Some("VEC3") {
            return Err(format!("glb: accessor {index} is not VEC3"));
        }
        if acc["componentType"].as_u64() != Some(5126) {
            return Err(format!("glb: accessor {index} is not FLOAT"));
        }
        let count = acc["count"].as_u64().unwrap_or(0) as usize;
        let view_ix = acc["bufferView"]
            .as_u64()
            .ok_or_else(|| format!("glb: accessor {index} has no bufferView"))?
            as usize;
        let view = &self.json["bufferViews"][view_ix];
        let base = view["byteOffset"].as_u64().unwrap_or(0) as usize
            + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = view["byteStride"].as_u64().unwrap_or(12) as usize;

        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let mut v = [0.0f32; 3];
            for (c, slot) in v.iter_mut().enumerate() {
                let at = base + i * stride + c * 4;
                let raw: [u8; 4] = self
                    .bin
                    .get(at..at + 4)
                    .ok_or_else(|| format!("glb: accessor {index} reads past the BIN chunk"))?
                    .try_into()
                    .map_err(|_| format!("glb: accessor {index} short read"))?;
                *slot = f32::from_le_bytes(raw);
            }
            out.push(v);
        }
        Ok(out)
    }

    /// Every `POSITION` vertex across every primitive.
    pub fn positions(&self) -> Result<Vec<[f32; 3]>, String> {
        let meshes = self.json["meshes"]
            .as_array()
            .ok_or_else(|| "glb: no meshes".to_owned())?;
        let mut out = Vec::new();
        for mesh in meshes {
            let Some(prims) = mesh["primitives"].as_array() else {
                continue;
            };
            for prim in prims {
                if let Some(ix) = prim["attributes"]["POSITION"].as_u64() {
                    out.extend(self.read_vec3(ix as usize)?);
                }
            }
        }
        if out.is_empty() {
            return Err("glb: no vertex positions".to_owned());
        }
        Ok(out)
    }

    /// **Derive `align.front`** — the degrees to add to an authored yaw so the mesh faces the way the
    /// engine convention (`forward = (sin yaw, cos yaw)`) reports.
    ///
    /// The method is `site::kit`'s, implemented here for the first time: compare the XZ centroid of
    /// the upper [`FRONT_UPPER_FRACTION`] of the mesh against the centroid of the whole. The top of a
    /// seat is its back, so that offset points backwards; the front is the opposite way.
    ///
    /// `Ok(None)` means **the mesh is symmetric and has no front**, which is a different claim from
    /// `Some(0.0)` and the one the kit deliberately records for a stool: "asserting a facing on a
    /// stool would be asserting a fact about the art that is not true."
    pub fn derive_front(&self) -> Result<Option<f32>, String> {
        let (offset, yaw) = self.front_detail()?;
        Ok((offset >= FRONT_MIN_OFFSET).then_some(yaw))
    }

    /// The raw measurement behind [`derive_front`]: how far the upper slice's XZ centroid sits from
    /// the whole mesh's, in metres, and the yaw that offset implies.
    ///
    /// Exposed because the importer should show it rather than only its verdict. "This mesh is 1.2 cm
    /// asymmetric — is that a front, or is it noise?" is a judgement an author can make and a
    /// threshold cannot.
    pub fn front_detail(&self) -> Result<(f32, f32), String> {
        let verts = self.positions()?;
        let (lo, hi) = self.bounds()?;
        let cut = hi[1] - (hi[1] - lo[1]) * FRONT_UPPER_FRACTION;

        let mut all = (0.0f64, 0.0f64, 0usize);
        let mut top = (0.0f64, 0.0f64, 0usize);
        for v in &verts {
            all.0 += v[0] as f64;
            all.1 += v[2] as f64;
            all.2 += 1;
            if v[1] >= cut {
                top.0 += v[0] as f64;
                top.1 += v[2] as f64;
                top.2 += 1;
            }
        }
        if all.2 == 0 || top.2 == 0 {
            return Ok((0.0, 0.0));
        }
        let back = (
            (top.0 / top.2 as f64 - all.0 / all.2 as f64) as f32,
            (top.1 / top.2 as f64 - all.1 / all.2 as f64) as f32,
        );
        // Front is opposite the back. Solve `(sin y, cos y) == front` for y.
        let front = (-back.0, -back.1);
        let deg = front.0.atan2(front.1).to_degrees();
        Ok((
            back.0.hypot(back.1),
            if deg < 0.0 { deg + 360.0 } else { deg },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal container so the reader is exercised without shipping a fixture asset.
    fn synth(json: &str, bin: &[u8]) -> Vec<u8> {
        let mut j = json.as_bytes().to_vec();
        while j.len() % 4 != 0 {
            j.push(b' ');
        }
        let mut b = bin.to_vec();
        while b.len() % 4 != 0 {
            b.push(0);
        }
        let total = 12 + 8 + j.len() + 8 + b.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(j.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&j);
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&b);
        out
    }

    /// A box 0.6 wide, 0.5 tall, 0.6 deep, base on the ground and centred on XZ.
    ///
    /// All **eight** corners, not two opposite ones: `derive_front` compares centroids, and a
    /// two-vertex "box" is a diagonal, which is not symmetric. The first draft of this fixture had two
    /// and the symmetry test correctly caught it.
    fn a_crate() -> Glb {
        let json = r#"{
          "accessors":[{"type":"VEC3","componentType":5126,"count":8,"bufferView":0,
                        "min":[-0.3,0.0,-0.3],"max":[0.3,0.5,0.3]}],
          "bufferViews":[{"byteOffset":0,"byteLength":96}],
          "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}]
        }"#;
        let mut bin = Vec::new();
        for x in [-0.3f32, 0.3] {
            for y in [0.0f32, 0.5] {
                for z in [-0.3f32, 0.3] {
                    for c in [x, y, z] {
                        bin.extend_from_slice(&c.to_le_bytes());
                    }
                }
            }
        }
        Glb::parse(&synth(json, &bin)).expect("parses")
    }

    #[test]
    fn a_mesh_measures_its_own_footprint_and_height() {
        let m = a_crate().measure().expect("measures");
        assert!((m.footprint.0 - 0.6).abs() < 1e-5);
        assert!((m.footprint.1 - 0.6).abs() < 1e-5);
        assert!((m.height - 0.5).abs() < 1e-5);
        assert!(m.pivot.0.abs() < 1e-5 && m.pivot.1.abs() < 1e-5);
        assert!(m.base_y.abs() < 1e-5);
        assert!(!m.suspect_centimetres);
    }

    #[test]
    fn origin_alignment_uses_the_asset_librarys_vocabulary() {
        assert_eq!(
            a_crate().origin_alignment().expect("classifies"),
            OriginAlignment::BaseAtOriginCentred
        );
    }

    /// The 100× case: valid glTF that renders correctly, and silently centimetres to anything reading
    /// accessor bounds. Measured once on a real door frame at 200.3 units for 2.003 m.
    #[test]
    fn centimetre_authoring_is_flagged_rather_than_believed() {
        let json = r#"{
          "accessors":[{"type":"VEC3","componentType":5126,"count":1,"bufferView":0,
                        "min":[-100.0,0.0,-10.0],"max":[100.0,200.3,10.0]}],
          "bufferViews":[{"byteOffset":0,"byteLength":12}],
          "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}]
        }"#;
        let g = Glb::parse(&synth(json, &[0u8; 12])).expect("parses");
        assert!(g.measure().expect("measures").suspect_centimetres);
    }

    /// A node scale makes accessor bounds a lie, and a measurement library must say so rather than
    /// return a confident wrong number.
    #[test]
    fn a_node_transform_is_reported_not_ignored() {
        let json = r#"{
          "nodes":[{"scale":[0.01,0.01,0.01]}],
          "accessors":[{"type":"VEC3","componentType":5126,"count":1,"bufferView":0,
                        "min":[0.0,0.0,0.0],"max":[1.0,1.0,1.0]}],
          "bufferViews":[{"byteOffset":0,"byteLength":12}],
          "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}]
        }"#;
        let g = Glb::parse(&synth(json, &[0u8; 12])).expect("parses");
        assert!(g.has_node_transform());
        assert!(!a_crate().has_node_transform());
    }

    /// The method `site::kit` describes and never implemented. A seat whose back is at −X fronts +X,
    /// which the engine convention reaches at yaw 90.
    #[test]
    fn a_chairs_front_is_derived_from_where_its_back_leans() {
        // Two low vertices spanning the seat, plus a high one offset to −X: a backrest.
        let json = r#"{
          "accessors":[{"type":"VEC3","componentType":5126,"count":3,"bufferView":0,
                        "min":[-0.25,0.0,-0.25],"max":[0.25,0.9,0.25]}],
          "bufferViews":[{"byteOffset":0,"byteLength":36}],
          "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}]
        }"#;
        let mut bin = Vec::new();
        for v in [
            [-0.25f32, 0.0, -0.25],
            [0.25, 0.0, 0.25],
            [-0.25, 0.9, 0.0],
        ] {
            for c in v {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        let g = Glb::parse(&synth(json, &bin)).expect("parses");
        let front = g.derive_front().expect("derives").expect("has a front");
        assert!(
            (front - 90.0).abs() < 1.0,
            "a back at −X means a front at +X, which is yaw 90 — got {front}"
        );
    }

    /// A stool has no front, and `None` says exactly that. `Some(0.0)` would be a claim about the art
    /// that is not true.
    #[test]
    fn a_symmetric_mesh_reports_no_front_at_all() {
        assert_eq!(a_crate().derive_front().expect("derives"), None);
    }

    #[test]
    fn a_malformed_container_is_refused_rather_than_misread() {
        assert!(Glb::parse(b"not a glb at all").is_err());
        assert!(Glb::parse(&[]).is_err());
    }
}
