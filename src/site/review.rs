//! **The O5 review and requisition, wired** (FVS-P-3).
//!
//! FVS-P-1 and P-2 shipped `super::o5` — 264 lines of correct, tested economy with **no plugin, no
//! systems and no resource**. `O5Standing` was never registered, `record()` and `buy()` had no callers,
//! and nothing carried a purchased consumable into an expedition. This is the half that makes it a
//! game rather than a library.
//!
//! # Where the report comes from, and why here
//!
//! `OnEnter(AppState::Debrief)`. That is the one funnel both terminal screens pass through, and — this
//! is the part that decides it — **the expedition world is still alive at that moment**. FVS-A-5 made
//! teardown happen on leaving `RunState::Active`, which is what `RETURN TO SITE` does *after* the
//! debrief, so a system here can still count living operatives, live `Contained` anomalies and uncapped
//! nests. Filing the report on `OnExit(RunState::Active)` would race those despawns.
//!
//! # The squad size is snapshotted, not counted at the end
//!
//! `squad::despawn_dead_units` removes the dead, so counting `Unit` at debrief gives *survivors* and
//! there is nothing left to compare them against. [`ExpeditionTally`] records the headcount at
//! insertion instead. It is run-scoped state held in a resource rather than derived, because the
//! quantity genuinely no longer exists by the time it is needed.
//!
//! # Determinism
//!
//! Windowed-only, and gated on `AppState::*`, which the deterministic core cannot see. Nothing here is
//! on `FixedUpdate`, so the O5 economy cannot reach `snapshot_hash`. The purchase path writes
//! `DeviceSupply`/`QuarantineSupply`, which *are* pinned — but only between expeditions, before
//! `reset_verbs` runs at `OnEnter(RunState::Active)`; see [`carry_purchases_into_the_expedition`].

use bevy::prelude::*;

use super::o5::{allowance, rate, Consumable, ExpeditionReport, O5Standing};
use crate::containment::{Contained, SiteSecured};
use crate::session::{RunOutcome, RunState};
use crate::squad::Unit;
use crate::ui::state::{despawn_scoped, AppState};

/// The squad headcount at insertion, captured before anyone can die.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExpeditionTally {
    pub squad_size: u32,
}

/// Consumables bought at the Site and not yet spent.
///
/// A **separate** store from `containment::DeviceSupply`, deliberately. That resource is the pouch for
/// *this* expedition and `reset_verbs` zeroes it from tuning at every insertion — so a purchase written
/// straight into it would be wiped by the next run. Purchases accumulate here and are folded in
/// afterwards, which also means a device bought and not used is not silently lost.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Requisitioned {
    pub devices: u32,
    pub quarantines: u32,
    pub medkits: u32,
}

impl Requisitioned {
    fn add(&mut self, item: Consumable) {
        match item {
            Consumable::CaptureDevice => self.devices += 1,
            Consumable::QuarantineCharge => self.quarantines += 1,
            Consumable::Medkit => self.medkits += 1,
        }
    }
}

/// Record the headcount the Council will measure against.
///
/// `RunBuild::PostPopulate` so the squad already exists — `Populate` is where `spawn_squad` runs, and
/// reading in the same set would be an ordering accident.
pub fn snapshot_squad_size(mut tally: ResMut<ExpeditionTally>, squad: Query<(), With<Unit>>) {
    tally.squad_size = squad.iter().count() as u32;
}

/// Build the report the Council rates, and fold it into the Director's standing.
///
/// **`extracted` is read from [`RunOutcome`], not re-derived.** The win condition already decided
/// whether the squad walked out, and computing it a second time here would be a second definition of
/// the same fact — free to drift from the one that actually ended the run.
pub fn file_expedition_report(
    mut standing: ResMut<O5Standing>,
    tally: Res<ExpeditionTally>,
    outcome: Res<RunOutcome>,
    secured: Option<Res<SiteSecured>>,
    survivors: Query<(), With<Unit>>,
    contained: Query<(), With<Contained>>,
) {
    let report = ExpeditionReport {
        squad_size: tally.squad_size,
        survivors: survivors.iter().count() as u32,
        // Live `Contained` anomalies, **not** `Specimen` records — the same distinction the win
        // condition draws. `Specimen` outlives the run, so counting it would credit this expedition
        // with everything ever captured.
        captures: contained.iter().count() as u32,
        extracted: matches!(*outcome, RunOutcome::Victory),
        // A breach is a nest left un-capped. `SiteSecured` is derived every tick and may legitimately
        // be absent in a world with no nests, which is zero breaches rather than unknown.
        breaches: secured.map(|s| s.total.saturating_sub(s.capped) as u32).unwrap_or(0),
    };
    standing.record(&report);
    info!(
        "O5: {:?} — {} → budget {}",
        rate(&report),
        allowance(&report),
        standing.budget
    );
}

/// Spend the budget. One key per item, the same idiom as the verb bar.
pub fn requisition_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut standing: ResMut<O5Standing>,
    mut held: ResMut<Requisitioned>,
) {
    // `B`/`N`/`M` are free — the digits are the time-control rungs, `C`/`Z`/`X`/`F` the containment
    // verbs, `R` the research bench, and `Q`/`E`/`WASD` the camera.
    let want = if keys.just_pressed(KeyCode::KeyB) {
        Some(Consumable::CaptureDevice)
    } else if keys.just_pressed(KeyCode::KeyN) {
        Some(Consumable::QuarantineCharge)
    } else if keys.just_pressed(KeyCode::KeyM) {
        Some(Consumable::Medkit)
    } else {
        None
    };
    // `buy` is the single writer of the budget and refuses an unaffordable purchase outright — no
    // partial buys, no debt.
    if let Some(item) = want {
        if standing.buy(item) {
            held.add(item);
        }
    }
}

/// Carry what was bought into the expedition about to start.
///
/// Ordered **after** `containment::verbs::reset_verbs`, which zeroes the pouch from tuning at every
/// insertion. Running before it would have the purchase overwritten — which is exactly the bug the
/// separate [`Requisitioned`] store exists to make impossible to write by accident.
pub fn carry_purchases_into_the_expedition(
    mut held: ResMut<Requisitioned>,
    mut devices: ResMut<crate::containment::DeviceSupply>,
    mut quarantines: ResMut<crate::containment::QuarantineSupply>,
) {
    devices.0 = devices.0.saturating_add(held.devices);
    quarantines.0 = quarantines.0.saturating_add(held.quarantines);
    // Spent: they are in the pouch now. Medkits stay held until an equipment system claims them —
    // there is no medkit consumer yet, and silently dropping them would be worse than carrying them.
    held.devices = 0;
    held.quarantines = 0;
}

/// Root marker for the requisition readout.
#[derive(Component)]
pub struct RequisitionPanel;

fn spawn_panel(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
) {
    commands
        .spawn((
            RequisitionPanel,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(theme.space_lg),
                left: Val::Px(theme.space_lg),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                ..default()
            },
            GlobalZIndex(crate::ui::theme::Z_MENU - 1),
        ))
        .with_children(|p| {
            p.spawn((
                RequisitionReadout,
                crate::ui::widgets::text_colored(&theme, &fonts, "", theme.font_body, theme.text),
            ));
        });
}

#[derive(Component)]
pub struct RequisitionReadout;

/// What the panel says. A pure function, so the wording is testable without an `App`.
pub fn requisition_text(standing: &O5Standing, held: &Requisitioned) -> String {
    let mut out = String::from("REQUISITION\n");
    out.push_str(&format!("O5 BUDGET: {}\n", standing.budget));
    if let Some(r) = standing.last_rating {
        // The remark, not just the grade — `Displeased` is phrased so it cannot be mistaken for a
        // dismissal, and printing only the enum name would throw that away.
        out.push_str(&format!("{}\n", r.remark()));
    }
    for (key, item) in
        [('B', Consumable::CaptureDevice), ('N', Consumable::QuarantineCharge), ('M', Consumable::Medkit)]
    {
        let price = item.price();
        // An unaffordable line is still shown, marked — hiding it would make the budget's effect
        // invisible and read as a broken key.
        let mark = if standing.budget >= price { ' ' } else { '!' };
        out.push_str(&format!("[{key}]{mark}{} — {price}\n", item.label()));
    }
    let carried = held.devices + held.quarantines + held.medkits;
    if carried > 0 {
        out.push_str(&format!(
            "CARRYING: {} DEVICE(S), {} CHARGE(S), {} MEDKIT(S)",
            held.devices, held.quarantines, held.medkits
        ));
    }
    out
}

fn update_panel(
    standing: Res<O5Standing>,
    held: Res<Requisitioned>,
    mut text_q: Query<&mut Text, With<RequisitionReadout>>,
) {
    let line = requisition_text(&standing, &held);
    for mut t in &mut text_q {
        if t.0 != line {
            t.0 = line.clone();
        }
    }
}

/// The O5 economy, wired. **Windowed-only**, like `persist` and the research bench.
pub struct O5Plugin;

impl Plugin for O5Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<O5Standing>()
            .init_resource::<ExpeditionTally>()
            .init_resource::<Requisitioned>()
            .add_systems(
                OnEnter(RunState::Active),
                (
                    snapshot_squad_size.in_set(crate::session::RunBuild::PostPopulate),
                    carry_purchases_into_the_expedition
                        .after(crate::containment::verbs::reset_verbs),
                ),
            )
            .add_systems(OnEnter(AppState::Debrief), file_expedition_report)
            .add_systems(OnEnter(AppState::Site), spawn_panel)
            .add_systems(OnExit(AppState::Site), despawn_scoped::<RequisitionPanel>)
            .add_systems(
                Update,
                (requisition_input, update_panel).run_if(in_state(AppState::Site)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::o5::Rating;

    #[test]
    fn a_purchase_moves_the_budget_into_the_pouch_and_survives_the_reset() {
        // The bug this shape prevents: `reset_verbs` zeroes `DeviceSupply` from tuning at every
        // insertion, so a purchase written straight into it would vanish at the start of the very
        // expedition it was bought for.
        let mut standing = O5Standing { budget: 100, ..default() };
        let mut held = Requisitioned::default();
        assert!(standing.buy(Consumable::CaptureDevice));
        held.add(Consumable::CaptureDevice);
        assert_eq!(standing.budget, 70);
        assert_eq!(held.devices, 1);

        let mut app = App::new();
        app.insert_resource(held)
            .insert_resource(crate::containment::DeviceSupply(3))
            .insert_resource(crate::containment::QuarantineSupply(1))
            .add_systems(Update, carry_purchases_into_the_expedition);
        app.update();
        assert_eq!(
            app.world().resource::<crate::containment::DeviceSupply>().0,
            4,
            "the purchased device must be ADDED to the tuned pouch, not replace it"
        );
        assert_eq!(app.world().resource::<Requisitioned>().devices, 0, "and then be spent");
    }

    #[test]
    fn an_unaffordable_purchase_is_refused_outright() {
        let mut standing = O5Standing { budget: 10, ..default() };
        assert!(!standing.buy(Consumable::QuarantineCharge));
        assert_eq!(standing.budget, 10, "a refused purchase must not partially spend");
    }

    #[test]
    fn the_panel_states_the_budget_the_verdict_and_what_is_affordable() {
        let standing =
            O5Standing { budget: 35, last_rating: Some(Rating::Displeased), expeditions: 1 };
        let out = requisition_text(&standing, &Requisitioned::default());
        assert!(out.contains("O5 BUDGET: 35"), "{out}");
        assert!(out.contains("NOT RELIEVED OF COMMAND"), "the remark, not just the grade: {out}");
        assert!(out.contains("[B] CAPTURE DEVICE"), "an affordable line is unmarked: {out}");
        assert!(out.contains("[N]!QUARANTINE CHARGE"), "an unaffordable line is marked: {out}");
    }

    #[test]
    fn a_wiped_expedition_still_leaves_enough_to_try_again() {
        // The floor's job is not generosity — it is that the loop stays *attemptable*. A Director who
        // cannot afford to contain anything is in a state the game offers no way out of.
        let mut standing = O5Standing::default();
        standing.record(&ExpeditionReport {
            squad_size: 5,
            survivors: 0,
            captures: 0,
            extracted: false,
            breaches: 3,
        });
        assert_eq!(standing.last_rating, Some(Rating::Displeased));
        assert!(
            standing.budget >= Consumable::CaptureDevice.price(),
            "the worst possible expedition must still fund one capture device"
        );
    }
}
