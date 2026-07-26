//! **Site-67** — the persistent hub the player returns to between expeditions (FVS-G-1 / G-4 / G-5 / D-4).
//!
//! Design: `docs/2026-07-26-site-hub-and-operative-knowledge.md` §2. Read it before adding to this
//! module; several of the shapes here are deliberate reversals of the obvious approach.
//!
//! **Status: greybox kit only.** [`pieces`] is landed; the layout, navigation, door, aperture shader
//! and specimen cells are not. What follows is the contract the rest must honour, recorded now so it
//! is not re-derived.
//!
//! ## Why the Site persists for free
//!
//! `session::run_scoped()` is `DespawnOnExit(RunState::Active)`, and its doc already names this module
//! as the exemption. Site entities persist simply by **not carrying it** — there is no exempt-list to
//! maintain and no teardown system to keep in step with every spawner. That is the surviving half of
//! FVS-A-4, and A-5 made it free.
//!
//! ## The Site is entities, NOT a `Dungeon`
//!
//! `Dungeon` is a single resource that A-5 regenerates per run, so a second procedural world would make
//! "which one does this system mean?" ambiguous everywhere. The hub must also be *learnable*, which
//! procedural generation actively fights. So the Site is hand-authored entities with its own small
//! walkable mask, and that mask must have a **different type name** from `Dungeon` so no system can
//! confuse the two.
//!
//! ## The constraint that decides squad presence
//!
//! Squad `Unit`s cannot stand here, and the reason is concrete rather than stylistic: `spawn_unit`
//! carries `run_scoped()`, and both `squad::unit_movement` and `fog::update_los` take `Res<Dungeon>` —
//! which while `Idle` is **absent** (first boot) or **stale** (post-run). Real units at the Site would
//! collide with a ghost dungeon and repaint a ghost fog grid. The Site therefore gets its own
//! `SiteAvatar`, never `squad::Unit`; promoting avatars to real operatives is FVS-G-3's job.
//!
//! ## Determinism
//!
//! The gameplay half (the Site root, the Site↔specimen relationship) is **harness-visible**, because
//! FVS-D-4's acceptance — "specimens accumulate across expeditions" — is otherwise the single most
//! important thing to test and the one thing untestable. That is the same mistake `src/session/`
//! documents about putting the win/lose decision in `AppState`.
//!
//! It cannot move `GOLDEN`, and the reason must survive someone later moving code around: the Site root
//! is **bodiless** (no `Transform`, no `Health`), so it contributes no row to `snapshot_hash` and no
//! actor to `liveness_violations` — exactly `squad::Squad`'s shape and for exactly that reason. The
//! presentation half (geometry, avatars, the aperture material) is windowed-only `SiteVisualsPlugin`,
//! and any body it spawns must carry a `Transform` **without** a `Health` to stay out of the fold.

pub mod pieces;

pub use pieces::SitePiece;
