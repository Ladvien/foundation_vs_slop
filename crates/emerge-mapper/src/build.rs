//! **Assembling a tile** — the Tiles tab.
//!
//! A tile is a floor mesh, a wall over it, a fixture on the wall, maybe a decal. The author walks a
//! grid inside the tile with the keyboard and drops meshes onto it; when it is finished they save it,
//! and the Map places it as one thing.
//!
//! # Why this is a tab and not a mode
//!
//! It was a mode for a day. Bringing a mesh in and arranging meshes looked like the same activity a
//! minute apart, so they shared a tab and a `C` key flipped between them — which worked, and was
//! wrong for a reason the key budget hid: `docs/research/2026-08-08-kitbashing-guidance.md` says *"A
//! good kit is hierarchical: parts -> sub-assemblies -> assemblies"*, the editor already gave a tab
//! to every other level, and only that one carried two.
//!
//! A mesh is a measurement written to `library.ron` and described once; a tile is an arrangement
//! written to `compositions.ron` and built constantly. Splitting them retired the mode key with the
//! mode: a tab strip is a mode nobody can forget, which is Raskin's whole condition. FVS-R-21.
//!
//! The twelve-row cap is what made two contexts affordable —
//! `no_context_carries_more_than_a_learnable_vocabulary`, and Liapis's **user fatigue**, one of whose
//! named causes is *"when there are too many options"*.
//!
//! # The grid is the tile's own, and it is the project's
//!
//! Cells are [`SnapLevel`] rungs over `policy.snap_divisor` — **the same ladder the Map places on**,
//! at a smaller scale. That is what makes a tile authored today abut a tile authored last month, and
//! it is Códices et al.'s conformity argument (`10.1109/access.2022.3168832`): a designer can *"define
//! a passage as n pins wide or tall, keeping consistency in the design of the layout of the individual
//! pieces being made separately."*
//!
//! The bare rung is the whole tile — one cell, which is the degenerate case — so building happens a
//! rung down by default.
//!
//! # Flush falls out; it is not a verb
//!
//! A piece lands with its **minimum corner** on the cell it was dropped in, the same rule
//! `grid::snap_corner` states for the Map. Cell zero's corner *is* the tile's edge, so a 0.1 m wall
//! dropped there has its centre at −0.45 — which is exactly the flush position
//! `docs/2026-08-09-compose-authoring-plan.md` §4 records as *"off the lattice by construction"* and
//! which the deleted `compose::flushed` verb existed to reach. Stating the rule on the corner rather
//! than the centre reaches it without a verb.

use bevy::prelude::*;

use emerge_core::composition::{Body, Composition, Envelope, Member};
use emerge_core::grid::SnapLevel;

/// **The tile being assembled**, and where the cursor is in it.
#[derive(Resource, Default)]
pub struct Build {
    /// The tile in hand, or `None` when nothing is being built. Absence is a real state — the tab
    /// opens in `Describe` and an author may never build anything.
    pub open: Option<Composition>,
    /// Which member has focus, as an index into `open`'s members. Out of range reads as "none", which
    /// is what happens when the focused member is dropped.
    pub focus: usize,
    /// The cell cursor, in whole cells from the envelope's **minimum** corner — `(0, 0, 0)` is the
    /// bottom south-west cell. Signed so walking off an edge is representable and then clamped, rather
    /// than wrapping through zero.
    pub at: (i32, i32, i32),
    /// The rung the cursor walks, **latched**.
    ///
    /// Bier's snap-dragging changes gravity modes with keyboard commands and holds nothing; of its 44
    /// commands the modal ones are all latched. StickyLines says why holding costs: its designers
    /// *"make extensive use of the keyboard … not only because it is faster, but also because 'there
    /// are too many options and menus' that clutter their screens and make them 'lose focus'."*
    /// Holding Shift is right for one nudge and wrong for a dressing session, and a dressing session
    /// is what building a tile is.
    ///
    /// Safe to latch because it is **visible**: the drawn grid redraws at the active rung, so this is
    /// not a mode anyone can forget they are in.
    pub rung: SnapLevel,
}

/// The rung a tile is built on before anyone changes it.
///
/// Not [`SnapLevel::Tile`]: that rung is the whole tile, one cell, and a grid with one square in it
/// cannot position anything. The first rung down is the author's "units", which is what
/// `policy.snap_divisor` names.
pub const DEFAULT_RUNG: SnapLevel = SnapLevel::Fine;

/// An empty tile of the standard size, ready to take members.
///
/// One cell across and `height` tall, because that is what a tile *is* — `grammar::from_compositions`
/// refuses a `Bounded` composition that is not exactly the grid's size, so a tile of any other width
/// is a group the solver can never place.
pub fn blank(id: &str, height: f32) -> Composition {
    Composition {
        id: id.to_owned(),
        envelope: Envelope::Bounded {
            size: (emerge_core::grid::TILE, height, emerge_core::grid::TILE),
        },
        members: Vec::new(),
        locations: Vec::new(),
        note: None,
    }
}

/// How many cells the envelope divides into at this rung, never zero.
///
/// Y divides on the same pitch as X and Z rather than on its own: *"up one"* and *"across one"* being
/// different distances is the confusion the single ladder exists to remove, and a tile 2.4 m tall on a
/// third-metre rung is seven layers, which is a number an author can hold.
pub fn cells(size: (f32, f32, f32), pitch: f32) -> (u32, u32, u32) {
    let n = |v: f32| ((v / pitch).round() as i64).clamp(1, i64::from(u32::MAX)) as u32;
    (n(size.0), n(size.1), n(size.2))
}

/// The **minimum corner** of a cell, in envelope-local metres.
///
/// X and Z are measured from the envelope's centre — the reading `Member::at` has — and Y from its
/// floor, which is the reading `Member::lift` has. Two origins because the envelope has two: a tile is
/// centred in plan and stands on its base.
pub fn cell_corner(size: (f32, f32, f32), pitch: f32, at: (i32, i32, i32)) -> (f32, f32, f32) {
    (
        -size.0 * 0.5 + at.0 as f32 * pitch,
        at.1 as f32 * pitch,
        -size.2 * 0.5 + at.2 as f32 * pitch,
    )
}

/// Keep the cursor inside the tile. Walking off an edge stops at it rather than wrapping.
pub fn clamp(size: (f32, f32, f32), pitch: f32, at: (i32, i32, i32)) -> (i32, i32, i32) {
    let (nx, ny, nz) = cells(size, pitch);
    let c = |v: i32, n: u32| v.clamp(0, n.saturating_sub(1) as i32);
    (c(at.0, nx), c(at.1, ny), c(at.2, nz))
}

/// **Where a piece of this footprint lands when dropped in this cell** — `(at, lift)`.
///
/// The piece's minimum corner goes on the cell's, so its centre is half a span in. This is
/// `grid::snap_corner`'s rule read forwards instead of backwards: there the centre is given and the
/// corner is solved for; here the corner is chosen and the centre follows.
///
/// `span` is the piece's footprint **already turned by its yaw** — the caller has the yaw and this
/// does not, the same split `editor::brush_at` makes.
pub fn drop_at(
    size: (f32, f32, f32),
    pitch: f32,
    at: (i32, i32, i32),
    span: (f32, f32),
) -> ((f32, f32), f32) {
    let (x, y, z) = cell_corner(size, pitch, at);
    ((x + span.0 * 0.5, z + span.1 * 0.5), y)
}

/// A member id that is not already taken, derived from the piece's own name.
///
/// `site/wall` becomes `wall`, then `wall_2`, `wall_3`. Derived rather than typed because naming
/// every member by hand is the kind of friction that makes an author stop using a tool — and the id
/// only has to be unique and stable, which a suffix achieves.
pub fn fresh_id(members: &[Member], descriptor: &str) -> String {
    let short = descriptor.rsplit('/').next().unwrap_or(descriptor);
    let taken = |id: &str| members.iter().any(|m| m.id == id);
    if !taken(short) {
        return short.to_owned();
    }
    // Bounded by the member count plus one, so this cannot spin: at most `n + 1` names can be taken.
    for n in 2..=members.len() + 2 {
        let candidate = format!("{short}_{n}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    short.to_owned()
}

/// **Members stay sorted by id**, which is what `composition::validate` holds every group to so that
/// one group has one encoding. Called after every insertion rather than at save, so the list an
/// author reads is the list that will be written.
pub fn insert_sorted(members: &mut Vec<Member>, m: Member) -> usize {
    let at = members.partition_point(|o| o.id < m.id);
    members.insert(at, m);
    at
}

/// Drop a descriptor into the cursor's cell, returning the index it landed at.
pub fn place(
    build: &mut Build,
    descriptor: &str,
    span: (f32, f32),
    yaw: f32,
    pitch: f32,
) -> Result<usize, String> {
    let Some(comp) = build.open.as_mut() else {
        return Err("no tile open — press N to start one".to_owned());
    };
    let Envelope::Bounded { size } = comp.envelope else {
        return Err(format!("`{}` claims no tile, so it has no grid to drop into", comp.id));
    };
    let (at, lift) = drop_at(size, pitch, build.at, span);
    let m = Member {
        id: fresh_id(&comp.members, descriptor),
        body: Body::Descriptor {
            id: descriptor.to_owned(),
            tip: (0, 0),
            on: None,
            patch: None,
        },
        at,
        yaw,
        lift,
        paint: 0,
        of_fingerprint: None,
        note: None,
    };
    Ok(insert_sorted(&mut comp.members, m))
}

/// Drop a **hole** into the cursor's cell — a position that says what may go here without saying what
/// does. Same gesture, same grid, same keys; see [`Body::Slot`].
pub fn place_slot(build: &mut Build, accepts: &str, pitch: f32) -> Result<usize, String> {
    let Some(comp) = build.open.as_mut() else {
        return Err("no tile open — press N to start one".to_owned());
    };
    let Envelope::Bounded { size } = comp.envelope else {
        return Err(format!("`{}` claims no tile, so it has no grid to drop into", comp.id));
    };
    // A hole has no footprint, so it sits at the cell's corner exactly. `validate` refuses one on or
    // outside the envelope, and the *last* cell's corner is inside by less than a rung — so a slot
    // dropped in the far corner is legal and a slot is never placed on the seam.
    let (at, lift) = drop_at(size, pitch, build.at, (0.0, 0.0));
    let m = Member {
        id: fresh_id(&comp.members, accepts),
        body: Body::Slot { accepts: accepts.to_owned() },
        at,
        yaw: 0.0,
        lift,
        paint: 0,
        of_fingerprint: None,
        note: None,
    };
    Ok(insert_sorted(&mut comp.members, m))
}

/// **Save the tile in hand**, and keep it open so the author can carry on.
///
/// Through [`crate::project::Project::commit_composition`] — the one door — so a tile assembled here
/// and a tile captured from a box on the Map are validated and written by the same sequence.
/// `tests/compose_is_read_only.rs` holds that.
///
/// # It stays open on success
///
/// Saving is not finishing. The loop this tab exists to shorten is *drop, look, adjust, save, look* —
/// Compton's grokloop, which Lai et al.'s second pillar asks to keep short — and clearing the tile on
/// save would put a "start again" step in the middle of it. Replacing an existing tile is how a tile
/// is *edited*, which is the same rule capture-over-a-name follows on the Map.
pub fn save(build: &Build, project: &mut crate::project::Project) -> Result<String, String> {
    let Some(comp) = build.open.as_ref() else {
        return Err("no tile open — nothing to save".to_owned());
    };
    if comp.members.is_empty() {
        // The refusal `Composition::validate` would give anyway, said in the author's terms and
        // before the file is touched: an empty tile stamps nothing, which looks like a stamp that
        // failed.
        return Err(format!("`{}` has nothing in it yet", comp.id));
    }
    let existed = project.compositions.compositions.iter().any(|c| c.id == comp.id);
    project.commit_composition(comp.clone())?;
    Ok(if existed {
        format!("`{}` updated — {} members", comp.id, comp.members.len())
    } else {
        format!("`{}` saved — {} members, and it is in the Map palette now", comp.id, comp.members.len())
    })
}

/// The rung pitch the cursor is walking, in metres.
pub fn pitch(build: &Build, project: &crate::project::Project) -> f32 {
    build.rung.pitch(project.policy.snap_divisor)
}

/// **Every BUILD verb, in one system.**
///
/// One system rather than eight, because the verbs share the tile in hand and Bevy would otherwise
/// need eight `ResMut<Build>` and a schedule opinion about their order. `editor.rs`'s key dispatcher
/// is the same shape for the same reason.
///
/// Takes `Res<Project>` and only escalates to `ResMut` where it writes — `redraw_stamps` is gated on
/// `Project::is_changed()`, and Bevy flags a resource when a system *dereferences* `ResMut`, not when
/// it mutates. A verb that correctly decides to do nothing must not look like an edit.
#[allow(clippy::too_many_arguments)]
pub fn build_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mode: Res<crate::tiles::Mode>,
    mut build: ResMut<Build>,
    mut state: ResMut<crate::tiles::ImportState>,
    // **`ResMut`, dereferenced mutably only in the save branch.** Bevy flags a resource changed when
    // a system *dereferences* `ResMut`, not when it mutates — and `editor::redraw_stamps` is gated on
    // `Project::is_changed()`, so a keystroke that correctly does nothing would otherwise tear down
    // and rebuild every stamped row in the map. That defect is on the record (2026-08-11 inspection,
    // D3: eighteen rebuilds of an identical picture in one session).
    mut project: ResMut<crate::project::Project>,
) {
    use crate::keys::{just_pressed, Action};
    if *mode != crate::tiles::Mode::Tiles {
        return;
    }
    let pressed = |a: Action| just_pressed(&keyboard, live.0, a);

    // **Arriving on the tab opens a tile**, so the first keystroke does something rather than asking
    // for another. This hung off a mode key until the tab existed; the tab becoming live is the same
    // moment, and one fewer thing to press.
    if build.open.is_none() {
        // **As tall as the space it fills**, taken from the map rather than from a constant. A number
        // here would be one facility's ceiling height baked into the editor — the mistake
        // `stack::datum` records fixing, where `OnCeiling` was hardcoded 2.4 m and hung the lights of
        // a 3.5 m room in mid-air. The map states its own height; that is the only number entitled to
        // answer this.
        build.open = Some(blank(&next_tile_id(&project), project.map.bounds.1));
        build.rung = DEFAULT_RUNG;
        build.at = (0, 0, 0);
        build.focus = 0;
        // **Arrive with something in hand.** Only fires when nothing was ever picked — the selection
        // otherwise persists — and without it the first `Enter` is a refusal, which is the worst
        // possible first impression of a tab. Liapis names the failure: a tool that will not let the
        // designer converge is where user fatigue starts.
        if state.editing(&project.library).is_none()
            && let Some(first) = project.library.descriptors.first().map(|d| d.id.clone())
        {
            state.selected_library_id = Some(first);
        }
        let id = build.open.as_ref().map(|c| c.id.clone()).unwrap_or_default();
        state
            .status
            .note(format!("building `{id}` — T F G H walk, Enter drops, Cmd+S saves"));
        return;
    }

    let Some(size) = build.open.as_ref().and_then(|c| match c.envelope {
        Envelope::Bounded { size } => Some(size),
        Envelope::Anchored => None,
    }) else {
        return;
    };
    let step = pitch(&build, &project);

    // The cursor. Walked, then clamped once — so a key held at an edge stops there rather than
    // accumulating an offset the next key has to undo.
    let mut at = build.at;
    if pressed(Action::BuildLeft) {
        at.0 -= 1;
    }
    if pressed(Action::BuildRight) {
        at.0 += 1;
    }
    if pressed(Action::BuildForward) {
        at.2 += 1;
    }
    if pressed(Action::BuildBack) {
        at.2 -= 1;
    }
    if pressed(Action::BuildUp) {
        at.1 += 1;
    }
    if pressed(Action::BuildDown) {
        at.1 -= 1;
    }
    if at != build.at {
        build.at = clamp(size, step, at);
        return;
    }

    // **The rung, latched.** Two rungs below the tile, because the tile itself is one cell and a grid
    // with one square positions nothing.
    if pressed(Action::BuildRung) {
        build.rung = match build.rung {
            SnapLevel::Tile | SnapLevel::Finer => SnapLevel::Fine,
            SnapLevel::Fine => SnapLevel::Finer,
        };
        let now = pitch(&build, &project);
        // Re-clamped: a coarser rung has fewer cells, so the cursor can be left outside one.
        build.at = clamp(size, now, build.at);
        let (nx, ny, nz) = cells(size, now);
        state
            .status
            .note(format!("rung {:.3} m — {nx} x {ny} x {nz} cells", now));
        return;
    }

    // **Drop what the library list has picked.** The piece is chosen on the mesh tab and dropped
    // here, which is the same right-hand list serving both tabs rather than a second browser — the
    // objection §3.2 of the compose-authoring plan raised against adding one.
    if pressed(Action::BuildDrop) {
        let Some(d) = state.editing(&project.library).cloned() else {
            state.status.problem("nothing picked — choose a piece in the list first".to_owned());
            return;
        };
        // **A member must name a library descriptor.** `ImportState::editing` falls back to the
        // focused *candidate* when nothing in the library is selected, and a candidate is a mesh
        // measured but not imported — so dropping one writes a tile naming an id `library.ron` does
        // not carry, which expands to nothing at stamp time. Refused at the door rather than
        // written and discovered later, which is this crate's rule for staged edits.
        if project.library.get(&d.id).is_none() {
            state.status.problem(format!(
                "`{}` is not in the library yet — accept it on the Meshes tab first",
                d.id
            ));
            return;
        }
        let span = crate::editor::brush_span(&d, 0.0);
        match place(&mut build, &d.id, span, 0.0, step) {
            Ok(i) => {
                // **Focus follows the drop.** `insert_sorted` answers where it landed, and the two
                // verbs that act on "this member" — turn and remove — mean the one you just put
                // down. Ignoring the index left them acting on whatever sorted first, which is a
                // different piece as soon as a tile holds two.
                build.focus = i;
                let n = build.open.as_ref().map_or(0, |c| c.members.len());
                state.status.note(format!("`{}` dropped — {n} in the tile", d.id));
            }
            Err(e) => state.status.problem(e),
        }
        return;
    }

    // A hole instead of a piece. `accepts` comes from the project's slot axis, first token — the
    // vocabulary is the closed list, so there is nothing here to invent.
    if pressed(Action::BuildSlot) {
        let Some(accepts) = project.vocab.slot.names().next().map(str::to_owned) else {
            state.status.problem(
                "no `slot` tokens declared — add one to vocab.ron before dropping a hole".to_owned(),
            );
            return;
        };
        match place_slot(&mut build, &accepts, step) {
            Ok(i) => {
                build.focus = i;
                state.status.note(format!("hole for `{accepts}` dropped"));
            }
            Err(e) => state.status.problem(e),
        }
        return;
    }

    // Turn and remove act on the **last member dropped**, which is what `focus` tracks. Turning is a
    // quarter, because a tile is a quarter-turn object: `from_compositions` learns one prototype per
    // quarter and anything between is a tile the solver cannot reproduce.
    if pressed(Action::BuildTurn) {
        // `focus` read before the mutable borrow — the closure would otherwise hold `build` twice.
        let focus = build.focus;
        if let Some(m) = build.open.as_mut().and_then(|c| c.members.get_mut(focus)) {
            m.yaw = (m.yaw + 90.0).rem_euclid(360.0);
            let said = format!("`{}` turned to {:.0}", m.id, m.yaw);
            state.status.note(said);
        }
        return;
    }
    if pressed(Action::BuildDropMember) {
        let focus = build.focus;
        let removed = build
            .open
            .as_mut()
            .and_then(|c| (focus < c.members.len()).then(|| c.members.remove(focus).id));
        match removed {
            Some(id) => {
                // Clamped to what is left rather than stepped back: removing the first member should
                // leave the focus on the new first, not underflow to the last.
                let left = build.open.as_ref().map_or(0, |c| c.members.len());
                build.focus = build.focus.min(left.saturating_sub(1));
                state.status.note(format!("`{id}` removed"));
            }
            None => state.status.note("nothing to remove".to_owned()),
        }
        return;
    }

    // **`Cmd+S` saves the tile.** The other half of the branch `editor::keys` guards: the key is
    // Global because the verb is, and what it saves is whatever the live context has open. Bound
    // once, so the census still holds every action to exactly one key.
    if pressed(Action::Save) {
        match save(&build, &mut project) {
            Ok(said) => state.status.note(said),
            Err(e) => state.status.problem(format!("NOT SAVED: {e}")),
        }
        return;
    }

    // A fresh tile, leaving whatever was saved on disk alone.
    if pressed(Action::BuildNew) {
        build.open = Some(blank(&next_tile_id(&project), project.map.bounds.1));
        build.at = (0, 0, 0);
        build.focus = 0;
        let id = build.open.as_ref().map(|c| c.id.clone()).unwrap_or_default();
        state.status.note(format!("new tile `{id}`"));
    }
}

/// The next unused `<kit>/tile_n` id, so `C` opens something rather than asking for a name first.
///
/// Named after the kit the project loaded, because a composition id shares a descriptor id's shape —
/// namespace and all — and a tile that does not carry its kit's name is one nobody can find later.
fn next_tile_id(project: &crate::project::Project) -> String {
    // **Only a real namespace counts.** `split('/').next()` on an id with no slash answers the whole
    // id, which named the fixture's first tile `wall/tile_1` after a wall. A kit whose pieces carry no
    // namespace has none to inherit, and `kit` is the honest stand-in.
    let kit = project
        .library
        .descriptors
        .first()
        .and_then(|d| d.id.split_once('/'))
        .map_or("kit", |(ns, _)| ns)
        .to_owned();
    for n in 1..=project.compositions.compositions.len() + 1 {
        let id = format!("{kit}/tile_{n}");
        if !project.compositions.compositions.iter().any(|c| c.id == id) {
            return id;
        }
    }
    format!("{kit}/tile")
}

/// One member of the tile as it currently stands on the stage.
#[derive(Component)]
pub struct StagedTile;

/// **Stand the tile up** — every member, through the same expander and the same spawner the Map uses.
///
/// # It goes through `composition::expand`, not straight at the members
///
/// `expand` is *"the one expander. The editor's ghost, the editor's save and the game's loader all
/// come through here, so a stamp cannot look one way in the tool and another in the game."* Spawning
/// the members directly would be a second reading of the same data — it would skip host resolution,
/// nested groups and paint order, and the tile would look right here and wrong once stamped. That is
/// the exact failure this crate keeps being rewritten to avoid.
///
/// # Rebuilt whole, on change
///
/// The tile is a handful of members and rebuilding is a despawn plus a spawn; incremental patching
/// would be a second model of what is on the stage, to be kept in step with the first. Gated on the
/// resources actually read, so a keystroke that changes nothing redraws nothing — the D3 defect
/// (2026-08-11) is what a rebuild fired on `is_changed()` alone costs.
pub fn drive_build_preview(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mode: Res<crate::tiles::Mode>,
    build: Res<Build>,
    project: Res<crate::project::Project>,
    mut state: ResMut<crate::tiles::ImportState>,
    staged: Query<Entity, With<StagedTile>>,
) {
    if !(build.is_changed() || mode.is_changed() || project.is_changed()) {
        return;
    }
    // A system that stops running cannot despawn what it drew, so leaving the mode clears here rather
    // than being gated away.
    for e in &staged {
        commands.entity(e).despawn();
    }
    if *mode != crate::tiles::Mode::Tiles {
        return;
    }
    let Some(comp) = build.open.as_ref() else {
        return;
    };
    if comp.members.is_empty() {
        return;
    }
    let Envelope::Bounded { size } = comp.envelope else {
        return;
    };

    // **The envelope as a map**, floor at zero and the declared bounds — the same scratch
    // `composition::interface` builds, so `stack::resolve_y` answers here exactly as it will in the
    // game. The stage's own offset is applied by `spawn_piece`'s `origin`, so the tile is authored at
    // the origin and drawn four kilometres away.
    let scratch = emerge_core::map::Map {
        version: emerge_core::map::MAP_VERSION,
        name: "build_stage".to_owned(),
        origin: (0.0, 0.0, 0.0),
        bounds: size,
        ..Default::default()
    };
    // The tile in hand is not on disk yet, so it is handed to `expand` alongside the saved ones —
    // which is also what lets a member nest a composition that *is* saved.
    let mut comps = project.compositions.compositions.clone();
    match comps.iter().position(|c| c.id == comp.id) {
        Some(i) => comps[i] = comp.clone(),
        None => comps.push(comp.clone()),
    }
    let stamp = emerge_core::composition::Stamped {
        id: "build".to_owned(),
        of: comp.id.clone(),
        ..Default::default()
    };
    let expanded =
        match emerge_core::composition::expand(&scratch, &[stamp], &comps, &project.library) {
            Ok(e) => e,
            // **Named, not swallowed.** A tile that cannot stand up is the author's next problem, and
            // a silently empty stage looks exactly like a tile with nothing in it.
            Err(e) => {
                if state.status.line() != e {
                    state.status.problem(e);
                }
                return;
            }
        };
    let mut with_rows = scratch.clone();
    with_rows.placements.extend(expanded.placements.iter().cloned());
    let ys = match emerge_core::stack::resolve_y(&with_rows, &project.library) {
        Ok(ys) => ys,
        Err(e) => {
            if state.status.line() != e {
                state.status.problem(e);
            }
            return;
        }
    };

    let stage = crate::stages::TILE;
    for (k, p) in expanded.placements.iter().enumerate() {
        let Some(base) = project.library.get(&p.descriptor) else {
            continue;
        };
        let d = match &p.patch {
            Some(patch) => base.patched_with(patch),
            None => base.clone(),
        };
        let Some(&y) = ys.get(k) else { continue };
        if let Some(e) = crate::editor::spawn_piece(
            &mut commands,
            &assets,
            &d,
            p.at,
            p.yaw,
            p.tip,
            (stage.x, stage.y, stage.z),
            y,
        ) {
            commands.entity(e).insert(StagedTile);
        }
    }
}

/// **The tile's box, its grid, and the cell the cursor is on.**
///
/// Gizmos rather than meshes: they are drawn per frame and need no despawning, which is right for
/// something that moves every keystroke. The cursor is the thing an author steers by — without it,
/// walking the grid moves a number in a panel and nothing on the stage.
pub fn draw_build_grid(
    mode: Res<crate::tiles::Mode>,
    build: Res<Build>,
    project: Res<crate::project::Project>,
    mut gizmos: Gizmos,
) {
    if *mode != crate::tiles::Mode::Tiles {
        return;
    }
    let Some(Envelope::Bounded { size }) = build.open.as_ref().map(|c| c.envelope) else {
        return;
    };
    let stage = crate::stages::TILE;
    let centre = stage + Vec3::new(0.0, size.1 * 0.5, 0.0);

    // The envelope. `cube` takes a transform whose SCALE is the size — 0.19 spells it that way.
    gizmos.cube(
        Transform::from_translation(centre).with_scale(Vec3::new(size.0, size.1, size.2)),
        crate::chrome::DIM,
    );

    // The floor grid at the live rung, so the squares drawn are the squares the cursor walks. Bounded
    // by a cell count rather than a plane, so it stops at the tile.
    let pitch = pitch(&build, &project);
    let (nx, _, nz) = cells(size, pitch);
    gizmos.grid(
        Isometry3d::new(stage, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        UVec2::new(nx, nz),
        Vec2::splat(pitch),
        crate::editor::GRID_LINE,
    );

    // **The cursor cell**, drawn as the box a piece dropped here would have its corner on. In the
    // accent colour, because it is the one thing on the stage that answers "where am I".
    let (cx, cy, cz) = cell_corner(size, pitch, build.at);
    let half = pitch * 0.5;
    gizmos.cube(
        Transform::from_translation(stage + Vec3::new(cx + half, cy + half, cz + half))
            .with_scale(Vec3::splat(pitch)),
        crate::chrome::ACCENT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: (f32, f32, f32) = (1.0, 2.4, 1.0);
    /// The default divisor's first rung — thirds of a tile.
    const FINE: f32 = 1.0 / 3.0;

    #[test]
    fn a_tile_divides_into_the_rungs_number_of_cells() {
        assert_eq!(cells(TILE, FINE), (3, 7, 3), "1 m across is 3 thirds; 2.4 m up is 7");
        assert_eq!(cells(TILE, 1.0), (1, 2, 1), "the bare rung is the whole tile");
        // Never zero, whatever the pitch: a grid with no cells has nowhere to stand.
        assert_eq!(cells(TILE, 99.0), (1, 1, 1));
    }

    /// **The flush position falls out of the corner rule.**
    ///
    /// `docs/2026-08-09-compose-authoring-plan.md` §4: *"A 0.5 m lattice cannot seat a 0.1 m wall.
    /// Flush is at −0.45, off the lattice by construction."* That is what `compose::flushed` was
    /// invented for, and what FVS-R-15 cut. Stating the rule on the piece's **minimum corner** rather
    /// than its centre reaches −0.45 with no verb at all: cell zero's corner *is* the tile's edge.
    #[test]
    fn a_wall_dropped_in_the_edge_cell_sits_flush() {
        let wall = (0.1, 1.0);
        let (at, lift) = drop_at(TILE, FINE, (0, 0, 0), wall);
        assert!((at.0 - -0.45).abs() < 1e-6, "{at:?} is not flush against the west edge");
        assert_eq!(lift, 0.0, "it stands on the tile's floor");

        // And the rung does not matter: the edge cell's corner is the edge at every rung, which is
        // what makes flush reachable without anybody choosing a divisor for it.
        for pitch in [1.0, FINE, FINE / 3.0, 0.25] {
            let (at, _) = drop_at(TILE, pitch, (0, 0, 0), wall);
            assert!((at.0 - -0.45).abs() < 1e-6, "pitch {pitch}: {at:?}");
        }
    }

    /// A cell-sized floor fills the tile, which is the other half of the same rule.
    #[test]
    fn a_floor_dropped_at_the_origin_fills_the_tile() {
        let (at, lift) = drop_at(TILE, 1.0, (0, 0, 0), (1.0, 1.0));
        assert_eq!((at, lift), ((0.0, 0.0), 0.0), "a 1 m piece centred in a 1 m tile");
    }

    /// Walking off an edge stops at it. Wrapping would move a piece to the far side of the tile in
    /// one keystroke, which is never what the key meant.
    #[test]
    fn the_cursor_clamps_to_the_tile_rather_than_wrapping() {
        assert_eq!(clamp(TILE, FINE, (-5, -5, -5)), (0, 0, 0));
        assert_eq!(clamp(TILE, FINE, (99, 99, 99)), (2, 6, 2));
        assert_eq!(clamp(TILE, FINE, (1, 3, 2)), (1, 3, 2), "an interior cell is untouched");
    }

    fn desc(id: &str) -> Member {
        Member {
            id: id.to_owned(),
            body: Body::Descriptor {
                id: format!("site/{id}"),
                tip: (0, 0),
                on: None,
                patch: None,
            },
            at: (0.0, 0.0),
            yaw: 0.0,
            lift: 0.0,
            paint: 0,
            of_fingerprint: None,
            note: None,
        }
    }

    /// Ids come from the piece and are made unique by a suffix — nobody types a member name.
    #[test]
    fn member_ids_are_derived_and_never_collide() {
        let mut ms: Vec<Member> = Vec::new();
        assert_eq!(fresh_id(&ms, "site/wall"), "wall");
        ms.push(desc("wall"));
        assert_eq!(fresh_id(&ms, "site/wall"), "wall_2");
        ms.push(desc("wall_2"));
        assert_eq!(fresh_id(&ms, "site/wall"), "wall_3");
        // A piece with no namespace keeps its whole name.
        assert_eq!(fresh_id(&ms, "floor"), "floor");
    }

    /// **The list stays in the canonical order**, so what an author reads is what gets written and
    /// `composition::validate` never has to refuse the file the editor just produced.
    #[test]
    fn members_are_kept_sorted_by_id() {
        let mut ms = Vec::new();
        for id in ["wall", "floor", "sconce"] {
            insert_sorted(&mut ms, desc(id));
        }
        let ids: Vec<&str> = ms.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["floor", "sconce", "wall"]);
    }

    /// The whole gesture, end to end: open a tile, walk the grid, drop a floor and a wall, and get a
    /// group `composition::validate` accepts.
    #[test]
    fn a_built_tile_validates() {
        use emerge_core::descriptor::{Descriptor, Extent};
        let mut b = Build { open: Some(blank("site/tile_wall_n", 2.4)), rung: DEFAULT_RUNG, ..Default::default() };

        place(&mut b, "site/floor", (1.0, 1.0), 0.0, 1.0).expect("the floor drops");
        b.at = (0, 0, 0);
        place(&mut b, "site/wall", (0.1, 1.0), 0.0, FINE).expect("the wall drops");

        let comp = b.open.take().expect("still open");
        assert_eq!(comp.members.len(), 2);

        let piece = |id: &str, w: f32, d: f32, h: f32| Descriptor {
            id: id.to_owned(),
            extent: Extent { footprint: Some((w, d)), height: Some(h) },
            ..Default::default()
        };
        let lib = emerge_core::library::Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: vec![
                piece("site/floor", 1.0, 1.0, 0.06),
                piece("site/wall", 0.1, 1.0, 2.4),
            ],
        };
        emerge_core::composition::validate(&[comp], &lib).expect("a built tile is a legal tile");
    }

    /// A hole drops on the same grid with the same gesture, and lands inside the envelope — which is
    /// what `composition::validate` requires of one.
    #[test]
    fn a_dropped_slot_lands_inside_the_envelope() {
        let mut b = Build { open: Some(blank("t", 2.4)), rung: DEFAULT_RUNG, ..Default::default() };
        // The far corner cell, which is the closest a slot can legally get to a seam.
        b.at = clamp((1.0, 2.4, 1.0), FINE, (99, 99, 99));
        place_slot(&mut b, "wall-fixture", FINE).expect("the hole drops");

        let comp = b.open.take().expect("still open");
        let m = &comp.members[0];
        assert!(m.at.0.abs() < 0.5 && m.at.1.abs() < 0.5, "{:?} is on or past a seam", m.at);
        assert!(m.lift >= 0.0 && m.lift < 2.4, "lift {} leaves the envelope", m.lift);
    }

    /// Dropping into a tile that was never opened is a refusal that says what to press, not a panic
    /// and not a silently-discarded keystroke.
    #[test]
    fn dropping_with_no_tile_open_says_so() {
        let mut b = Build::default();
        let e = place(&mut b, "site/floor", (1.0, 1.0), 0.0, 1.0).expect_err("nothing to drop into");
        assert!(e.contains("no tile open"), "{e}");
    }
}
