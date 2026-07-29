//! **The screen layout owner** — a 3×3 region grid that panels are parented into.
//!
//! # The bug this exists to make impossible
//!
//! Every panel used to pick its own corner with `position_type: Absolute` and a hardcoded offset.
//! Nothing owned the screen, so nothing could notice a collision — and there was one: in
//! `AppState::InGame`, `containment_hud` anchored bottom-left at `space_lg` (20 px) while the
//! `hud` roster strip anchored bottom-left at `space_md` (12 px). They drew on top of each other.
//! `AppState::Site` was worse: `site_hud`, `research_hud`, `review` and `records` claimed all four
//! corners independently, with no way to add a fifth panel without picking a fight.
//!
//! With a region owner, two panels in the same region **stack in a flex column** instead of
//! overlapping, and a test can assert that every panel resolved to a region at all.
//!
//! # Why a grid and not nine absolutely-positioned boxes
//!
//! The regions must partition the screen exactly, with no gaps and no overlap, at any resolution.
//! That is what CSS Grid is for, and Bevy 0.19 ships it (Taffy 0.10). Nine equal `flex(1.0)` tracks
//! in row-major auto-flow order.
//!
//! Deliberately **no [`GridPlacement`](bevy::ui::GridPlacement)**: its `start`/`end`/`span`
//! constructors panic on `0` and have no `try_` variant, which this codebase's no-panic rule cannot
//! accept. Auto-flow places the nine children by spawn order and has no panic surface.
//!
//! # Picking
//!
//! The frame covers the whole window, so the frame and every region carry [`Pickable::IGNORE`] or
//! they would swallow every click meant for the world. `Pickable` is per-entity, so panels parented
//! into a region stay pickable — which is what lets the verb bar have real hit targets.
//!
//! Windowed-only, `Update`/`OnEnter`/`OnExit`, reads nothing the sim writes.

use bevy::prelude::*;

use super::state::AppState;
use super::theme::{UiTheme, Z_PANEL};

/// A screen region. Row-major order **is** the grid auto-flow order — do not reorder the variants
/// without reordering the spawn loop in [`spawn_frame`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Region {
    TopLeft,
    TopCenter,
    TopRight,
    MidLeft,
    MidCenter,
    MidRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Region {
    /// Row-major, matching the grid's auto-flow.
    pub const ALL: [Region; 9] = [
        Region::TopLeft,
        Region::TopCenter,
        Region::TopRight,
        Region::MidLeft,
        Region::MidCenter,
        Region::MidRight,
        Region::BottomLeft,
        Region::BottomCenter,
        Region::BottomRight,
    ];

    fn index(self) -> usize {
        match self {
            Region::TopLeft => 0,
            Region::TopCenter => 1,
            Region::TopRight => 2,
            Region::MidLeft => 3,
            Region::MidCenter => 4,
            Region::MidRight => 5,
            Region::BottomLeft => 6,
            Region::BottomCenter => 7,
            Region::BottomRight => 8,
        }
    }

    /// Which edge panels hug horizontally.
    fn align(self) -> AlignItems {
        match self {
            Region::TopLeft | Region::MidLeft | Region::BottomLeft => AlignItems::Start,
            Region::TopCenter | Region::MidCenter | Region::BottomCenter => AlignItems::Center,
            Region::TopRight | Region::MidRight | Region::BottomRight => AlignItems::End,
        }
    }

    /// Which edge panels hug vertically. Bottom regions justify to `End` so a second panel added to
    /// a region grows *upward* from the bottom edge rather than pushing the first one off-screen.
    fn justify(self) -> JustifyContent {
        match self {
            Region::TopLeft | Region::TopCenter | Region::TopRight => JustifyContent::Start,
            Region::MidLeft | Region::MidCenter | Region::MidRight => JustifyContent::Center,
            Region::BottomLeft | Region::BottomCenter | Region::BottomRight => JustifyContent::End,
        }
    }
}

/// Marks the frame root so it can be despawned with the screen.
#[derive(Component)]
pub struct HudFrame;

/// Marks a region container. Carries its own [`Region`] so a test can verify the mapping without
/// reaching into [`HudRegions`].
#[derive(Component)]
pub struct RegionNode(pub Region);

/// Region container entities, indexed by [`Region::index`].
///
/// A fixed array rather than a `HashMap` so there is no iteration order to reason about — this is a
/// UI resource, but `tests/determinism_lint.rs` scans `src/ui/` and the cheapest way to be right is
/// to have no map to iterate.
#[derive(Resource, Default)]
pub struct HudRegions([Option<Entity>; 9]);

impl HudRegions {
    pub fn get(&self, r: Region) -> Option<Entity> {
        self.0[r.index()]
    }
}

pub struct HudLayoutPlugin;

impl Plugin for HudLayoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudRegions>()
            // Both screens that host panels get a frame. `OnEnter` ordering matters: every panel
            // spawner must run *after* this, which is expressed at the panel's registration with
            // `.after(layout::spawn_frame)`.
            .add_systems(OnEnter(AppState::InGame), spawn_frame)
            .add_systems(OnEnter(AppState::Site), spawn_frame)
            .add_systems(OnExit(AppState::InGame), despawn_frame)
            .add_systems(OnExit(AppState::Site), despawn_frame);
    }
}

/// Spawn the frame and record every region entity.
///
/// Public so panel plugins can order themselves `.after(spawn_frame)`.
pub fn spawn_frame(mut commands: Commands, theme: Res<UiTheme>, mut regions: ResMut<HudRegions>) {
    let mut slots: [Option<Entity>; 9] = [None; 9];

    commands
        .spawn((
            HudFrame,
            Node {
                display: Display::Grid,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                // Three equal columns and rows. `flex(1.0)` rather than `fr(1.0)`: `fr` gives each
                // track a content-based minimum, so one wide panel would shove the whole grid out
                // of alignment. `flex` is `minmax(0, 1fr)` and holds the thirds.
                grid_template_columns: RepeatedGridTrack::flex(3, 1.0),
                grid_template_rows: RepeatedGridTrack::flex(3, 1.0),
                padding: UiRect::all(Val::Px(theme.space_md)),
                ..default()
            },
            GlobalZIndex(Z_PANEL),
            // The frame is the size of the window. Without this it would eat every click intended
            // for the world.
            Pickable::IGNORE,
        ))
        .with_children(|frame| {
            // Spawn order IS the grid placement — see the module note on avoiding `GridPlacement`.
            for region in Region::ALL {
                let id = frame
                    .spawn((
                        RegionNode(region),
                        Node {
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            align_items: region.align(),
                            justify_content: region.justify(),
                            row_gap: Val::Px(theme.space_sm),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .id();
                slots[region.index()] = Some(id);
            }
        });

    regions.0 = slots;
}

fn despawn_frame(
    mut commands: Commands,
    frames: Query<Entity, With<HudFrame>>,
    mut regions: ResMut<HudRegions>,
) {
    for e in &frames {
        commands.entity(e).despawn();
    }
    regions.0 = [None; 9];
}

/// Spawn `bundle` as a panel inside `region`.
///
/// Returns the panel entity so the caller can add children. If the frame is not up yet this returns
/// `None` and spawns nothing — callers must order themselves `.after(spawn_frame)`, and a panel
/// that silently vanished is caught by `layout_liveness` in `tests/replay.rs` rather than by a
/// player noticing a missing HUD.
pub fn panel_in<'a>(
    commands: &'a mut Commands,
    regions: &HudRegions,
    region: Region,
    bundle: impl Bundle,
) -> Option<EntityCommands<'a>> {
    let parent = regions.get(region)?;
    let mut ec = commands.spawn(bundle);
    ec.insert(ChildOf(parent));
    Some(ec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_region_has_a_unique_index_covering_the_grid() {
        // The index IS the grid slot, so a duplicate or a gap would silently stack two regions in
        // one cell and leave another empty.
        let mut seen = [false; 9];
        for r in Region::ALL {
            let i = r.index();
            assert!(!seen[i], "{r:?} reuses index {i}");
            seen[i] = true;
        }
        assert!(seen.iter().all(|&s| s), "some grid slot has no region");
    }

    #[test]
    fn all_is_in_row_major_order() {
        // Grid auto-flow places children by spawn order, and `spawn_frame` iterates `ALL`. If the
        // two ever disagree, every panel lands in the wrong corner — a bug that would look like a
        // layout mistake rather than an ordering one.
        for (i, r) in Region::ALL.iter().enumerate() {
            assert_eq!(r.index(), i, "{r:?} is out of row-major order in ALL");
        }
    }

    #[test]
    fn bottom_regions_grow_upward_and_top_regions_downward() {
        // A second panel added to a bottom region must stack *above* the first, not push it off
        // the bottom edge of the screen.
        assert_eq!(Region::BottomLeft.justify(), JustifyContent::End);
        assert_eq!(Region::TopLeft.justify(), JustifyContent::Start);
        assert_eq!(Region::MidLeft.justify(), JustifyContent::Center);
    }

    #[test]
    fn regions_hug_the_edge_their_name_claims() {
        assert_eq!(Region::TopLeft.align(), AlignItems::Start);
        assert_eq!(Region::TopCenter.align(), AlignItems::Center);
        assert_eq!(Region::TopRight.align(), AlignItems::End);
        assert_eq!(Region::BottomRight.align(), AlignItems::End);
    }

    #[test]
    fn an_empty_registry_yields_no_parent() {
        // `panel_in` must degrade to "spawn nothing" rather than unwrapping a missing region.
        let regions = HudRegions::default();
        for r in Region::ALL {
            assert!(regions.get(r).is_none());
        }
    }
}
