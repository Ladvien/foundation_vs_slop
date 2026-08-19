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
    /// **Kits beside the default one**, as `(directory name, library rows)`. See [`Fixture::kit`].
    kits: Vec<(String, Vec<String>)>,
    /// The ids each of those kits was asked for, so the binding can name the namespace they carry
    /// rather than guessing it from the directory.
    ///
    /// The default kit is in here too, under [`DEFAULT_KIT`]: a fixture may call
    /// `.descriptor("site/wall", ..)`, and binding that library as `furniture` is refused at load —
    /// correctly, since the directory says one thing and the ids say another.
    ids: Vec<(String, Vec<String>)>,
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

/// **Where a fixture's unnamed descriptors live.**
///
/// The project root stopped being a kit on 2026-08-16, so `Fixture::descriptor` needs a kit to put
/// them in. `furniture` is what the shipped project calls the same set — 75 flat ids — so a fixture
/// and the real thing agree about where an unnamespaced library lives.
pub const DEFAULT_KIT: &str = "furniture";

/// The namespace a fixture kit is bound as: whatever its ids carry, or its own directory name when
/// they carry none.
///
/// **The same rule `kits::bound_library` verifies**, applied where the file is written so the two
/// cannot disagree — a fixture binding `greybox` for a library of `site/*` would be refused at open,
/// which is right for a real project and useless as a fixture.
fn namespace_of(dir: &str, ids: &[String]) -> String {
    ids.iter()
        .find_map(|id| id.split_once('/').map(|(ns, _)| ns.to_owned()))
        .unwrap_or_else(|| dir.to_owned())
}

/// **One library row, and the mesh it names**, written under `pack` inside `root`.
///
/// The file is named for the id's **last segment**, so `site/floor` writes `ozea/floor.glb` rather
/// than a `site/` subdirectory nobody asked for — the shape the shipped kits actually have, where
/// the namespace is in the id and never in the mesh path.
///
/// Free rather than a method, because two callers want it: the root kit's builder pushes the row
/// onto its own list, and [`Fixture::kit`] collects rows for a directory of its own.
fn descriptor_row(root: &Path, id: &str, pack: &str, y_offset: f32) -> String {
    let stem = id.rsplit('/').next().unwrap_or(id);
    let at = root.join("assets").join(pack);
    std::fs::create_dir_all(&at).unwrap_or_else(|e| panic!("cannot make {at:?}: {e}"));
    std::fs::write(at.join(format!("{stem}.glb")), glb(1.0, 0.5, 1.0))
        .unwrap_or_else(|e| panic!("cannot write {stem}.glb: {e}"));
    format!(
        r#"        (
            id: "{id}",
            mesh: Some("{pack}/{stem}.glb"),
            align: ( scale: None, stretch_y: None, y_offset: Some({y_offset:.4}), pivot: None, front: None ),
            extent: ( footprint: Some((1.0, 1.0)), height: Some(0.5) ),
            mount: Some(OnFloor),
            clearance: [],
            offers: ( surfaces: [], sockets: [] ),
            kind: ["prop"],
            effects: ["inert"],
            look: ["plain"],
            subgrid: None,
            note: Some("a fixture piece"),
            placement: ( rooms: [], group: None ),
        ),"#
    )
}

impl Fixture {
    /// An empty project: a vocabulary, an empty library, a policy, and the font.
    pub fn new(name: &str) -> Fixture {
        // A unique directory per test, since these run in parallel. The process id and the test's
        // own name are enough — no clock, so a rerun is reproducible.
        let dir =
            std::env::temp_dir().join(format!("emerge-fixture-{}-{name}", std::process::id()));
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
    effects: (tokens: [( name: "inert", note: "does nothing" )]),
    look: (tokens: [( name: "plain", note: "unremarkable" )]),
    surfaces: (tokens: [( name: "worktop", note: "a top" )]),
    capabilities: (tokens: []),
    edge: (tokens: [( name: "wall", note: "a solid run-face" )]),
    slot: (tokens: []),
)"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));

        std::fs::write(
            emerge.join("project.ron"),
            "(\n    version: 2,\n    note: None,\n    patches: [],\n)",
        )
        .unwrap_or_else(|e| panic!("{e}"));

        Fixture {
            dir,
            descriptors: Vec::new(),
            placements: Vec::new(),
            compositions: Vec::new(),
            kits: Vec::new(),
            ids: Vec::new(),
        }
    }

    /// **A kit beside the root one**, at `assets/emerge/<name>/` — a `library.ron` providing `ids`
    /// and the `project.ron` `policy::layered_library` requires of every kit.
    ///
    /// Every other helper here writes into the root kit, because until 2026-08-16 a fixture could
    /// only ever make one — so nothing in the suite had two, and multi-kit behaviour was pinned by
    /// a single asset-contract test reading the shipped corpus.
    ///
    /// **Ids are written verbatim, which is the point.** Calling this twice with the same `ids` and
    /// different `name`s builds a **re-skin pair**: two directories providing one namespace, which
    /// is what `site/` and `site_greybox/` are on disk and the case every question about deleting,
    /// binding or resolving a kit turns on.
    pub fn kit(mut self, name: &str, pack: &str, ids: &[&str]) -> Fixture {
        let dir = self.dir.join("assets/emerge").join(name);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("cannot make {dir:?}: {e}"));
        let rows = ids
            .iter()
            .map(|id| descriptor_row(&self.dir, id, pack, 0.0))
            .collect();
        self.ids
            .push((name.to_owned(), ids.iter().map(|s| (*s).to_owned()).collect()));
        self.kits.push((name.to_owned(), rows));
        self
    }

    /// **Write the project's binding by hand**, for the tests that are about binding itself.
    ///
    /// [`Self::build`] binds every kit as the namespace its ids carry, which is right for the
    /// ordinary case and impossible for a **re-skin pair**: two directories providing `site/*`
    /// cannot both be bound, because that is the ambiguity binding exists to resolve. A test about
    /// the pair binds one at a time, which is what a real project does.
    pub fn bind(root: &Path, binds: &[(&str, &str)], authoring: &str) {
        let list = binds
            .iter()
            .map(|(ns, dir)| format!("(namespace: \"{ns}\", dir: \"{dir}\")"))
            .collect::<Vec<_>>()
            .join(", ");
        let path = root.join("assets/emerge/kits.ron");
        std::fs::write(
            &path,
            format!(
                "(version: {}, bind: [{list}], authoring: Some(\"{authoring}\"))",
                emerge_core::kits::KITS_VERSION
            ),
        )
        .unwrap_or_else(|e| panic!("{path:?}: {e}"));
    }

    /// **A descriptor of a stated footprint**, for a test about what the footprint *does*.
    ///
    /// [`Self::descriptor`] writes a 1 x 1 m piece, which is the ordinary case and exactly the size
    /// that cannot show an envelope growing. This states the number instead of measuring whatever
    /// the generated mesh happens to be.
    pub fn sized_descriptor(mut self, id: &str, pack: &str, w: f32, d: f32) -> Fixture {
        self = self.descriptor(id, pack);
        if let Some(last) = self.descriptors.last_mut() {
            let was = "footprint: Some((1.0, 1.0))";
            assert!(
                last.contains(was),
                "the fixture's descriptor shape changed under this helper"
            );
            *last = last.replace(was, &format!("footprint: Some(({w}, {d}))"));
        }
        self
    }

    /// **A piece that must sit on something** — `mount: OnSurface(class)`.
    ///
    /// The shape every fixture, lamp and screen in a real kit has, and the one a tile refuses when
    /// nothing under it offers the class.
    pub fn mounted_descriptor(mut self, id: &str, pack: &str, class: &str) -> Fixture {
        self = self.descriptor(id, pack);
        if let Some(last) = self.descriptors.last_mut() {
            let was = "mount: Some(OnFloor)";
            assert!(
                last.contains(was),
                "the fixture's descriptor shape changed under this helper"
            );
            *last = last.replace(
                was,
                &format!("mount: Some(OnSurface( class: \"{class}\" ))"),
            );
        }
        self
    }

    /// **A piece that offers a surface** — a desk, a table, a shelf.
    ///
    /// The other half of the pair: without one in the library, a refusal about a missing host has
    /// nothing true to point at.
    pub fn surface_descriptor(mut self, id: &str, pack: &str, class: &str) -> Fixture {
        self = self.descriptor(id, pack);
        if let Some(last) = self.descriptors.last_mut() {
            let was = "offers: ( surfaces: [], sockets: [] )";
            assert!(
                last.contains(was),
                "the fixture's descriptor shape changed under this helper"
            );
            *last = last.replace(
                was,
                &format!("offers: ( surfaces: [\"{class}\"], sockets: [] )"),
            );
        }
        self
    }

    /// **A `slot` token, so a tile can declare a hole.**
    ///
    /// `new` writes an empty slot axis, which is the honest default: a project that has not grown
    /// one refuses `Shift+Enter` by name rather than inventing a token. A test about holes needs
    /// one, and rewriting the file is how any other axis would be set too.
    /// **Declare edge tokens**, so a test can exercise both sides of the derivation's commit door.
    ///
    /// The fixture ships one token, `wall`, which is deliberately *not* what
    /// `adjacency::derive_edges` names — so the refusal branch is the default and a test has to opt
    /// in to the accepting one.
    pub fn edge_tokens(self, names: &[&str]) -> Fixture {
        let at = self.dir.join("assets/emerge/vocab.ron");
        let was =
            std::fs::read_to_string(&at).unwrap_or_else(|e| panic!("cannot read {at:?}: {e}"));
        let one = r#"edge: (tokens: [( name: "wall", note: "a solid run-face" )]),"#;
        assert!(
            was.contains(one),
            "the fixture's edge axis must be the shipped one, or this is a no-op"
        );
        let mut rows = vec![r#"( name: "wall", note: "a solid run-face" )"#.to_owned()];
        for n in names {
            rows.push(format!(r#"( name: "{n}", note: "derived from the mesh" )"#));
        }
        let full = format!("edge: (tokens: [{}]),", rows.join(", "));
        std::fs::write(&at, was.replace(one, &full))
            .unwrap_or_else(|e| panic!("cannot write {at:?}: {e}"));
        self
    }

    pub fn slot_token(self, name: &str) -> Fixture {
        let at = self.dir.join("assets/emerge/vocab.ron");
        let was =
            std::fs::read_to_string(&at).unwrap_or_else(|e| panic!("cannot read {at:?}: {e}"));
        let empty = "slot: (tokens: []),";
        assert!(
            was.contains(empty),
            "the fixture's slot axis must start empty, or this is a no-op"
        );
        let full = format!("slot: (tokens: [( name: \"{name}\", note: \"a hole\" )]),");
        std::fs::write(&at, was.replace(empty, &full))
            .unwrap_or_else(|e| panic!("cannot write {at:?}: {e}"));
        self
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

    /// **A piece that still owes judgement** — no `effects`, no `look`, no note.
    ///
    /// The `Fixture` default is fully judged, because most tests are about placing and composing and
    /// an unjudged piece is invisible to the Tiles palette. This is the other entity: what the VLM
    /// batch is FOR, and what `the_tiles_palette_lists_only_judged_meshes` contrasts against.
    pub fn unjudged_descriptor(mut self, id: &str, pack: &str) -> Fixture {
        self = self.descriptor(id, pack);
        if let Some(last) = self.descriptors.last_mut() {
            *last = last
                .replace(r#"effects: ["inert"],"#, "effects: [],")
                .replace(r#"look: ["plain"],"#, "look: [],")
                .replace(r#"note: Some("a fixture piece"),"#, "note: None,");
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
        self.descriptors
            .push(descriptor_row(&self.dir, id, pack, y_offset));
        match self.ids.iter_mut().find(|(k, _)| k == DEFAULT_KIT) {
            Some((_, v)) => v.push(id.to_owned()),
            None => self
                .ids
                .push((DEFAULT_KIT.to_owned(), vec![id.to_owned()])),
        }
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
    /// A **bounded** group — one that claims a tile, and so the only kind with a lattice to seat in.
    ///
    /// A separate constructor rather than an option on the one below, because `Anchored` and
    /// `Bounded` are two different things a group can be and not two settings of one: an anchored
    /// group presents no interface and has no envelope to be seated inside, which is exactly what
    /// `compose::seated` refuses on.
    pub fn bounded_composition(
        self,
        id: &str,
        size: (f32, f32, f32),
        members: &[(&str, &str, (f32, f32))],
    ) -> Fixture {
        let envelope = format!(
            "Bounded( size: ({:.1}, {:.1}, {:.1}) )",
            size.0, size.1, size.2
        );
        self.composition_with(id, &envelope, members)
    }

    pub fn composition(self, id: &str, members: &[(&str, &str, (f32, f32))]) -> Fixture {
        self.composition_with(id, "Anchored", members)
    }

    fn composition_with(
        mut self,
        id: &str,
        envelope: &str,
        members: &[(&str, &str, (f32, f32))],
    ) -> Fixture {
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
            envelope: {envelope},
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

        // **The project root is not a kit.** `vocab.ron`, `kits.ron`, `compositions.ron` and
        // `maps/` are the project's; every library lives in a kit directory beside them. This used
        // to write a `library.ron` here, which is exactly the conflation the 2026-08-16 split undid.
        //
        // The unnamed descriptors go into `furniture` — the same name the shipped project uses for
        // the same set, so a fixture and the real thing agree about where flat ids live.
        let mut binds = vec![DEFAULT_KIT.to_owned()];
        binds.extend(self.kits.iter().map(|(n, _)| n.clone()));
        for (name, rows) in std::iter::once(&(DEFAULT_KIT.to_owned(), self.descriptors.clone()))
            .chain(self.kits.iter())
        {
            let dir = emerge.join(name);
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{dir:?}: {e}"));
            std::fs::write(
                dir.join("library.ron"),
                format!(
                    "(\n    version: 1,\n    note: None,\n    descriptors: [\n{}\n    ],\n)",
                    rows.join("\n")
                ),
            )
            .unwrap_or_else(|e| panic!("{e}"));
            // *"A project states its policy, even when its policy is nothing"* — a kit missing
            // `project.ron` is not a kit with defaults, it is a kit the editor refuses.
            std::fs::write(
                dir.join("project.ron"),
                "(\n    version: 2,\n    note: None,\n    patches: [],\n)",
            )
            .unwrap_or_else(|e| panic!("{e}"));
        }

        // **Bound as their own names.** A fixture kit built with `Fixture::kit` may carry `site/*`
        // ids and be called something else — that is the re-skin shape — so the binding is verified
        // against the library at load and a mismatch is a refusal, not a silent re-point. Tests
        // that want the pair write their own `kits.ron` on top.
        let bind = binds
            .iter()
            .map(|n| {
                let ids = self
                    .ids
                    .iter()
                    .find(|(k, _)| k == n)
                    .map(|(_, v)| v.as_slice())
                    .unwrap_or(&[]);
                format!("(namespace: \"{}\", dir: \"{n}\")", namespace_of(n, ids))
            })
            .collect::<Vec<_>>()
            .join(", ");
        std::fs::write(
            emerge.join("kits.ron"),
            format!(
                "(version: {}, bind: [{bind}], authoring: Some(\"{DEFAULT_KIT}\"))",
                emerge_core::kits::KITS_VERSION
            ),
        )
        .unwrap_or_else(|e| panic!("{e}"));

        // **One collection, at the project root.** Its absence and an empty one mean the same
        // thing, so a project that stamps nothing writes no file.
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

        let maps = emerge.join("maps");
        std::fs::create_dir_all(&maps).unwrap_or_else(|e| panic!("{maps:?}: {e}"));
        std::fs::write(
            maps.join(format!("{map}.map.ron")),
            format!(
                "(\n    version: 3,\n    name: \"{map}\",\n    origin: (0.0, 0.0, 0.0),\n    \
                 bounds: (16.0, 3.0, 16.0),\n    placements: [\n{}\n    ],\n    stamps: [],\n    \
                 locations: [],\n)",
                self.placements.join("\n")
            ),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        self.dir
    }
}
