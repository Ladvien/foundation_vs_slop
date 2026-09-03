# Citations, and whether they resolve locally

Every paper this crate's code comments name, with its DOI and whether it is in the local
[home-still](https://github.com/Ladvien) corpus on host `big` (`/mnt/home-still`, catalog + the
`academic_papers` distill collection).

**Why this file exists.** A code comment that names a paper is a promise the reader can check. 18 of
the 24 papers below have **no open-access PDF**, so `hs paper download --doi <DOI>` refuses them — and
that refusal is recorded here rather than retried by hand, so the comment still names something a
reader can resolve through a library or a publisher. Ingestion run 2026-09-02 on `big` while holding
the `papers` GPU claim; the exact failure text was
`Download failed: Not found: No open-access PDF found for DOI: …` in every case.

## In the local corpus

| DOI | Paper | What the code takes from it |
|---|---|---|
| `10.1145/3549540` | Sellán et al., *Breaking Good: Fracture Modes for Realtime Destruction* | The limitation `bevy_carnage`'s loading-mode fracture closes: their fault is the same regardless of impact directionality (§6) |
| `10.1103/PhysRevFluids.3.063901` | Comiskey, Yarin & Attinger, *Forward spatter of blood from a gunshot* | The percolation model, `BLOOD_DENSITY`, `BLOOD_SURFACE_TENSION`, `FORWARD_SPATTER_SPEED`, and the inverse size–speed correlation |
| `10.1111/1556-4029.13855` | Williams et al., *Blood drop release from swinging objects* | Cast-off is **tangential**, not centrifugal (`patterns::cast_off`) |
| `10.1016/j.forsciint.2016.08.005` | Laan et al., *Morphology of drying blood pools* | Drying curves of different masses collapse onto one normalised curve; the serum halo above ~50 % RH (`dry`) |
| `10.1038/s41598-020-65465-4` | Smith, Nicloux & Brutin, *A new forensic tool to date human blood pools* | The rim-first drying front (`dry::Appearance::rim`) |
| `10.1109/tg.2021.3072241` | Pichlmair & Johansen, *Designing Game Feel: A Survey* | The practised hit-stop duration behind `CarnageSettings::hitstop_seconds` |

## Cited, not open access — resolve through a library

| DOI | Paper | What the code takes from it |
|---|---|---|
| `10.1063/1.329934` | Grady, *Local inertial effects in dynamic fragmentation* | The characteristic fragment size `s = (24·G_c / (ρ·ε̇²))^(1/3)` behind `grady_mott_target` |
| `10.1098/rspa.1947.0042` | Mott, *Fragmentation of shell cases* | The fragment-size distribution `audit.rs` already measures against |
| `10.1016/j.cmpb.2022.106980` | Parra-Cabrera et al., *Fracture pattern projection on 3D bone models* | Fracture morphology on bone geometry |
| `10.3233/BME-1991-1102` | Miyasaka et al., *Bending and torsion fractures in long bones* | Torsion cracks along a spiral at 45° to the long axis (`FaultPolicy::Morphology`, `LoadingMode::Torsion`) |
| `10.1016/j.forsciint.2021.110899` | Isa et al., *Investigating reverse butterfly fractures* | The butterfly's **order**: tension-face transverse portion forms last; tension faces are flat, compression faces jagged |
| `10.1016/j.forsciint.2016.04.035` | Cohen et al., *Impact velocity and bone fracture pattern* | Comminution at high energy |
| `10.3390/jimaging11060187` | *Biomechanics of spiral fractures: periosteal effects via DIC* | Greenstick as an **outcome**, not a mode: the tension cortex opens, the far cortex does not |
| `10.1103/physrevfluids.2.073906` | Comiskey, Yarin & Attinger, *Hydrodynamics of back spatter* | `BACK_SPATTER_SPEED` |
| `10.1520/jfs2003224` | Hulse-Smith et al., *Deducing drop size and impact velocity from circular bloodstains* | `minor / major = sin θ` (`stain::stain_shape`, `origin`) |
| `10.1111/j.1556-4029.2007.00505.x` | Knock & Davison, *Predicting the position of the source of blood stains* | `SPINE_COEFF = 0.76`, spines `∝ We^0.5 · sin³θ`, R² ≈ 0.9 |
| `10.1016/j.forsciint.2011.12.002` | Adam, *Fundamental studies of bloodstain formation* | Spine onset and saturation (`SPINE_MAX = 24`); substrate roughness shortens the stain and merges spines |
| `10.1016/j.forsciint.2019.109934` | Adam, *Release of blood droplets from weapon tips* | The 150 µL pendant cap (`BloodSettings::cast_off_max_ml`) |
| `10.1007/s00414-010-0498-5` | Donaldson et al., *Expirated bloodstain pattern formation* | Bubble rings only above 3 mm, in only ~20 % of patterns |
| `10.1016/j.forsciint.2011.07.027` | Bremmer et al., *Forensic quest for age determination of bloodstains* | The colour walk oxyHb → metHb → hemichrome (`dry::SRGB_*`) |
| `10.1111/cgf.13326` | Deul et al., *Direct position-based solver for stiff rods* | The XPBD compliance form in `bevy_viscera` |
| `10.1145/1399504.1360662` | Bergou et al., *Discrete elastic rods* | The rod formulation behind the same solver |
| `10.1016/j.entcom.2020.100359` | Kao, *The effects of juiciness in an action RPG* | Juice is an inverted-U: extreme is worse than none (`GorePolicy::intensity` default 0.6) |
| `10.1080/02699931.2010.496997` | Oum, Lieberman & Aylward, *A feel for disgust* | **Wetness, not colour, is the cue** — why the drying model's roughness channel is specular |

## Constants that are tuned rather than measured

Named here as well as in their own doc comments, because a tuned constant that says so is honest and
one dressed as a measurement is not.

| Constant | Status |
|---|---|
| `stain::SPINE_WE_MIN = 30.0` | **TUNED.** Adam 2012 documents that a spine onset *exists*; it gives no single threshold that transfers to this model's units, and the paper is not open access, so the value could not be confirmed. |
| `stain::SPLASH_K_CRIT = 57.7` | **Sourced.** Mundo, Sommerfeld & Tropea (1995) deposition/splash boundary in `K = We^0.5 · Re^0.25`, equivalently `Oh · Re^1.25`. |
| `rheo::PERFUSION_STRESS_PA = 10.0` | **TUNED.** A stress scale, not a measurement: chosen so a fresh wound flows against the Casson yield stress and the clot arrest lands inside the bleed taper. |
| `BloodSettings::hct_exponent = 2.5` | **TUNED.** Carreau–Yasuda was fitted at Hct ≈ 45 %; hematocrit-dependent variants exist and are deliberately not adopted. |
| `dry::DRY_REF_TICKS = 1800` | **Compressed, and stated.** Laan et al. measure tens of minutes; the *shape* is theirs, the clock is 30 s at 60 Hz so a game can show it. |
