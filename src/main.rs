//! Foundation vs. Slop — binary entry point.
//!
//! All game logic lives in the library crate (`src/lib.rs`) so integration tests under `tests/`
//! and the headless `sim_harness` can reuse the same modules. This binary is a thin launcher.

fn main() {
    foundation_vs_slop::run();
}
