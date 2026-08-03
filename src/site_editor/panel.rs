//! **The editor's chrome** — the palette, the fault list, and the status line.
//!
//! Built the way `research_room::editor` builds its palette: stock `bevy_ui` plus `ui::widgets`,
//! spawned and despawned on toggle rather than hidden, at `Z_MENU` with a `TabGroup` so the shared
//! menu systems give it hover tint and keyboard navigation for free.
//!
//! # One observer, not forty-five
//!
//! A Bevy observer closure must be non-capturing, which is why `research_room::editor` unrolls its
//! nine prop buttons through a macro guarded by `const _: () = assert!(PROPS.len() == 9, ..)`. That
//! does not scale to the site kit's 45 pieces.
//!
//! Bevy 0.19's `ui_widgets::Activate` carries the activated entity (`Activate { entity }`), so the
//! piece can live on the button as a [`PaletteEntry`] component and **one** global observer can read
//! it back. No macro, no const assert, and adding a piece to `SitePiece::ALL` needs no edit here.
//!
//! # Chrome constraints this obeys
//!
//! * `theme.rs` unit-tests `MAX_UI_CHROMA = 0.12`; only `anomaly`, `danger` and `warn` are exempt.
//!   Every colour used here is a theme token, so a new saturated one cannot sneak in.
//! * The shipped font is 1350 codepoints and `✓ ▶ ⚠ ★` are **not** among them. Text here stays ASCII
//!   or uses `ui::theme::glyph`.
//! * `Val::Px` is scaled by `UiScale`; `text_colored` emits `FontSize::Rem`, so the panel follows the
//!   accessibility text-scale setting without doing anything.

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, ScrollArea};

use crate::site::pieces::SitePiece;
use crate::ui::theme::{FontAssets, UiTheme, Z_MENU};
use crate::ui::widgets::{button_visual, panel, text, text_colored};

use super::EditorState;

/// Root of the panel; despawned wholesale when the editor closes.
#[derive(Component)]
pub struct EditorRoot;

/// The kit piece a palette button places. Read by the one observer below.
#[derive(Component, Clone, Copy)]
pub struct PaletteEntry(pub SitePiece);

/// The node the fault list is rebuilt into.
#[derive(Component)]
pub struct FaultList;

/// The one-line readout of the last thing that happened.
#[derive(Component)]
pub struct StatusLabel;

/// The line naming the current brush and manipulation mode.
#[derive(Component)]
pub struct ModeLabel;

/// How tall the two scrolling lists are allowed to get, in logical px.
const FAULTS_MAX_H: f32 = 190.0;
const PALETTE_MAX_H: f32 = 300.0;

/// Edge of a palette row's preview box, logical px. Reserved whether or not the bake has reached that
/// piece, so arriving thumbnails never reflow the list under the cursor.
const THUMB_SLOT: f32 = 34.0;

/// Build the panel. Mirrors `research_room::editor::spawn_palette`'s shape.
pub fn spawn(
    commands: &mut Commands,
    theme: &UiTheme,
    fonts: &FontAssets,
    thumbs: Option<&super::thumbs::Thumbnails>,
) {
    let root = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(theme.space_md),
        top: Val::Px(theme.space_md),
        width: Val::Px(330.0),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(theme.space_sm),
        padding: UiRect::all(Val::Px(theme.space_md)),
        ..default()
    };

    commands
        .spawn((
            EditorRoot,
            panel(theme, root),
            GlobalZIndex(Z_MENU),
            TabGroup::new(0),
        ))
        .with_children(|p| {
            p.spawn(text_colored(
                theme,
                fonts,
                "SITE-67 EDITOR",
                theme.font_body,
                theme.accent,
            ));
            // The keys, stated up front. Every one is an F-key, a chord or a non-activation key,
            // because a focused palette button eats Space and Enter (see `input::KeyboardOwned`).
            p.spawn(text_colored(
                theme,
                fonts,
                "F7 close · drag to move · [ ] rotate\n\
                 G gizmo mode · R translate/rotate · Del delete\n\
                 Ctrl+Z undo · Ctrl+Y redo · Ctrl+S save",
                theme.font_body * 0.8,
                theme.text_muted,
            ));

            p.spawn((
                text_colored(theme, fonts, "", theme.font_body * 0.85, theme.text),
                StatusLabel,
            ));
            p.spawn((
                text_colored(theme, fonts, "", theme.font_body * 0.85, theme.text_muted),
                ModeLabel,
            ));

            p.spawn(text_colored(
                theme,
                fonts,
                "RULES",
                theme.font_body * 0.85,
                theme.warn,
            ));
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(theme.space_xs),
                    max_height: Val::Px(FAULTS_MAX_H),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ScrollArea::default(),
                FaultList,
            ));

            p.spawn(text_colored(
                theme,
                fonts,
                "PLACE",
                theme.font_body * 0.85,
                theme.warn,
            ));
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(theme.space_xs),
                    max_height: Val::Px(PALETTE_MAX_H),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ScrollArea::default(),
            ))
            .with_children(|list| {
                // Every piece the kit knows, in `SitePiece::ALL` order — which is hand-maintained and
                // grouped by role (structure, dressing, the living half), so the palette reads the way
                // the kit is organised rather than the way the enum happens to be declared.
                for piece in SitePiece::ALL {
                    let thumb = thumbs.and_then(|t| t.image(*piece));
                    list.spawn((button_visual(theme), PaletteEntry(*piece)))
                        // `button_visual` already carries a `Node` (min-width 220 px, centred — sized
                        // for a menu button). Passing a second one in the same bundle is a **panic**,
                        // not an override: Bevy rejects a bundle with duplicate components. Inserting
                        // afterwards replaces it, which is what "a narrower button" actually means.
                        .insert(Node {
                            min_width: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            padding: UiRect::axes(
                                Val::Px(theme.space_sm),
                                Val::Px(theme.space_xs),
                            ),
                            justify_content: JustifyContent::FlexStart,
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|b| {
                            // The preview. A fixed-size box whether or not the bake has reached this
                            // piece yet, so rows never reflow as the thumbnails arrive.
                            let mut slot = b.spawn(Node {
                                width: Val::Px(THUMB_SLOT),
                                height: Val::Px(THUMB_SLOT),
                                margin: UiRect::right(Val::Px(theme.space_sm)),
                                flex_shrink: 0.0,
                                ..default()
                            });
                            if let Some(image) = thumb {
                                // `ImageNode::new`, never `default()` — the default is an invisible
                                // 1x1 transparent texture (docs/ui.md §5 trap 6).
                                slot.insert(ImageNode::new(image));
                            }
                            b.spawn(text(theme, fonts, format!("{piece:?}"), theme.font_body * 0.8));
                        });
                }
            });
        });
}

/// The single observer serving every palette button.
///
/// Arms the brush rather than placing immediately: a click that dropped a prop would have to invent a
/// position, and the only honest one is where the author points next.
pub fn on_palette_click(
    activate: On<Activate>,
    entries: Query<&PaletteEntry>,
    state: Option<ResMut<EditorState>>,
) {
    let Ok(entry) = entries.get(activate.entity) else {
        // Not one of ours — this observer sees every button in the game.
        return;
    };
    let Some(mut state) = state else { return };
    state.brush = entry.0;
    state.status = format!("{:?} armed — click the floor to place", entry.0);
}

/// Repaint the status and mode lines. Cheap enough to run every frame, and guarded so it only writes
/// when the text actually changes — the same shape as `research_room::editor::refresh_quantity_label`.
pub fn refresh_labels(
    state: Res<EditorState>,
    mut status: Query<&mut Text, (With<StatusLabel>, Without<ModeLabel>)>,
    mut mode: Query<&mut Text, (With<ModeLabel>, Without<StatusLabel>)>,
) {
    let dirty = state
        .doc
        .as_ref()
        .is_some_and(|d| d.dirty)
        .then_some(" *UNSAVED*")
        .unwrap_or("");
    for mut t in &mut status {
        let want = format!("{}{dirty}", state.status);
        if t.0 != want {
            t.0 = want;
        }
    }
    for mut t in &mut mode {
        let sel = match state.selected {
            Some(i) => format!("#{i}"),
            None => "none".to_owned(),
        };
        let want = format!(
            "brush {:?}  ·  selected {sel}  ·  {}",
            state.brush,
            if state.gizmo_mode { "GIZMO" } else { "DRAG" }
        );
        if t.0 != want {
            t.0 = want;
        }
    }
}

/// Rebuild the fault list, but only when something actually changed.
///
/// `EditorState::panel_dirty` is set by whatever mutated the document, rather than this system
/// diffing the fault strings every frame — the flag is exact and the diff would allocate 60 times a
/// second to usually learn nothing.
pub fn refresh_faults(
    mut commands: Commands,
    mut state: ResMut<EditorState>,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    lists: Query<Entity, With<FaultList>>,
) {
    if !state.panel_dirty {
        return;
    }
    state.panel_dirty = false;

    let faults: Vec<String> = state
        .doc
        .as_ref()
        .map(|d| d.faults.iter().map(|f| f.message.clone()).collect())
        .unwrap_or_default();

    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|p| {
            if faults.is_empty() {
                // §1.4 of docs/ui.md: an unmet condition is an instruction, and an empty panel reads
                // as a bug. Say what the state IS.
                p.spawn(text_colored(
                    &theme,
                    &fonts,
                    "all placements legal",
                    theme.font_body * 0.8,
                    theme.text_muted,
                ));
                return;
            }
            for f in &faults {
                p.spawn(text_colored(
                    &theme,
                    &fonts,
                    f.clone(),
                    theme.font_body * 0.75,
                    theme.danger,
                ));
            }
        });
    }
}
