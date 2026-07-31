//! User settings — player preferences persisted to disk, distinct from the version-controlled,
//! fail-loud dev config (`assets/config/config.ron`). This is *user* data: a missing file is
//! seeded with defaults, and a malformed one falls back to defaults with a `warn!` (it must never
//! panic the game the way the dev config deliberately does) — **and disables persistence for the
//! whole session** ([`SettingsLoadState`]), so the damaged file is never overwritten with the
//! defaults we substituted for it.
//!
//! **Windowed-only.** [`SettingsPlugin`] is added by `ui::UiPlugin` (registered in `lib::run`), so
//! the headless replay harness performs no filesystem IO and its settings resources stay at
//! defaults.
//!
//! Scope note: this carries **HUD**, **accessibility** and **keybinding** preferences. Audio volumes
//! are added when that gated phase lands; every field is `#[serde(default)]` so growing the schema
//! never breaks an existing save file.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// How much of the squad roster strip to show — the core of player-controllable HUD density.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RosterDetail {
    Hidden,
    Compact,
    Full,
}

/// Player-controllable HUD density (`docs/ui.md` §2). A [`Resource`] and serialized.
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

/// **Access** preferences (`docs/ui.md` §1.3 / §4) — how the game is presented, never how hard it is.
///
/// The Access/Challenge split is Power et al. 2019 (DOI 10.1016/j.ijhcs.2019.06.010). Nothing here
/// may become an RL/QD genome gene: these describe the *player*, not the world, so evolving them
/// would be optimising against whoever happened to be sitting at the keyboard.
///
/// **There is deliberately no `colorblind_safe` flag.** There used to be, and it was dead — written
/// by the settings menu, read by nobody. It is gone rather than implemented because the encoding it
/// existed to work around is gone: threat now rides the ACS **luminosity** ramp with a redundant
/// glyph (`ui::theme::Hazard`), and roster chips carry their operative's **role letter**
/// (`ui::hud::role_letter`), so no readout in the game depends on telling two hues apart. Redundant
/// coding beats an opt-in palette on both counts — it needs no toggle to find, and it helps everyone
/// reading the HUD in peripheral vision while looking at the world, not only the ~8% of men with a
/// red-green deficiency.
#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AccessibilitySettings {
    /// Multiplies UI text size, via the `RemSize` resource. 0.75..=1.5.
    pub text_scale: f32,
    /// Damp the VHS tape glitch — scanline shimmer, chroma split, noise. Damped to a quarter rather
    /// than switched off: the glitch is a narrative tell, so removing it would delete information
    /// rather than soften it. Applied in `vhs::drive_fade`.
    pub reduce_flashing: bool,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            text_scale: 1.0,
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
    /// Keyboard overrides (`crate::input`). Stores only what the player *changed*, so the action
    /// list can grow — and a shipped default can be improved — without a stale save silently
    /// pinning the old binding.
    pub input: crate::input::InputSettings,
}

impl UserSettings {
    fn from_resources(
        hud: &HudSettings,
        acc: &AccessibilitySettings,
        input: &crate::input::InputSettings,
    ) -> Self {
        Self {
            hud: hud.clone(),
            accessibility: acc.clone(),
            // The **persisted** overrides, verbatim — never re-derived from the live `KeyBindings`.
            // See `autosave_on_change` for why that distinction is the difference between preserving
            // the player's file and destroying it.
            input: input.clone(),
        }
    }
}

/// How the settings file loaded at startup — the gate every later write must pass.
///
/// `Damaged` means the file **existed but could not be read or parsed**. The session then runs on
/// defaults, and [`autosave_on_change`] refuses to persist anything: the file on disk holds
/// settings this process never saw, so any write from here would replace the player's full
/// settings with a degraded substitute — the exact write the global fail-loud rule forbids, on the
/// one file that is genuinely user data. The carried string is the read/parse error, kept so the
/// settings menu can say *why* changes aren't saving once that surface lands (its UX is an open
/// design call; the invariant is not).
#[derive(Resource, Clone, PartialEq, Eq, Debug)]
pub enum SettingsLoadState {
    /// Read, parsed, or freshly seeded — changes persist normally.
    Loaded,
    /// Unreadable or malformed — nothing writes to the file this session.
    Damaged(String),
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let (settings, load_state) = load_or_seed();
        // `to_bindings` resolves the stored overrides against the shipped defaults, dropping any
        // that are unreadable or that would collide — with a `warn!`, never a panic. This is the
        // ONE place the live table is built; `input::InputPlugin` only claims the resource.
        app.insert_resource(settings.input.to_bindings())
            .insert_resource(settings.input)
            .insert_resource(settings.hud)
            .insert_resource(settings.accessibility)
            .insert_resource(load_state)
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

/// Load the settings file; seed it with defaults if missing; fall back to defaults (with a warning,
/// and persistence disabled — see [`SettingsLoadState`]) if it can't be read or parsed. Never
/// panics — this is user data, not the fail-loud dev config.
fn load_or_seed() -> (UserSettings, SettingsLoadState) {
    let Some(path) = settings_path() else {
        warn!("settings: no config dir (HOME/XDG/APPDATA unset); using defaults, not persisting");
        // Nothing to protect: with no resolvable path, `autosave_on_change` has nowhere to write.
        return (UserSettings::default(), SettingsLoadState::Loaded);
    };
    load_from(&path)
}

/// The filesystem half of [`load_or_seed`], split on the path so tests can aim it at a scratch
/// directory without touching the process environment (`settings_path` reads env vars, and mutating
/// those from parallel unit tests races).
fn load_from(path: &Path) -> (UserSettings, SettingsLoadState) {
    match std::fs::read_to_string(path) {
        Ok(text) => match ron::from_str::<UserSettings>(&text) {
            Ok(s) => (s, SettingsLoadState::Loaded),
            Err(e) => {
                warn!(
                    "settings: {} is malformed ({e}); running on defaults, and changes will NOT be \
                     saved this session — fix or delete the file to re-enable persistence",
                    path.display()
                );
                (UserSettings::default(), SettingsLoadState::Damaged(e.to_string()))
            }
        },
        // Only a genuinely absent file is seeded. Any other read failure (permissions, IO) is a
        // file we could not inspect — seeding over it would destroy settings we never saw, which
        // is exactly the substitute-write the load state exists to refuse.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let defaults = UserSettings::default();
            write_settings(path, &defaults);
            (defaults, SettingsLoadState::Loaded)
        }
        Err(e) => {
            warn!(
                "settings: could not read {} ({e}); running on defaults, and changes will NOT be \
                 saved this session",
                path.display()
            );
            (UserSettings::default(), SettingsLoadState::Damaged(e.to_string()))
        }
    }
}

/// Persist the current settings resources whenever one changes.
///
/// **It writes [`crate::input::InputSettings`], not the resolved [`crate::input::KeyBindings`]**, and
/// that is a correctness requirement rather than a style choice. `to_bindings` may legitimately drop
/// an override it cannot use — an unreadable key name, a table that is still self-contradictory — and
/// substitute the shipped default. Deriving the file back from that resolved table erased the
/// offending line **permanently on the next launch**: one typo in a hand-edited `user_settings.ron`
/// and the entry was gone, with nothing but a log line the player never sees. The global rule is
/// explicit that a path which cannot produce a usable result must fail loudly rather than write a
/// degraded substitute to storage, and re-deriving was exactly that write.
///
/// So the persisted form is the source of truth for persistence, and the live table is downstream of
/// it. **When an interactive remap screen lands it must update `InputSettings` too**, not only
/// `KeyBindings`, or the rebind will not survive a restart. `KeyBindings` is deliberately *not* a
/// trigger here: it changes on insert at startup, which is what fired the destructive write.
///
/// Two further guards, one invariant: **a load failure must never lead to a write, and no write
/// happens on a frame the player didn't act.**
/// - The first run is skipped outright. Bevy reports a freshly inserted resource as changed on a
///   system's first sight of it, so every trigger here fires on frame 1 — that is the startup
///   insert, not the player. Writing then would re-serialize a healthy file it just read
///   (normalizing away a hand-edit) and, after a failed load, replace the player's file with the
///   substituted defaults.
/// - A [`SettingsLoadState::Damaged`] session never writes at all — not even for a deliberate
///   change, because the write would carry that one changed value plus a defaulted
///   everything-else, over a file whose real contents this process never saw.
fn autosave_on_change(
    state: Res<SettingsLoadState>,
    hud: Res<HudSettings>,
    acc: Res<AccessibilitySettings>,
    input: Res<crate::input::InputSettings>,
    mut startup_seen: Local<bool>,
) {
    if !*startup_seen {
        *startup_seen = true;
        return;
    }
    if !(hud.is_changed() || acc.is_changed() || input.is_changed()) {
        return;
    }
    if matches!(*state, SettingsLoadState::Damaged(_)) {
        return;
    }
    let Some(path) = settings_path() else { return };
    write_settings(&path, &UserSettings::from_resources(&hud, &acc, &input));
}

/// Atomic write (tmp + rename) so a crash mid-write can't corrupt the settings file.
fn write_settings(path: &Path, settings: &UserSettings) {
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
                reduce_flashing: true,
            },
            input: crate::input::InputSettings::default(),
        };
        let text = ron::ser::to_string_pretty(&original, ron::ser::PrettyConfig::default()).unwrap();
        let parsed: UserSettings = ron::from_str(&text).unwrap();
        assert_eq!(parsed.hud.roster_detail, original.hud.roster_detail);
        assert!((parsed.hud.hud_scale - 1.25).abs() < f32::EPSILON);
        assert!((parsed.accessibility.text_scale - 1.5).abs() < f32::EPSILON);
        assert!(parsed.accessibility.reduce_flashing);
    }

    #[test]
    fn a_rebound_key_survives_the_round_trip_to_disk() {
        // The whole point of persisting bindings: a player who remaps a key must find it remapped
        // next launch. Covers the seam between `input`'s name-based storage and this file's RON.
        use crate::input::{Action, Binding, Chord, KeyBindings};
        use bevy::prelude::KeyCode;

        // Written as the PERSISTED form, which is what `autosave_on_change` now saves — deriving the
        // file back from a resolved `KeyBindings` is what used to erase a dropped override from disk.
        let saved = UserSettings::from_resources(
            &HudSettings::default(),
            &AccessibilitySettings::default(),
            &crate::input::InputSettings {
                overrides: vec![crate::input::StoredBinding {
                    action: Action::ArmDevice.id().to_string(),
                    primary: "T".to_string(),
                    alternate: None,
                }],
            },
        );
        let text = ron::ser::to_string_pretty(&saved, ron::ser::PrettyConfig::default()).unwrap();
        let parsed: UserSettings = ron::from_str(&text).unwrap();

        assert_eq!(
            parsed.input.to_bindings().get(Action::ArmDevice).primary.key,
            KeyCode::KeyT
        );
        // Everything untouched still reads its shipped default rather than a frozen copy.
        assert_eq!(
            parsed.input.to_bindings().get(Action::ArmCap),
            Action::ArmCap.default_binding()
        );
    }

    #[test]
    fn a_malformed_file_survives_the_load_byte_for_byte() {
        // THE clobber regression: a file that fails to parse must (a) mark the session `Damaged`,
        // (b) leave the player on defaults, and (c) not be touched on disk — the old path wrote
        // defaults over it on the first autosave, destroying a hand-edited file over one typo.
        let dir = std::env::temp_dir().join(format!("fvs_settings_damaged_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("user_settings.ron");
        let damaged = "(hud: (roster_detail: Hidde"; // truncated mid-token — a hand-edit typo
        std::fs::write(&path, damaged).unwrap();

        let (settings, state) = load_from(&path);
        assert!(matches!(state, SettingsLoadState::Damaged(_)));
        assert_eq!(settings.hud.roster_detail, RosterDetail::Full, "runs on defaults");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            damaged,
            "the damaged file must survive the load untouched"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_seeded_and_the_session_stays_writable() {
        let dir = std::env::temp_dir().join(format!("fvs_settings_seed_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("user_settings.ron");
        std::fs::remove_file(&path).ok(); // a stale file from a crashed prior run would fake the seed

        let (_, state) = load_from(&path);
        assert_eq!(state, SettingsLoadState::Loaded);
        let seeded: UserSettings = ron::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(seeded.hud.show_boss_bar, "the seeded file parses back to defaults");
        std::fs::remove_dir_all(&dir).ok();
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
