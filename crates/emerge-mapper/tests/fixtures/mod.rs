//! **A project made of nothing** — for tests that are about the editor rather than about the corpus.
//!
//! `tests/headless.rs` used to boot the shipped `assets/` directory, and several of its assertions
//! read the candidate list, the pack set, or a named descriptor. That makes importing a kit a
//! breaking change to the test suite — and importing kits is the thing this editor exists to do. A
//! fixture writes exactly the project a test is about, so a test fails when the *editor* changes and
//! not when the art does.
//!
//! # What is real here, and why
//!
//! **The font, and only the font.** `harness::install_font` reads
//! `assets/fonts/FiraMono-Regular.ttf` and returns `Err` without it, so a project cannot boot
//! without one — and `Font::from_bytes` rejects a made-up file, so there is nothing to synthesise.
//! It is infrastructure rather than corpus: no test asserts anything about it, and no import can
//! change it.
//!
//! The meshes are built here, in memory, by [`Fixture::pack`].
//!
//! # Asset-contract tests are the deliberate exception
//!
//! A test whose *purpose* is "does the shipped valkyrie still measure the way `rigs.ron` claims"
//! must read the shipped valkyrie; that is not corpus dependence, that is the assertion. Those say
//! so in their own doc comment and stay on the real project. Everything else belongs here.

#![allow(dead_code)] // Each test uses a different corner of the builder.

use std::path::{Path, PathBuf};

/// A throwaway project directory, deleted when this is dropped.
pub struct Fixture {
    dir: PathBuf,
    descriptors: Vec<String>,
    placements: Vec<String>,
    compositions: Vec<String>,
}

/// Where the workspace lives, for the one file that has to be borrowed.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|e| panic!("cannot resolve the workspace root: {e}"))
}

/// **A minimal binary glTF**, laid out the way `emerge_core::glb` reads one: a JSON chunk then a BIN
/// chunk, each padded to four bytes.
///
/// The same construction `emerge-core`'s own `glb` tests use to exercise the reader "without shipping
/// a fixture asset" — restated here rather than reached for, because that one is private to a
/// `#[cfg(test)]` module in another crate.
fn glb(width: f32, height: f32, depth: f32) -> Vec<u8> {
    let (hx, hz) = (width * 0.5, depth * 0.5);
    // **All eight corners.** `derive_front` compares centroids, so a two-vertex "box" is a diagonal
    // and is not symmetric — a mistake `emerge-core`'s fixture made first and its own test caught.
    let mut bin = Vec::new();
    for x in [-hx, hx] {
        for y in [0.0f32, height] {
            for z in [-hz, hz] {
                for c in [x, y, z] {
                    bin.extend_from_slice(&c.to_le_bytes());
                }
            }
        }
    }
    let json = format!(
        r#"{{"accessors":[{{"type":"VEC3","componentType":5126,"count":8,"bufferView":0,
            "min":[{:.4},0.0,{:.4}],"max":[{:.4},{:.4},{:.4}]}}],
            "bufferViews":[{{"byteOffset":0,"byteLength":{}}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}]}}"#,
        -hx,
        -hz,
        hx,
        height,
        hz,
        bin.len()
    );

    let mut j = json.into_bytes();
    while j.len() % 4 != 0 {
        j.push(b' ');
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + j.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&j);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

impl Fixture {
    /// An empty project: a vocabulary, an empty library, a policy, and the font.
    pub fn new(name: &str) -> Fixture {
        // A unique directory per test, since these run in parallel. The process id and the test's
        // own name are enough — no clock, so a rerun is reproducible.
        let dir = std::env::temp_dir().join(format!("emerge-fixture-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let emerge = dir.join("assets/emerge");
        std::fs::create_dir_all(&emerge).unwrap_or_else(|e| panic!("cannot make {emerge:?}: {e}"));

        // The one borrowed file. See the module note.
        let fonts = dir.join("assets/fonts");
        std::fs::create_dir_all(&fonts).unwrap_or_else(|e| panic!("{e}"));
        let from = workspace().join("assets/fonts/FiraMono-Regular.ttf");
        std::fs::copy(&from, fonts.join("FiraMono-Regular.ttf"))
            .unwrap_or_else(|e| panic!("cannot copy the shipped font from {from:?}: {e}"));

        std::fs::write(
            emerge.join("vocab.ron"),
            r#"(
    kind: (tokens: [( name: "prop", note: "a thing" )]),
    effects: (tokens: []),
    look: (tokens: []),
    surfaces: (tokens: [( name: "worktop", note: "a top" )]),
    capabilities: (tokens: []),
    edge: (tokens: [( name: "wall", note: "a solid run-face" )]),
    anchor: (tokens: []),
)"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));

        std::fs::write(
            emerge.join("project.ron"),
            "(\n    version: 1,\n    note: None,\n    divisions: 1,\n    patches: [],\n)",
        )
        .unwrap_or_else(|e| panic!("{e}"));

        Fixture { dir, descriptors: Vec::new(), placements: Vec::new(), compositions: Vec::new() }
    }

    /// **A pack of meshes on disk**, none of them in the library — i.e. import candidates.
    pub fn pack(self, pack: &str, meshes: &[&str]) -> Fixture {
        let at = self.dir.join("assets").join(pack);
        std::fs::create_dir_all(&at).unwrap_or_else(|e| panic!("{e}"));
        for m in meshes {
            std::fs::write(at.join(format!("{m}.glb")), glb(0.6, 0.5, 0.6))
                .unwrap_or_else(|e| panic!("{e}"));
        }
        self
    }

    /// A library entry naming a mesh in `pack`. The mesh is written too, so the entry resolves.
    pub fn descriptor(self, id: &str, pack: &str) -> Fixture {
        self.sunk_descriptor(id, pack, 0.0)
    }

    /// The same, recessed into its own floor by `y_offset` metres — `emerge_core::stack::datum`'s
    /// ordinary case, and the thing a backdrop has to sit under.
    pub fn sunk_descriptor(mut self, id: &str, pack: &str, y_offset: f32) -> Fixture {
        let at = self.dir.join("assets").join(pack);
        std::fs::create_dir_all(&at).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(at.join(format!("{id}.glb")), glb(1.0, 0.5, 1.0))
            .unwrap_or_else(|e| panic!("{e}"));
        self.descriptors.push(format!(
            r#"        (
            id: "{id}",
            mesh: Some("{pack}/{id}.glb"),
            align: ( scale: None, stretch_y: None, y_offset: Some({y_offset:.4}), pivot: None, front: None ),
            extent: ( footprint: Some((1.0, 1.0)), height: Some(0.5) ),
            mount: Some(OnFloor),
            clearance: [],
            offers: ( surfaces: [], sockets: [] ),
            kind: ["prop"],
            effects: [],
            look: [],
            subgrid: None,
            note: None,
            placement: ( rooms: [], group: None ),
        ),"#
        ));
        self
    }

    /// Place a descriptor already added by [`Fixture::descriptor`], minting `<id>@<n>`.
    pub fn place(self, id: &str, at: (f32, f32)) -> Fixture {
        let n = self.placements.len();
        self.place_as(&format!("{id}@{n}"), id, at)
    }

    /// Place one under an id you choose — for the tests that are about id minting.
    pub fn place_as(mut self, row: &str, id: &str, at: (f32, f32)) -> Fixture {
        self.placements.push(format!(
            r#"        (
            id: "{row}",
            descriptor: "{id}",
            at: ({:.1}, {:.1}),
            yaw: 0.0,
            tip: (0, 0),
            lift: 0.0,
            on: None,
            owned: false,
            owned_because: None,
            note: None,
            patch: None,
        ),"#,
            at.0, at.1
        ));
        self
    }

    /// **A reusable group of two members**, both descriptors already added.
    ///
    /// `Anchored` — it claims no tile, so it has no boundary for anything to abut and needs no
    /// derived edge interface. That is the simplest thing a stamp can be, which is what a test about
    /// stamping wants.
    pub fn composition(mut self, id: &str, members: &[(&str, &str, (f32, f32))]) -> Fixture {
        // **Sorted by member id**, which the schema requires rather than prefers: one group must
        // have one encoding, or two authors building the same thing produce diffs that differ
        // without meaning to. `Composition::validate_shape` refuses otherwise, and names the order.
        let mut members = members.to_vec();
        members.sort_by(|a, b| a.0.cmp(b.0));
        let rows: Vec<String> = members
            .iter()
            .map(|(member, descriptor, at)| {
                format!(
                    r#"                (
                    id: "{member}",
                    body: Descriptor( id: "{descriptor}", tip: (0, 0), on: None, patch: None ),
                    at: ({:.1}, {:.1}),
                    yaw: 0.0,
                    lift: 0.0,
                    of_fingerprint: None,
                    note: None,
                ),"#,
                    at.0, at.1
                )
            })
            .collect();
        self.compositions.push(format!(
            r#"        (
            id: "{id}",
            envelope: Anchored,
            note: None,
            members: [
{}
            ],
            locations: [],
        ),"#,
            rows.join("\n")
        ));
        self
    }

    /// Write the library and the map, and hand back the root [`crate::harness::build_headless`] opens.
    pub fn build(self, map: &str) -> PathBuf {
        let emerge = self.dir.join("assets/emerge");
        std::fs::write(
            emerge.join("library.ron"),
            format!(
                "(\n    version: 1,\n    note: None,\n    descriptors: [\n{}\n    ],\n)",
                self.descriptors.join("\n")
            ),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        if !self.compositions.is_empty() {
            std::fs::write(
                emerge.join("compositions.ron"),
                format!(
                    "(\n    version: 1,\n    note: None,\n    compositions: [\n{}\n    ],\n)",
                    self.compositions.join("\n")
                ),
            )
            .unwrap_or_else(|e| panic!("{e}"));
        }
        std::fs::write(
            emerge.join(format!("{map}.map.ron")),
            format!(
                "(\n    version: 1,\n    name: \"{map}\",\n    origin: (0.0, 0.0, 0.0),\n    \
                 bounds: (16.0, 3.0, 16.0),\n    placements: [\n{}\n    ],\n    stamps: [],\n    \
                 locations: [],\n)",
                self.placements.join("\n")
            ),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        self.dir
    }
}
