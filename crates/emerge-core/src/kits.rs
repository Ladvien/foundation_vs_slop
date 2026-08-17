//! **Which directory provides which namespace** — the project's binding, and the loader that reads
//! it.
//!
//! # A namespace is an interface; a directory is a skin
//!
//! `assets/emerge/site/` and `assets/emerge/site_greybox/` define the **identical** 45 `site/*` ids.
//! They are not two kits with two namespaces: they are two *implementations* of one, which is what
//! `the_site_kit_is_swappable_by_authoring_one_project` exists to prove. So "which pieces does
//! `site/floor` mean" is a question about the **project**, not about either directory, and it is what
//! this file answers.
//!
//! # Declared, then verified
//!
//! `kits.ron` states the binding; loading **checks it against what the directory actually defines**
//! (`Library::namespace`). Declared-only drifts the first time a kit is re-authored; derived-only
//! cannot express the skin pair at all, because both directories answer `site`. Stating it and
//! checking it is the discipline Mesa's symbol files put on separate compilation — Smits, Konat &
//! Visser (`10.48550/arXiv.2002.06183`) — and it is why a summary rather than a whole module is what
//! crosses the boundary here.
//!
//! **Always declared, never declared-only-when-ambiguous.** A requirement that applies in some
//! projects and not others is two paths through one loader, and only one of them gets exercised.
//!
//! # What merging buys, and what catches it going wrong
//!
//! Every bound kit's library is layered with **its own** policy first — patches are local, because
//! `Policy::apply` refuses a rule that matches nothing and a kit's rules name a kit's pieces — and
//! the results are then concatenated into the one library a map resolves against. That is the whole
//! feature: a composition may name `site/wall` and `lab/bench` in one tile, and a map may stamp it
//! without caring which directory either came from.
//!
//! Nothing new guards the merge. `Library::validate` already refuses a duplicate id *"because a map
//! references descriptors by id, so a duplicate makes every reference to it ambiguous"* — which is
//! exactly what binding two providers of one namespace would produce, so the existing rule catches
//! the exact mistake this file exists to prevent.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::composition::Compositions;
use crate::library::Library;
use crate::policy::Policy;

/// Bumped whenever the shape below changes. A mismatch is refused, never migrated.
///
/// `2` added [`Kits::lattice`], which arrived here from [`crate::map::Map`] after one stop in
/// between — see [`Lattice`].
pub const KITS_VERSION: u32 = 2;

/// The binding file, at the project root beside `vocab.ron`.
pub const KITS_FILE: &str = "kits.ron";

/// **The ceiling on both divisions below.**
///
/// Not a number anyone should need: at 8 a face band is 62 mm, finer than the meshes it describes,
/// and a 3 m wall carries 48 x 40 x 8 cells. The ceiling exists because divisions are derived and
/// multiplied by a piece's span, so a typo here is not one absurd tile but every tile at once.
///
/// It bounds [`Lattice::snap_divisor`] for a different reason with the same shape: the ladder
/// squares the divisor, so 8 puts the finest rung at 15.6 mm — below the precision any of this art
/// was authored to.
pub const MAX_DIVISIONS: u32 = 8;

/// **The one grid this project's kits, tiles and maps all agree on.**
///
/// # It has been in three places, and this is the level that works
///
/// It started in each kit's `project.ron`, while a kit *was* a project. That broke the moment a map
/// could draw on several kits: two kits disagreeing about how finely a face is read have faces that
/// cannot be compared, so adjacency stops meaning anything, silently, with both values legal.
///
/// So on 2026-08-16 it moved to [`crate::map::Map`], on the argument that a map has exactly one
/// lattice. The first half of that holds. The second does not survive a project with two maps in
/// it: [`crate::composition::interface`] takes the band count as an argument, so two maps at
/// different band counts give **the same tile two different adjacency contracts** — a kit of tiles
/// is coherent at exactly one band count, and which one cannot be a property of whichever map
/// happens to be open.
///
/// The project is the level at which neither two kits nor two maps can disagree, so the project is
/// what owns it. Found from the other side, while splitting the editor into one door per entity:
/// the door that authors tiles has to derive an interface with no map open at all.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lattice {
    /// **How finely a piece's subgrid of EDGE TOKENS is indexed** — the lattice a face is read on.
    ///
    /// A band is `grid::SNAP / face_bands` on every axis, and a piece spanning N cells gets
    /// `N * face_bands` of them. See [`crate::descriptor::divisions`] for the derivation and
    /// [`crate::descriptor::Subgrid`] for why this is one project-wide number rather than a
    /// per-piece one.
    ///
    /// **Not [`Self::snap_divisor`], deliberately.** One number used to serve both jobs: indexing
    /// edge tokens *and* deciding how finely a member is seated. They belong to different objects.
    /// **Edge tokens belong to the face** — a 2-D component, where a token should be one word per
    /// face however finely the interior is cut. **Space belongs to the volume.** Tying them
    /// together would also mean a kit author editing an unrelated token count silently moved every
    /// existing placement off-lattice.
    ///
    /// Splitting them keeps a deferred migration deferred: edge-token indexing is still blocked on
    /// the edge-versus-corner question, so raising this to seat a sconce would re-author every token
    /// in the kit on a format that may change again. Merrell names the other half of the price —
    /// *"small objects require closely spaced planes while large objects require large volumes,
    /// which together means that many planes must be created"* — a finer face vocabulary buys the
    /// adjacency problem nothing.
    ///
    /// **1 by default**, so a band is `grid::SNAP` itself — the half-metre grid the kits are already
    /// authored on, on which a 3 m wall is 6 bands and a 2.4 m one is 5 layers.
    #[serde(default = "one")]
    pub face_bands: u32,
    /// **How finely a tile divides, once per rung — this project's one spatial lattice.**
    ///
    /// `grid::SnapLevel::Fine` is `grid::TILE / snap_divisor` and `Finer` is `TILE / snap_divisor²`,
    /// so at the default 3 the rungs are 1 m, 333 mm and 111 mm.
    ///
    /// # One number, because a tile and the map it sits on are the same grid
    ///
    /// This used to be two. `seating_divisions` divided `grid::SNAP` and governed how far a *member*
    /// moved inside a tile; another divided `grid::TILE` and governed where a *piece* landed on the
    /// map. Two spatial lattices for one act of placing something, and they did not even agree on
    /// what they divided — so "divide a tile into four" gave eight squares, because the thing being
    /// quartered was the half-metre.
    ///
    /// They are now the same ladder at two scales, which is what makes a tile authored today fit
    /// beside a tile authored last month. Códices et al. (`10.1109/access.2022.3168832`) state the
    /// property this buys: a designer can *"define a passage as n pins wide or tall, **keeping
    /// consistency in the design of the layout of the individual pieces being made separately**"* —
    /// pieces agree by construction rather than by discipline.
    ///
    /// # The centre is a legal position
    ///
    /// Rungs are multiples of the pitch measured from the piece's minimum corner
    /// (`grid::snap_corner`), so nudging out and back returns exactly where it started. Dividing a
    /// tile into cells and seating at cell *centres* would not: at 4 those are 0.125 / 0.375 / 0.625
    /// / 0.875, with nothing in the middle.
    ///
    /// **3 by default**, and note it is not a superset of the old half-metre snap — 0.5 is not a
    /// multiple of a third. A kit authored on halves sets 2, which makes the middle rung exactly
    /// `grid::SNAP`. It does not make a flush verb redundant either: `site/wall` is 0.1 m thick and
    /// sits flush at −0.45, which is a multiple of no rung, because art is authored to look right
    /// rather than to tile.
    #[serde(default = "three")]
    pub snap_divisor: u32,
    /// **How tall a blank tile starts**, in metres.
    ///
    /// A tile is one grid unit of floor and however much headroom the kit is built for, so this is
    /// the kit's storey height. It was read off `Map::bounds.1` — *this map's ceiling* — which meant
    /// the same kit produced differently-shaped blank tiles depending on which map was open, and
    /// meant nothing at all on a door that has no map.
    ///
    /// **4 m by default**, which is what `Map::default`'s ten-by-four-by-ten already gave every
    /// blank tile made so far.
    #[serde(default = "four_metres")]
    pub cell_height: f32,
}

impl Default for Lattice {
    fn default() -> Self {
        Lattice {
            face_bands: one(),
            snap_divisor: three(),
            cell_height: four_metres(),
        }
    }
}

impl Lattice {
    fn validate(&self) -> Result<(), String> {
        if self.snap_divisor < 2 || self.snap_divisor > MAX_DIVISIONS {
            return Err(format!(
                "kits: `snap_divisor` is {}; a tile divides between 2 and {MAX_DIVISIONS} ways. \
                 One leaves no rung between the corners, and past {MAX_DIVISIONS} the finest rung \
                 is smaller than the meshes it positions.",
                self.snap_divisor
            ));
        }
        if self.face_bands == 0 || self.face_bands > MAX_DIVISIONS {
            return Err(format!(
                "kits: `face_bands` is {}; a face reads between 1 and {MAX_DIVISIONS} ways. Zero \
                 leaves every piece without cells, and past {MAX_DIVISIONS} the lattice is finer \
                 than the art.",
                self.face_bands
            ));
        }
        if !(self.cell_height.is_finite() && self.cell_height > 0.0) {
            return Err(format!(
                "kits: `cell_height` is {}; a tile has a real, positive height in metres.",
                self.cell_height
            ));
        }
        Ok(())
    }
}

/// The default for [`Lattice::face_bands`]. A free function because `serde(default = ..)` needs a
/// path.
fn one() -> u32 {
    1
}

/// The default for [`Lattice::snap_divisor`].
fn three() -> u32 {
    3
}

/// The default for [`Lattice::cell_height`].
fn four_metres() -> f32 {
    4.0
}

/// **One namespace, and the directory that provides it.**
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bind {
    /// The `site` in `site/wall_corner` — what a map's ids name.
    pub namespace: String,
    /// A directory under the project root. **Not the namespace**, and the difference is the point:
    /// `site_greybox` provides `site`.
    pub dir: String,
}

/// **The project's kit bindings**, read from `kits.ron`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kits {
    pub version: u32,
    #[serde(default)]
    pub note: Option<String>,
    /// **The grid every kit, tile and map in this project shares** — see [`Lattice`].
    ///
    /// Defaulted rather than required, because the default *is* the grid the shipped kits are
    /// authored on. A project that says nothing gets the half-metre face band and the thirds ladder,
    /// which is what every project meant before the field existed.
    #[serde(default)]
    pub lattice: Lattice,
    /// Every namespace this project can resolve, and where each comes from. File order is load
    /// order, which decides nothing — a duplicate id is refused rather than resolved by position,
    /// so there is no last-wins rule to remember.
    pub bind: Vec<Bind>,
    /// **Where new work lands** when the command line does not say: the `dir` of one of the binds.
    ///
    /// Separate from the binding because it answers a different question. Binding says what
    /// `site/floor` *means*; this says which kit an imported mesh joins and which namespace a new
    /// tile is named in. Every kit is loaded either way — that is what makes a composition able to
    /// cross them.
    ///
    /// `None` **only** when [`Self::bind`] is empty, which is a project nobody has made a kit in
    /// yet — a real state, and the one the chooser exists to get an author out of. Not a default and
    /// not a fallback: the two fields are empty together or full together, and [`Self::validate`]
    /// refuses every other combination.
    #[serde(default)]
    pub authoring: Option<String>,
}

impl Kits {
    pub fn parse(text: &str) -> Result<Kits, String> {
        let k: Kits = ron::from_str(text).map_err(|e| format!("kits: {e}"))?;
        k.validate()?;
        Ok(k)
    }

    pub fn to_ron(&self) -> Result<String, String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("kits: serialize: {e}"))
    }

    /// The bind whose `dir` is [`Self::authoring`]. Present by construction — [`Self::validate`]
    /// refuses a file where it is not — so this is a lookup and not a question.
    pub fn authoring_bind(&self) -> Option<&Bind> {
        let want = self.authoring.as_deref()?;
        self.bind.iter().find(|b| b.dir == want)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != KITS_VERSION {
            return Err(format!(
                "kits: version {} but this build reads {KITS_VERSION}. The binding is a schema, not \
                 a setting — regenerate it rather than editing the number.",
                self.version
            ));
        }
        self.lattice.validate()?;
        for (i, b) in self.bind.iter().enumerate() {
            if !crate::naming::is_snake_case(&b.namespace) {
                return Err(format!(
                    "kits: `{}` is not a usable namespace. A namespace is one snake_case segment — \
                     the `site` in `site/wall_corner`.",
                    b.namespace
                ));
            }
            if !crate::naming::is_snake_case(&b.dir) {
                return Err(format!(
                    "kits: `{}` is not a usable directory name. Kits are snake_case directories \
                     under the project root.",
                    b.dir
                ));
            }
            // **Two directories bound to one namespace is the mistake binding exists to prevent**,
            // not a merge order to resolve. `site` and `site_greybox` are alternatives, and a
            // project picks one.
            if let Some(j) = self.bind.iter().position(|o| o.namespace == b.namespace) {
                if j != i {
                    return Err(format!(
                        "kits: `{}` is bound twice — to `{}` and to `{}`. A namespace is an \
                         interface and a directory is one implementation of it, so a project binds \
                         exactly one. Pick the skin this project uses.",
                        b.namespace, self.bind[j].dir, b.dir
                    ));
                }
            }
            if let Some(j) = self.bind.iter().position(|o| o.dir == b.dir) {
                if j != i {
                    return Err(format!(
                        "kits: `{}` is bound twice, to `{}` and to `{}`. One directory provides one \
                         namespace; a directory answering to two is a library that disagrees with \
                         itself, which `Library::namespace` refuses at load.",
                        b.dir, self.bind[j].namespace, b.namespace
                    ));
                }
            }
        }
        // **Empty together or full together.** A project with no kits has nowhere for work to land
        // and says so; a project with kits must point at one of them, because an `authoring` naming
        // an unbound directory would put an imported mesh in a kit nothing reads — which looks
        // exactly like the import silently failing.
        match (&self.authoring, self.bind.is_empty()) {
            (None, true) => {}
            (None, false) => {
                return Err(format!(
                    "kits: {} kit(s) bound and no `authoring`. New meshes and new tiles have to land \
                     somewhere; name one of them.",
                    self.bind.len()
                ));
            }
            (Some(a), true) => {
                return Err(format!(
                    "kits: `authoring` names `{a}` and nothing is bound. A project with no kits has \
                     nowhere for work to land, and says so by leaving this out."
                ));
            }
            (Some(a), false) if self.authoring_bind().is_none() => {
                return Err(format!(
                    "kits: `authoring` names `{a}`, which is not a bound directory. New meshes and \
                     new tiles have to land in a kit this project actually loads."
                ));
            }
            (Some(_), false) => {}
        }
        Ok(())
    }
}

/// One bound kit, layered.
#[derive(Clone, Debug)]
pub struct KitLayer {
    /// The directory it was read from.
    pub dir: PathBuf,
    /// The namespace it provides, as bound **and** as verified against its own ids.
    pub namespace: String,
    /// `library.ron` exactly as parsed — what an editor writes back for **this** kit.
    pub measured: Library,
    /// [`Self::measured`] with this kit's own policy applied.
    pub library: Library,
    pub policy: Policy,
}

/// **A project's whole loadable world**: every bound kit, the one library they merge into, and the
/// compositions authored over them.
#[derive(Clone, Debug)]
pub struct Bound {
    /// In `kits.ron` order.
    pub kits: Vec<KitLayer>,
    /// **Every bound kit's layered library, concatenated.** What a map resolves against, what the
    /// palette shows, and what a composition may draw from regardless of which kit authored it.
    pub library: Library,
    /// The project's compositions — one collection, not one per kit. Empty when the project has
    /// none, which means exactly what a file with none in it means.
    pub compositions: Compositions,
}

/// **Read a project**: every bound kit, merged, with its compositions validated against the result.
///
/// `project` is the directory holding `kits.ron` — `assets/emerge`, not a kit inside it.
pub fn bound_library(project: &Path, kits: &Kits) -> Result<Bound, String> {
    let mut layers = Vec::with_capacity(kits.bind.len());
    let mut descriptors = Vec::new();

    for b in &kits.bind {
        let dir = project.join(&b.dir);
        let layered = crate::policy::layered_library(&dir)?;

        // **Declared, then verified.** A binding that says `site` about a directory defining
        // `lab/*` is a project that will resolve neither, and the failure without this check is a
        // missing-descriptor error somewhere far away naming a piece nobody moved.
        //
        // A library carrying no namespace at all contradicts nothing — the furniture kit's 75 ids
        // are flat — so it is bound on the project's word. That is the same rule
        // `Project::namespace` follows and the same reason: an unnamespaced library has nothing to
        // disagree with.
        match layered.measured.namespace()? {
            Some(ns) if ns != b.namespace => {
                return Err(format!(
                    "{}: bound as `{}` but its pieces are `{ns}/*`. A directory is a skin and the \
                     namespace is the interface it implements — bind it as `{ns}`, or point `{}` at \
                     the directory that provides it.",
                    dir.display(),
                    b.namespace,
                    b.namespace
                ));
            }
            _ => {}
        }

        descriptors.extend(layered.library.descriptors.iter().cloned());
        layers.push(KitLayer {
            dir,
            namespace: b.namespace.clone(),
            measured: layered.measured,
            library: layered.library,
            policy: layered.policy,
        });
    }

    let library = Library {
        version: crate::library::LIBRARY_VERSION,
        note: Some(format!(
            "Derived: {} bound kit(s) merged. Not a file — edit a kit's own library.ron.",
            layers.len()
        )),
        descriptors,
    };
    // **The merge's only guard, and it is the one that was already there.** A duplicate id across
    // two kits is precisely an unbound skin pair, and `validate` already refuses it *"because a map
    // references descriptors by id"*.
    library.validate().map_err(|e| {
        format!(
            "{}: {e} — two bound kits define it. Check `{KITS_FILE}`: a namespace binds to one \
             directory, and two skins of one kit cannot both be loaded.",
            project.display()
        )
    })?;

    // **Compositions are the project's, not a kit's**, which is the whole point of binding: a tile
    // may seat `site/wall` beside `lab/bench`, and neither kit could hold it. Their absence is not a
    // degraded mode — a project that stamps nothing has no file, and that is the same state as a
    // file holding no compositions.
    let comp_path = project.join(Compositions::FILE);
    let compositions = if comp_path.exists() {
        let text = std::fs::read_to_string(&comp_path)
            .map_err(|e| format!("{}: {e}", comp_path.display()))?;
        Compositions::parse(&text).map_err(|e| format!("{}: {e}", comp_path.display()))?
    } else {
        Compositions {
            version: crate::composition::COMPOSITIONS_VERSION,
            ..Default::default()
        }
    };
    crate::composition::validate(&compositions.compositions, &library)
        .map_err(|e| format!("{}: {e}", comp_path.display()))?;

    Ok(Bound {
        kits: layers,
        library,
        compositions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kits(bind: &[(&str, &str)], authoring: &str) -> Kits {
        Kits {
            version: KITS_VERSION,
            note: None,
            lattice: Lattice::default(),
            bind: bind
                .iter()
                .map(|(n, d)| Bind {
                    namespace: (*n).to_owned(),
                    dir: (*d).to_owned(),
                })
                .collect(),
            authoring: (!authoring.is_empty()).then(|| authoring.to_owned()),
        }
    }

    #[test]
    fn a_binding_round_trips() {
        let k = kits(&[("site", "site_greybox"), ("furniture", "furniture")], "furniture");
        let text = k.to_ron().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Kits::parse(&text).unwrap_or_else(|e| panic!("{e}")), k);
    }

    /// **A namespace binds to exactly one directory.** Two providers is the state binding exists to
    /// resolve, so offering both is not a merge order — it is the question left unanswered.
    #[test]
    fn one_namespace_cannot_bind_to_two_directories() {
        let text = kits(&[("site", "site"), ("site", "site_greybox")], "site")
            .to_ron()
            .unwrap_or_else(|e| panic!("{e}"));
        let e = Kits::parse(&text).err().unwrap_or_default();
        assert!(e.contains("bound twice"), "{e}");
        assert!(e.contains("site_greybox"), "and it names both skins: {e}");
    }

    /// The other direction: one directory answering to two namespaces is a library disagreeing with
    /// itself, which `Library::namespace` refuses — so the binding refuses to ask for it.
    #[test]
    fn one_directory_cannot_provide_two_namespaces() {
        let text = kits(&[("site", "muddle"), ("lab", "muddle")], "muddle")
            .to_ron()
            .unwrap_or_else(|e| panic!("{e}"));
        let e = Kits::parse(&text).err().unwrap_or_default();
        assert!(e.contains("bound twice"), "{e}");
    }

    /// **Work has to land somewhere the project loads.** An `authoring` naming an unbound directory
    /// would put an imported mesh in a kit nothing reads, which looks exactly like the import
    /// silently failing.
    #[test]
    fn authoring_must_name_a_bound_kit() {
        let text = kits(&[("site", "site")], "furniture")
            .to_ron()
            .unwrap_or_else(|e| panic!("{e}"));
        let e = Kits::parse(&text).err().unwrap_or_default();
        assert!(e.contains("not a bound directory"), "{e}");
    }

    /// **A project with no kits is a state, not a failure** — it is where the chooser starts, and
    /// the `+ new kit` row is the way out of it. What is refused is the pair disagreeing.
    #[test]
    fn a_project_with_no_kits_has_nowhere_for_work_to_land_and_says_so() {
        let text = kits(&[], "").to_ron().unwrap_or_else(|e| panic!("{e}"));
        Kits::parse(&text).unwrap_or_else(|e| panic!("an empty project is openable: {e}"));

        let text = kits(&[], "furniture").to_ron().unwrap_or_else(|e| panic!("{e}"));
        let e = Kits::parse(&text).err().unwrap_or_default();
        assert!(e.contains("nothing is bound"), "{e}");

        let text = kits(&[("site", "site")], "").to_ron().unwrap_or_else(|e| panic!("{e}"));
        let e = Kits::parse(&text).err().unwrap_or_default();
        assert!(e.contains("no `authoring`"), "{e}");
    }

    /// **A project that says nothing about its grid gets the grid the shipped kits are authored
    /// on** — which is what every project meant before the field existed, so the migration is the
    /// `serde(default)` and nothing else.
    #[test]
    fn a_project_that_states_no_lattice_gets_the_authored_one() {
        let text = r#"(version: 2, bind: [(namespace: "f", dir: "f")], authoring: Some("f"))"#;
        let k = Kits::parse(text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(k.lattice, Lattice::default());
        assert_eq!(k.lattice.face_bands, 1, "the half-metre grid the kits use");
        assert_eq!(k.lattice.snap_divisor, 3, "the thirds ladder");
        assert_eq!(k.lattice.cell_height, 4.0, "what Map::default's ceiling already gave a tile");
    }

    /// The range checks came here with the fields, so a typo is still one refusal rather than every
    /// tile at once.
    #[test]
    fn a_lattice_outside_the_ceiling_is_refused() {
        let bad = |set: fn(&mut Lattice)| {
            let mut k = kits(&[("f", "f")], "f");
            set(&mut k.lattice);
            let text = ron::ser::to_string(&k).unwrap_or_default();
            Kits::parse(&text).err().unwrap_or_default()
        };
        assert!(bad(|l| l.face_bands = 0).contains("a face reads between 1 and"));
        assert!(bad(|l| l.face_bands = MAX_DIVISIONS + 1).contains("a face reads between 1 and"));
        assert!(bad(|l| l.snap_divisor = 1).contains("a tile divides between 2 and"));
        assert!(bad(|l| l.snap_divisor = MAX_DIVISIONS + 1).contains("a tile divides between 2 and"));
        assert!(bad(|l| l.cell_height = 0.0).contains("real, positive height"));
        assert!(bad(|l| l.cell_height = f32::NAN).contains("real, positive height"));
    }

    #[test]
    fn a_binding_from_another_schema_is_refused() {
        let mut k = kits(&[("site", "site")], "site");
        k.version = KITS_VERSION + 1;
        let text = k.to_ron().unwrap_or_else(|e| panic!("{e}"));
        let e = Kits::parse(&text).err().unwrap_or_default();
        assert!(e.contains("this build reads"), "{e}");
    }
}
