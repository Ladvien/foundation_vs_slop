//! **Importing a mesh** — measure it, propose a descriptor, and say what is wrong with it.
//!
//! `docs/2026-08-03-asset-schema-audit.md` §5 lists four fields nothing in this project validates:
//! `footprint` against the mesh, `scale`, `DoorPiece::opening`, and `front` — whose derivation method
//! is *written down* in `site::kit` and was implemented nowhere, having been measured once by hand for
//! two chairs. [`crate::glb`] made those measurable. This turns measuring into importing.
//!
//! # A proposal, not a decision
//!
//! Everything here produces a *candidate*: a descriptor with the measurable fields filled in and a
//! list of [`Finding`]s about the mesh. An author edits and commits it. That split matters, because
//! the measurable half and the authored half are different kinds of fact — a footprint is a
//! measurement and a `kind` is a judgement, and a tool that guesses the second one is a tool people
//! stop reading.
//!
//! # Why the findings are the point
//!
//! Every one of them is a failure this project has actually had, and every one is invisible at the
//! moment it happens:
//!
//! * **Centimetre units.** `SM_DoorFrame_Double` measured 200.3 units for a 2.003 m door. Nothing
//!   errors — it renders, at a hundred times the size.
//! * **Origin.** A centred mesh sinks half into the floor; base-at-origin is this project's
//!   convention and `tests/ozea_asset.rs` pins it to 5 mm.
//! * **Grid fit.** A footprint that does not land on the authoring snap tiles badly, and you cannot
//!   see it until you flood-fill a room and find stripes of bare floor.
//! * **Triangle budget.** `tests/valkyrie_asset.rs` exists because a re-export forgot to decimate and
//!   silently handed back 15× the geometry.
//! * **Duplicates.** The same mesh re-exported under a new name is two library entries that are one
//!   asset, and the second one is found by whoever wonders why there are two crates.
//! * **Node transforms.** They make accessor bounds a lie. `tests/prop_footprint_contract.rs` reports
//!   and skips such meshes rather than misreading them, on the grounds that *"a silently mismeasured
//!   pass would be worse than no pass"*. Same rule here.

use std::path::{Path, PathBuf};

use crate::descriptor::{Align, Descriptor, Extent};
use crate::glb::{Glb, Measured, OriginAlignment};
use crate::library::Library;
use crate::grid;
use crate::naming;

/// Above this, a mesh is worth a second look before it goes in a library that places hundreds of it.
///
/// Not a limit — a hero prop can be dense on purpose. It is the number that makes someone check
/// whether the exporter decimated, which is the question `tests/valkyrie_asset.rs` was written to ask.
pub const BUSY_TRIANGLES: usize = 20_000;

/// The largest dimension a piece of set dressing plausibly has, metres. Past this it is architecture
/// or a unit error, and the two look identical in a file.
pub const IMPLAUSIBLE_METRES: f32 = 30.0;

/// How serious a [`Finding`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing. The import is fine.
    Note,
    /// Probably wrong. The import will work and the result will look off.
    Warn,
    /// The measurement itself cannot be trusted, so the proposal is not evidence of anything.
    Blocking,
}

/// One thing worth saying about a candidate mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    /// What is true, in one sentence, with the number that makes it checkable.
    pub message: String,
    /// What to do about it, when there is an obvious answer.
    pub fix: Option<String>,
}

impl Finding {
    fn note(message: impl Into<String>) -> Finding {
        Finding {
            severity: Severity::Note,
            message: message.into(),
            fix: None,
        }
    }
    fn warn(message: impl Into<String>, fix: impl Into<String>) -> Finding {
        Finding {
            severity: Severity::Warn,
            message: message.into(),
            fix: Some(fix.into()),
        }
    }
    fn blocking(message: impl Into<String>, fix: impl Into<String>) -> Finding {
        Finding {
            severity: Severity::Blocking,
            message: message.into(),
            fix: Some(fix.into()),
        }
    }
}

/// A mesh that could become a library entry.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// Path relative to the project's asset root — what a descriptor's `mesh` field holds.
    pub mesh: String,
    /// The descriptor as measured. The author edits this before committing.
    pub proposed: Descriptor,
    /// What the measurement says, unrounded, for anyone who wants to check the proposal.
    pub measured: Option<Measured>,
    /// The mesh's asymmetry in metres and the threshold it was judged against — the numbers behind
    /// `align.front`, because a borderline call should be overrulable by a person who can see it.
    pub front_detail: Option<(f32, f32)>,
    pub triangles: usize,
    pub findings: Vec<Finding>,
}

impl Candidate {
    /// True when nothing here can be trusted enough to import.
    pub fn blocked(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Blocking)
    }

    pub fn worst(&self) -> Option<Severity> {
        self.findings.iter().map(|f| f.severity).max()
    }
}

/// Every `.glb` under `dir` that the library does not already have, measured.
///
/// `dir` is walked relative to `asset_root`; the returned `mesh` paths are relative to `asset_root`,
/// because that is the frame a descriptor and the engine both use.
pub fn scan(asset_root: &Path, dir: &Path, library: &Library) -> Result<Vec<Candidate>, String> {
    let mut paths = Vec::new();
    collect_glb(dir, &mut paths)?;
    // Sorted, so two machines produce the same list in the same order. A directory walk is not
    // ordered, and an importer whose list shuffles between runs is one nobody can talk about.
    paths.sort();

    let known: Vec<&str> = library
        .descriptors
        .iter()
        .filter_map(|d| d.mesh.as_deref())
        .collect();

    let mut out = Vec::new();
    for path in paths {
        let rel = path
            .strip_prefix(asset_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if known.contains(&rel.as_str()) {
            continue;
        }
        out.push(measure(&path, &rel, library));
    }
    Ok(out)
}

fn collect_glb(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("import: {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_glb(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "glb") {
            out.push(path);
        }
    }
    Ok(())
}

/// Measure one mesh and propose a descriptor for it.
pub fn measure(path: &Path, rel: &str, library: &Library) -> Candidate {
    let mut findings = Vec::new();
    // An id is a starting point, not a decision: the file name in the project's one spelling.
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let id = naming::to_snake_case(&stem);
    if id.is_empty() {
        findings.push(Finding::warn(
            format!("`{stem}` leaves nothing usable as an id"),
            "type one before committing",
        ));
    }

    let mut proposed = Descriptor {
        id,
        mesh: Some(rel.to_owned()),
        ..Descriptor::default()
    };

    let glb = match Glb::open(path) {
        Ok(g) => g,
        Err(e) => {
            findings.push(Finding::blocking(
                format!("this file cannot be read: {e}"),
                "re-export it, or check it is a binary glTF rather than a .gltf + .bin pair",
            ));
            return Candidate {
                mesh: rel.to_owned(),
                proposed,
                measured: None,
                front_detail: None,
                triangles: 0,
                findings,
            };
        }
    };

    // A node transform used to be blocking, on the grounds that it makes accessor bounds a lie. It
    // did — until `Glb::bounds` learned to compose the scene graph, which is the right fix and covers
    // every multi-part kit in this project.
    //
    // What is still true is narrower and worth saying: `derive_front` reads raw vertex data, so its
    // centroid is taken in mesh-local space and means nothing for a model assembled from placed
    // parts. Bounds are trustworthy; the facing is not, and it is left unset rather than guessed.
    let assembled = glb.has_node_transform();

    let measured = match glb.measure() {
        Ok(m) => m,
        Err(e) => {
            findings.push(Finding::blocking(
                format!("this file has no measurable geometry: {e}"),
                "check the export included a mesh",
            ));
            return Candidate {
                mesh: rel.to_owned(),
                proposed,
                measured: None,
                front_detail: None,
                triangles: 0,
                findings,
            };
        }
    };

    proposed.extent = Extent {
        footprint: Some(measured.footprint),
        height: Some(measured.height),
    };
    proposed.align = Align {
        // The base sits `base_y` above the ground; the offset that seats it is the negation.
        y_offset: Some(-measured.base_y),
        pivot: Some(measured.pivot),
        ..Align::default()
    };

    let front_detail = if assembled { None } else { glb.front_detail().ok() };
    proposed.align.front = if assembled {
        findings.push(Finding::note(
            "this model is assembled from parts placed by node transforms, so no facing is derived \
             — the size is measured from the assembled scene, but a centroid over raw vertices would \
             not be",
        ));
        None
    } else {
        glb.derive_front().ok().flatten()
    };

    let tris = triangles(&glb);
    findings.extend(inspect(&measured, front_detail, tris, rel, library));

    Candidate {
        mesh: rel.to_owned(),
        proposed,
        measured: Some(measured),
        front_detail,
        triangles: tris,
        findings,
    }
}

/// Everything worth saying about a measured mesh.
fn inspect(
    m: &Measured,
    front: Option<(f32, f32)>,
    triangles: usize,
    rel: &str,
    library: &Library,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let (w, d) = m.footprint;

    if m.suspect_centimetres {
        out.push(Finding::warn(
            format!(
                "this measures {w:.1} x {d:.1} x {:.1} — that is centimetre data read as metres",
                m.height
            ),
            "set scale to 0.01, or re-export with the unit baked in",
        ));
    } else if w.max(d).max(m.height) > IMPLAUSIBLE_METRES {
        out.push(Finding::warn(
            format!(
                "this is {:.1} m at its largest — architecture, or a unit mistake",
                w.max(d).max(m.height)
            ),
            "check the export scale before placing hundreds of it",
        ));
    }

    match origin_verdict(m) {
        OriginAlignment::BaseAtOriginCentred => {}
        OriginAlignment::Centred => out.push(Finding::warn(
            format!(
                "the origin is at the centre of the mesh, so it would sink {:.2} m into the floor",
                -m.base_y
            ),
            "the proposed y_offset already corrects this; re-export base-at-origin to make it true \
             of the file",
        )),
        OriginAlignment::Offset => out.push(Finding::warn(
            format!(
                "the origin is off-centre by ({:.2}, {:.2}) and {:.2} m off the base",
                m.pivot.0, m.pivot.1, m.base_y
            ),
            "the proposed pivot and y_offset correct this; re-export centred with its base at zero \
             to make it true of the file",
        )),
    }

    // **Grid fit**, stated as a fact rather than an alarm.
    //
    // The first version said "does not land on the grid" and fired on 40 of 44 shipped meshes, which
    // is a warning that has taught the reader to ignore it by the third asset. Real meshes are not
    // grid multiples; what is actually worth knowing is how many cells this occupies and what tiling
    // it would look like, and both are plain facts.
    //
    // The numbers come from `grid::cells`, which is also what the flood fill lays down — an importer
    // reporting a different cell count from the tool that places the piece is worse than one that
    // says nothing.
    let (cells_x, slack_x) = grid::cells(w);
    let (cells_z, slack_z) = grid::cells(d);
    let tiling = if slack_x.abs() < 1e-3 && slack_z.abs() < 1e-3 {
        "tiles exactly".to_owned()
    } else {
        let phrase = |slack: f32| {
            if slack >= 0.0 {
                format!("{:.0} mm gap", slack * 1000.0)
            } else {
                format!("{:.0} mm overlap", -slack * 1000.0)
            }
        };
        format!("tiled it leaves {} x {}", phrase(slack_x), phrase(slack_z))
    };
    out.push(Finding::note(format!(
        "{w:.2} x {d:.2} m occupies {cells_x} x {cells_z} cells on the {} m grid; {tiling}",
        grid::SNAP
    )));

    // `front_detail` returns (asymmetry, derived yaw) — NOT (asymmetry, threshold). Reading it as the
    // latter compared 0.166 m against 90 degrees and reported that a chair has no front, which the
    // shipped kit contradicts in writing. The threshold is the constant.
    if let Some((asymmetry, yaw)) = front {
        let limit = crate::glb::FRONT_MIN_OFFSET;
        out.push(Finding::note(if asymmetry >= limit {
            format!(
                "the upper mass sits {:.0} mm off centre, so this has a front at {yaw:.0} deg \
                 (threshold {:.0} mm)",
                asymmetry * 1000.0,
                limit * 1000.0
            )
        } else {
            format!(
                "the upper mass is within {:.0} mm of centre, so no front is asserted (threshold \
                 {:.0} mm)",
                asymmetry * 1000.0,
                limit * 1000.0
            )
        }));
    }

    if triangles > BUSY_TRIANGLES {
        out.push(Finding::warn(
            format!("{triangles} triangles — dense for a piece a map may hold hundreds of"),
            "check the exporter decimated; a re-export that forgets to can silently return 15x the \
             geometry",
        ));
    }

    // A re-export under a new name is two entries that are one asset, and the duplicate is found by
    // whoever wonders why the palette has two crates.
    //
    // **Compared as a sorted triple, not axis by axis.** A stored `extent` is already rotated by its
    // descriptor's `align.rotate` while this measurement is raw, so the two can be in different
    // frames — but a quarter turn only *permutes* the three spans, so the multiset is the same in
    // every frame. It also makes the check stronger than it was: a re-export that arrived on its
    // side is now caught rather than missed.
    let spans = |w: f32, h: f32, d: f32| {
        let mut v = [w, h, d];
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v
    };
    let mine = spans(m.footprint.0, m.height, m.footprint.1);
    if let Some(twin) = library.descriptors.iter().find(|other| {
        other.mesh.as_deref() != Some(rel)
            && match (other.extent.footprint, other.extent.height) {
                (Some((w, d)), Some(h)) => spans(w, h, d)
                    .iter()
                    .zip(mine.iter())
                    .all(|(a, b)| (a - b).abs() < 1e-3),
                _ => false,
            }
    }) {
        out.push(Finding::note(format!(
            "`{}` already has these exact measurements — this may be the same asset re-exported",
            twin.id
        )));
    }

    out
}

/// Re-derive the origin verdict from the measurement, so this does not need the file a second time.
fn origin_verdict(m: &Measured) -> OriginAlignment {
    let centred_xz = m.pivot.0.abs() <= crate::glb::ORIGIN_TOL
        && m.pivot.1.abs() <= crate::glb::ORIGIN_TOL;
    let on_base = m.base_y.abs() <= crate::glb::ORIGIN_TOL;
    let centred_y = (m.base_y + m.height * 0.5).abs() <= crate::glb::ORIGIN_TOL;
    match (centred_xz, on_base, centred_y) {
        (true, true, _) => OriginAlignment::BaseAtOriginCentred,
        (true, false, true) => OriginAlignment::Centred,
        _ => OriginAlignment::Offset,
    }
}

/// **Re-measure a descriptor for the rotation it now carries.** The one place a rotation is baked.
///
/// `align.rotate` is a render instruction and the `extent` beside it is already rotated — see
/// [`crate::descriptor::Align::rotate`] for why that trade was taken. The invariant only holds if
/// changing the rotation re-derives the measurements, so this is what an editor calls when it does.
///
/// Rewrites `extent.footprint`, `extent.height`, `align.pivot` and `align.y_offset`, because a
/// quarter turn moves all four. Leaves `front`, `scale` and `stretch_y` alone: a facing is a fact
/// about which way the art points and is expressed in the mesh's own frame, and the two scales are
/// corrections that a rotation does not touch.
///
/// # An authored lift or sink survives the turn
///
/// `was` is the rotation the descriptor's *current* measurements were taken under — what
/// `align.rotate` held before the caller changed it. It is needed because `y_offset` is not purely a
/// measurement: it is `-base_y` **plus** whatever the author added, and this used to overwrite the sum
/// with the term. Measured across the shipped kits, every non-zero `y_offset` is entirely the authored
/// term — all six meshes sit at `base_y = 0`, so `site/floor`'s `-0.06` sink and the three decals'
/// `+0.002` lifts were the whole value, and one press of a rotate chip zeroed them. The decals then
/// land coplanar with the floor plate they were lifted off, which `Align::y_offset`'s own doc calls
/// out as leaving "the depth winner undefined".
///
/// So the authored term is measured out before the turn and put back after it. A vertical nudge is
/// still vertical after a turn about Y; after one about X or Z the author's `0.002` is applied to the
/// piece as it now stands, which is the same claim they made about it standing the other way.
pub fn remeasure_rotated(
    d: &mut Descriptor,
    glb: &Glb,
    was: Option<(i32, i32, i32)>,
) -> Result<(), String> {
    let quarters = match d.align.rotate {
        Some(rotate) => crate::descriptor::quarter_turns_xyz(rotate, &d.id)?,
        None => (0, 0, 0),
    };
    let before_quarters = match was {
        Some(rotate) => crate::descriptor::quarter_turns_xyz(rotate, &d.id)?,
        None => (0, 0, 0),
    };
    // One read of the file, turned twice: `Measured::rotated` works off the recorded bounds.
    let raw = glb.measure()?;
    let m = raw.rotated(quarters);
    let before = raw.rotated(before_quarters);
    // What the author added on top of the measurement. Absent means they never said, which is the
    // same as adding nothing.
    let authored_lift = d.align.y_offset.unwrap_or(-before.base_y) + before.base_y;
    d.extent.footprint = Some(m.footprint);
    d.extent.height = Some(m.height);
    d.align.pivot = Some(m.pivot);
    d.align.y_offset = Some(-m.base_y + authored_lift);
    Ok(())
}

/// Triangles across every primitive, from the accessor counts — no vertex data is read.
///
/// Public because the editor wants this for library entries too, not only for import candidates: a
/// palette that shows what a piece costs is a palette an author can budget with.
pub fn triangles(glb: &Glb) -> usize {
    let Some(meshes) = glb.json["meshes"].as_array() else {
        return 0;
    };
    let count = |accessor: Option<&serde_json::Value>| -> usize {
        accessor
            .and_then(|i| i.as_u64())
            .and_then(|i| glb.json["accessors"].get(i as usize))
            .and_then(|a| a["count"].as_u64())
            .unwrap_or(0) as usize
    };
    meshes
        .iter()
        .filter_map(|m| m["primitives"].as_array())
        .flatten()
        .map(|p| {
            // Indexed geometry counts indices; unindexed counts positions. Both are three per face.
            let n = match p.get("indices") {
                Some(_) => count(p.get("indices")),
                None => count(p["attributes"].get("POSITION")),
            };
            n / 3
        })
        .sum()
}

/// **Does this triangle touch this axis-aligned box?** Exact, by the separating-axis theorem.
///
/// Akenine-Möller 2001, *Fast 3D Triangle-Box Overlap Testing* — thirteen axes: the box's three face
/// normals, the triangle's normal, and the nine cross products of a box axis with a triangle edge.
/// Finding a single axis on which the two projections do not overlap proves they are disjoint;
/// surviving all thirteen proves they intersect.
///
/// Coordinates are already relative to the box centre, and `half` is the box's half-extent.
fn triangle_hits_box(tri: [[f32; 3]; 3], half: f32) -> bool {
    let [a, b, c] = tri;
    // The box's own three axes.
    for i in 0..3 {
        let (mn, mx) = (a[i].min(b[i]).min(c[i]), a[i].max(b[i]).max(c[i]));
        if mn > half || mx < -half {
            return false;
        }
    }

    let sub = |p: [f32; 3], q: [f32; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    let cross = |p: [f32; 3], q: [f32; 3]| {
        [
            p[1] * q[2] - p[2] * q[1],
            p[2] * q[0] - p[0] * q[2],
            p[0] * q[1] - p[1] * q[0],
        ]
    };
    let dot = |p: [f32; 3], q: [f32; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];

    // The triangle's plane against the box.
    let n = cross(sub(b, a), sub(c, a));
    let d = dot(n, a);
    let r = half * (n[0].abs() + n[1].abs() + n[2].abs());
    if d.abs() > r {
        return false;
    }

    // The nine edge-vs-box-axis cross products.
    let edges = [sub(b, a), sub(c, b), sub(a, c)];
    let verts = [a, b, c];
    for e in edges {
        for axis in 0..3 {
            let mut unit = [0.0f32; 3];
            unit[axis] = 1.0;
            let ax = cross(unit, e);
            // A degenerate axis separates nothing, and normalising it would divide by zero.
            if ax[0].abs() + ax[1].abs() + ax[2].abs() < 1e-12 {
                continue;
            }
            let p: Vec<f32> = verts.iter().map(|v| dot(ax, *v)).collect();
            let (mn, mx) = p.iter().fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
            let r = half * (ax[0].abs() + ax[1].abs() + ax[2].abs());
            if mn > r || mx < -r {
                return false;
            }
        }
    }
    true
}

/// **Which lattice cells the mesh actually occupies.**
///
/// Nobody hand-marks a lattice — a 3 m wall at the shipped setting is 30 cells and it only goes up
/// from there — so occupancy has to come off the geometry. This is the read that does it.
///
/// # Every cell a triangle passes through, not every cell holding a vertex
///
/// This began as vertex occupancy, which was cheap and honest and **wrong for exactly the meshes
/// that matter**. A wall slab has vertices only at its eight corners, so a 2.40 m wall came back
/// with its top and bottom layers marked and the six between them open — measured, on the shipped
/// kit, at 4 cells of 10. Architecture is the low-poly half of any kit and the half the lattice
/// exists for.
///
/// Rasterising is the correct method rather than a better heuristic: a triangle either passes
/// through a cell or it does not, and `triangle_hits_box` answers that exactly. A bounding box
/// cannot answer it at all — one box per mesh *is* the whole extent, so every cell comes out solid.
///
/// # It reads the assembled model
///
/// Triangles come from [`Glb::triangle_vertices`], which walks the scene graph and applies node
/// transforms. Reading raw accessors would see a kitbashed mesh's parts piled at the origin — the
/// same defect `Glb::bounds` was fixed for, and one this would otherwise have repeated.
///
/// # Normalised, so units and the policy layer cannot reach it
///
/// Cells are assigned in the mesh's own bounding box rather than in metres. Two consequences, both
/// tested: a centimetre-authored mesh (the FBX exporter's `scale: 0.01` over 100x vertex data, which
/// [`crate::glb::Measured::suspect_centimetres`] flags) marks exactly what its metre twin does; and
/// `align.stretch_y` scales an axis uniformly, so a project's architecture cannot change which cells
/// a mesh is said to fill.
///
/// # In the frame the divisions were derived in
///
/// `rotate` is the piece's [`crate::descriptor::Align::rotate`], as quarter turns. Every vertex goes
/// through it before anything is measured, because `div` comes from the **rotated** extent: leaving it
/// out mapped the Y and Z divisions onto the wrong mesh axes for any piece exported the wrong way up,
/// and returned a transposed lattice for every non-symmetric one — an L-desk, a pipe corner, a window
/// cutout. Pass `(0, 0, 0)` for a piece that carries no rotation.
///
/// Returns cells in ascending order with no duplicates, so the result is the same on every machine.
pub fn occupancy(
    glb: &Glb,
    div: (u32, u32, u32),
    rotate: (u8, u8, u8),
) -> Result<Vec<(u32, u32, u32)>, String> {
    let (dx, dy, dz) = div;
    if dx == 0 || dy == 0 || dz == 0 {
        return Err(format!(
            "a {dx}x{dy}x{dz} lattice has no cells for a mesh to occupy"
        ));
    }
    let tris: Vec<[[f32; 3]; 3]> = glb
        .triangle_vertices()?
        .into_iter()
        .map(|t| t.map(|v| crate::glb::spin(v, rotate)))
        .collect();

    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for t in &tris {
        for v in t {
            for a in 0..3 {
                if !v[a].is_finite() {
                    return Err("glb: a vertex position is not a finite number".to_owned());
                }
                lo[a] = lo[a].min(v[a]);
                hi[a] = hi[a].max(v[a]);
            }
        }
    }

    let n = [dx, dy, dz];
    // Into lattice coordinates, where a cell is the unit box from `i` to `i + 1`. A flat axis — a
    // decal, or a plane — has one row and everything lands in it; scaling by zero would be a NaN.
    let to_cell = |v: [f32; 3]| {
        let mut out = [0.0f32; 3];
        for a in 0..3 {
            let span = hi[a] - lo[a];
            out[a] = if span <= 0.0 {
                0.5
            } else {
                (v[a] - lo[a]) / span * n[a] as f32
            };
        }
        out
    };

    let mut marked = vec![false; (dx as usize) * (dy as usize) * (dz as usize)];
    for t in &tris {
        let p = [to_cell(t[0]), to_cell(t[1]), to_cell(t[2])];
        // Only the cells this triangle's own bounds reach are worth testing.
        let mut min = [0u32; 3];
        let mut max = [0u32; 3];
        for a in 0..3 {
            let l = p[0][a].min(p[1][a]).min(p[2][a]).floor().max(0.0);
            let h = p[0][a].max(p[1][a]).max(p[2][a]).ceil();
            min[a] = (l as u32).min(n[a] - 1);
            max[a] = (h.max(0.0) as u32).min(n[a] - 1);
        }
        for x in min[0]..=max[0] {
            for y in min[1]..=max[1] {
                for z in min[2]..=max[2] {
                    let at = ((x as usize) * dy as usize + y as usize) * dz as usize + z as usize;
                    if marked[at] {
                        continue;
                    }
                    let centre = [x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5];
                    let rel = p.map(|v| {
                        [v[0] - centre[0], v[1] - centre[1], v[2] - centre[2]]
                    });
                    if triangle_hits_box(rel, 0.5) {
                        marked[at] = true;
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    for x in 0..dx {
        for y in 0..dy {
            for z in 0..dz {
                let at = ((x as usize) * dy as usize + y as usize) * dz as usize + z as usize;
                if marked[at] {
                    out.push((x, y, z));
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LIBRARY_VERSION;

    fn empty_library() -> Library {
        Library {
            version: LIBRARY_VERSION,
            note: None,
            descriptors: Vec::new(),
        }
    }

    fn measured(footprint: (f32, f32), height: f32, base_y: f32, pivot: (f32, f32)) -> Measured {
        Measured {
            lo: [0.0, base_y, 0.0],
            hi: [footprint.0, base_y + height, footprint.1],
            footprint,
            height,
            pivot,
            base_y,
            suspect_centimetres: false,
        }
    }

    fn messages(f: &[Finding]) -> String {
        f.iter().map(|f| f.message.clone()).collect::<Vec<_>>().join(" | ")
    }

    #[test]
    fn a_clean_mesh_only_reports_notes() {
        let m = measured((1.0, 1.0), 0.5, 0.0, (0.0, 0.0));
        let f = inspect(&m, None, 500, "kit/floor.glb", &empty_library());
        assert!(
            f.iter().all(|f| f.severity == Severity::Note),
            "unexpected: {}",
            messages(&f)
        );
    }

    /// The 100x case, measured once for real: a 2.003 m door that read as 200.3.
    #[test]
    fn centimetre_data_is_caught_and_the_fix_is_named() {
        let mut m = measured((200.3, 20.0), 200.3, 0.0, (0.0, 0.0));
        m.suspect_centimetres = true;
        let f = inspect(&m, None, 100, "kit/door.glb", &empty_library());
        let hit = f
            .iter()
            .find(|f| f.message.contains("centimetre"))
            .unwrap_or_else(|| panic!("{}", messages(&f)));
        assert_eq!(hit.severity, Severity::Warn);
        assert!(hit.fix.as_deref().unwrap_or_default().contains("0.01"));
    }

    /// A centred mesh sinks half into the floor, and the proposal already corrects it — but the file
    /// is still wrong and someone should know.
    #[test]
    fn a_centred_origin_is_reported_with_how_far_it_would_sink() {
        let m = measured((1.0, 1.0), 2.0, -1.0, (0.0, 0.0));
        let f = inspect(&m, None, 100, "kit/x.glb", &empty_library());
        let hit = f
            .iter()
            .find(|f| f.message.contains("centre of the mesh"))
            .unwrap_or_else(|| panic!("{}", messages(&f)));
        assert!(hit.message.contains("1.00 m"), "{}", hit.message);
    }

    /// The finding the flood fill taught — and it must agree with the fill, because they now share
    /// `grid::cells`.
    #[test]
    fn the_grid_note_gives_cells_and_the_tiling_slack() {
        let m = measured((1.45, 0.5), 0.8, 0.0, (0.0, 0.0));
        let f = inspect(&m, None, 100, "kit/drawer.glb", &empty_library());
        let hit = f
            .iter()
            .find(|f| f.message.contains("occupies"))
            .unwrap_or_else(|| panic!("{}", messages(&f)));
        assert!(hit.message.contains("3 x 1 cells"), "{}", hit.message);
        assert!(hit.message.contains("50 mm gap"), "{}", hit.message);
    }

    /// A piece wider than its cell overlaps its neighbours, which reads differently from a gap.
    #[test]
    fn an_oversized_piece_reports_an_overlap_not_a_gap() {
        let m = measured((0.55, 0.5), 0.8, 0.0, (0.0, 0.0));
        let f = inspect(&m, None, 100, "kit/bench.glb", &empty_library());
        assert!(messages(&f).contains("50 mm overlap"), "{}", messages(&f));
    }

    #[test]
    fn a_footprint_on_the_grid_says_so_too() {
        let m = measured((1.5, 0.5), 0.8, 0.0, (0.0, 0.0));
        let f = inspect(&m, None, 100, "kit/x.glb", &empty_library());
        assert!(messages(&f).contains("tiles exactly"), "{}", messages(&f));
    }

    /// The number, not just the verdict — 166 mm and 12 mm are both "asymmetric" and only one is a
    /// backrest.
    ///
    /// The second element of `front_detail` is the derived YAW, not the threshold. Reading it as a
    /// threshold made a chair — which the shipped kit records as `front: Some(90.0)` — report that it
    /// has no front, because 0.166 is not >= 90.
    #[test]
    fn the_front_finding_carries_the_asymmetry_and_the_angle() {
        let m = measured((0.5, 0.5), 1.0, 0.0, (0.0, 0.0));
        let backed = inspect(&m, Some((0.166, 90.0)), 100, "a.glb", &empty_library());
        assert!(messages(&backed).contains("166 mm"), "{}", messages(&backed));
        assert!(messages(&backed).contains("has a front at 90 deg"), "{}", messages(&backed));

        let stool = inspect(&m, Some((0.012, 90.0)), 100, "b.glb", &empty_library());
        assert!(messages(&stool).contains("no front is asserted"), "{}", messages(&stool));
    }

    #[test]
    fn a_dense_mesh_is_flagged_with_the_decimation_question() {
        let m = measured((1.0, 1.0), 1.0, 0.0, (0.0, 0.0));
        let f = inspect(&m, None, BUSY_TRIANGLES + 1, "x.glb", &empty_library());
        let hit = f
            .iter()
            .find(|f| f.message.contains("triangles"))
            .unwrap_or_else(|| panic!("{}", messages(&f)));
        assert!(hit.fix.as_deref().unwrap_or_default().contains("decimate"));
    }

    /// A re-export under a new name is two entries that are one asset.
    #[test]
    fn a_mesh_matching_an_existing_entrys_measurements_is_flagged() {
        let mut lib = empty_library();
        lib.descriptors.push(Descriptor {
            id: "crate".into(),
            mesh: Some("kit/crate.glb".into()),
            extent: Extent {
                footprint: Some((0.6, 0.6)),
                height: Some(0.6),
            },
            ..Descriptor::default()
        });
        let m = measured((0.6, 0.6), 0.6, 0.0, (0.0, 0.0));
        let f = inspect(&m, None, 100, "kit/crate_v2.glb", &lib);
        assert!(messages(&f).contains("`crate` already has these"), "{}", messages(&f));
    }

    #[test]
    fn severity_orders_so_the_worst_finding_can_be_found() {
        assert!(Severity::Blocking > Severity::Warn);
        assert!(Severity::Warn > Severity::Note);
    }
}

/// Occupancy, over synthetic containers so no fixture asset is needed.
#[cfg(test)]
mod occupancy_tests {
    use super::*;

    /// A minimal GLB carrying exactly the triangles given, in metres.
    fn mesh(tris: &[[[f32; 3]; 3]]) -> Glb {
        let count = tris.len() * 3;
        // **The accessor declares its bounds**, as a real exporter's does — glTF requires `min`/`max`
        // on a POSITION accessor, and `Glb::measure` reads them rather than the vertex data. Without
        // them this fixture could only be used for occupancy, which reads the buffer.
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for t in tris {
            for v in t {
                for a in 0..3 {
                    lo[a] = lo[a].min(v[a]);
                    hi[a] = hi[a].max(v[a]);
                }
            }
        }
        let list = |v: [f32; 3]| format!("[{},{},{}]", v[0], v[1], v[2]);
        let json = format!(
            r#"{{
              "accessors":[{{"type":"VEC3","componentType":5126,"count":{count},"bufferView":0,"min":{},"max":{}}}],
              "bufferViews":[{{"byteOffset":0,"byteLength":{}}}],
              "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}]
            }}"#,
            list(lo),
            list(hi),
            count * 12
        );
        let mut bin = Vec::new();
        for t in tris {
            for v in t {
                for c in v {
                    bin.extend_from_slice(&c.to_le_bytes());
                }
            }
        }
        let j = json.as_bytes();
        let mut jj = j.to_vec();
        jj.extend(std::iter::repeat_n(b' ', (4 - j.len() % 4) % 4));
        let total = 12 + 8 + jj.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(jj.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&jj);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        Glb::parse(&out).unwrap_or_else(|e| panic!("{e}"))
    }

    /// An axis-aligned quad in the XZ plane at height `y`, as two triangles.
    fn slab(x: (f32, f32), y: f32, z: (f32, f32)) -> Vec<[[f32; 3]; 3]> {
        let (x0, x1) = x;
        let (z0, z1) = z;
        vec![
            [[x0, y, z0], [x1, y, z0], [x1, y, z1]],
            [[x0, y, z0], [x1, y, z1], [x0, y, z1]],
        ]
    }

    /// **A turn re-measures the piece; it does not un-author the correction on it.**
    ///
    /// `y_offset` is `-base_y` **plus** whatever the author added, and `remeasure_rotated` used to
    /// overwrite the sum with the term. Measured across the shipped kits every non-zero `y_offset` is
    /// entirely the authored part — all six meshes sit at `base_y = 0` — so one press of a rotate chip
    /// zeroed `site/floor`'s `-0.06` sink and all three decals' `+0.002` lifts, putting the decals
    /// coplanar with the plate they were lifted off.
    #[test]
    fn a_turn_keeps_the_lift_the_author_added() {
        // Geometry that starts above its own origin, so `-base_y` is not zero and the arithmetic has
        // something to get wrong.
        let glb = mesh(&box_mesh([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]));
        let raw = glb.measure().unwrap_or_else(|e| panic!("{e}"));
        assert!((raw.base_y - 0.5).abs() < 1e-6, "base_y {}", raw.base_y);

        for (authored, lift) in [(-0.5f32, 0.0f32), (-0.4, 0.1), (-0.56, -0.06)] {
            for turn in [(0, 90, 0), (90, 0, 0), (0, 0, 90)] {
                let mut d = Descriptor {
                    id: "plate".to_owned(),
                    align: Align {
                        y_offset: Some(authored),
                        rotate: Some(turn),
                        ..Align::default()
                    },
                    ..Descriptor::default()
                };
                // `was: None` — the authored value was taken with the piece unturned.
                remeasure_rotated(&mut d, &glb, None).unwrap_or_else(|e| panic!("{e}"));
                let base_now = glb
                    .measure()
                    .unwrap_or_else(|e| panic!("{e}"))
                    .rotated(crate::descriptor::quarter_turns_xyz(turn, "plate").unwrap_or_else(|e| panic!("{e}")))
                    .base_y;
                let got = d.align.y_offset.unwrap_or(f32::NAN);
                assert!(
                    (got - (-base_now + lift)).abs() < 1e-5,
                    "turn {turn:?}, authored {authored}: y_offset {got}, wanted {} \
                     (measurement {} plus the author's {lift})",
                    -base_now + lift,
                    -base_now
                );
            }
        }
    }

    /// And a turn that changes nothing must change nothing — the identity is the case a delta
    /// calculation gets wrong first.
    #[test]
    fn an_identity_turn_leaves_the_offset_exactly_where_it_was() {
        let glb = mesh(&box_mesh([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]));
        let mut d = Descriptor {
            id: "plate".to_owned(),
            align: Align {
                y_offset: Some(-0.56),
                ..Align::default()
            },
            ..Descriptor::default()
        };
        remeasure_rotated(&mut d, &glb, None).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            (d.align.y_offset.unwrap_or(f32::NAN) + 0.56).abs() < 1e-6,
            "{:?}",
            d.align.y_offset
        );
    }

    /// **Rasterising a rotated mesh is rasterising the rotated mesh.**
    ///
    /// `occupancy` normalised vertices into the raw file's bounding box while its `div` came from the
    /// already-rotated `extent`, so a piece carrying `align.rotate` had the Y and Z divisions applied
    /// to the wrong mesh axes. Stated as an invariant rather than as a table of expected cells: the
    /// answer for a mesh turned at read time must equal the answer for a mesh whose vertices were
    /// turned first, which is the only thing "in the same frame" can mean.
    ///
    /// The shape is deliberately asymmetric on all three axes — a symmetric one cannot tell a
    /// transposition from the truth, which is why this went unnoticed.
    #[test]
    fn a_rotation_reads_the_mesh_in_the_frame_its_divisions_came_from() {
        let wedge = {
            let mut out = box_mesh([0.0, 0.0, 0.0], [1.0, 0.25, 0.5]);
            out.extend(box_mesh([0.0, 0.0, 0.0], [0.25, 0.75, 0.5]));
            out
        };
        // Every quarter turn about each axis, and one composed turn — quarter turns do not commute,
        // so a fix that happened to work for a single axis is not a fix.
        for rotate in [(1, 0, 0), (3, 0, 0), (0, 1, 0), (0, 0, 1), (1, 1, 0), (2, 1, 3)] {
            let turned: Vec<[[f32; 3]; 3]> = wedge
                .iter()
                .map(|t| t.map(|v| crate::glb::spin(v, rotate)))
                .collect();
            let div = (3, 2, 4);
            assert_eq!(
                occupancy(&mesh(&wedge), div, rotate).unwrap_or_else(|e| panic!("{e}")),
                occupancy(&mesh(&turned), div, (0, 0, 0)).unwrap_or_else(|e| panic!("{e}")),
                "rotate {rotate:?}: reading the mesh turned must equal reading the turned mesh"
            );
        }
    }

    /// A closed box as 12 triangles — what a low-poly wall, crate or column actually is.
    fn box_mesh(lo: [f32; 3], hi: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
        let c = |x: usize, y: usize, z: usize| {
            [
                if x == 0 { lo[0] } else { hi[0] },
                if y == 0 { lo[1] } else { hi[1] },
                if z == 0 { lo[2] } else { hi[2] },
            ]
        };
        let quad = |a, b, cc, d| vec![[a, b, cc], [a, cc, d]];
        let mut out = Vec::new();
        for (a, b, cc, d) in [
            (c(0,0,0), c(1,0,0), c(1,1,0), c(0,1,0)),
            (c(0,0,1), c(1,0,1), c(1,1,1), c(0,1,1)),
            (c(0,0,0), c(0,0,1), c(0,1,1), c(0,1,0)),
            (c(1,0,0), c(1,0,1), c(1,1,1), c(1,1,0)),
            (c(0,0,0), c(1,0,0), c(1,0,1), c(0,0,1)),
            (c(0,1,0), c(1,1,0), c(1,1,1), c(0,1,1)),
        ] {
            out.extend(quad(a, b, cc, d));
        }
        out
    }

    /// **The reason this stopped being vertex occupancy.**
    ///
    /// A wall is a slab with vertices only at its eight corners. Marking the cell each vertex falls
    /// in left the middle of it open — measured at 4 of 10 cells on the shipped `site/wall`, which is
    /// most of the piece reported as thin air. A solid box is solid all the way through.
    #[test]
    fn a_wall_slab_is_solid_all_the_way_up() {
        // 3 m wide, 2.4 m tall, 0.5 m deep — the shipped wall, at the shipped divisions.
        let wall = box_mesh([0.0, 0.0, 0.0], [3.0, 2.4, 0.5]);
        let div = (6, 5, 1);
        let got = occupancy(&mesh(&wall), div, (0, 0, 0)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            got.len() as u32,
            emerge_core_volume(div),
            "every cell of a solid slab is solid; got {} of {}",
            got.len(),
            emerge_core_volume(div)
        );
    }

    fn emerge_core_volume(div: (u32, u32, u32)) -> u32 {
        crate::descriptor::Subgrid::volume(div)
    }

    /// The eight corners of a box fill all eight cells of a 2x2x2 lattice.
    #[test]
    fn a_box_fills_the_lattice_it_spans() {
        let got = occupancy(&mesh(&box_mesh([0.0; 3], [1.0; 3])), (2, 2, 2), (0, 0, 0))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.len(), 8, "{got:?}");
    }

    /// **The far corner belongs to the last cell**, not to one past the end and not wrapped to the
    /// first. Off by one here would mark the wrong face of every mesh.
    #[test]
    fn the_far_corner_lands_in_the_last_cell() {
        let tri = [[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 1.0]]];
        let got = occupancy(&mesh(&tri), (4, 4, 4), (0, 0, 0)).unwrap_or_else(|e| panic!("{e}"));
        assert!(got.contains(&(0, 0, 0)), "{got:?}");
        assert!(got.contains(&(3, 3, 3)), "the vertex at the maximum bound: {got:?}");
    }

    /// **The shape, not the bounding box.** An L leaves its inner corner clear — which a bounding box
    /// could never report, and which is the whole reason this reads geometry.
    #[test]
    fn an_l_shape_leaves_its_inner_corner_clear() {
        // Two arms of a flat L over a 2x1x2 lattice, each stopping short of the far cell.
        let mut l = slab((0.0, 2.0), 0.0, (0.0, 0.9));
        l.extend(slab((0.0, 0.9), 0.0, (0.0, 2.0)));
        let got = occupancy(&mesh(&l), (2, 1, 2), (0, 0, 0)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got, vec![(0, 0, 0), (0, 0, 1), (1, 0, 0)]);
        assert!(!got.contains(&(1, 0, 1)), "the inner corner must stay open");
    }

    /// **Unit-free.** The FBX importer writes centimetre authoring as 100x vertex data under a node
    /// `scale: 0.01`. Normalised bucketing means such a mesh marks exactly what its metre twin does.
    #[test]
    fn a_centimetre_mesh_marks_the_same_cells_as_its_metre_twin() {
        let m = box_mesh([0.0, 0.0, 0.0], [2.0, 1.0, 2.0]);
        let cm: Vec<[[f32; 3]; 3]> = m
            .iter()
            .map(|t| t.map(|v| [v[0] * 100.0, v[1] * 100.0, v[2] * 100.0]))
            .collect();
        assert_eq!(
            occupancy(&mesh(&m), (4, 2, 4), (0, 0, 0)).unwrap_or_else(|e| panic!("{e}")),
            occupancy(&mesh(&cm), (4, 2, 4), (0, 0, 0)).unwrap_or_else(|e| panic!("{e}"))
        );
    }

    /// **Invariant to the policy layer.** `stretch_y` scales an axis uniformly, so one game's ceiling
    /// height cannot change which cells a mesh is said to fill.
    #[test]
    fn stretching_an_axis_does_not_move_a_single_cell() {
        let base = box_mesh([0.0, 0.0, 0.0], [2.0, 1.0, 2.0]);
        let tall: Vec<[[f32; 3]; 3]> = base
            .iter()
            .map(|t| t.map(|v| [v[0], v[1] * 2.4, v[2]]))
            .collect();
        assert_eq!(
            occupancy(&mesh(&base), (3, 3, 3), (0, 0, 0)).unwrap_or_else(|e| panic!("{e}")),
            occupancy(&mesh(&tall), (3, 3, 3), (0, 0, 0)).unwrap_or_else(|e| panic!("{e}"))
        );
    }

    /// A decal has no thickness. One layer, everything in it — no divide by zero.
    #[test]
    fn a_flat_mesh_occupies_one_layer_rather_than_dividing_by_zero() {
        let got = occupancy(&mesh(&slab((0.0, 1.0), 0.0, (0.0, 1.0))), (2, 1, 2), (0, 0, 0))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.len(), 4);
        assert!(got.iter().all(|c| c.1 == 0));
    }

    /// Sorted, and each cell once, so two machines reading one mesh get one answer.
    #[test]
    fn the_answer_is_sorted_and_each_cell_appears_once() {
        let got = occupancy(&mesh(&box_mesh([0.0; 3], [1.0; 3])), (3, 3, 3), (0, 0, 0))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut sorted = got.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(got, sorted);
    }

    /// A degenerate lattice is refused rather than silently marking nothing.
    #[test]
    fn a_lattice_with_no_cells_is_refused() {
        let err = occupancy(&mesh(&slab((0.0, 1.0), 0.0, (0.0, 1.0))), (2, 0, 2), (0, 0, 0))
            .err()
            .unwrap_or_default();
        assert!(err.contains("no cells"), "{err}");
    }
}
