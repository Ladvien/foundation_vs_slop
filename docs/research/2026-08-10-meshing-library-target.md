# The meshing library you should have — target list, acquisition status

**Date:** 2026-08-10
**Question:** what is the complete canonical set of meshing algorithms, what do you already own, and what
still has to be acquired by hand?
**Method:** ~185 papers enumerated across 10 families; every DOI resolved against Crossref / OpenAlex /
Semantic Scholar / CORE (never from memory); presence checked with `catalog_read` on the DOI-derived stem,
**never** `distill_search`; then `paper_download` run across every missing DOI.
**Companion docs:** `2026-08-10-meshing-algorithm-catalog.md`,
`2026-08-10-home-still-curation-agent-prompt.md`.

---

## 0. Result

| Outcome | Count |
|---|---:|
| Already in corpus | 38 |
| **Downloaded today** | **42** |
| Saved as HTML paywall stub — must be deleted | 14 |
| Paywalled, no open-access route | 77 |
| Book / course notes / thesis / blog — no DOI | 16 |

**Biggest wins today.** Six of the gaps flagged as blocking in yesterday's catalog are now closed:
**Surface Nets** (Gibson 1998), **Cubical Marching Squares**, **Curless & Levoy 1996 (TSDF)**,
**KinectFusion**, **voxel hashing**, and **Subgrid Marching Tetrahedra**. The entire modern neural line
also landed — FlexiCubes, Neural Dual Contouring, TetWeave, MeshGPT, MeshAnything v1+v2, TRELLIS,
EdgeRunner, nvdiffrec, GET3D, PolyGen, NeuS, MeshSDF, SuGaR, IM-NET — because arXiv preprints resolve
where publisher DOIs do not.

**Still blocked and still mattering most:** Transvoxel, Manifold Dual Contouring, Extended Marching Cubes
(Kobbelt), geometry clipmaps, ball-pivoting, the original 2006 Poisson, TetGen, TetWild, isosurface
stuffing, and appearance-preserving simplification. All are §12.

**The structural finding:** `paper_download` resolves DOIs through Unpaywall and provider APIs only. It
cannot reach author-hosted PDFs, HAL, CORE download URLs, or raw links — and a large fraction of the
classical SIGGRAPH/Eurographics canon is *freely available* on author pages while being invisible to that
resolver. Roughly 30–40 of the 77 "paywalled" items below are actually free on the web; they are simply
unreachable through the tool. That is a tooling gap, not an access problem.

**Status codes:** `HAVE` = already owned · `NEW` = downloaded today · `STUB` = HTML landing page saved,
delete it · `PAYWALL` = no OA route · `NO DOI` = book/notes/thesis/blog, see §11.

---

## 1. Isosurface extraction on regular grids

| Algorithm | Citation | DOI | Status |
|---|---|---|---|
| Marching Cubes | Lorensen & Cline 1987 | `10.1145/37401.37422` | HAVE |
| Polygonization of implicit surfaces | Bloomenthal 1988 | `10.1016/0167-8396(88)90013-1` | PAYWALL |
| Dürst hole report | Dürst 1988 | `10.1145/378267.378271` | PAYWALL |
| Asymptotic Decider | Nielson & Hamann 1991 | `10.1109/visual.1991.175782` | HAVE |
| Marching Tetrahedra (original) | Doi & Koide 1991 | — | NO DOI |
| BONO octrees | Wilhelms & Van Gelder 1992 | `10.1145/130881.130882` | PAYWALL |
| Topological considerations | Van Gelder & Wilhelms 1994 | `10.1145/195826.195828` | PAYWALL |
| Modified LUT (implicit disambiguation) | Montani, Scateni, Scopigno 1994 | `10.1007/bf01900830` | HAVE |
| Discretized Marching Cubes | Montani, Scateni, Scopigno 1994 | `10.1109/visual.1994.346308` | HAVE |
| Body-saddle disambiguation | Natarajan 1994 | `10.1007/bf01900699` | PAYWALL |
| Adaptive Marching Cubes | Shu, Zhou, Kankanhalli 1995 | `10.1007/BF01901516` | PAYWALL |
| **Marching Cubes 33** | Chernyaev 1995 | — | NO DOI |
| NOISE / span space | Livnat, Shen, Johnson 1996 | `10.1109/2945.489388` | PAYWALL |
| Interval trees | Cignoni et al. 1997 | `10.1109/2945.597798` | PAYWALL |
| Adaptive Skeleton Climbing | Poston, Wong, Heng 1998 | `10.1111/1467-8659.00261` | HAVE |
| Regularised Marching Tetrahedra | Treece, Prager, Gee 1999 | `10.1016/s0097-8493(99)00076-x` | PAYWALL |
| Adaptive trilinear isosurfaces | Cignoni et al. 2000 | `10.1016/s0097-8493(00)00036-4` | PAYWALL |
| Exact isosurfaces for MC | Theisel 2002 | `10.1111/1467-8659.00563` | PAYWALL |
| **MC33 w/ topological guarantees** | Lewiner et al. 2003 | `10.1080/10867651.2003.10487582` | HAVE |
| On Marching Cubes | Nielson 2003 | `10.1109/tvcg.2003.1207437` | PAYWALL |
| Robust & accurate MC | Lopes & Brodlie 2003 | `10.1109/tvcg.2003.1175094` | PAYWALL |
| MC survey | Newman & Yi 2006 | `10.1016/j.cag.2006.07.021` | HAVE |
| HistoPyramid MC | Dyken et al. 2008 | `10.1111/j.1467-8659.2008.01182.x` | PAYWALL |
| Extended LUT / SnapMC | Raman & Wenger 2008 | `10.1111/j.1467-8659.2008.01209.x` | HAVE |
| Edge transformations (MACET) | Dietrich et al. 2008 | `10.1109/tvcg.2008.60` | HAVE (abstract only) |
| **MC33 practical correctness** | Custodio et al. 2013 | `10.1016/j.cag.2013.04.004` | **NEW** |
| Manifold isosurfaces | Grosso 2016 | `10.1111/cgf.12975` | PAYWALL |
| Robust asymptotic decider | Grosso 2017 | `10.1145/3095140.3095179` | PAYWALL |
| Extended MC33 triangulation | Custodio, Pesco, Silva 2019 | `10.1186/s13173-019-0086-6` | HAVE |
| **Neural Marching Cubes** | Chen & Zhang 2021 | `10.1145/3478513.3480518` | **NEW** |
| **Subgrid Marching Tetrahedra** | Baktash, Gillespie, Crane 2026 | `10.48550/arXiv.2606.00454` | **NEW** |

## 2. Dual methods, sharp features, adaptive LOD

| Algorithm | Citation | DOI | Status |
|---|---|---|---|
| **Constrained Elastic Surface Nets** | Gibson 1998 | `10.1007/bfb0056277` | **NEW** |
| Distance maps for surface representation | Gibson 1998 | `10.1145/288126.288142` | PAYWALL |
| Topology-preserving multires isosurfaces | Gerstner & Pajarola 2000 | `10.1109/visual.2000.885703` | PAYWALL |
| Adaptively Sampled Distance Fields | Frisken et al. 2000 | `10.1145/344779.344899` | HAVE (zero chunks) |
| **Extended Marching Cubes** | Kobbelt et al. 2001 | `10.1145/383259.383265` | PAYWALL |
| Kizamu | Perry & Frisken 2001 | `10.1145/383259.383264` | PAYWALL |
| **Dual Contouring of Hermite Data** | Ju et al. 2002 | `10.1145/566570.566586` | HAVE (zero chunks) |
| Dual MC: primal contouring of dual grid | Schaefer & Warren 2004/05 | `10.1109/pccga.2004.1348336` | PAYWALL |
| Dual Marching Cubes | Nielson 2004 | `10.1109/visual.2004.28` | HAVE |
| **Cubical Marching Squares** | Ho et al. 2005 | `10.1111/j.1467-8659.2005.00879.x` | **NEW** |
| Intersection-free contouring on an octree | Ju & Udeshi 2006 | — | NO DOI |
| **Manifold Dual Contouring** | Schaefer, Ju, Warren 2007 | `10.1109/tvcg.2007.1012` | PAYWALL |
| Dual Marching Tetrahedra | Nielson 2008 | `10.1007/978-3-540-89639-5_18` | PAYWALL |
| **Transvoxel / transition cells** | Lengyel 2010 | `10.1080/2151237x.2011.563682` | PAYWALL |
| Isosurfaces over simplicial partitions | Manson & Schaefer 2010 | `10.1111/j.1467-8659.2009.01607.x` | PAYWALL |
| GPU contouring / Macet | Schmitz et al. 2010 | `10.1111/j.1467-8659.2010.01825.x` | PAYWALL |
| Watertight 2-manifold DC via tet decomposition | Rashid et al. 2016 | `10.1016/j.proeng.2016.11.037` | PAYWALL* |
| Parallel Dual Marching Cubes | Grosso & Zint 2021 | `10.1007/s00371-021-02139-w` | HAVE |
| **Neural Dual Contouring** | Chen et al. 2022 | `10.48550/arXiv.2202.01999` | **NEW** |

\* Procedia Engineering is nominally gold OA — the failure looks like an Elsevier resolver gap. Retry.

## 3. Terrain, voxel, runtime LOD, GPU-driven geometry

| Technique | Citation | DOI | Status |
|---|---|---|---|
| Hierarchical Z-buffer | Greene, Kass, Miller 1993 | `10.1145/166117.166147` | PAYWALL |
| Dynamic view-dependent simplification | Xia & Varshney 1996 | `10.1109/visual.1996.568126` | **NEW** |
| **Continuous LOD height fields** | Lindstrom et al. 1996 | `10.1145/237170.237217` | **NEW** |
| ROAM | Duchaineau et al. 1997 | `10.1109/visual.1997.663860` | HAVE (paywall stub — re-download) |
| View-dependent refinement of PM | Hoppe 1997 | `10.1145/258734.258843` | PAYWALL (orphan, unfixed) |
| Smooth view-dependent LOD / terrain | Hoppe 1998 | `10.1109/visual.1998.745282` | **NEW** |
| The clipmap: a virtual mipmap | Tanner, Migdal, Jones 1998 | `10.1145/280814.280855` | **NEW** |
| Geomipmapping | de Boer 2000 | — | NO DOI |
| Visualization of large terrains made easy | Lindstrom & Pascucci 2001 | `10.1109/visual.2001.964533` | PAYWALL |
| Terrain simplification simplified | Lindstrom & Pascucci 2002 | `10.1109/tvcg.2002.1021577` | PAYWALL |
| Chunked LOD | Ulrich 2002 | — | NO DOI |
| BDAM | Cignoni et al. 2003 | `10.1111/1467-8659.00698` | PAYWALL |
| **P-BDAM (planet-scale)** | Cignoni et al. 2003 | `10.1109/visual.2003.1250366` | **NEW** |
| **Geometry clipmaps** | Losasso & Hoppe 2004 | `10.1145/1015706.1015799` | PAYWALL |
| GPU-based geometry clipmaps | Asirvatham & Hoppe 2005 | — | NO DOI (GPU Gems 2) |
| Adaptive TetraPuzzles | Cignoni et al. 2004 | `10.1145/1015706.1015802` | PAYWALL |
| Quick-VDR | Yoon et al. 2004 | `10.1109/visual.2004.86` | PAYWALL |
| Batched Multi-Triangulation | Cignoni et al. 2005 | `10.1109/visual.2005.1532797` | PAYWALL |
| Progressive Buffers | Sander & Mitchell 2005 | `10.2312/sgp/sgp05/129-138` | PAYWALL (notes HAVE) |
| Frostbite procedural shader splatting | Andersson 2007 | `10.1145/1281500.1281668` | PAYWALL (notes HAVE) |
| Semi-regular terrain LOD survey | Pajarola & Gobbetti 2007 | `10.1007/s00371-007-0163-2` | STUB |
| Coherent Hierarchical Culling | Bittner et al. 2004 | `10.1111/j.1467-8659.2004.00793.x` | PAYWALL (orphan, unfixed) |
| CHC++ | Mattausch et al. 2008 | `10.1111/j.1467-8659.2008.01119.x` | PAYWALL |
| **GigaVoxels** | Crassin et al. 2009 | `10.1145/1507149.1507152` | **NEW** |
| CDLOD | Strugar 2009 | `10.1080/2151237x.2009.10129287` | PAYWALL |
| Efficient sparse voxel octrees | Laine & Karras 2010 | `10.1145/1730804.1730814` | PAYWALL |
| **High resolution sparse voxel DAGs** | Kämpe et al. 2013 | `10.1145/2461912.2462024` | **NEW** |
| SSVDAG | Villanueva et al. 2016 | `10.1145/2856400.2856420` | PAYWALL |
| Concurrent Binary Trees | Dupuy 2020 | `10.1145/3406186` | STUB (HAL PDF exists) |
| Nanite virtualized geometry | Karis et al. 2021 | — | NO DOI (HAVE as notes) |
| Conservative meshlet bounds | Unterguggenberger et al. 2021 | `10.1111/cgf.14401` | PAYWALL |
| CBT for large-scale game components | Benyoub & Dupuy 2024 | `10.1145/3675371` | PAYWALL (notes HAVE) |
| End-to-end compressed meshlet rendering | Mlakar et al. 2024 | `10.1111/cgf.15002` | PAYWALL |
| Greedy meshing | Lysenko 2012 | — | NO DOI (blog) |
| Binary greedy meshing | cgerikj, 2021– | — | NO DOI (repo) |

## 4. Unstructured mesh generation

| Algorithm | Citation | DOI | Status |
|---|---|---|---|
| Advancing front (2D) | Lo 1985 | `10.1002/nme.1620210805` | PAYWALL |
| **Guaranteed-quality triangular meshes** | Chew 1989 | `10.21236/ada210101` | **NEW** |
| Delaunay refinement | Ruppert 1995 | `10.1006/jagm.1995.1021` | PAYWALL |
| Triangle | Shewchuk 1996 | `10.1007/bfb0014497` | PAYWALL |
| **Robust geometric predicates** | Shewchuk 1997 | `10.1007/pl00009321` | **NEW** |
| NETGEN | Schöberl 1997 | `10.1007/s007910050004` | STUB |
| Tetrahedral mesh gen by Delaunay refinement | Shewchuk 1998 | `10.1145/276884.276894` | PAYWALL |
| Survey of unstructured mesh generation | Owen 1998 | — | NO DOI |
| Sliver exudation | Cheng et al. 2000 | `10.1145/355483.355487` | PAYWALL |
| Delaunay refinement algorithms | Shewchuk 2002 | `10.1016/s0925-7721(01)00047-5` | STUB |
| Red-green BCC tet meshing | Molino et al. 2003 | — | NO DOI |
| Variational tetrahedral meshing | Alliez et al. 2005 | `10.1145/1073204.1073238` | PAYWALL |
| **Isosurface stuffing** | Labelle & Shewchuk 2007 | `10.1145/1275808.1276448` | PAYWALL |
| Aggressive tet mesh improvement (Stellar) | Klingner & Shewchuk 2007 | `10.1007/978-3-540-75103-8_1` | PAYWALL |
| Gmsh `[FEM]` | Geuzaine & Remacle 2009 | `10.1002/nme.2579` | STUB |
| Isosurface stuffing improved | Doran, Chang, Bridson 2013 | `10.1145/2504459.2504507` | PAYWALL |
| Incremental CDT, finite precision | Si & Shewchuk 2014 | `10.1007/s00366-013-0331-0` | PAYWALL |
| **TetGen** | Si 2015 | `10.1145/2629697` | PAYWALL |
| **TetWild** | Hu et al. 2018 | `10.1145/3197517.3201353` | PAYWALL (not on arXiv) |
| Unstructured Mesh Generation (chapter) | Shewchuk 2012 | `10.1201/b11644-11` | HAVE |
| fTetWild | Hu et al. 2020 | `10.1145/3386569.3392385` | **NEW** (also HAVE as arXiv) |
| **Constrained Delaunay Tetrahedrization** | Diazzi et al. 2023 | `10.48550/arXiv.2309.09805` | **NEW** |

## 5. Surface reconstruction

| Algorithm | Citation | DOI | Status |
|---|---|---|---|
| Surface reconstruction from unorganized points | Hoppe et al. 1992 | `10.1145/133994.134011` | PAYWALL |
| Three-dimensional alpha shapes | Edelsbrunner & Mücke 1994 | `10.1145/174462.156635` | PAYWALL |
| Alpha shapes → C¹ Bézier reconstruction | Bajaj, Bernardini, Xu 1995 | `10.1145/218380.218424` | HAVE |
| **Volumetric method / TSDF** | Curless & Levoy 1996 | `10.1145/237170.237269` | **NEW** |
| Crust | Amenta, Bern, Kamvysselis 1998 | `10.1145/280814.280947` | HAVE |
| **Surface reconstruction by Voronoi filtering** | Amenta & Bern 1999 | `10.1007/pl00009475` | **NEW** |
| **Ball-pivoting** | Bernardini et al. 1999 | `10.1109/2945.817351` | PAYWALL |
| Power crust | Amenta, Choi, Kolluri 2001 | `10.1145/376957.376986` | PAYWALL |
| RBF reconstruction | Carr et al. 2001 | `10.1145/383259.383266` | PAYWALL |
| Point set surfaces (MLS) | Alexa et al. 2003 | `10.1109/tvcg.2003.1175093` | PAYWALL |
| MPU implicits | Ohtake et al. 2003 | `10.1145/882262.882293` | PAYWALL |
| **Poisson surface reconstruction** | Kazhdan, Bolitho, Hoppe 2006 | `10.2312/sgp/sgp06/061-070` | PAYWALL |
| RIMLS | Öztireli, Guennebaud, Gross 2009 | `10.1111/j.1467-8659.2009.01388.x` | STUB |
| SSD | Calakli & Taubin 2011 | `10.1111/j.1467-8659.2011.02058.x` | HAVE |
| **KinectFusion** | Newcombe et al. 2011 | `10.1109/ismar.2011.6092378` | **NEW** |
| Screened Poisson | Kazhdan & Hoppe 2013 | `10.1145/2487228.2487237` | HAVE |
| **Voxel hashing** | Nießner et al. 2013 | `10.1145/2508363.2508374` | **NEW** |
| Reconstruction benchmark | Berger et al. 2013 | `10.1145/2451236.2451246` | PAYWALL |
| **Floating Scale Surface Reconstruction** | Fuhrmann & Goesele 2014 | `10.1145/2601097.2601163` | **NEW** |
| Reconstruction survey | Berger et al. 2017 | `10.1111/cgf.12802` | STUB |
| SPR with envelope constraints | Kazhdan et al. 2020 | `10.1111/cgf.14077` | HAVE |
| Point-cloud noise/outlier removal | Wolff et al. 2016 | — | HAVE |

## 6. Simplification and LOD

| Algorithm | Citation | DOI | Status |
|---|---|---|---|
| Decimation of triangle meshes | Schroeder, Zarge, Lorensen 1992 | `10.1145/133994.134010` | HAVE |
| Vertex clustering | Rossignac & Borrel 1993 | `10.1007/978-3-642-78114-8_29` | PAYWALL |
| Mesh optimization | Hoppe et al. 1993 | `10.1145/166117.166119` | HAVE |
| Progressive Meshes | Hoppe 1996 | `10.1145/237170.237216` | HAVE |
| Simplification Envelopes | Cohen et al. 1996 | `10.1145/237170.237220` | HAVE |
| Hierarchical dynamic simplification | Luebke & Erikson 1997 | `10.1145/258734.258847` | PAYWALL |
| **QEM** | Garland & Heckbert 1997 | `10.1145/258734.258849` | HAVE |
| Attribute QEM | Garland & Heckbert 1998 | `10.1109/visual.1998.745312` | HAVE |
| Memoryless simplification | Lindstrom & Turk 1998 | `10.1109/visual.1998.745314` | HAVE |
| **Appearance-preserving simplification** | Cohen, Olano, Manocha 1998 | `10.1145/280814.280832` | PAYWALL |
| Metro (Hausdorff measurement) | Cignoni, Rocchini, Scopigno 1998 | `10.1111/1467-8659.00236` | PAYWALL |
| Preserving attribute values | Cignoni et al. 1998 | `10.1109/visual.1998.745285` | PAYWALL |
| **Wedge quadric w/ appearance attributes** | Hoppe 1999 | `10.1109/visual.1999.809869` | **NEW** |
| Image-driven simplification | Lindstrom & Turk 2000 | `10.1145/353981.353995` | PAYWALL |
| Out-of-core simplification | Lindstrom 2000 | `10.1145/344779.344912` | HAVE |
| Silhouette clipping | Sander et al. 2000 | `10.1145/344779.344935` | HAVE |
| Texture mapping progressive meshes | Sander et al. 2001 | `10.1145/383259.383307` | PAYWALL |
| Quadric simplification in any dimension | Garland & Zhou 2005 | `10.1145/1061347.1061350` | PAYWALL |
| Triangle reordering (vertex cache) | Sander, Nehab, Barczak 2007 | `10.1145/1276377.1276489` | PAYWALL |
| Probabilistic quadrics | Trettner & Kobbelt 2020 | `10.1111/cgf.13933` | PAYWALL |
| Simplifying textured meshes in the wild | Liu, Zhang, Yuksel 2025 | `10.1145/3763277` | HAVE (arXiv) |
| Fast & robust simplification for 3D assets | Bhosikar et al. 2026 | `10.48550/arXiv.2605.14029` | HAVE |

## 7. Remeshing

| Algorithm | Citation | DOI | Status |
|---|---|---|---|
| Anisotropic polygonal remeshing | Alliez et al. 2003 | `10.1145/882262.882296` | PAYWALL |
| Explicit surface remeshing | Surazhsky & Gotsman 2003 | `10.2312/sgp/sgp03/020-030` | PAYWALL |
| **Incremental isotropic remeshing** | Botsch & Kobbelt 2004 | `10.1145/1057432.1057457` | PAYWALL |
| ACVD discrete Voronoi remeshing | Valette et al. 2008 | `10.1109/tvcg.2007.70430` | STUB |
| Isotropic remeshing / exact RVD | Yan et al. 2009 | `10.1111/j.1467-8659.2009.01521.x` | STUB |
| **Adaptive remeshing for real-time deformation** | Dunyach et al. 2013 | `10.2312/conf/eg2013/short/029-032` | **NEW** |
| Error-bounded feature-preserving remeshing | Hu et al. 2016 | `10.48550/arXiv.1611.02147` | HAVE |
| CVT multi-facet-clipping remeshing | Fei et al. 2025 | `10.48550/arXiv.2505.14306` | HAVE |

## 8. Quad and field-aligned meshing

| Algorithm | Citation | DOI | Status |
|---|---|---|---|
| Periodic global parameterization | Ray et al. 2006 | `10.1145/1183287.1183297` | STUB |
| QuadCover | Kälberer, Nieser, Polthier 2007 | `10.1111/j.1467-8659.2007.01060.x` | PAYWALL |
| **Mixed-Integer Quadrangulation** | Bommes, Zimmer, Kobbelt 2009 | `10.1145/1531326.1531383` | PAYWALL |
| **QEx: robust quad mesh extraction** | Ebke et al. 2013 | `10.1145/2508363.2508372` | **NEW** |
| Quad-mesh generation survey | Bommes et al. 2013 | `10.1111/cgf.12014` | PAYWALL |
| Frame fields | Panozzo et al. 2014 | `10.1145/2601097.2601179` | HAVE (filename stem) |
| Instant field-aligned meshes | Jakob et al. 2015 | `10.1145/2816795.2818078` | HAVE (filename stem) |
| **Directional field synthesis (STAR)** | Vaxman et al. 2016 | `10.1111/cgf.12864` | **NEW** |
| Quad-patch partitioning survey | Campen 2017 | `10.1111/cgf.13153` | PAYWALL |
| All-quad meshing without cleanup | Rushdi et al. 2016 | `10.1016/j.cad.2016.07.009` | HAVE |
| Hex-mesh generation survey | Pietroni et al. 2022 | `10.48550/arXiv.2202.12670` | HAVE |

## 9. Mesh repair and robustness

| Algorithm | Citation | DOI | Status |
|---|---|---|---|
| Volumetric repair (voxelize round-trip) | Nooruddin & Turk 2003 | `10.1109/tvcg.2003.1196006` | HAVE |
| Robust repair of polygonal models | Ju 2004 | `10.1145/1015706.1015815` | PAYWALL |
| MeshFix | Attene 2010 | `10.1007/s00371-010-0416-3` | PAYWALL |
| Mesh repairing survey | Attene, Campen, Kobbelt 2013 | `10.1145/2431211.2431214` | PAYWALL |
| Generalized winding numbers | Jacobson, Kavan, Sorkine-Hornung 2013 | `10.1145/2461912.2461916` | HAVE |
| Consistent volumetric discretizations | Sacht et al. 2013 | `10.1111/cgf.12181` | HAVE (filename stem) |
| Mesh arrangements for solid geometry | Zhou et al. 2016 | `10.1145/2897824.2925901` | PAYWALL |
| TriWild | Hu et al. 2019 | `10.1145/3306346.3323011` | PAYWALL |
| Fast robust mesh arrangements (fp) | Cherchi et al. 2020 | `10.1145/3414685.3417818` | PAYWALL |

## 10. Neural, differentiable, generative — and collision proxies

| Algorithm | Citation | DOI | Status |
|---|---|---|---|
| DeepSDF | Park et al. 2019 | `10.1109/CVPR.2019.00025` | HAVE (zero chunks) |
| Occupancy Networks | Mescheder et al. 2019 | `10.1109/cvpr.2019.00459` | HAVE |
| **IM-NET** | Chen & Zhang 2019 | `10.48550/arXiv.1812.02822` | **NEW** |
| BSP-Net | Chen, Tagliasacchi, Zhang 2020 | `10.1109/cvpr42600.2020.00012` | HAVE |
| **MeshSDF** | Remelli et al. 2020 | `10.48550/arXiv.2006.03997` | **NEW** |
| **PolyGen** | Nash et al. 2020 | `10.48550/arXiv.2002.10880` | **NEW** |
| DefTet | Gao et al. 2020 | `10.48550/arXiv.2011.01437` | HAVE |
| DMTet | Shen et al. 2021 | `10.48550/arXiv.2111.04276` | HAVE |
| **NeuS** | Wang et al. 2021 | `10.48550/arXiv.2106.10689` | **NEW** |
| VolSDF | Yariv et al. 2021 | `10.48550/arXiv.2106.12052` | HAVE |
| **nvdiffrec** | Munkberg et al. 2022 | `10.48550/arXiv.2111.12503` | **NEW** |
| **GET3D** | Gao et al. 2022 | `10.48550/arXiv.2209.11163` | **NEW** |
| **FlexiCubes** | Shen et al. 2023 | `10.48550/arXiv.2308.05371` | **NEW** |
| 3D Gaussian Splatting | Kerbl et al. 2023 | `10.1145/3592433` | HAVE (HTML only) |
| 2D Gaussian Splatting | Huang et al. 2024 | `10.1145/3641519.3657428` | HAVE |
| **SuGaR** | Guédon & Lepetit 2024 | `10.48550/arXiv.2311.12775` | **NEW** |
| **MeshGPT** | Siddiqui et al. 2024 | `10.48550/arXiv.2311.15475` | **NEW** |
| **MeshAnything** | Chen et al. 2024 | `10.48550/arXiv.2406.10163` | **NEW** |
| **MeshAnything V2** | Chen et al. 2024 | `10.48550/arXiv.2408.02555` | **NEW** |
| **EdgeRunner** | Tang et al. 2024 | `10.48550/arXiv.2409.18114` | **NEW** |
| **TRELLIS** | Xiang et al. 2025 | `10.48550/arXiv.2412.01506` | **NEW** |
| **TetWeave** | Binninger et al. 2025 | `10.48550/arXiv.2505.04590` | **NEW** (also filename stem) |
| RigNet | Xu et al. 2020 | `10.48550/arXiv.2005.00559` | HAVE |
| UniRig | Zhang et al. 2025 | `10.48550/arXiv.2504.12451` | HAVE |
| Pinocchio auto-rigging | Baran & Popović 2007 | `10.1145/1276377.1276467` | PAYWALL |
| ACD of polyhedra | Lien & Amato 2007 | `10.1145/1236246.1236265` | STUB |
| HACD | Mamou & Ghorbel 2009 | `10.1109/icip.2009.5414068` | PAYWALL |
| Virtual node algorithm | Molino et al. 2004 | `10.1145/1015706.1015734` | PAYWALL |
| Arbitrary cutting of deformable tets | Sifakis et al. 2007 | `10.2312/sca/sca07/073-080` | PAYWALL |
| VACD real-time fracture | Müller, Chentanez, Kim 2013 | `10.1145/2461912.2461934` | HAVE |
| **CoACD** | Wei et al. 2022 | `10.48550/arXiv.2205.02961` | **NEW** |
| Navigation-driven ACD | Andrews 2024 | `10.1145/3641519.3657479` | HAVE |
| **DEACCON navmesh** | Hale, Youngblood, Dixit 2008 | `10.1609/aiide.v4i1.18693` | **NEW** |
| **3D spatial decomposition for navmesh** | Hale & Youngblood 2009 | `10.1609/aiide.v5i1.12376` | **NEW** |
| NEOGEN | Oliva & Pelechano 2013 | `10.1016/j.cag.2013.03.004` | PAYWALL |
| Navmesh comparative study | van Toll et al. 2016 | `10.1145/2994258.2994262` | STUB |
| Position-based simulation survey | Bender et al. 2014 | `10.1111/cgf.12346` | PAYWALL |

---

## 11. Books, course notes, theses — the no-DOI list

### Already in the corpus

| Item | Stem |
|---|---|
| Gregory, *Game Engine Architecture*, 3rd ed. (2018) | `Game Engine Architecture, Third Edition…` |
| Fernando (ed.), *GPU Gems* (2004) | `GPU Gems- Programming Techniques…` |
| Karis et al., "A Deep Dive into Nanite", SIGGRAPH 2021 course | `s2021-advances` |
| Deliot et al., "Concurrent Binary Trees: Large Scale Terrain", SIGGRAPH 2021 | `s2021-advances-pdf-5-mb` |
| Benyoub & Dupuy, CBT for game components, SIGGRAPH 2024 | `s2024-advances-pdf-12-5-mb` |
| Evans, "Learning from Failure" (Dreams), SIGGRAPH 2015 | `s2015-advances-*` |
| Haar & Aaltonen, "GPU-Driven Rendering Pipelines", SIGGRAPH 2015 | `s2015-advances-ppt-13-mb-pdf-4-mb-2` |
| Andersson, "Terrain Rendering in Frostbite", SIGGRAPH 2007 | `s2007-advances-course-notes-1-6-mb` |
| Sander & Mitchell, "Progressive Buffers", SIGGRAPH 2006 | `s2006-advances-*` |
| Geffroy et al., "Rendering the Hellscape of DOOM Eternal", SIGGRAPH 2020 | `s2020-advances-pdf-slides` |
| Cao, "Adaptive LOD Pipeline on Mobile", SIGGRAPH 2024 | `s2024-advances-pdf-2-6-mb` |
| Deolikar & Lupiani, *Procedural Content Generation for Games* (2025) | `Procedural Content Generation for Games…` |
| Xu, *Practical GPU Graphics with wgpu and Rust* (2021) | `Practical GPU Graphics with wgpu and Rust…` |
| *Game AI Pro* 1–3 (selected chapters) | `gameaipro*` |

### Missing, no DOI — acquire by hand

| Item | Where |
|---|---|
| **Chernyaev, "Marching Cubes 33: Construction of Topologically Correct Isosurfaces" (1995)** | CERN CN/95-17 tech report — free at cern.ch |
| **Lengyel, "Voxel-Based Terrain for Real-Time Virtual Simulations" (2010 PhD diss.)** | UC Davis; the Transvoxel source — transvoxel.org |
| Doi & Koide, "An efficient method of triangulating equi-valued surfaces…" (1991) | IEICE Trans. E74-D(1), 214–224 |
| Ju & Udeshi, "Intersection-free Contouring on an Octree Grid" (2006) | Pacific Graphics short paper |
| de Boer, "Fast Terrain Rendering Using Geometrical MipMapping" (2000) | flipcode archive |
| Ulrich, "Rendering Massive Terrains using Chunked LOD" (2002) | SIGGRAPH 2002 Course 35 — tulrich.com/geekstuff/sig-notes.pdf |
| Asirvatham & Hoppe, "Terrain Rendering Using GPU-Based Geometry Clipmaps" (2005) | *GPU Gems 2*, Ch. 2 — free on NVIDIA developer site |
| Owen, "A Survey of Unstructured Mesh Generation Technology" (1998) | 7th International Meshing Roundtable, Sandia |
| Molino et al., "A Crystalline, Red Green Strategy…" (2003) | 12th International Meshing Roundtable |
| Mikkelsen, "Simulation of Wrinkled Surfaces Revisited" (2008) | MSc thesis, U. Copenhagen — the MikkTSpace normative reference |
| Lysenko, "Meshing in a Minecraft Game" (2012) | 0fps.net — greedy meshing, parts 1 & 2 |
| cgerikj, `binary-greedy-meshing` | GitHub — no paper exists |
| Mamou, "Volumetric Hierarchical Approximate Convex Decomposition" (2016) | *Game Engine Gems 3*, CRC — DOI `10.1201/b21177-15` exists but is paywalled |

### Books you should own that are not in the corpus at all

| Book | Why |
|---|---|
| **Botsch, Kobbelt, Pauly, Alliez, Lévy — *Polygon Mesh Processing* (2010)** | The single best reference for this whole document. Covers remeshing, simplification, parameterization, repair, smoothing in one coherent treatment. |
| **Cheng, Dey, Shewchuk — *Delaunay Mesh Generation* (2012)** | The rigorous treatment behind §4; where the guarantees and impossibility results actually live. |
| **Luebke, Reddy, Cohen, Varshney, Watson, Huebner — *Level of Detail for 3D Graphics* (2002)** | The LOD canon consolidated; predates Nanite but the error metrics and view-dependence theory are unchanged. |
| **Ericson — *Real-Time Collision Detection* (2004)** | Collision proxies, BVH construction, convex hulls — the §10 collision half. |
| **Akenine-Möller, Haines, Hoffman et al. — *Real-Time Rendering*, 4th ed. (2018)** | Ch. 16 (polygonal techniques) and Ch. 19 (acceleration/LOD) are the practical bridge. |
| **de Berg, Cheong, van Kreveld, Overmars — *Computational Geometry: Algorithms and Applications* (2008)** | Delaunay, Voronoi, BSP fundamentals underneath everything in §4–5. |
| **Schneider & Eberly — *Geometric Tools for Computer Graphics* (2002)** | Practical predicates, intersection, and mesh-query recipes. |
| *GPU Gems 2* and *3* | You have GPU Gems 1; Ch. 2 of GPU Gems 2 is the geometry-clipmaps chapter listed above. |

---

## 12. Paywalled DOIs — the manual acquisition list

77 items. Ordered by how much each blocks the Bevy engine/editor work. Many of these have free
author-hosted PDFs that `paper_download` cannot reach — the third column says where to look.

### Tier 1 — blocking

| DOI | Paper | Likely free at |
|---|---|---|
| `10.1080/2151237x.2011.563682` | Lengyel, Transvoxel transition cells | transvoxel.org; the 2010 dissertation is free |
| `10.1109/tvcg.2007.1012` | Schaefer, Ju, Warren — Manifold Dual Contouring | cs.wustl.edu (Ju's page) |
| `10.1145/383259.383265` | Kobbelt et al. — Extended Marching Cubes | MPG PuRe repository |
| `10.1145/1015706.1015799` | Losasso & Hoppe — Geometry clipmaps | hhoppe.com |
| `10.1145/258734.258843` | Hoppe — View-dependent refinement of PM | hhoppe.com — **also the corpus orphan** |
| `10.1145/288126.288142` | Gibson — Distance maps for surface representation | merl.com |
| `10.1109/visual.2000.885703` | Gerstner & Pajarola — topology-preserving multires | — |

### Tier 2 — high value

| DOI | Paper | Likely free at |
|---|---|---|
| `10.2312/sgp/sgp06/061-070` | Kazhdan, Bolitho, Hoppe — Poisson (2006) | hhoppe.com / cs.jhu.edu |
| `10.1109/2945.817351` | Bernardini et al. — Ball-pivoting | research.ibm.com mirrors |
| `10.1145/2629697` | Si — TetGen | wias-berlin.de preprint 1762 |
| `10.1145/3197517.3201353` | Hu et al. — TetWild | cims.nyu.edu (not on arXiv) |
| `10.1145/1275808.1276448` | Labelle & Shewchuk — Isosurface stuffing | cs.berkeley.edu/~jrs |
| `10.1145/280814.280832` | Cohen, Olano, Manocha — Appearance-preserving simplification | cs.unc.edu |
| `10.1145/1057432.1057457` | Botsch & Kobbelt — Remeshing approach | graphics.rwth-aachen.de |
| `10.1145/1531326.1531383` | Bommes et al. — Mixed-Integer Quadrangulation | graphics.rwth-aachen.de |
| `10.1145/1730804.1730814` | Laine & Karras — Efficient sparse voxel octrees | nvidia research |
| `10.1145/2897824.2925901` | Zhou et al. — Mesh arrangements | cs.columbia.edu / libigl |
| `10.1145/3414685.3417818` | Cherchi et al. — Fast robust mesh arrangements | — |
| `10.1111/cgf.13933` | Trettner & Kobbelt — Probabilistic quadrics | graphics.rwth-aachen.de |
| `10.1145/1015706.1015815` | Ju — Robust repair of polygonal models | cs.wustl.edu |
| `10.1145/3306346.3323011` | Hu et al. — TriWild | cims.nyu.edu |

### Tier 3 — completeness

`10.1145/378267.378271` · `10.1145/130881.130882` · `10.1007/bf01900699` · `10.1109/2945.489388` ·
`10.1109/2945.597798` · `10.1109/tvcg.2003.1207437` · `10.1109/tvcg.2003.1175094` ·
`10.1111/j.1467-8659.2008.01182.x` · `10.1111/cgf.12975` · `10.1145/3095140.3095179` ·
`10.1016/0167-8396(88)90013-1` · `10.1145/195826.195828` · `10.1007/BF01901516` ·
`10.1016/s0097-8493(99)00076-x` · `10.1016/s0097-8493(00)00036-4` · `10.1111/1467-8659.00563` ·
`10.1145/383259.383264` · `10.1109/pccga.2004.1348336` · `10.1007/978-3-540-89639-5_18` ·
`10.1111/j.1467-8659.2009.01607.x` · `10.1111/j.1467-8659.2010.01825.x` · `10.1016/j.proeng.2016.11.037` ·
`10.1109/visual.2001.964533` · `10.1109/tvcg.2002.1021577` · `10.1111/1467-8659.00698` ·
`10.1145/1015706.1015802` · `10.1109/visual.2004.86` · `10.1109/visual.2005.1532797` ·
`10.2312/sgp/sgp05/129-138` · `10.1145/1281500.1281668` · `10.1080/2151237x.2009.10129287` ·
`10.1145/2856400.2856420` · `10.1145/166117.166147` · `10.1111/j.1467-8659.2004.00793.x` ·
`10.1111/j.1467-8659.2008.01119.x` · `10.1111/cgf.14401` · `10.1111/cgf.15002` · `10.1145/3675371` ·
`10.1002/nme.1620210805` · `10.1006/jagm.1995.1021` · `10.1007/bfb0014497` · `10.1145/276884.276894` ·
`10.1145/355483.355487` · `10.1145/1073204.1073238` · `10.1007/978-3-540-75103-8_1` ·
`10.1007/s00366-013-0331-0` · `10.1145/2504459.2504507` · `10.1145/133994.134011` ·
`10.1145/174462.156635` · `10.1145/376957.376986` · `10.1145/383259.383266` ·
`10.1109/tvcg.2003.1175093` · `10.1145/882262.882293` · `10.1145/2451236.2451246` ·
`10.1007/978-3-642-78114-8_29` · `10.1145/258734.258847` · `10.1111/1467-8659.00236` ·
`10.1109/visual.1998.745285` · `10.1145/383259.383307` · `10.1109/visual.2002.1183787` ·
`10.1145/965139.507101` · `10.1080/2151237x.2010.10390651` · `10.1145/218380.218391` ·
`10.1145/274363.274365` · `10.20380/gi1998.04` · `10.1145/344779.344922` · `10.1145/383259.383281` ·
`10.1145/566570.566589` · `10.1145/2693443` · `10.1145/1276377.1276489` · `10.1145/882262.882296` ·
`10.2312/sgp/sgp03/020-030` · `10.1111/j.1467-8659.2007.01060.x` · `10.1111/cgf.12014` ·
`10.1111/cgf.13153` · `10.1007/s00371-010-0416-3` · `10.1145/2431211.2431214` ·
`10.1145/1276377.1276467` · `10.1109/icip.2009.5414068` · `10.1016/j.cag.2013.03.004` ·
`10.1145/2614028.2615399` · `10.2312/sca/sca07/073-080` · `10.1145/1015706.1015734` ·
`10.1111/cgf.12346` · `10.1201/b21177-15` · `10.1016/j.cad.2012.10.032` · `10.1016/j.cagd.2008.05.003` ·
`10.1145/3272127.3275029` · `10.1145/1061347.1061350` · `10.1145/353981.353995` · `10.1145/3763277`

---

## 13. Cleanup required before any of this is usable

**Delete these — they are HTML landing pages, not papers.** Left in place they will convert into garbage
markdown and poison the index, exactly like the ROAM stub already in the corpus.

```
10.1145_3406186.html                      Concurrent Binary Trees      (real PDF: HAL hal-02898121)
10.1007_s00371-007-0163-2.html            Terrain LOD survey           (real PDF: CORE 51249127)
10.1007_s007910050004.html                NETGEN
10.1002_nme.2579.html                     Gmsh
10.1016_s0925-7721(01)00047-5.html        Shewchuk 2002  (2.7 KB — Elsevier block page)
10.1111_j.1467-8659.2009.01388.x.html     RIMLS
10.1111_cgf.12802.html                    Berger reconstruction survey (real PDF: CORE 49355291)
10.1111_j.1467-8659.2009.01521.x.html     Yan et al. isotropic remeshing
10.1109_tvcg.2007.70430.html              ACVD
10.1145_1183287.1183297.html              Periodic global parameterization
10.1145_1236246.1236265.html              Lien & Amato ACD
10.1016_j.cagd.2008.05.003.html           Lien & Amato 2008  (identical sha256 to the line above)
10.1145_2994258.2994262.html              Navmesh comparative study
10.1109_visual.1997.663860.html           ROAM  (pre-existing)
```

**Delete this — it is the wrong paper.** A subagent guessed an arXiv ID from memory rather than resolving
it, and pulled down an unrelated condensed-matter physics paper:

```
10.48550_arXiv.1806.02158.pdf   "Impurity-induced orbital magnetization in a Rashba electron gas"
                                (was intended to be TetWild — TetWild is not on arXiv at all)
```

**Then convert and index the 42 new PDFs:**

```
distill_backfill(dry_run=false, retry_skipped=true, limit=25)   # repeat until candidates = 0
```

New arrivals need `scribe_convert` first if they were downloaded without auto-conversion — check
`catalog_repair(dry_run=true)` → `stuck_convert` for the count. Note four of the new files are large
(FlexiCubes 36 MB, TetWeave 46 MB, GET3D 38 MB, directional-field STAR 36 MB) and will be slow through
the VLM.

**Tooling fix worth filing:** `paper_download` picks an HTML landing page over a sibling PDF even when the
PDF URL is already recorded in the catalog's own `download_urls` field (confirmed for `10.1145/3406186`
and `10.1007/s00371-007-0163-2`). It also accepts DOIs only — no HAL ids, CORE ids, or direct URLs —
which is what puts ~30–40 genuinely-free papers in §12 rather than in the library.
