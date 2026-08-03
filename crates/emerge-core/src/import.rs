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
    if let Some(twin) = library.descriptors.iter().find(|other| {
        other.mesh.as_deref() != Some(rel)
            && other.extent.footprint.is_some_and(|f| close(f, m.footprint))
            && other.extent.height.is_some_and(|h| (h - m.height).abs() < 1e-3)
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

fn close(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
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
