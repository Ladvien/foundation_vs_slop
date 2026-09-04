# bevy_fracture_modes

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

Fracture modes on a convex decomposition: precompute, once, the sparse mass-orthonormal modes of a cell graph — the few ways a shape *wants* to come apart — then project an impact onto them at runtime so a body breaks across its real weaknesses instead of along whatever a Voronoi diagram happened to draw. Sellán et al.'s *Breaking Good* (2022), reduced to the one place a runtime fracture system already has a null space of rigid motions: its cells.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. Issues and PRs belong upstream.

![A necked bar breaking at its neck under a blow at either end](https://raw.githubusercontent.com/Ladvien/bevy_fracture_modes/main/docs/fracture_modes.gif)

## The family

This crate is one of eight that make up one gore stack. **`bevy_carnage` is the umbrella**: depend on it alone and every kernel below is re-exported under its own name, so a game needs one dependency line and can never end up with two versions of a leaf. Depend on a kernel directly only when you want it without the rest — each one stands alone, and none depends on `bevy_carnage` back.

| crate | what it is | reach it as |
|---|---|---|
| [`bevy_carnage`](https://github.com/Ladvien/bevy_carnage) · [crates.io](https://crates.io/crates/bevy_carnage) | the umbrella — plane cuts with watertight caps, bores, energy-driven fracture, wounds, decals, impact feel; **re-exports every crate below** | `bevy_carnage` |
| [`bloodstain`](https://github.com/Ladvien/bloodstain) · [crates.io](https://crates.io/crates/bloodstain) | blood as a material: Carreau–Yasuda rheology, Comiskey spatter, stain morphology, drying, spectral colour by thickness and oxygenation — engine-free, `no_std` | `bevy_carnage::blood` |
| [`bevy_wetmap`](https://github.com/Ladvien/bevy_wetmap) · [crates.io](https://crates.io/crates/bevy_wetmap) | texture-space blood that runs, spreads and dries — CPU-authoritative, so a canvas can be hashed | `bevy_carnage::wetmap` |
| [`bevy_viscera`](https://github.com/Ladvien/bevy_viscera) · [crates.io](https://crates.io/crates/bevy_viscera) | XPBD strands with a tearing mesentery: guts that spill, coil, tether and tear | `bevy_carnage::viscera` |
| [`bevy_cross_section`](https://github.com/Ladvien/bevy_cross_section) · [crates.io](https://crates.io/crates/bevy_cross_section) | anatomical bands on a cut face from a sourced per-region thickness table, via `UV_1` | `bevy_carnage::cross_section` |
| [`bevy_flaymap`](https://github.com/Ladvien/bevy_flaymap) · [crates.io](https://crates.io/crates/bevy_flaymap) | texture-space flaying: skin, fat, muscle, cortex peel under hits, with a once-per-canvas bone handoff | `bevy_carnage::flaymap` |
| [`bevy_laceration`](https://github.com/Ladvien/bevy_laceration) · [crates.io](https://crates.io/crates/bevy_laceration) | a cut that gapes on a time curve, scaled by skin tension and Langer-line orientation | `bevy_carnage::laceration` |
| **`bevy_fracture_modes` — this crate** | Sellán's fracture modes on a cell graph: a fixed-schedule bake, impact projection, gluing partition | `bevy_carnage::fracture_modes` |

Every crate is deterministic where it can be — fixed schedules, no clocks, frozen digests over its CPU state — and every one carries the same *Vibe Coded* warning as this file. The four added on 2026-09-04 (`bevy_cross_section`, `bevy_flaymap`, `bevy_laceration`, `bevy_fracture_modes`) are kernels `bevy_carnage` composes; `bloodstain` is the one with no engine in it at all. Fourteen of the family's examples run in a browser at [ladvien.github.io/foundation_vs_slop](https://ladvien.github.io/foundation_vs_slop/).

## Why this exists

Every realtime destruction system precomputes fragments and swaps them in on impact, and the paper names the fault of every one of them: Voronoi and level-set prefracture *"often miss obvious structural weaknesses"* and produce *"recognizable, unrealistic pieces"*, because they look at the shape's volume and never at where it is thin. A fracture mode is the opposite: a displacement field that pays for the *number* of faces it opens, not for how far, so its minimisers are piecewise-constant with jumps on a sparse set of faults — the thin neck, the narrow wrist, the seam. Compute a few, and any impact is a linear combination of them.

Everything here is deterministic to the bit. The eigen-initialisation is a fixed-sweep Jacobi decomposition, the solver runs a fixed number of ADMM steps, and the partition is a union-find whose unions are order-independent; a mode set is a golden and two machines bake the same one.

## The model

The paper's mode problem (their Eq. 15):

```text
argmin_{UᵀMU = I}   ½ trace(UᵀQU) + ω Σ_i E_D(U_i),      E_D(u) = Σ_faces √( ∫ ‖D(u)‖² )
```

`Q` is a strain Hessian whose only job is its null space (their §3.7: *"only its null space matters"*); `E_D` is a group-ℓ1 over fault patches, which is what makes the modes sparse. On a convex decomposition every cell already moves as one piece, so the unknowns collapse to one translation per cell, the fault patches are the shared faces, and — because `E_D` cannot tell directions apart — the modes are **scalar** functions on the cell graph with `M = diag(cell masses)` and an area-weighted graph Laplacian for `Q`.

At runtime (their §3.4) an impact is blurred one implicit step along the graph, `g = (M + τL)⁻¹ M δ_p`, projected onto the modes, and every face whose two cells' responses differ by less than `σ = 10⁻³` is **glued** back; what is left apart is the fracture. The projection is precomputed into one row per mode (their `A_i`), so an impact costs `k × cells` multiplications. Because the projection is linear, *"scaling σ and scaling the magnitude of the impact are equivalent"* — the impulse is the dial.

**One departure from the paper, stated.** The paper projects with every mode weighted equally. This crate divides each mode's contribution by its discontinuity energy — the area-weighted sum of the jumps it opens, which is the work its fault set costs — so a weak fault opens under a small blow and a strong one needs a large one. On a graph small enough that a few modes span most of it, equal weighting lets an impulse excite whichever modes are large *at the impact* and a thin neck elsewhere never opens first; with the weighting, the neck is the first face to give, which `the_neck_is_the_first_face_to_break` pins.

What this crate does not do, and the paper does not either: secondary fractures of the pieces, and directional loading (their §6). Both belong to the crate that owns the cells.

## Compatibility

| bevy | bevy_fracture_modes |
|---|---|
| 0.19 | 0.1 |

## What it exposes

- `CellGraph`, `Face` — cells with masses and centres, faces with areas; `CellGraph::bar` is the necked test shape.
- `ModeSettings` (a `Resource`), `ModeSet::bake(&graph, &settings)` → `ModeSet { modes: Vec<Mode> }` with energies ascending and the precomputed impact rows.
- `Impact { cell, magnitude }`, `ModeSet::response`, `ModeSet::partition` → `Partition { groups, broken, group_of }`. `SIGMA` is the paper's tolerance.
- `FractureModesPlugin` — adds `ModeSettings` and `FractureModeCache` (keyed by a caller `u64`). **Registers no systems**: only the crate that owns a decomposition knows when a graph is complete.

No components, no system sets.

## References

- Sellán, S., Ni, J., Stein, O., Jacobson, A. et al., *Breaking Good: Fracture Modes for Realtime Destruction*, ACM Transactions on Graphics 42(1), 2022. `doi:10.1145/3549540`
- Brandt, C. & Hildebrandt, K., *Compressed vibration modes of elastic bodies*, Computer Aided Geometric Design 52–53, 2017 — the ICCM scheme the paper adapts and this crate's ADMM stands in for.

## Examples

```sh
cargo run --example fracture_modes_bar   # terminal only — the necked bar's modes, and a blow at each end
cargo run --example fracture_modes       # a wall of cells with a weak seam, broken by clicks  (needs a GPU)
```

## License

MIT OR Apache-2.0, at your option.
