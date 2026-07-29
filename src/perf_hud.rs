//! Dev-only performance overlay — **debug builds only** (gated exactly like `devshot`).
//!
//! You cannot optimize what you cannot measure. This registers Bevy's frame-time / entity-count /
//! system-information diagnostics and paints a compact top-left readout (FPS, frame ms, entity count,
//! process CPU %, process memory) that toggles with **F4**. It is the "before/after ruler" for the
//! performance work: watch FPS + frame ms while flipping a change on/off.
//!
//! For *per-system* attribution (which system costs what), build with `--features bevy/trace_tracy` and
//! read the flamegraph — the heavy simulation systems carry `info_span!`s for exactly this. An on-screen
//! per-system breakdown would need custom timing plumbing; tracy is the idiomatic, zero-maintenance path.
//!
//! Determinism note: everything here is windowed-only `Update`/measurement work — it never touches the
//! pinned `FixedUpdate` state, and the headless `sim_harness` never registers this plugin, so goldens are
//! untouched by construction. Stripped from release with the module (see `lib.rs`), same as `devshot`.

use bevy::diagnostic::{
    DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
    SystemInformationDiagnosticsPlugin,
};
use bevy::prelude::*;

/// Whether the overlay is currently painted. Starts hidden — press **F4** to reveal.
#[derive(Resource, Default)]
struct PerfHudVisible(bool);

/// Marker on the single overlay text node.
#[derive(Component)]
struct PerfHudText;

pub struct PerfHudPlugin;

impl Plugin for PerfHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
            SystemInformationDiagnosticsPlugin,
        ))
        .init_resource::<PerfHudVisible>()
        // After the on-disk face is loaded, so the overlay can use `FontAssets` rather than the
        // embedded subset (see `spawn_perf_hud`).
        .add_systems(Startup, spawn_perf_hud.after(crate::ui::theme::load_fonts))
        .add_systems(Update, (toggle_perf_hud, update_perf_hud).chain());
    }
}

/// Spawn the overlay once, hidden. A single UI text node, top-left, on a dim panel for legibility
/// over any scene.
///
/// **Uses `FontAssets`, not `Handle::default()`.** This was the one place in the codebase still
/// reaching for Bevy's embedded face, which `ui::theme` documents as a **95-codepoint** subset that
/// renders every non-ASCII glyph as tofu. It happened to look fine only because this overlay's copy
/// is pure ASCII — i.e. it was one `·` away from being a bug, and nothing said so.
fn spawn_perf_hud(mut commands: Commands, fonts: Res<crate::ui::theme::FontAssets>, theme: Res<crate::ui::theme::UiTheme>) {
    commands.spawn((
        PerfHudText,
        Text::new("perf: F4"),
        TextFont {
            font: FontSource::Handle(fonts.body.clone()),
            font_size: FontSize::Px(14.0),
            ..default()
        },
        // From the theme rather than a copied literal — this used to hold the old phosphor green as
        // a hardcoded `srgb(0.55, 1.0, 0.62)` with a comment claiming it "matches the UI theme
        // accent", which stopped being true the moment the palette desaturated.
        TextColor(theme.accent),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(6.0),
            left: Val::Px(6.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
            display: Display::None, // hidden until toggled
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        GlobalZIndex(1000), // above the HUD (10) and menus (100)
        Pickable::IGNORE,
    ));
}

/// **F4** flips the overlay's visibility (its node `display`).
fn toggle_perf_hud(
    actions: crate::input::Actions,
    mut visible: ResMut<PerfHudVisible>,
    mut node: Query<&mut Node, With<PerfHudText>>,
) {
    if !actions.just_pressed(crate::input::Action::DevPerfHud) {
        return;
    }
    visible.0 = !visible.0;
    if let Ok(mut node) = node.single_mut() {
        node.display = if visible.0 { Display::Flex } else { Display::None };
    }
}

/// Repaint the readout while visible. Skips all string work when hidden.
fn update_perf_hud(
    visible: Res<PerfHudVisible>,
    diagnostics: Res<DiagnosticsStore>,
    mut text: Query<&mut Text, With<PerfHudText>>,
) {
    if !visible.0 {
        return;
    }
    let Ok(mut text) = text.single_mut() else {
        return;
    };

    // `smoothed()` for the noisy frame metrics; current `value()` for the counts.
    let read = |path: &bevy::diagnostic::DiagnosticPath, smoothed: bool| -> Option<f64> {
        diagnostics
            .get(path)
            .and_then(|d| if smoothed { d.smoothed() } else { d.value() })
    };

    let fps = read(&FrameTimeDiagnosticsPlugin::FPS, true).unwrap_or(0.0);
    let frame_ms = read(&FrameTimeDiagnosticsPlugin::FRAME_TIME, true).unwrap_or(0.0);
    let ents = read(&EntityCountDiagnosticsPlugin::ENTITY_COUNT, false).unwrap_or(0.0);
    let cpu = read(&SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE, true).unwrap_or(0.0);
    let mem = read(&SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE, true).unwrap_or(0.0);

    text.0 = format!(
        "FPS  {fps:6.1}\nms   {frame_ms:6.2}\nents {ents:6.0}\ncpu  {cpu:5.1}%\nmem  {mem:6.1} MB",
    );
}
