# Kitbashing and Kit-Driven Game Editors

**Date:** 2026-08-08
**Sources:** home-still local corpus (`distill_search`, `abstract_search`, `catalog_read`) + web
**Scope:** what kitbashing actually is, the research underneath it, and how to build an editor that consumes kits

---

## 0. The framing problem (read this first)

"Kitbashing" gets used for two related but genuinely different disciplines, and conflating them is the main reason people build the wrong editor:

| | **Asset kitbashing** | **Modular kit level design** |
|---|---|---|
| Unit of work | One hero object (a mech, a building, a prop) | A whole level or environment |
| Pieces | Greebles, panels, bolts, pipes — arbitrary, overlapping | Walls, floors, corners, doors — dimensioned, non-overlapping |
| Alignment | Eyeball + artistic judgement | Grid, pivot, and socket contracts |
| Output | A single merged mesh | A scene graph of instanced references |
| Editor needs | Fast library browse, transform gizmos, boolean/merge | Snapping solver, adjacency rules, instancing, serialization |

Both are legitimate. But **"set up a custom editor to use kits" is almost entirely the right-hand column.** The left column is a modeling problem you solve in Blender/ZBrush; the right column is a tools-engineering problem.

The rest of this doc treats them separately, then shows where they meet.

---

## 1. Kitbashing: the practice

### Origins and definition

The term comes from physical scale modeling — hobbyists cannibalizing plastic model kits for parts to build something new. Digital kitbashing kept the idea and dropped the scarcity: you assemble environments, architecture, vehicles, and characters from libraries of pre-built parts rather than modeling each element from scratch. <cite index="1-1">What was a hobbyist workaround is now a standard production pipeline in both AAA studios and indie teams.</cite>

The canonical workflow is four steps: <cite index="2-1">component selection from a library, modification/scaling to fit the target design, composition and arrangement in the scene, and then detailing.</cite>

### Why studios build their own kits

The industry pattern is to build kits **in-house** rather than only buying them. <cite index="1-1">Making your own modular elements — doors, windows, machinery, props — that get reused across levels gives visual consistency, avoids licensing disputes over third-party content, and preserves a studio's distinct look.</cite> Gnomon's *Designing a Custom Kitbash Library* (Gavin Manners) is the standard reference on this: <cite index="3-1">block out components that work across multiple compositions, build them as modular textured parts that combine into sub-assemblies, and organize them for production reuse.</cite>

That "sub-assembly" idea matters. A good kit is hierarchical: parts → sub-assemblies → assemblies. Flat kits of 500 unrelated meshes are far less useful than 80 parts with clean combination rules.

### What a well-formed commercial kit looks like

KitBash3D is the reference implementation of kit hygiene, and it's worth stealing their spec: <cite index="7-1">clean geometry, organized scene files, non-overlapping UVs, tileable textures, PBR materials, native formats per DCC, standardized relative texture paths, and geometry deliberately split into logical modular parts so you can swap a roof from one building onto another.</cite>

That last property — *logical parts* — is the thing most home-grown kits get wrong. If your wall mesh includes the window, the window can never be swapped.

### The honest caveat

Kitbashing has a real failure mode: it produces environments that read as generic because everyone is drawing from the same asset packs. The efficiency argument is strong, the originality argument is not automatic. In-house kits are the mitigation.

---

## 2. What the research says (home-still corpus)

Your local corpus is unusually well-stocked here. Kitbashing has an academic name — **assembly-based modeling** — and a 20-year literature.

### 2.1 Assembly-based 3D modeling

**Chaudhuri, Kalogerakis, Guibas & Koltun (2011), "Probabilistic Reasoning for Assembly-Based 3D Modeling"** — `10.1145/1964921.1964930`, in your corpus.

This is the single most relevant paper you have. Its framing: modeling reduces to *selection and placement of components* rather than authoring new geometry. The central problem it identifies is the one every kit editor eventually hits:

> The hard part isn't placement. It's **surfacing the right component out of a large heterogeneous library at the current moment in the modeling session.**

Prior approaches used text search or geometric matching; neither accounts for semantic or stylistic relationships between what's already on the canvas and what's in the library. Their fix: train a probabilistic graphical model over a labeled shape repository that captures component categories, adjacency, symmetry, and geometric style, then use inference to dynamically re-rank the palette as the model evolves. The UI is explicitly modeled on the Spore creature creator — categories of parts (heads, arms, torsos) — but with the presented categories updating live.

Two design lessons transfer directly to a custom editor:

1. **The palette is the product.** A static grid of thumbnails is the naive baseline they beat. Context-aware suggestion measurably increased relevance.
2. **Parts need labels and hierarchy.** They preprocess the repository semi-automatically into labeled components (torso → upper torso / lower torso) using learned mesh segmentation, rather than requiring hand-crafted parts.

The lineage from this paper is worth knowing: Funkhouser et al.'s *Modeling by Example* (2004, `10.1145/1015706.1015775`) pioneered cut-and-glue assembly with search-based retrieval; Chaudhuri & Koltun's *Data-Driven Suggestions* (2010, `10.1145/1882261.1866205`) added purely geometric suggestion; Kraevoy et al. (2007) did part interchange but assumed all shapes share a component count.

**Neither of the first two is in your corpus** — see §7.

### 2.2 Automatic part decomposition

**PartField (Liu, Uy, Xiang, Su, Fidler, Sharp & Gao, NVIDIA, 2025)** — `10.48550/arXiv.2504.11451`, in your corpus.

Feedforward model that predicts a part-based 3D feature field for any shape, no templates or text prompts required. Cluster the field and you get a part decomposition; use agglomerative clustering over mesh face adjacency and you get crisp, connectivity-respecting parts plus a **hierarchical tree** you can drill into interactively. They report up to 20% better accuracy than open-world part-segmentation baselines and orders of magnitude faster runtime (a single forward pass vs. the minutes-to-hours of per-shape optimization methods like SAMPart3D or Ultrametric).

This is the automation layer for kit *creation*: point it at a mesh corpus, get parts, get a kit. It's also, notably, roughly what your `trellis2:segment_mesh` tool does.

### 2.3 Constraint-based assembly (the procedural layer)

Your corpus has strong coverage of the algorithm family that turns a kit into a generator:

- **Merrell (2009 / `merrell09`)** — model synthesis with explicit geometric constraint types: dimensional (a stair step is a fixed height), algebraic (length = 2× height), incidence (supporting non-trihedral vertices, so pyramids and octahedra become generatable), connectivity (no isolated road loops), and large-scale (a voxel grid specifying what should appear where). This is the constraint vocabulary a serious kit editor should expose.
- **Kim, Hahn, Kim & Kang (2020), "Graph Based Wave Function Collapse"** — `10.1587/transinf.2019edp7295`. Generalizes WFC off the square grid onto arbitrary connection graphs (Voronoi cells, etc.) by dropping direction from the propagator. Relevant if your kit isn't grid-aligned. Also the cleanest explanation in the corpus of WFC's two models (overlapping vs. simple tiled) and the observe/propagate loop.
- **Cooper (2022), "Sturgeon"** — `10.1609/aiide.v18i1.21944`. Tile-based generation via MaxSAT rather than WFC's greedy propagation, which buys you *reachability constraints* — guaranteeing the generated level is completable. It also introduces a clean separation worth copying: a **functional grid** (gameplay: solid, goal) and an **image grid** (looks: brick, stone), generated either simultaneously or sequentially. Sequential is faster because pathfinding doesn't need to know what a tile looks like. Plus **tags** as a layer of indirection between the two.
- **Heese (2024)** — `10.1109/mcg.2024.3447775`. Quantum WFC; mostly interesting for its worked platformer ruleset (8 tiles, 15 rules) as a minimal example of a hand-authored adjacency spec.

Note the historical thread: WFC (Gumin, 2016) is a rediscovery of Merrell's model synthesis (2007). Karth & Smith's framing — that WFC is constraint solving in the wild — is the right mental model.

### 2.4 Mixed-initiative authoring

`pcgbook-ch11-mixed-initiative-content-creation` and `fdg2014_fdg2014_paper_37` (Liapis, Yannakakis & Alexopoulos, "Mixed-Initiative Co-Creativity") are directly about the editor question.

Their scale runs from pure CAD (human drives, computer executes) to interactive evolution (computer proposes, human reacts). The FDG paper is pointed about where conventional tools sit: level editors like Bethesda's Creation Kit and UDK limit the computer's initiative to interpolation, pathfinding, and rendering — efficient, but the human is the sole creative driver. At the other end, generators like SpeedTree or Oblige limit the human to setting parameters before the run and editing after it. Neither is co-creation.

The exemplars to study: **Tanagra** (constraint solving completes human-sketched platformer levels and re-guarantees playability as you edit), **Sentient Sketchbook** (low-res strategy-map sketching seeds an evolutionary generator; low resolution reduces designer strain *and* makes pattern detection easier), and **Ropossum** (Cut the Rope levels, with playability testing and partial regeneration in the loop).

If you want your editor to be more than a placement tool, this is the design space to target.

### 2.5 The AI-generation reality check

**Begemann & Hutson (2026), "Prompted Props, Human Pipelines"** — `10.55677/ijhrsss/15-2026-vol03i06`, in your corpus. Directly relevant given your TRELLIS setup.

Practice-led comparison of a stylized fantasy tavern built two ways: human-authored in Blender vs. AI-assisted with Meshy 6 and Hunyuan 3D, against a fixed asset list. Headline numbers: **238 minutes AI-assisted vs. 716 minutes human-authored** for first-pass production.

But the time saving came with what they call technical debt: dense triangulated geometry, fragmented UVs, inconsistent prompt adherence, material-editing constraints, clipping during placement, and texture integrity loss under decimation. Their thesis is blunt and worth internalizing — **AI doesn't eliminate asset-production labor, it relocates it to later, more technical stages.**

They propose a six-stage human-in-the-loop pipeline: ideation → curation → technical audit (tri count, topology type, UV coherence, material org, scale, pivot, texture res, export compat) → optimization (retopo, responsible decimation, bake, UV rebuild, LODs) → engine validation (frame rate, draw calls, collision, lighting, memory) → documentation (licensing, prompts, settings, revisions).

Their concept of **asset-readiness** — the combined visual, technical, and procedural qualities that determine whether an object can enter a pipeline — is exactly the gate your editor's importer should enforce. And their recommendation for studios is a repository gatekeeping step at the moment generated assets enter source control.

---

## 3. The kit contract (what to standardize before writing any code)

Every workable kit system rests on a small set of conventions. Decide these *first*; retrofitting them is expensive.

### 3.1 Grid and scale

The classic Gamasutra/Game Developer piece on modular art is still the best statement of the principle: <cite index="8-1">the key to modular art that level designers can actually use is the grid and the pivot points, and model dimensions should land on powers of two — a wall segment should not be 248 × 240 units.</cite>

Two grid conventions exist. <cite index="8-1">Meter grids (Far Cry, Max Payne) use round values like 1/5/10/20/50m. Unit grids (Doom, Unreal) put the player at ~96 units tall — about 53 units per meter — with spacing at powers of two, mirroring texture resolutions.</cite> UE4 onward moved to centimeters, though <cite index="13-1">the power-of-two behavior is still available via Editor Preferences → Viewports → "Use Power of Two Snap Size."</cite>

Practical checklist from a working modular-kit build: <cite index="9-1">model in meters and export in centimeters for UE; set the modeling grid to 0.25m to match snapping; exaggerate walls and doorways because they need to be larger in-game for collision and camera clearance; record the character's max step height, jump height, and climb height as hard constraints; model cylinders with a side count divisible by 4 so 90° rotations stay clean.</cite>

**Whatever you pick, write it down as a numeric constant your editor validates against on import.**

### 3.2 Pivots

Pivot placement is what makes snapping work at all. Rule: put the pivot at the **connection origin**, not the visual center. A wall's pivot goes at the bottom-left of its footprint, on the grid; a door's at the floor plane center of its opening. Consistent pivots mean "place at grid cell" is a single assignment rather than a per-asset offset table.

### 3.3 Sockets / snap points

Grid snapping alone is insufficient, and this is the crux of the whole design. <cite index="30-1">Grid snap doesn't know about doorways, mandatory connections, or which way a piece faces — so you drop a piece, hit snap, nudge it because the grid landed it in the wrong half-cell, rotate it because the doorway faces wrong, and discover at playtest that the doorway opens onto nothing.</cite>

A real connection model fixes this. The vocabulary that has converged in practice (StraySpark's UE5 tool is a clean articulation of it, though the design is generic):

- **Type** — Wall, Floor, Ceiling, Corner, Door, Window, Connector, Custom
- **Direction** — an outward vector. <cite index="30-1">Two snap points only match when their outward directions are roughly anti-parallel: a Wall snap facing +Y connects to one facing −Y, never to one facing +Y.</cite>
- **Radius** — match tolerance
- **MustConnect** — validation flag; an unmatched mandatory socket is an error, which is how you catch doors-to-nowhere before playtest
- **Rule set** — a shared data asset defining what connects to what, version-controlled and swappable per kit

<cite index="27-1">Rule sets as data assets can be shared across a team, version-controlled, and swapped to change kit conventions.</cite> That's the property you want: connection rules are *data*, not code.

### 3.4 Tags

Borrow Sturgeon's tag layer: a label attached to one or more pieces that restricts what can go in a slot, with a default tag meaning "anything." The tile/tag distinction can be intentionally blurred — a functional tile can act as a tag constraining which visual tiles are allowed there. This is what lets you swap an entire art set under a fixed layout.

---

## 4. Editor architecture

### 4.1 Decide your build-vs-configure position honestly

There are four tiers, and most people over-build:

| Tier | What it is | When it's right |
|---|---|---|
| **Configure an existing editor** | UE grid snap + sockets; Godot GridMap + MeshLibrary; Unity ProBuilder | You want levels, not tools. Godot's GridMap is genuinely a 3D tilemap: <cite index="45-1">a MeshLibrary of meshes with optional collision and navigation shapes, cells that reference tiles, painted with click/shift-click.</cite> |
| **Plugin/editor script on top** | Editor Utility Widgets, custom inspectors, snap tooling | You need kit-specific rules the host engine doesn't model |
| **Runtime editor inside your game** | GILES-style, or Godot's GridMap driven from script | Players build things, or you want in-game iteration |
| **Standalone editor app** | ImGui/Qt + your own renderer | You have a custom engine, or the data model diverges hard from any engine's |

Worth noting: <cite index="42-1">Godot's GridMap works from both the editor and from scripts, specifically so you can build in-game level editors</cite> — which collapses tiers 1 and 3 if you're already in Godot. GILES is the reference open-source runtime editor for Unity (selection manager, grid snapping, translate/rotate/scale handles, JSON scene save/load via reflection, delta-only writes when prefabs are used, undo/redo).

**Candid take:** unless you're on a custom engine, tier 2 gets you 90% of the value for 10% of the cost. Build the *snapping and rule layer* as data + a plugin; don't rewrite viewport, gizmos, and asset management.

### 4.2 Core subsystems (if you are building)

**Asset registry.** Stable GUIDs per piece, decoupled from file paths. Every kit piece is `{id, mesh_ref, kit_id, category, tags[], sockets[], bounds, grid_footprint, lod_refs[], collision_ref}`. This manifest is the actual product — the meshes are interchangeable.

**Palette / browser.** Category tree, tag filter, thumbnail cache. Per Chaudhuri et al., context-aware re-ranking beats a static grid; the cheap version is "sort by pieces whose socket types match an open socket in the current selection," which needs no ML and captures much of the benefit.

**Placement tool.** Ghost preview → candidate socket search → validation → commit. The candidate search is a spatial query (grid hash or BVH) over open sockets within radius, filtered by type compatibility and anti-parallel direction, ranked by distance. `FindBestSnap` / `MakeConnection` / `BreakConnection` is the right API shape, with events on connection made/broken so gameplay can hook in.

**Undo/redo.** Command pattern is the consensus choice — the same one Qt's Undo Framework uses. The realistic command set from someone who shipped it: new, delete, copy, deep copy, deep delete, modify property. Three implementation notes people learn the hard way:

- **Build a CompoundCommand early.** Dragging updates position and rotation together and must be one undo step. Retrofitting composition after 20 command types is painful.
- **Bound the stack.** Most users never undo past 20–30 steps; unbounded history holds a lot of state for no benefit.
- **Decide upfront whether history serializes.** It enables undo past last-save, but the complexity of "what counts as serializable state" escalates fast.

The alternative — snapshot the affected subtree on every meaningful change and restore by deserialization — is easier to implement and can't be broken by a code path that bypasses the command API, but it's intrusive in the scene description and scales badly with scene size.

**Serialization.** Human-readable (JSON/TOML) for the level: a list of `{piece_id, transform, overrides{}, connections[]}`. Store *references and deltas*, not baked geometry. GILES' approach — write only state deltas when prefabs are used — is the right default.

**Rendering.** Kits are inherently instance-heavy. Batch by mesh + material; use GPU instancing from day one. Godot's GridMap handles this by splitting into sparse octants for rendering and physics.

### 4.3 The importer as quality gate

This is where §2.5's asset-readiness check lives. On import, hard-fail or warn on: non-power-of-two dimensions (if that's your convention), pivot not at the connection origin, triangle count over budget, overlapping UVs, missing collision, missing LODs, unlabeled sockets, scale mismatch. Automating this is what keeps a kit usable at 500 pieces instead of 50.

---

## 5. The procedural layer (optional but cheap once you have the contract)

Here's the payoff for doing §3 properly: **the socket/adjacency data you authored for interactive snapping is the same data a constraint solver needs.** No second authoring pass.

Two solver families:

- **WFC / model synthesis** — greedy observe-and-propagate. Fast, easy, no completeness guarantee (contradictions happen; deciding whether a tileset admits a solution is NP-hard). Use graph-based WFC if your kit isn't on a regular grid.
- **SAT/MaxSAT (Sturgeon)** — slower but expresses constraints WFC can't, notably reachability. Buys level *infilling*, segment *linking*, and level *repair* — all of which are exactly the mixed-initiative operations you want in an editor: designer places key pieces, solver fills the rest and guarantees it's traversable.

The reference implementation to study is Townscaper: <cite index="33-1">Oskar Stålberg combined WFC with marching cubes on irregular grids</cite>, and gave talks specifically on the *mixed-initiative* town generation aspect (EPC2021, SGC21, Konsoll 2021, AI and Games). Bad North and Caves of Qud are the other production examples.

Practical constraints worth adding beyond raw adjacency, per the WFC literature: **fixed tiles** (pre-place entry/exit points and boundaries before the solve, letting WFC fill a partially-designed level) and **path constraints** (force a connection between two points so rooms actually get doors).

---

## 6. Applied to your specific setup

You have `trellis2` (generate_3d, generate_orbit_views, segment_mesh, rig_model) and `home-still`. That suggests a pipeline like:

```
prompt/image
  → generate_3d (TRELLIS.2)
  → segment_mesh  ← this is your PartField-equivalent; parts fall out here
  → [MANUAL GATE] retopo / UV / decimate / LOD / collision
  → socket annotation (semi-automatic: sockets at part-boundary planes)
  → kit manifest entry
  → editor palette
  → snapping solver ─┬─ interactive placement
                     └─ WFC/SAT generation (same adjacency data)
```

Three candid warnings:

1. **The manual gate is not optional.** Begemann & Hutson's numbers are the direct evidence: 3× faster first pass, but with dense triangulation, fragmented UVs, and decimation that destroys texture integrity. Generated meshes are *candidate forms*, not kit pieces. Budget the audit/optimization time explicitly or your kit will be unusable at scale.
2. **Socket annotation is the bottleneck, and it's the thing worth automating.** `segment_mesh` gives you part IDs per face. The boundary between two adjacent parts is a plane with a normal — that's a socket candidate with a direction for free. Getting this semi-automatic is the highest-leverage engineering in the whole pipeline.
3. **Generated parts won't respect your grid.** TRELLIS doesn't know your 0.25m module. Either snap-fit generated parts to the grid on import (scale/quantize bounds) or accept that generated content is for the *asset kitbashing* column, not the *modular level design* column. These are different pipelines with different quality bars.

---

## 7. Gaps and next steps

**Corpus gaps worth filling** (confirmed absent — `distill_exists` returned false for the first):

| Paper | DOI | Why |
|---|---|---|
| Funkhouser et al. (2004), *Modeling by Example* | `10.1145/1015706.1015775` | Foundational assembly-based modeling; the retrieval-then-cut-and-glue paradigm |
| Chaudhuri & Koltun (2010), *Data-Driven Suggestions for Creativity Support in 3D Modeling* | `10.1145/1882261.1866205` | The geometric-suggestion baseline the 2011 paper improves on |

Run `paper_download` on both and they'll convert + index automatically.

**Also missing from the corpus and worth sourcing** (I did not verify DOIs for these, so resolve before ingest): Kalogerakis et al. (2012) *A Probabilistic Model for Component-Based Shape Synthesis*; Karth & Smith (2017) *WaveFunctionCollapse is Constraint Solving in the Wild*; Smith, Whitehead & Mateas (2011) *Tanagra*.

**Note on search:** a `paper_search` for modular level design / kit assembly returned almost entirely off-topic results (behavior trees, soft robotics, 3D-printed construction). The multi-provider keyword search doesn't handle this vocabulary well — targeted DOI ingest is the more reliable route here.

**Decision to make before building anything:** which tier in §4.1, and which of the two kitbashing columns in §0 you're actually serving. Those two answers determine everything downstream.

---

## Source list

**Local corpus (home-still):**
- Chaudhuri, Kalogerakis, Guibas & Koltun (2011). *Probabilistic Reasoning for Assembly-Based 3D Modeling.* SIGGRAPH '11. `10.1145/1964921.1964930`
- Liu, Uy, Xiang, Su, Fidler, Sharp & Gao (2025). *PartField: Learning 3D Feature Fields for Part Segmentation and Beyond.* `10.48550/arXiv.2504.11451`
- Cooper (2022). *Sturgeon: Tile-Based Procedural Level Generation via Learned and Designed Constraints.* AIIDE. `10.1609/aiide.v18i1.21944`
- Kim, Hahn, Kim & Kang (2020). *Graph Based Wave Function Collapse Algorithm for PCG in Games.* `10.1587/transinf.2019edp7295`
- Heese (2024). *Quantum Wave Function Collapse for Procedural Content Generation.* IEEE CG&A. `10.1109/mcg.2024.3447775`
- Merrell (`merrell09`). Model synthesis with geometric constraints.
- Begemann & Hutson (2026). *Prompted Props, Human Pipelines.* `10.55677/ijhrsss/15-2026-vol03i06`
- Liapis, Yannakakis & Alexopoulos (2014). *Mixed-Initiative Co-Creativity.* FDG. (`fdg2014_fdg2014_paper_37`)
- Liapis, Smith & Shaker. *Mixed-Initiative Content Creation*, ch. 11 of *Procedural Content Generation in Games*. (`pcgbook-ch11-...`)
- Gal et al. *3D Collage: Expressive Non-Realistic Modeling.* (`3D-Collage-...` — catalog year field reads 2001 and looks wrong; verify before citing)
- Gao & Juluri (2026). *From Idea to Co-Creation: A Planner-Actor-Critic Framework for Agent Augmented 3D Modeling.* `10.48550/arXiv.2601.05016`

**Web:**
- RenderHub — *Is Kitbashing an Efficient Worldbuilding Tool or Just an Artistic Shortcut?* / *Exploring the World of Kitbashing in 3D*
- The Gnomon Workshop — *Designing a Custom Kitbash Library* (Gavin Manners)
- Game Developer — *Creating Modular Game Art for Fast Level Design*
- Epic — *UDN: Workflow and Modularity*; World of Level Design — *UE4 Guide to Player Scale and World Architecture Dimensions*
- StraySpark — *Modular Kit Snapping in UE5: Grid Snap vs Sockets vs Modeling Mode vs Plugins* and the Modular Kit Snapping Tool announcement (vendor content, but the clearest public write-up of the snap-point data model)
- Godot docs — *Using GridMaps*, `GridMap` class reference
- GameDev.net — *Custom editor undo/redo system*; Moonjump forum — command pattern in a Godot level editor
- GitHub — `mxgmn/WaveFunctionCollapse` (Townscaper / Bad North / Caves of Qud references); `COLLEC1/giles`
- Unity — *Realizing rapid conceptual design with kitbashing*; KitBash3D kit specification
- AI and Games / Game Developer — *How Townscaper Works*