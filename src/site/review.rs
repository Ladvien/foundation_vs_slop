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
use serde::{Deserialize, Serialize};

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
///
/// **Serialized alongside [`O5Standing`]**, and it has to be: the budget and the stock it was spent on
/// are two halves of one quantity. Saving only the budget would delete a purchase on every restart —
/// the money gone and nothing to show for it, which reads as a game bug rather than as a rule.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// A request to buy one consumable, from **either** input route — the key or the panel button.
///
/// Routed as a message for exactly the reason `selection::ArmRequest` is: [`requisition_input`] stays
/// the single caller of `O5Standing::buy`, so the click and the key cannot drift apart and grow two
/// copies of the affordability rule. Before this existed, the three purchases were keyboard-only —
/// the panel printed `[B] CAPTURE DEVICE — 60` beside two others and none of them could be clicked,
/// which is the "row of things that look like buttons and are not" failure `ui::verb_bar` argues
/// against and `docs/ui.md` §4.2's operability lens forbids.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PurchaseRequest(pub Consumable);

/// Spend the budget. One key per item, the same idiom as the verb bar.
pub fn requisition_input(
    actions: crate::input::Actions,
    mut requests: MessageReader<PurchaseRequest>,
    mut standing: ResMut<O5Standing>,
    mut held: ResMut<Requisitioned>,
) {
    // `crate::input::Action` owns the binding; `input::the_key_space_has_no_collisions` is what
    // keeps this key from quietly colliding with another.
    let mut want = if actions.just_pressed(crate::input::Action::BuyCaptureDevice) {
        Some(Consumable::CaptureDevice)
    } else if actions.just_pressed(crate::input::Action::BuyQuarantineCharge) {
        Some(Consumable::QuarantineCharge)
    } else if actions.just_pressed(crate::input::Action::BuyMedkit) {
        Some(Consumable::Medkit)
    } else {
        None
    };
    // A clicked button arrives here rather than calling `buy` itself. Every request is drained, so an
    // unread one cannot be redelivered next frame and buy twice.
    for req in requests.read() {
        want = Some(req.0);
    }
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

/// The node whose children are the readout's rows.
#[derive(Component)]
pub struct RequisitionReadout;

/// One clickable purchase button, tagged with what it buys.
///
/// **Spawned once and never rebuilt.** `rows::sync_rows` despawns and respawns a panel's children
/// whenever its content changes, and the budget changes on every purchase — so a button living
/// inside that subtree would be destroyed under a cursor that was mid-click. `ui::verb_bar` hit this
/// exact problem and records the fix: keep the *label* rewritable and the *entity* stable.
#[derive(Component, Clone, Copy)]
pub struct BuyButton(pub Consumable);

/// A buy button's label node, so the price/affordability text can be rewritten without respawning
/// the button.
#[derive(Component, Clone, Copy)]
pub struct BuyButtonLabel(pub Consumable);

fn spawn_panel(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
    regions: Res<crate::ui::layout::HudRegions>,
) {
    // Parented into the shared region grid rather than absolutely positioned. The Site runs FOUR
    // panels at once (curriculum, research, requisition, records) and each used to claim a corner
    // independently, with no owner able to notice a collision or make room for a fifth.
    let panel = (
            RequisitionPanel,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
                min_width: Val::Px(300.0),
                ..default()
            },
            BackgroundColor(theme.panel),
            crate::ui::widgets::border_all(theme.panel_border),
            // The panel container ignores clicks so it cannot eat one meant for the world; the
            // buttons below are individually pickable, which is what `Pickable` being per-entity buys.
            Pickable::IGNORE,
    );
    let Some(mut ec) = crate::ui::layout::panel_in(
        &mut commands,
        &regions,
        crate::ui::layout::Region::BottomLeft,
        panel,
    ) else {
        error!("requisition: no layout frame at spawn — the O5 budget readout is not shown");
        return;
    };
    ec.with_children(|p| {
        // The readout half: rows, so the budget line and an unaffordable item can differ in
        // EMPHASIS. As one `\n`-joined `Text` node this panel had a single `TextColor`, so
        // "you cannot afford this" was findable only by reading for a `!` — the precise cost
        // `ui::rows`' module header describes.
        p.spawn((
            RequisitionReadout,
            crate::ui::rows::RowPanel::default(),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                ..default()
            },
            Pickable::IGNORE,
        ));

        // The action half: three stable buttons.
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                margin: UiRect::top(Val::Px(theme.space_sm)),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|col| {
            for item in Consumable::ALL {
                col.spawn((
                    BuyButton(item),
                    // `bevy_ui_widgets::Button` emits `Activate` on release; `Hovered` is what the
                    // styler reads. Same pair the verb chips use.
                    bevy::ui_widgets::Button,
                    bevy::picking::hover::Hovered::default(),
                    Node {
                        padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_xs)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme.radius)),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    crate::ui::widgets::border_all(theme.panel_border),
                ))
                .observe(move |_: On<bevy::ui_widgets::Activate>, mut out: MessageWriter<PurchaseRequest>| {
                    // Sends the intent; `requisition_input` applies it. One caller of `buy`.
                    out.write(PurchaseRequest(item));
                })
                .with_children(|b| {
                    b.spawn((
                        BuyButtonLabel(item),
                        crate::ui::widgets::text_colored(&theme, &fonts, "", theme.font_body, theme.text),
                        Pickable::IGNORE,
                    ));
                });
            }
        });
    });
}

/// What the readout says, as rows.
///
/// Pure, so the wording *and the emphasis* are testable without an `App` — the property `ui::rows`
/// exists to give every panel. Replaces `requisition_text`, which returned one `\n`-joined `String`
/// and therefore had exactly one `TextColor`: an affordable item and an unaffordable one rendered in
/// identical ink, so "you cannot buy this" was findable only by reading for a `!`.
pub fn requisition_rows(standing: &O5Standing, held: &Requisitioned) -> Vec<crate::ui::rows::Row> {
    use crate::ui::rows::Row;
    let mut rows =
        vec![Row::header("REQUISITION"), Row::kv("O5 BUDGET", standing.budget.to_string())];
    if let Some(r) = standing.last_rating {
        // The remark, not just the grade — `Displeased` is phrased so it cannot be mistaken for a
        // dismissal, and printing only the enum name would throw that away.
        rows.push(Row::note(r.remark()));
    }
    for item in Consumable::ALL {
        let price = item.price();
        // An unaffordable line is still shown, and now it is *visibly* the blocked one.
        // `met`/`unmet` map to luminance, never hue (`docs/ui.md` §1.3).
        rows.push(if standing.budget >= price {
            Row::met(item.label(), price.to_string())
        } else {
            Row::unmet(item.label(), price.to_string())
        });
    }
    let carried = held.devices + held.quarantines + held.medkits;
    if carried > 0 {
        rows.push(Row::kv(
            "CARRYING",
            format!("{}D  {}Q  {}M", held.devices, held.quarantines, held.medkits),
        ));
    } else {
        // `docs/ui.md` §1.4: an empty panel reads as a bug, so name the state instead of omitting it.
        rows.push(Row::note("CARRYING NOTHING INTO THE FIELD"));
    }
    rows
}

/// What one buy button reads. Pure, and it **states its key** — the rule
/// `verb_bar::every_verb_states_its_key` enforces for the containment verbs, extended to the Site so
/// a mouse-driven player is not taught the keyboard route is absent, or the reverse.
pub fn buy_button_label(item: Consumable, budget: u32) -> String {
    let price = item.price();
    let key = buy_key(item);
    if budget >= price {
        format!("{key}  {} — {price}", item.label())
    } else {
        // Names the shortfall rather than only dimming: an unmet condition is an instruction.
        format!("{key}  {} — {price}  (NEED {})", item.label(), price - budget)
    }
}

/// The key each purchase is bound to, read from the registry so this label cannot drift from the
/// binding the way the hardcoded `[B]`/`[N]`/`[M]` in the old string did.
fn buy_key(item: Consumable) -> char {
    let action = match item {
        Consumable::CaptureDevice => crate::input::Action::BuyCaptureDevice,
        Consumable::QuarantineCharge => crate::input::Action::BuyQuarantineCharge,
        Consumable::Medkit => crate::input::Action::BuyMedkit,
    };
    crate::input::key_name(action.default_binding().primary.key)
        .and_then(|n| n.chars().next())
        .unwrap_or('?')
}

fn update_panel(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
    standing: Res<O5Standing>,
    held: Res<Requisitioned>,
    mut panels: Query<(Entity, &mut crate::ui::rows::RowPanel), With<RequisitionReadout>>,
    mut labels: Query<(&BuyButtonLabel, &mut Text)>,
) {
    let rows = requisition_rows(&standing, &held);
    for (entity, mut panel) in &mut panels {
        crate::ui::rows::sync_rows(&mut commands, entity, &mut panel, &theme, &fonts, rows.clone());
    }
    for (label, mut text) in &mut labels {
        let want = buy_button_label(label.0, standing.budget);
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// Affordable / hovered styling for the buy buttons.
///
/// Marked by **luminance and border**, never a hue change — the encoding rule the verb chips follow,
/// so an unaffordable item is findable without depending on hue discrimination.
fn style_buy_buttons(
    theme: Res<crate::ui::theme::UiTheme>,
    standing: Res<O5Standing>,
    mut buttons: Query<(
        &BuyButton,
        &bevy::picking::hover::Hovered,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut labels: Query<(&BuyButtonLabel, &mut TextColor)>,
) {
    for (btn, hovered, mut bg, mut border) in &mut buttons {
        let affordable = standing.budget >= btn.0.price();
        let want_bg = if hovered.0 && affordable {
            theme.panel_border.with_alpha(0.16)
        } else {
            theme.panel
        };
        let want_border = if affordable {
            theme.panel_border
        } else {
            theme.panel_border.with_alpha(0.25)
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want = crate::ui::widgets::border_all(want_border);
        if border.top != want.top {
            *border = want;
        }
    }
    for (label, mut color) in &mut labels {
        let want = if standing.budget >= label.0.price() {
            theme.text
        } else {
            theme.text_muted
        };
        if color.0 != want {
            color.0 = want;
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
            // The clickable buy buttons send this; `requisition_input` is the single reader and the
            // single caller of `O5Standing::buy`.
            .add_message::<PurchaseRequest>()
            .add_systems(
                OnEnter(RunState::Active),
                (
                    snapshot_squad_size.in_set(crate::session::RunBuild::PostPopulate),
                    carry_purchases_into_the_expedition
                        .after(crate::containment::verbs::reset_verbs),
                ),
            )
            .add_systems(OnEnter(AppState::Debrief), file_expedition_report)
            .add_systems(
                OnEnter(AppState::Site),
                spawn_panel.after(crate::ui::layout::spawn_frame),
            )
            .add_systems(OnExit(AppState::Site), despawn_scoped::<RequisitionPanel>)
            .add_systems(
                Update,
                (requisition_input, update_panel, style_buy_buttons)
                    .run_if(in_state(AppState::Site)),
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
        use crate::ui::rows::Emphasis;
        let standing = O5Standing {
            budget: 35,
            last_rating: Some(Rating::Displeased),
            expeditions: 1,
            last_report: None,
        };
        let rows = requisition_rows(&standing, &Requisitioned::default());
        let labels: Vec<&str> = rows.iter().filter_map(|r| r.label()).collect();
        assert!(labels.contains(&"REQUISITION"));
        assert!(labels.contains(&"O5 BUDGET"));
        assert!(
            labels.iter().any(|l| l.contains("NOT RELIEVED OF COMMAND")),
            "the remark, not just the grade: {labels:?}"
        );

        // The assertion that got STRONGER with the conversion. The old string test could only check
        // for a `!` character; this checks the thing the player actually perceives — which row is the
        // bright one — and that is the whole argument in `ui::rows`' module header.
        let row_for = |name: &str| {
            rows.iter()
                .find(|r| r.label() == Some(name))
                .unwrap_or_else(|| panic!("{name} is missing from the panel"))
        };
        assert_eq!(
            row_for(Consumable::CaptureDevice.label()).emphasis,
            Emphasis::Muted,
            "an affordable item recedes"
        );
        assert_eq!(
            row_for(Consumable::QuarantineCharge.label()).emphasis,
            Emphasis::Alert,
            "an unaffordable item is the loud row"
        );
    }

    #[test]
    fn an_empty_pouch_says_so_rather_than_omitting_the_line() {
        // `docs/ui.md` §1.4: an empty panel reads as a bug. The old string dropped the CARRYING line
        // entirely when nothing was held, so the player could not tell "carrying nothing" from
        // "this panel forgot to render".
        let rows = requisition_rows(&O5Standing::default(), &Requisitioned::default());
        assert!(
            rows.iter().any(|r| r.label().is_some_and(|l| l.contains("CARRYING NOTHING"))),
            "an empty pouch must name its state"
        );
        let held = Requisitioned { devices: 2, quarantines: 1, medkits: 0 };
        let rows = requisition_rows(&O5Standing::default(), &held);
        assert!(rows.iter().any(|r| r.label() == Some("CARRYING")));
    }

    #[test]
    fn every_buy_button_states_its_key_and_never_goes_blank() {
        // The operability rule (`docs/ui.md` §4.2): everything reachable by mouse is reachable by
        // keyboard and vice versa. These buttons are now clickable, so each must still name the key —
        // and the key is read from the registry, so it cannot drift from the binding the way the
        // hardcoded `[B]`/`[N]`/`[M]` in the old string could.
        for item in Consumable::ALL {
            let rich = buy_button_label(item, 10_000);
            let broke = buy_button_label(item, 0);
            for l in [&rich, &broke] {
                assert!(!l.trim().is_empty());
                assert!(l.contains(item.label()), "{l} does not name the item");
                assert!(
                    l.starts_with(buy_key(item)),
                    "{l} must lead with its key {}",
                    buy_key(item)
                );
            }
            // Unaffordable states the shortfall rather than only dimming.
            assert!(broke.contains("NEED"), "{broke} must say what is missing");
            assert_ne!(rich, broke);
        }
    }

    #[test]
    fn the_buy_keys_are_distinct_and_resolvable() {
        // A `?` here would mean `input::keyname` has no name for a key the registry binds — which is
        // also the condition under which that binding could not be persisted or shown on the controls
        // screen, so this catches all three at once.
        let keys: Vec<char> = Consumable::ALL.iter().map(|i| buy_key(*i)).collect();
        for (i, a) in keys.iter().enumerate() {
            assert_ne!(*a, '?', "a purchase is bound to a key with no name");
            for b in &keys[i + 1..] {
                assert_ne!(a, b, "two purchases share the key {a}");
            }
        }
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
