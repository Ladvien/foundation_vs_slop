//! User settings — player preferences persisted to disk, distinct from the version-controlled,
//! fail-loud dev config (`assets/config/config.ron`). This is *user* data: a missing file is
//! seeded with defaults, and a malformed one falls back to defaults with a `warn!` (it must never
//! panic the game the way the dev config deliberately does).
//!
//! **Windowed-only.** [`SettingsPlugin`] is added by `ui::UiPlugin` (registered in `lib::run`), so
//! the headless replay harness performs no filesystem IO and its settings resources stay at
//! defaults.
//!
//! Scope note: this currently carries **HUD** + **accessibility** preferences. Audio volumes and
//! keybindings are added when their gated phases land (they depend on sibling-worktree rewrites);
//! every field is `#[serde(default)]` so growing the schema never breaks an existing save file.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// How much of the squad roster strip to show — the core of player-controllable HUD density.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RosterDetail {
    Hidden,
    Compact,
    Full,
}

/// Player-controllable HUD density (Game-UI Guidance §2). A [`Resource`] and serialized.
#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct HudSettings {
    pub show_boss_bar: bool,
    pub roster_detail: RosterDetail,
    /// Whole-HUD scale (also nudged by the accessibility text-scale). 0.75..=1.5.
    pub hud_scale: f32,
}

impl Default for HudSettings {
    fn default() -> Self {
        Self {
            show_boss_bar: true,
            roster_detail: RosterDetail::Full,
            hud_scale: 1.0,
        }
    }
}

/// Accessibility preferences (Game-UI Guidance §1.3 / §4 — Presentation lens).
#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AccessibilitySettings {
    /// Multiplies UI font sizes. 0.75..=1.5.
    pub text_scale: f32,
    /// Swap the unit/threat/health palette for a colorblind-safe set.
    pub colorblind_safe: bool,
    /// Damp scanline shimmer, hit-flash, and muzzle/blood strobing.
    pub reduce_flashing: bool,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            text_scale: 1.0,
            colorblind_safe: false,
            reduce_flashing: false,
        }
    }
}

/// On-disk container. Grows additively (each field `#[serde(default)]`), so older save files load
/// fine as newer preference groups are added.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct UserSettings {
    pub hud: HudSettings,
    pub accessibility: AccessibilitySettings,
}

impl UserSettings {
    fn from_resources(hud: &HudSettings, acc: &AccessibilitySettings) -> Self {
        Self {
            hud: hud.clone(),
            accessibility: acc.clone(),
        }
    }
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let settings = load_or_seed();
        app.insert_resource(settings.hud)
            .insert_resource(settings.accessibility)
            .add_systems(Update, autosave_on_change);
    }
}

/// Resolve the settings file path from the platform config dir, dependency-free.
fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
    Some(base.join("FoundationVsSlop").join("user_settings.ron"))
}

/// Load the settings file; seed it with defaults if missing; fall back to defaults (with a warning)
/// if it can't be read or parsed. Never panics — this is user data, not the fail-loud dev config.
fn load_or_seed() -> UserSettings {
    let Some(path) = settings_path() else {
        warn!("settings: no config dir (HOME/XDG/APPDATA unset); using defaults, not persisting");
        return UserSettings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match ron::from_str::<UserSettings>(&text) {
            Ok(s) => s,
            Err(e) => {
                warn!("settings: {} is malformed ({e}); using defaults", path.display());
                UserSettings::default()
            }
        },
        Err(_) => {
            // Missing (or unreadable) → seed defaults so the file exists for next launch.
            let defaults = UserSettings::default();
            write_settings(&path, &defaults);
            defaults
        }
    }
}

/// Persist the current settings resources whenever either changes. `is_changed()` fires once on
/// insert too, which conveniently rewrites the file on first boot after a schema addition.
fn autosave_on_change(hud: Res<HudSettings>, acc: Res<AccessibilitySettings>) {
    if !(hud.is_changed() || acc.is_changed()) {
        return;
    }
    let Some(path) = settings_path() else { return };
    write_settings(&path, &UserSettings::from_resources(&hud, &acc));
}

/// Atomic write (tmp + rename) so a crash mid-write can't corrupt the settings file.
fn write_settings(path: &PathBuf, settings: &UserSettings) {
    let Some(parent) = path.parent() else { return };
    if let Err(e) = std::fs::create_dir_all(parent) {
        warn!("settings: could not create {}: {e}", parent.display());
        return;
    }
    let text = match ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default()) {
        Ok(t) => t,
        Err(e) => {
            warn!("settings: serialize failed: {e}");
            return;
        }
    };
    let tmp = path.with_extension("ron.tmp");
    if std::fs::write(&tmp, text).is_err() {
        warn!("settings: could not write {}", tmp.display());
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        warn!("settings: could not replace {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_settings_round_trips_through_ron() {
        let original = UserSettings {
            hud: HudSettings {
                show_boss_bar: false,
                roster_detail: RosterDetail::Compact,
                hud_scale: 1.25,
            },
            accessibility: AccessibilitySettings {
                text_scale: 1.5,
                colorblind_safe: true,
                reduce_flashing: true,
            },
        };
        let text = ron::ser::to_string_pretty(&original, ron::ser::PrettyConfig::default()).unwrap();
        let parsed: UserSettings = ron::from_str(&text).unwrap();
        assert_eq!(parsed.hud.roster_detail, original.hud.roster_detail);
        assert!((parsed.hud.hud_scale - 1.25).abs() < f32::EPSILON);
        assert!(parsed.accessibility.colorblind_safe);
        assert!(parsed.accessibility.reduce_flashing);
    }

    #[test]
    fn malformed_and_partial_input_falls_back_via_serde_default() {
        // A partial file (missing accessibility, missing a hud field) must load, not error —
        // proving the additive-schema promise for future audio/keybind groups.
        let partial = "(hud: (roster_detail: Hidden))";
        let parsed: UserSettings = ron::from_str(partial).unwrap();
        assert_eq!(parsed.hud.roster_detail, RosterDetail::Hidden);
        assert!(parsed.hud.show_boss_bar, "missing field takes its default");
        assert_eq!(parsed.accessibility.text_scale, 1.0, "missing group takes default");
    }
}
