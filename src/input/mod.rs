//! **The keyboard registry** — one place that knows every key this game binds.
//!
//! # The bug this exists to make impossible
//!
//! Key allocation used to be coordinated by *prose comments*. Five modules each carried a
//! hand-written census of which keys were taken — `selection.rs`, `site/review.rs`,
//! `knowledge/records.rs`, `antagonist.rs`, `ui/research_hud.rs` — and every one of them was
//! wrong in the same way: all five named `T` as taken, months after the `T` dialogue hotkey was
//! deleted. A census maintained by hand is a census that drifts, and the cost of the drift is a
//! silent collision — two systems reading one key, one of them apparently broken.
//!
//! [`Action`] is that census as data, and [`the_key_space_has_no_collisions`] is the test that
//! keeps it honest.
//!
//! # Contexts, because some collisions are legal
//!
//! `W` is both "pan the camera forward" and "move up a menu". That is not a bug — the two can
//! never be live at the same time, and the camera already suppresses panning while a menu is open.
//! A flat uniqueness rule would reject it and force a worse binding.
//!
//! So every action declares a [`Context`], and the collision test asks whether two actions'
//! contexts can be live *simultaneously* ([`Context::overlaps`]). That is the property the prose
//! comments were groping at and could not express.
//!
//! # The focus guard, which used to be per-system
//!
//! When a menu button holds [`InputFocus`], or the dev note box is taking raw text, the keyboard
//! belongs to *that* — a keystroke must not also fire a gameplay action. `research_room::editor`
//! discovered this the hard way (one `Space` both clicked the focused palette button and toggled
//! the pause) and grew a local guard. The guard now lives here, applied once, to every
//! non-[`Context::Menu`] action, so no future binding can forget it.
//!
//! # Scope
//!
//! Windowed-only, like `crate::ui`: [`InputPlugin`] is registered in `lib::run` and never in the
//! headless harness, so the deterministic core never reads a binding. Nothing here may become an
//! RL/QD genome gene — bindings describe the *player*, not the world, which is the Access side of
//! Power et al. 2019's split (`docs/ui.md` §4.1/§4.4).
//!
//! Remapping is an **Access** option and must never be gated by difficulty. Note the finer point
//! that split makes and which is easy to miss: *Input* options (which key) and *Control* options
//! (hold vs toggle, how many actions a task needs) are **separate categories**. This module is the
//! Input half.

use bevy::ecs::system::SystemParam;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

mod keyname;
pub use keyname::{key_from_name, key_name};

/// When an action can be live. Two actions may share a chord iff their contexts cannot both be
/// live at once — see [`Context::overlaps`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Context {
    /// Only while a blocking menu or overlay owns the keyboard.
    Menu,
    /// Any time the world is on screen — during a run *or* at the Site. Camera and time control.
    Play,
    /// During an expedition only.
    InGame,
    /// At Site-67 only.
    Site,
    /// Debug builds. Deliberately overlaps every play context rather than being carved out: a dev
    /// key that shadows a player key is exactly the collision this registry exists to catch, and
    /// the dev bindings stay out of the way by using F-keys and modifier chords, not by being
    /// exempt from the rule.
    Dev,
}

impl Context {
    /// Can these two contexts be live at the same moment?
    ///
    /// [`Context::Menu`] is the only isolated one — while a menu is open the sim is blocked and
    /// the camera is suppressed, so nothing else is reading the keyboard.
    pub fn overlaps(self, other: Context) -> bool {
        use Context::*;
        match (self, other) {
            (Menu, Menu) => true,
            (Menu, _) | (_, Menu) => false,
            // Everything else shares the screen with everything else, except that a run and the
            // Site are mutually exclusive states.
            (InGame, Site) | (Site, InGame) => false,
            _ => true,
        }
    }
}

/// The modifier a chord requires.
///
/// **`Shift` is deliberately absent.** In this game Shift is a *click* modifier (queueing an order
/// onto a unit's list), not a key modifier, so requiring "no Shift held" would break panning the
/// camera while queuing. [`Mods::None`] therefore means "no Ctrl and no Alt", and ignores Shift.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Mods {
    None,
    Ctrl,
    Alt,
}

impl Mods {
    fn held(self, keys: &ButtonInput<KeyCode>) -> bool {
        let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
            || keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]);
        let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
        match self {
            Mods::None => !ctrl && !alt,
            Mods::Ctrl => ctrl,
            Mods::Alt => alt,
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Mods::None => "",
            Mods::Ctrl => "Ctrl+",
            Mods::Alt => "Alt+",
        }
    }
}

/// One physical key plus its required modifier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    pub mods: Mods,
    pub key: KeyCode,
}

impl Chord {
    pub const fn plain(key: KeyCode) -> Self {
        Chord { mods: Mods::None, key }
    }

    pub const fn ctrl(key: KeyCode) -> Self {
        Chord { mods: Mods::Ctrl, key }
    }

    pub const fn alt(key: KeyCode) -> Self {
        Chord { mods: Mods::Alt, key }
    }

    /// Player-facing name, e.g. `Ctrl+P`. Returns `None` for a key with no name in
    /// [`keyname`] — which is also the condition under which it could not be persisted, so a
    /// binding that cannot be shown is a binding that cannot be saved, and one test covers both.
    pub fn label(self) -> Option<String> {
        key_name(self.key).map(|k| format!("{}{k}", self.mods.prefix()))
    }
}

/// Where an action is bound. An `alternate` exists because dropping the arrow keys as a synonym
/// for `WASD` would be a real accessibility regression, not a simplification.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    pub primary: Chord,
    pub alternate: Option<Chord>,
}

impl Binding {
    pub const fn one(primary: Chord) -> Self {
        Binding { primary, alternate: None }
    }

    pub const fn two(primary: Chord, alternate: Chord) -> Self {
        Binding { primary, alternate: Some(alternate) }
    }

    fn chords(&self) -> impl Iterator<Item = Chord> + '_ {
        std::iter::once(self.primary).chain(self.alternate)
    }

    /// Player-facing name, e.g. `W  /  ↑`.
    pub fn label(&self) -> String {
        let mut parts = self.chords().filter_map(Chord::label);
        let first = parts.next().unwrap_or_else(|| "—".to_string());
        match parts.next() {
            Some(second) => format!("{first}  /  {second}"),
            None => first,
        }
    }
}

/// Every keyboard action in the game.
///
/// A closed enum rather than free strings, so a new binding cannot reach the player without
/// someone declaring its context, its default chord, and whether it is rebindable — which is
/// exactly the set of decisions the five prose censuses were failing to record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    // --- Camera (Play). Held, not tapped — read with `pressed`. ---
    CameraPanForward,
    CameraPanBack,
    CameraPanLeft,
    CameraPanRight,
    CameraRotateLeft,
    CameraRotateRight,
    /// Snap the view back to the squad. The camera follows nothing by design (it is a free-panning
    /// RTS rig), which is fine right up until the squad walks off-screen and the player has to
    /// hunt for them by hand.
    CameraRecenter,

    // --- Time control (Play). ---
    TogglePause,
    SpeedDown,
    SpeedUp,

    // --- Containment verbs (InGame). ---
    ArmDevice,
    ArmQuarantine,
    ArmCap,
    ToggleHoldFire,
    /// Deploy the Engineer's sensor drone — the only thing that turns the minimap on.
    DeploySensor,
    /// Order the selection to advance to contact instead of holding (`squad::PushOrder`).
    TogglePush,

    // --- Readouts (InGame). ---
    CycleHudDensity,
    ToggleRoster,

    // --- Site-67 (Site). ---
    CycleSpecimen,
    RunTopExperiment,
    FileFindings,
    CurateArchive,
    BuyCaptureDevice,
    BuyQuarantineCharge,
    BuyMedkit,

    // --- Screens. ---
    /// Open / close the pause menu. **Not rebindable** — a player who rebinds their only way out
    /// of the game has locked themselves in.
    PauseMenu,
    /// Back out of a settings screen.
    MenuBack,
    MenuUp,
    MenuDown,
    /// `Enter` and `Space` are handled by `bevy_ui_widgets::Button` itself; this restores the
    /// numpad variant it does not cover (see `ui::widgets`).
    MenuActivate,

    // --- Dev (Dev). ---
    DevAiOverlay,
    DevPerfHud,
    DevResearchRoom,
    DevForceVictory,
    DevRegionCapture,
}

impl Action {
    pub const ALL: [Action; 35] = [
        Action::CameraPanForward,
        Action::CameraPanBack,
        Action::CameraPanLeft,
        Action::CameraPanRight,
        Action::CameraRotateLeft,
        Action::CameraRotateRight,
        Action::CameraRecenter,
        Action::TogglePause,
        Action::SpeedDown,
        Action::SpeedUp,
        Action::ArmDevice,
        Action::ArmQuarantine,
        Action::ArmCap,
        Action::ToggleHoldFire,
        Action::DeploySensor,
        Action::TogglePush,
        Action::CycleHudDensity,
        Action::ToggleRoster,
        Action::CycleSpecimen,
        Action::RunTopExperiment,
        Action::FileFindings,
        Action::CurateArchive,
        Action::BuyCaptureDevice,
        Action::BuyQuarantineCharge,
        Action::BuyMedkit,
        Action::PauseMenu,
        Action::MenuBack,
        Action::MenuUp,
        Action::MenuDown,
        Action::MenuActivate,
        Action::DevAiOverlay,
        Action::DevPerfHud,
        Action::DevResearchRoom,
        Action::DevForceVictory,
        Action::DevRegionCapture,
    ];

    /// Dense index into [`KeyBindings`]'s table. A fixed array rather than a map, for the same
    /// reason `ui::layout::HudRegions` is one: no iteration order to reason about.
    pub fn index(self) -> usize {
        // `ALL` is asserted dense and in declaration order by a test, so position IS the index.
        Action::ALL.iter().position(|a| *a == self).unwrap_or(0)
    }

    pub fn context(self) -> Context {
        use Action::*;
        match self {
            CameraPanForward | CameraPanBack | CameraPanLeft | CameraPanRight | CameraRotateLeft
            | CameraRotateRight | CameraRecenter | TogglePause | SpeedDown | SpeedUp | PauseMenu => {
                Context::Play
            }
            ArmDevice | ArmQuarantine | ArmCap | ToggleHoldFire | DeploySensor | TogglePush
            | CycleHudDensity | ToggleRoster => Context::InGame,
            CycleSpecimen | RunTopExperiment | FileFindings | CurateArchive | BuyCaptureDevice
            | BuyQuarantineCharge | BuyMedkit => Context::Site,
            MenuBack | MenuUp | MenuDown | MenuActivate => Context::Menu,
            DevAiOverlay | DevPerfHud | DevResearchRoom | DevForceVictory | DevRegionCapture => {
                Context::Dev
            }
        }
    }

    /// Escape is the one key a player must never be able to lose.
    pub fn is_rebindable(self) -> bool {
        !matches!(
            self,
            Action::PauseMenu
                | Action::MenuBack
                | Action::MenuUp
                | Action::MenuDown
                | Action::MenuActivate
        )
    }

    pub fn is_dev(self) -> bool {
        self.context() == Context::Dev
    }

    /// The name persisted in `user_settings.ron`. **Stable** — a saved override is keyed on this
    /// string, not on the enum's discriminant, so reordering [`Action::ALL`] can never silently
    /// rebind someone's keyboard.
    pub fn id(self) -> &'static str {
        use Action::*;
        match self {
            CameraPanForward => "camera_pan_forward",
            CameraPanBack => "camera_pan_back",
            CameraPanLeft => "camera_pan_left",
            CameraPanRight => "camera_pan_right",
            CameraRotateLeft => "camera_rotate_left",
            CameraRotateRight => "camera_rotate_right",
            CameraRecenter => "camera_recenter",
            TogglePause => "toggle_pause",
            SpeedDown => "speed_down",
            SpeedUp => "speed_up",
            ArmDevice => "arm_device",
            ArmQuarantine => "arm_quarantine",
            ArmCap => "arm_cap",
            ToggleHoldFire => "toggle_hold_fire",
            DeploySensor => "deploy_sensor",
            TogglePush => "toggle_push",
            CycleHudDensity => "cycle_hud_density",
            ToggleRoster => "toggle_roster",
            CycleSpecimen => "cycle_specimen",
            RunTopExperiment => "run_top_experiment",
            FileFindings => "file_findings",
            CurateArchive => "curate_archive",
            BuyCaptureDevice => "buy_capture_device",
            BuyQuarantineCharge => "buy_quarantine_charge",
            BuyMedkit => "buy_medkit",
            PauseMenu => "pause_menu",
            MenuBack => "menu_back",
            MenuUp => "menu_up",
            MenuDown => "menu_down",
            MenuActivate => "menu_activate",
            DevAiOverlay => "dev_ai_overlay",
            DevPerfHud => "dev_perf_hud",
            DevResearchRoom => "dev_research_room",
            DevForceVictory => "dev_force_victory",
            DevRegionCapture => "dev_region_capture",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.id() == id)
    }

    /// The line shown on the controls screen. An instruction, not a noun, per `docs/ui.md` §1.4.
    pub fn label(self) -> &'static str {
        use Action::*;
        match self {
            CameraPanForward => "PAN FORWARD",
            CameraPanBack => "PAN BACK",
            CameraPanLeft => "PAN LEFT",
            CameraPanRight => "PAN RIGHT",
            CameraRotateLeft => "ROTATE VIEW LEFT",
            CameraRotateRight => "ROTATE VIEW RIGHT",
            CameraRecenter => "CENTRE ON SQUAD",
            TogglePause => "PAUSE / RESUME",
            SpeedDown => "SLOW DOWN",
            SpeedUp => "SPEED UP",
            ArmDevice => "ARM CAPTURE DEVICE",
            ArmQuarantine => "ARM QUARANTINE",
            ArmCap => "ARM NEST CAP",
            ToggleHoldFire => "HOLD FIRE",
            DeploySensor => "DEPLOY SENSOR",
            TogglePush => "ADVANCE TO CONTACT",
            CycleHudDensity => "CYCLE HUD DENSITY",
            ToggleRoster => "OPEN ROSTER",
            CycleSpecimen => "SELECT SPECIMEN",
            RunTopExperiment => "RUN THE TOP TEST",
            FileFindings => "FILE FINDINGS",
            CurateArchive => "CURATE ARCHIVE",
            BuyCaptureDevice => "BUY CAPTURE DEVICE",
            BuyQuarantineCharge => "BUY QUARANTINE CHARGE",
            BuyMedkit => "BUY MEDKIT",
            PauseMenu => "PAUSE MENU",
            MenuBack => "BACK",
            MenuUp => "MENU UP",
            MenuDown => "MENU DOWN",
            MenuActivate => "CONFIRM",
            DevAiOverlay => "DEV — AI STATE OVERLAY",
            DevPerfHud => "DEV — PERFORMANCE HUD",
            DevResearchRoom => "DEV — RESEARCH ROOM",
            DevForceVictory => "DEV — FORCE VICTORY",
            DevRegionCapture => "DEV — CAPTURE A REGION",
        }
    }

    /// The shipped default. Every chord here is checked for collisions by
    /// [`the_key_space_has_no_collisions`] and for persistability by
    /// [`every_default_binding_can_be_written_to_disk`].
    pub fn default_binding(self) -> Binding {
        use Action::*;
        match self {
            CameraPanForward => Binding::two(Chord::plain(KeyCode::KeyW), Chord::plain(KeyCode::ArrowUp)),
            CameraPanBack => Binding::two(Chord::plain(KeyCode::KeyS), Chord::plain(KeyCode::ArrowDown)),
            CameraPanLeft => Binding::two(Chord::plain(KeyCode::KeyA), Chord::plain(KeyCode::ArrowLeft)),
            CameraPanRight => {
                Binding::two(Chord::plain(KeyCode::KeyD), Chord::plain(KeyCode::ArrowRight))
            }
            CameraRotateLeft => Binding::one(Chord::plain(KeyCode::KeyQ)),
            CameraRotateRight => Binding::one(Chord::plain(KeyCode::KeyE)),
            // `Home` (with `Backspace` alongside) is the RTS convention for "take me back", and both
            // are outside the letter block the verbs and Site keys compete for.
            CameraRecenter => Binding::two(Chord::plain(KeyCode::Home), Chord::plain(KeyCode::Backspace)),

            TogglePause => Binding::one(Chord::plain(KeyCode::Space)),
            SpeedDown => Binding::one(Chord::plain(KeyCode::Minus)),
            SpeedUp => Binding::one(Chord::plain(KeyCode::Equal)),

            ArmDevice => Binding::one(Chord::plain(KeyCode::KeyC)),
            ArmQuarantine => Binding::one(Chord::plain(KeyCode::KeyZ)),
            ArmCap => Binding::one(Chord::plain(KeyCode::KeyX)),
            ToggleHoldFire => Binding::one(Chord::plain(KeyCode::KeyF)),
            // `V` is free and sits beside the C/Z/X verb cluster.
            DeploySensor => Binding::one(Chord::plain(KeyCode::KeyV)),
            // `G` is free and sits beside the other stance key, `F` (hold fire).
            TogglePush => Binding::one(Chord::plain(KeyCode::KeyG)),

            CycleHudDensity => Binding::one(Chord::plain(KeyCode::KeyH)),
            ToggleRoster => Binding::one(Chord::plain(KeyCode::KeyL)),

            CycleSpecimen => Binding::one(Chord::plain(KeyCode::Tab)),
            RunTopExperiment => Binding::one(Chord::plain(KeyCode::KeyR)),
            FileFindings => Binding::one(Chord::plain(KeyCode::KeyK)),
            CurateArchive => Binding::one(Chord::plain(KeyCode::KeyJ)),
            BuyCaptureDevice => Binding::one(Chord::plain(KeyCode::KeyB)),
            BuyQuarantineCharge => Binding::one(Chord::plain(KeyCode::KeyN)),
            BuyMedkit => Binding::one(Chord::plain(KeyCode::KeyM)),

            PauseMenu => Binding::one(Chord::plain(KeyCode::Escape)),
            MenuBack => Binding::one(Chord::plain(KeyCode::Escape)),
            MenuUp => Binding::two(Chord::plain(KeyCode::KeyW), Chord::plain(KeyCode::ArrowUp)),
            MenuDown => Binding::two(Chord::plain(KeyCode::KeyS), Chord::plain(KeyCode::ArrowDown)),
            MenuActivate => Binding::one(Chord::plain(KeyCode::NumpadEnter)),

            DevAiOverlay => Binding::one(Chord::plain(KeyCode::F3)),
            DevPerfHud => Binding::one(Chord::plain(KeyCode::F4)),
            DevResearchRoom => Binding::one(Chord::plain(KeyCode::F6)),
            DevForceVictory => Binding::one(Chord::plain(KeyCode::F10)),
            DevRegionCapture => Binding::one(Chord::ctrl(KeyCode::KeyP)),
        }
    }
}

/// The live binding table. Indexed by [`Action::index`] — a fixed array, no map, no iteration
/// order.
#[derive(Resource, Clone)]
pub struct KeyBindings([Binding; Action::ALL.len()]);

impl Default for KeyBindings {
    fn default() -> Self {
        let mut table = [Binding::one(Chord::plain(KeyCode::F24)); Action::ALL.len()];
        for a in Action::ALL {
            table[a.index()] = a.default_binding();
        }
        KeyBindings(table)
    }
}

impl KeyBindings {
    pub fn get(&self, action: Action) -> Binding {
        self.0[action.index()]
    }

    /// The single character a panel prints beside an action's name, from the **live** table.
    ///
    /// Every button label in the game used to read `Action::default_binding()` (or, in the verb bar, a
    /// hardcoded `char`), so a player who rebound a key was shown the *shipped* one — told a key that
    /// does nothing, which is worse than being told none. `'?'` when the chord has no single-character
    /// name (`Tab`, `F6`, a modifier chord); callers wanting the full form use `Binding::label`.
    pub fn key_char(&self, action: Action) -> char {
        let chord = self.get(action).primary;
        if chord.mods != Mods::None {
            return '?';
        }
        key_name(chord.key)
            .and_then(|n| if n.chars().count() == 1 { n.chars().next() } else { None })
            .unwrap_or('?')
    }

    /// Short player-facing name of an action's primary chord, from the live table — `Tab`, `Ctrl+P`,
    /// `F6`. For labels that are not single characters.
    pub fn key_label(&self, action: Action) -> String {
        self.get(action).primary.label().unwrap_or_else(|| "?".to_string())
    }

    /// Rebind. Returns the action this chord already belongs to, without applying the change, if
    /// it would collide within a live context — the caller (the controls screen) shows that as
    /// `ALREADY BOUND TO <x>` rather than silently producing two owners for one key.
    pub fn rebind(&mut self, action: Action, binding: Binding) -> Result<(), Action> {
        if !action.is_rebindable() {
            return Err(action);
        }
        for other in Action::ALL {
            if other == action || !action.context().overlaps(other.context()) {
                continue;
            }
            if binding.chords().any(|c| self.get(other).chords().any(|o| o == c)) {
                return Err(other);
            }
        }
        self.0[action.index()] = binding;
        Ok(())
    }

    /// Is the whole table self-consistent? `Err((a, b))` names the first pair sharing a chord whose
    /// contexts can be live together.
    ///
    /// Separate from [`Self::rebind`] because they answer different questions. `rebind` asks "may this
    /// one change go in?", which is right for an interactive remap screen. Loading a *file* needs
    /// "is the finished table legal?", and asking the first question per entry rejects legal swaps.
    pub fn validate(&self) -> Result<(), (Action, Action)> {
        for (i, a) in Action::ALL.iter().enumerate() {
            for b in &Action::ALL[i + 1..] {
                if !a.context().overlaps(b.context()) {
                    continue;
                }
                let clash = self
                    .get(*a)
                    .chords()
                    .any(|ca| self.get(*b).chords().any(|cb| ca == cb));
                if clash {
                    return Err((*a, *b));
                }
            }
        }
        Ok(())
    }

    /// Only the entries that differ from the shipped defaults, for persistence. Ordered by
    /// [`Action::ALL`] so the written file is stable.
    fn overrides(&self) -> Vec<StoredBinding> {
        Action::ALL
            .iter()
            .copied()
            .filter(|a| self.get(*a) != a.default_binding())
            .filter_map(|a| StoredBinding::of(a, self.get(a)))
            .collect()
    }
}

/// One persisted override. Chords are stored as **names** (`"Ctrl+P"`), not as `KeyCode`
/// discriminants: the file stays human-editable, and it does not bind the save format to a Bevy
/// enum's layout.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoredBinding {
    pub action: String,
    pub primary: String,
    #[serde(default)]
    pub alternate: Option<String>,
}

impl StoredBinding {
    fn of(action: Action, b: Binding) -> Option<StoredBinding> {
        Some(StoredBinding {
            action: action.id().to_string(),
            primary: b.primary.label()?,
            alternate: b.alternate.and_then(Chord::label),
        })
    }
}

/// Parse `"Ctrl+P"` / `"W"` into a chord.
pub fn chord_from_label(s: &str) -> Option<Chord> {
    let (mods, rest) = if let Some(r) = s.strip_prefix("Ctrl+") {
        (Mods::Ctrl, r)
    } else if let Some(r) = s.strip_prefix("Alt+") {
        (Mods::Alt, r)
    } else {
        (Mods::None, s)
    };
    key_from_name(rest).map(|key| Chord { mods, key })
}

/// Player input settings. Persisted through `crate::settings::UserSettings`.
///
/// **Access-side** (`docs/ui.md` §4.1): never gated by difficulty, never an RL/QD gene.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct InputSettings {
    /// Only the chords the player changed. Anything absent takes the shipped default, so the
    /// action list can grow without invalidating a save.
    pub overrides: Vec<StoredBinding>,
}

impl InputSettings {
    /// Build the live table from the persisted overrides.
    ///
    /// **All overrides are applied first, then the result is validated once.** Applying them one at a
    /// time against a table still holding the shipped defaults rejected any legal *swap*: writing
    /// `arm_quarantine: "X"` and `arm_cap: "Z"` failed on both halves, because each collided with the
    /// other's not-yet-replaced default. Both edits vanished and the keyboard silently reverted — and
    /// since hand-editing the file is currently the only remap route that exists, that was the whole
    /// feature.
    ///
    /// A file that is *still* self-contradictory after everything is applied falls back to the
    /// defaults with a `warn!` naming the pair. That fallback is sanctioned here and nowhere else:
    /// `crate::settings` documents that this file is **user data** and must never panic the game, the
    /// way the fail-loud dev config deliberately does. What must not happen is the fallback being
    /// written back over the player's file — see `settings::autosave_on_change`.
    pub fn to_bindings(&self) -> KeyBindings {
        let mut bindings = KeyBindings::default();
        for stored in &self.overrides {
            let Some(action) = Action::from_id(&stored.action) else {
                warn!("input: unknown action {:?} in settings; ignoring", stored.action);
                continue;
            };
            if !action.is_rebindable() {
                warn!("input: {} is not rebindable; ignoring the override", stored.action);
                continue;
            }
            let Some(primary) = chord_from_label(&stored.primary) else {
                warn!("input: unreadable key {:?} for {}; keeping default", stored.primary, stored.action);
                continue;
            };
            let alternate = match &stored.alternate {
                Some(a) => match chord_from_label(a) {
                    Some(c) => Some(c),
                    None => {
                        warn!("input: unreadable alternate {a:?} for {}; dropping it", stored.action);
                        None
                    }
                },
                None => None,
            };
            // Written directly, collisions and all. `validate` below is what judges the finished table.
            bindings.0[action.index()] = Binding { primary, alternate };
        }
        if let Err((a, b)) = bindings.validate() {
            warn!(
                "input: {} and {} end up sharing a key and can be live together; \
                 falling back to the default keyboard (your settings file is NOT overwritten)",
                a.id(),
                b.id()
            );
            return KeyBindings::default();
        }
        bindings
    }

    pub fn from_bindings(bindings: &KeyBindings) -> Self {
        InputSettings { overrides: bindings.overrides() }
    }
}

/// The keys `bevy_ui_widgets::Button` activates a focused button on, and which therefore cannot also
/// mean something else while a menu holds focus.
///
/// `NumpadEnter` is deliberately absent: Bevy does *not* handle it (that is why
/// [`Action::MenuActivate`] exists), so it is ours to bind freely.
const BUTTON_ACTIVATION_KEYS: [KeyCode; 2] = [KeyCode::Space, KeyCode::Enter];

/// Who currently owns the keyboard, and it is **two different conditions** that must not be conflated.
///
/// This started as one boolean, and that was a bug with a wide blast radius. Menu focus is set the
/// moment *any* menu spawns (`ui::widgets::menu_keyboard_nav` seeds `InputFocus` onto the first
/// button), so a single flag suppressing every non-[`Context::Menu`] action meant **Escape could open
/// the pause menu but never close it**, F6 could open the Research Room palette but never close it,
/// and the camera, pause and Ctrl+P capture all went dead behind any overlay. The guard was written to
/// stop one `Space` both clicking a focused button and toggling the pause; it was doing far more.
///
/// The two conditions:
///
/// - [`Self::text_entry`] — the dev note box is taking **raw text**. Every keystroke is a character,
///   so *every* non-menu action must stand down. This is the broad case, and it is rare.
/// - [`Self::menu_focus`] — a menu button holds [`InputFocus`], so Bevy's `Button` will consume
///   [`BUTTON_ACTIVATION_KEYS`] itself. Only actions bound to *those keys* conflict. Escape, F6 and
///   Ctrl+P never did.
///
/// **Why a resource and not read live inside [`Actions`].** `Actions` originally held
/// `Res<InputFocus>` directly, which made it unusable in the very systems that *drive* focus —
/// `ui::widgets::menu_keyboard_nav` takes `ResMut<InputFocus>`, and Bevy rejects `Res` + `ResMut`
/// of one resource in a single system (B0002). Computing the flags once, in one writer, removes the
/// conflict and gives the guard a single owner.
///
/// The cost is that the flags are one frame old, which is harmless: focus only appears when a menu
/// opens, and at that point the sim is already blocked.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardOwned {
    /// Raw text entry is in progress. Suppresses every non-menu action.
    pub text_entry: bool,
    /// A menu button holds focus. Suppresses only [`BUTTON_ACTIVATION_KEYS`].
    pub menu_focus: bool,
}

impl KeyboardOwned {
    /// Is anything at all holding the keyboard? For consumers that read the mouse *and* keyboard and
    /// simply want to stand down while a menu is up — `selection::control_group_input`, which must not
    /// rebind a control group to a digit the player pressed while reading a menu.
    pub fn any(&self) -> bool {
        self.text_entry || self.menu_focus
    }
}

/// Sole writer of [`KeyboardOwned`]. `PreUpdate`, so every `Update` reader sees this frame's value
/// for the note box and last frame's for menu focus.
fn track_keyboard_owner(
    // `Option` because a bare `App` (the UI-liveness test) has no `InputFocus`, and a missing
    // `Res<T>` panics the system in Bevy 0.19 rather than skipping it.
    focus: Option<Res<InputFocus>>,
    note_box: Option<Res<crate::NoteInputActive>>,
    mut owned: ResMut<KeyboardOwned>,
) {
    let want = KeyboardOwned {
        text_entry: note_box.is_some(),
        menu_focus: focus.is_some_and(|f| f.get().is_some()),
    };
    if *owned != want {
        *owned = want;
    }
}

/// The read side. Take this instead of `Res<ButtonInput<KeyCode>>` — it applies the guard that
/// every gameplay hotkey needs and that a hand-rolled reader can forget.
#[derive(SystemParam)]
pub struct Actions<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    bindings: Res<'w, KeyBindings>,
    owned: Res<'w, KeyboardOwned>,
}

impl Actions<'_> {
    /// Menu actions are exempt from the guard — a focused button is precisely when menu navigation
    /// must keep working. Everything else is judged against the *narrower* of the two conditions in
    /// [`KeyboardOwned`]: raw text entry stops everything, but mere menu focus only stops the keys a
    /// focused `Button` would consume.
    fn gated(&self, action: Action) -> bool {
        if action.context() == Context::Menu {
            return false;
        }
        if self.owned.text_entry {
            return true;
        }
        // Escape, F6, Ctrl+P and the camera keys are NOT activation keys, so a menu holding focus
        // leaves them alone — which is what lets the key that opened an overlay also close it.
        self.owned.menu_focus
            && self
                .bindings
                .get(action)
                .chords()
                .any(|c| BUTTON_ACTIVATION_KEYS.contains(&c.key))
    }

    pub fn just_pressed(&self, action: Action) -> bool {
        !self.gated(action)
            && self.bindings.get(action).chords().any(|c| {
                self.keys.just_pressed(c.key) && c.mods.held(&self.keys)
            })
    }

    /// For held actions (camera panning).
    pub fn pressed(&self, action: Action) -> bool {
        !self.gated(action)
            && self.bindings.get(action).chords().any(|c| {
                self.keys.pressed(c.key) && c.mods.held(&self.keys)
            })
    }

    pub fn binding(&self, action: Action) -> Binding {
        self.bindings.get(action)
    }
}

/// Claim the resources [`Actions`] reads, so a system taking it cannot panic on a missing one.
///
/// A missing `Res<T>` panics the system in Bevy 0.19 rather than skipping it, and the readers of
/// this table are spread across windowed *and* harness-visible plugins (`selection` is
/// harness-visible; `ui` is not). So this follows the rule already documented at `ui::mod` and
/// `selection::SelectionPlugin`: **the plugin that registers a reader claims the resource**, and
/// `init_resource` is idempotent so claiming is free.
///
/// `settings::SettingsPlugin` later `insert_resource`s the table resolved from the player's saved
/// overrides, which replaces whatever default got claimed first — so plugin build order does not
/// matter.
pub fn claim_bindings(app: &mut App) {
    app.init_resource::<KeyBindings>()
        .init_resource::<KeyboardOwned>();
}

/// Registers the one writer of [`KeyboardOwned`].
///
/// Windowed-only (added in `lib::run`). The headless harness has no menus and presses no keys, so
/// the flag stays at its `false` default there and every action reads ungated — which is exactly
/// right, and means the deterministic core never depends on this plugin existing.
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        claim_bindings(app);
        app.add_systems(PreUpdate, track_keyboard_owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_is_dense_and_in_declaration_order() {
        // `index()` is a position lookup into `ALL`, and that index is the table slot. A duplicate
        // or a missing entry would silently alias two actions onto one binding.
        for (i, a) in Action::ALL.iter().enumerate() {
            assert_eq!(a.index(), i, "{a:?} is out of order in ALL");
        }
        for (i, a) in Action::ALL.iter().enumerate() {
            for b in &Action::ALL[i + 1..] {
                assert_ne!(a, b, "{a:?} appears twice in ALL");
            }
        }
    }

    #[test]
    fn the_key_space_has_no_collisions() {
        // THE test this module exists for. It replaces five hand-written prose censuses in
        // `selection.rs`, `site/review.rs`, `knowledge/records.rs`, `antagonist.rs` and
        // `ui/research_hud.rs` — all five of which had drifted and named `T` as taken long after
        // the `T` hotkey was deleted.
        let bindings = KeyBindings::default();
        for (i, a) in Action::ALL.iter().enumerate() {
            for b in &Action::ALL[i + 1..] {
                if !a.context().overlaps(b.context()) {
                    continue;
                }
                for ca in bindings.get(*a).chords() {
                    for cb in bindings.get(*b).chords() {
                        assert_ne!(
                            ca,
                            cb,
                            "{a:?} ({:?}) and {b:?} ({:?}) both want {} and can be live together",
                            a.context(),
                            b.context(),
                            ca.label().unwrap_or_else(|| "?".into())
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_bare_digits_are_left_for_control_groups() {
        // `1`–`9` are the control-group row (`selection::control_group_input`) and `Ctrl` + digit
        // binds one. They are a single mechanism with nine slots rather than nine independently
        // rebindable actions, so they are deliberately NOT `Action` variants — but nothing else may
        // claim them, or a group recall would be silently shadowed. This is the same guarantee the
        // collision test gives registered actions, extended to the one mechanism that sits outside it.
        //
        // `Alt` + digit is the debug inspection ladder (`time_control::read_inspection_ladder`); it
        // does not conflict, because the modifier is part of the chord.
        let digits = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
        ];
        let bindings = KeyBindings::default();
        for a in Action::ALL {
            if a.context() == Context::Menu {
                continue; // A menu is open; the selection is not taking commands.
            }
            for c in bindings.get(a).chords() {
                assert!(
                    !(c.mods == Mods::None && digits.contains(&c.key)),
                    "{a:?} took a bare digit — that row belongs to the control groups"
                );
            }
        }
        // `Ctrl+A` (select all) is a registered-action-free chord too; assert nothing grabbed it.
        for a in Action::ALL {
            for c in bindings.get(a).chords() {
                assert!(
                    !(c.mods == Mods::Ctrl && c.key == KeyCode::KeyA),
                    "{a:?} took Ctrl+A, which is select-all"
                );
            }
        }
    }

    #[test]
    fn the_legal_collisions_are_the_ones_we_meant() {
        // The flip side: `W` really is both "pan forward" and "menu up", and that must stay legal
        // or the collision test above would force a worse binding. If someone widens
        // `Context::overlaps`, this fails and names the pair that has to be re-bound.
        assert!(!Context::Play.overlaps(Context::Menu));
        assert!(!Context::InGame.overlaps(Context::Site));
        assert_eq!(
            Action::CameraPanForward.default_binding().primary,
            Action::MenuUp.default_binding().primary
        );
        // Dev is deliberately NOT carved out — a dev key shadowing a player key is a real bug.
        assert!(Context::Dev.overlaps(Context::InGame));
    }

    #[test]
    fn every_default_binding_can_be_written_to_disk() {
        // A chord whose key has no name in `keyname` cannot be persisted OR shown on the controls
        // screen. Both failures are silent, so catch them at the binding table instead.
        for a in Action::ALL {
            for c in a.default_binding().chords() {
                assert!(
                    c.label().is_some(),
                    "{a:?} is bound to a key with no name — it can be neither saved nor displayed"
                );
            }
        }
    }

    #[test]
    fn a_binding_round_trips_through_its_label() {
        for a in Action::ALL {
            for c in a.default_binding().chords() {
                let label = c.label().expect("checked by the test above");
                assert_eq!(chord_from_label(&label), Some(c), "{a:?} did not round-trip via {label:?}");
            }
        }
    }

    #[test]
    fn every_action_has_a_distinct_id_and_a_label() {
        for (i, a) in Action::ALL.iter().enumerate() {
            assert!(!a.label().trim().is_empty(), "{a:?} has no player-facing label");
            assert_eq!(Action::from_id(a.id()), Some(*a), "{a:?} does not round-trip through its id");
            for b in &Action::ALL[i + 1..] {
                assert_ne!(a.id(), b.id(), "{a:?} and {b:?} share the persisted id {:?}", a.id());
            }
        }
    }

    #[test]
    fn defaults_persist_as_nothing() {
        // An untouched keyboard must write an EMPTY override list, or every future change to a
        // default binding would be silently overridden by a stale saved copy of the old one.
        let bindings = KeyBindings::default();
        assert!(
            InputSettings::from_bindings(&bindings).overrides.is_empty(),
            "a default table must serialise to no overrides"
        );
    }

    #[test]
    fn a_rebind_round_trips_and_a_colliding_one_is_refused() {
        let mut bindings = KeyBindings::default();
        // `T` is genuinely free — the fact the old prose census claimed otherwise is why this
        // module exists.
        assert!(bindings
            .rebind(Action::ArmDevice, Binding::one(Chord::plain(KeyCode::KeyT)))
            .is_ok());
        assert_eq!(bindings.get(Action::ArmDevice).primary.key, KeyCode::KeyT);

        let settings = InputSettings::from_bindings(&bindings);
        assert_eq!(settings.overrides.len(), 1);
        assert_eq!(settings.to_bindings().get(Action::ArmDevice).primary.key, KeyCode::KeyT);

        // Taking a live sibling's key is refused, and the refusal NAMES the owner so the controls
        // screen can say so rather than showing an empty failure.
        assert_eq!(
            bindings.rebind(Action::ArmCap, Binding::one(Chord::plain(KeyCode::KeyZ))),
            Err(Action::ArmQuarantine)
        );
        // ...but a key owned only by a context that cannot be live at the same time is fine.
        assert!(bindings
            .rebind(Action::ArmCap, Binding::one(Chord::plain(KeyCode::KeyR)))
            .is_ok());
    }

    #[test]
    fn escape_can_never_be_rebound() {
        let mut bindings = KeyBindings::default();
        assert!(!Action::PauseMenu.is_rebindable());
        assert!(bindings
            .rebind(Action::PauseMenu, Binding::one(Chord::plain(KeyCode::KeyY)))
            .is_err());
        assert_eq!(bindings.get(Action::PauseMenu).primary.key, KeyCode::Escape);
    }

    #[test]
    fn a_malformed_override_is_dropped_not_fatal() {
        // User data, not the fail-loud dev config (`crate::settings`).
        let settings = InputSettings {
            overrides: vec![
                StoredBinding { action: "no_such_action".into(), primary: "T".into(), alternate: None },
                StoredBinding {
                    action: "arm_device".into(),
                    primary: "NotAKey".into(),
                    alternate: None,
                },
                // Collides with `arm_quarantine`, which shares its context.
                StoredBinding { action: "arm_cap".into(), primary: "Z".into(), alternate: None },
            ],
        };
        let bindings = settings.to_bindings();
        assert_eq!(bindings.get(Action::ArmDevice), Action::ArmDevice.default_binding());
        assert_eq!(bindings.get(Action::ArmCap), Action::ArmCap.default_binding());
    }

    #[test]
    fn mods_none_ignores_shift_but_not_ctrl() {
        // Shift is a CLICK modifier in this game (queueing orders), so requiring "no shift" would
        // break panning the camera while queuing. Ctrl and Alt must still discriminate, or
        // `Ctrl+P` would also fire a bare `P` action.
        let mut keys = ButtonInput::<KeyCode>::default();
        assert!(Mods::None.held(&keys));
        keys.press(KeyCode::ShiftLeft);
        assert!(Mods::None.held(&keys), "shift must not suppress an unmodified action");
        keys.press(KeyCode::ControlLeft);
        assert!(!Mods::None.held(&keys), "ctrl must suppress an unmodified action");
        assert!(Mods::Ctrl.held(&keys));
        assert!(!Mods::Alt.held(&keys));
    }
}
