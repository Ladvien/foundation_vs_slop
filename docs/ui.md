# Game UI Guidance — `foundation_vs_slop`

The contract for `src/ui/`. Four places in the codebase cite this document by section
(`src/ui/hud.rs`, `src/settings.rs` ×2, `tests/replay.rs`), and for a long time it did not exist —
the citations pointed at nothing. This is that document.

Read it before adding a panel, a readout, or a setting.

> **Provenance.** Every empirical claim traces to a paper in the home-still library (stem given) or
> to a DOI. Where a finding comes from an abstract rather than full text — because the conversion
> pipeline was down when it was pulled — that is stated inline. Nothing here is asserted from
> memory.

---

## §1 Foundations

### §1.1 What the UI is for

The player is a Director running MTF expeditions out of Site-67. The UI has exactly two jobs:

1. **In an expedition** — support the decisions the run is made of: where to move, which verb to
   arm, whether containment is progressing and why not.
2. **At the Site** — support the decisions *between* runs: what to study, what to file, what to buy.

Anything on screen that supports no decision is noise, and noise is not neutral (see §1.2).

### §1.2 Over-informing is the failure mode

Llanos & Jørgensen 2011 (`10.26503_dl.v2011i1.514`) found players accept superimposed overlays as
long as they are "clearly motivated, and provide relevant and sufficient information" — but "once the
players receive more information than they need, superimposed UI elements become annoying and for
this reason they may risk ruining the sense of involvement."

**Rule: every widget names the decision it supports.** If you cannot name one, cut the widget.

Vicente & Rasmussen 1992 (`10.1109_21.156574`) sharpen the axis, and it is not element *count*. Their
prescription is "first, not to force processing to a higher level than the demands of the task
require, and second, to support each of the three levels of cognitive control" (skills / rules /
knowledge). So the test is **"does this force interpretation?"**, not "how many things are on
screen?" A dense display the player reads at a glance is better than a sparse one that demands
inference. Note the symmetric second half, which a pure-minimalism reading drops: the interface must
still "support the entire range of activities that operators will be faced with."

*(Abstract only — the three named principles are in the unconverted PDF. See §6.)*

### §1.3 Encoding — the accessibility rules that are not optional

**Baseline reality is desaturated, and saturation is reserved.** `ui::theme` is warm-neutral
throughout — chroma under `MAX_UI_CHROMA` (0.12), `red >= blue`, and separation between `accent`,
`text` and `text_muted` riding on **luminance alone**. Two tests pin it:
`the_foundation_has_no_house_palette` and `baseline_reality_is_slightly_warm`.

This replaced a phosphor-green CRT palette whose accent had a chroma of 0.45. Two independent
arguments landed in the same place, as with the hazard ramp below:

- *In-fiction*: `docs/lore/2026-07-12-scp-color-language.md` §6's core rule is **"Desaturation =
  reality. Saturation = anomaly"**, with baseline reality described as *"near-monochrome and slightly
  warm — like a photocopied document"*; §7's guardrail is **"Don't: Give the Foundation a house
  palette. It doesn't have one and shouldn't"**, and **"Make color mean deviation, not danger."**
- *Perceptual*: Wolfe 2021, *Guided Search 6.0* (`10.3758/s13423-020-01859-9`) — only a small set of
  features guide attention at all, and colour is one of them. A saturated hue spent on permanent
  chrome is the one channel that could have pulled the eye to a threat, spent on a background.
  Rosenholtz 2016 (`10.1146/annurev-vision-082114-035733`) adds the periphery half: a screen-edge
  element is encoded as **summary statistics** — a colour blob and gross motion — so hue is doing
  *more* work out there, not less, and icon detail or small text at the edge is wasted pixels.

`UiTheme::anomaly` holds the vacated saturation and is exempt from the ceiling by design, along with
`danger` (destructive actions) and `warn` (cautionary copy). The exemptions are asserted **loudly** —
if one of them desaturates, it has stopped carrying its meaning.

**Threat is luminosity, never hue.** `ui::theme::Hazard` implements the SCP ACS Disruption scale
(Dark → Vlam → Keneq → Ekhi → Amida) as a brightness ramp on one hue. Two independent arguments land
in the same place:

- *In-fiction*: `docs/lore/2026-07-12-scp-color-language.md` §6 — "Use the ACS luminosity scale, not
  a color scale."
- *Perceptual*: a green→amber→red ramp puts the whole signal in the channel that fails for red-green
  colour vision deficiency. Machado, Oliveira & Fernandes 2009 (`10.1109_tvcg.2009.113`) put that
  population at ~200 million.

**Every status carries a second, non-colour channel.** A glyph (`ui::theme::glyph`), a length
(`rows::Cell::Bar`), or a word. Redundant coding beats an opt-in colourblind palette on both counts:
there is no toggle to find, and it helps *everyone* reading the HUD in peripheral vision while
looking at the world. This is why there is no `colorblind_safe` setting — see `src/settings.rs`.

**Length beats colour for magnitude.** Cleveland & McGill's accuracy ordering for elementary
perceptual tasks runs position > length > angle/area > colour. A bar is a length; a tinted swatch is
not. *(Not in the library — DOI `10.1080/01621459.1984.10478080`. §6.)*

**Persistent alerts are medium contrast, not maximum.** Lewandowska, Dziśko & Jankowski 2022
(`10.1038_s41598-022-16284-2`), on peripheral GUI alerts: "A medium contrast level, a horizontal or
vertical display localization, and a flashing frequency of **2 Hz** are sufficient to obtain the best
visibility in the peripheral area", and "a high visual intensity is not necessarily needed for the
best impact." High contrast is a **burst-only** tool: reserve it for short-lived critical alerts,
because sustained high contrast "can cause unnecessary irritation or even cognitive load for more
extended usage."

This is why `reduce_flashing` **damps** the VHS pass to a quarter rather than switching it off
(`vhs::drive_fade`): the glitch is a narrative tell, and deleting it removes information rather than
softening it.

**Two scale knobs, both Bevy-native, one path each.** `accessibility.text_scale` → `RemSize` (all
text is emitted as `FontSize::Rem`); `hud.hud_scale` → `UiScale` (scales every `Val::Px`). Do **not**
add a third hand-rolled multiplier: the one that used to exist reached only `font_size`, so raising
it grew glyphs inside boxes that stayed the same size and text overflowed the chrome.

### §1.4 Copy discipline — an unmet condition is an instruction

The strongest rule in this codebase, set by FVS-L-1 and enforced by unit tests in four panels.

- ✅ `RAISE OBSERVATION  ≥ 0.50   now 0.10` ❌ `observation: unmet`
- ✅ `LOCKED — NEEDS: DEPLOY MORALE FIELD` ❌ a padlock icon
- ✅ `NO INFORMATIVE TEST REMAINS` ❌ an empty list

An empty panel reads as a bug. If there is nothing to show, **say what state that is** and, where
one exists, say the route out of it. `tests/lore_canon.rs` additionally lints all UI copy for
deprecated antagonist terms.

### §1.5 Operability liveness — the oracle

This environment cannot produce a meaningful pixel screenshot (no monitor → black drawable), so
screen liveness is asserted structurally instead.
`tests/replay.rs::ui_screens_spawn_and_pause_blocks_the_sim` boots the **real** `UiPlugin` headless
and asserts:

- `Boot → Title` within 40 frames, and the title blocks the sim;
- entering the game unblocks it and the HUD's **named parts** spawn (roster, boss bar, speed
  readout) — not a root count, because the HUD is legitimately three entities in three regions and a
  count would pass for three boss bars;
- the layout frame exists **once** and owns **all nine** regions, each exactly once — the
  machine-checkable form of the overlap bug that had two panels drawing on top of each other;
- every in-game panel resolved into a region rather than silently vanishing;
- the pause menu spawns and re-blocks the sim.

Extend this test when you add a screen. A panel that fails to find its region renders nothing at all,
and this is what catches it.

---

## §2 Density

`HudSettings` is the player's control over how much the HUD says. `H` cycles the roster preset; the
same values live in the settings menu and persist.

Iacovides, Cox, Kennedy, Cairns & Jennett 2015 (`10.1145_2793107.2793120`) is the evidence: removing
the HUD **helped experts and did nothing for novices**. The interface × expertise interaction ran
F(1,20)=4.32, p=0.051, partial η²=0.178, driven almost entirely by experts (overall IEQ 133.83
diegetic vs 119.00 with the HUD present; novices flat at 124.50 vs 126.25).

**What the HUD costs experts is specifically control and cognitive involvement** — Control
F(1,20)=10.05, p<0.01, η²=0.334; Cognitive Involvement F(1,20)=7.80, p<0.05, η²=0.280. Real-world
dissociation and emotional involvement did not move.

So the things to cut first at lower density are the ones competing for **attention and agency** —
markers, tags, pickup toasts — never atmosphere, audio, or the anomaly reveals.

**A lower density preset must never break playability.** Study 1 of the same paper found no
significant difference in enjoyment (t=−0.17, df=8, p=0.87, d=0.056) or frustration (d=0.34) between
versions. If a run cannot be completed at the minimal preset, the information was load-bearing and
belongs *in the world*, not on the density ramp.

---

## §3 Information architecture

### §3.1 Panels are rows, not strings

`src/ui/rows.rs`. Every panel builder is a **pure function returning `Vec<Row>`**. A renderer maps
rows to nodes with a shared column rhythm.

This is not a style preference. When a panel was one `Text` node holding a `\n`-joined string it had
one `TextColor` — so in the containment readout, whose entire job is saying *why* a capture is
progressing, the met clause and the actionable unmet one rendered in identical ink. The player had to
*read* the panel to find the line to act on rather than *see* it.

Keep the builders pure. Tests assert on structure (`Emphasis`, `glyph`, which cell carries a
`Delta`), which pins what the player perceives rather than the string that encoded it.

### §3.2 Show the delta, not only the state

Andersen, Miller, Kiverstein & Deterding 2022 (`10.3389_fpsyg.2022.924953`): players are "sensitive
not just to absolute error, but also to changes in the rate of error reduction: positive affect
emerges when error reduction accelerates, that is, when we are doing better than expected." Optimal
challenge affords not maximal but "maximally *consumable*" uncertainty.

So: a level the player cannot act on is worth less than the change their next action would produce.
Show both, delta first. `research_hud` pairs `REMAINING UNCERTAINTY` with the signed
`+bits` each offered test would yield; `debrief` pairs the standing `O5 BUDGET` with what this
expedition **earned**. `rows::Cell::Delta` always prints its sign — the direction is the message.

### §3.3 Always name the next goal

Phan, Keebler & Chaparro 2016 (`10.1177_0018720816669646`, N=629, Cronbach's α=0.84) validated the
GUESS. In the Usability/Playability subscale, the **lowest-scoring item in the whole subscale** was
*"I always know my next goal when I finish an event"* (M=5.46 of 7) — the industry's weakest link.

Two places in this game are literally "when I finish an event", and both are tested:

- the objective line, which switches to the extraction instruction the moment the quota is met
  (`verb_bar::objective_line`, `the_objective_is_never_blank_in_any_phase`);
- the debrief, which ends with a `NEXT` section whatever the outcome
  (`debrief::debrief_rows`, `the_debrief_always_names_the_next_goal`).

Note also from the same paper that **visual aesthetics correlates with overall satisfaction (≈0.43)
about as strongly as usability does (≈0.31)**. The look is not decoration applied after the layout
works; build both together.

### §3.4 Alerts have a budget

Ancker et al. 2017 (`10.1186_s12911-017-0430-8`): acceptance of clinical advisories dropped **~30%
for each additional alert per encounter**, and ~10% for every 5-percentage-point rise in the share of
*repeated* alerts. Critically, the mechanism is **not** desensitisation — across six newly deployed
alerts, acceptance showed no decline over time. It is cognitive overload from uninformative volume,
and an uninformative alert "is essentially a false alarm."

**So you cannot fix alert spam by animating harder or recolouring.** Delete the low-informativeness
alerts. One interruptive alert per tactical beat; dedupe by entity so the same anomaly cannot fire
twice in one encounter; everything else goes to a passive line.

`ui::event_line` implements exactly that, and it is the game's **only** notification surface — there
was none at all before it. Three consequences of the finding, made structural rather than advisory:
`Severity::Passive` has *no surface* (deleted, not demoted — volume is the mechanism, intensity is
not); `ReportedSubjects` dedupes by entity per expedition, which is the unit Ancker measured
"encounter" as; and a blank line is refused outright, because an uninformative alert "is essentially a
false alarm."

Two further rules the line obeys, both already sourced elsewhere in this document:

- **It fades.** Lewandowska et al. found a *constant* peripheral element habituates away and stops
  working when it matters. Arriving-and-leaving is a contrast **delta**; permanent is a steady state
  that has stopped carrying information.
- **It pips.** Van der Burg, Olivers, Bronkhorst & Theeuwes 2008 (`10.1037/0096-1523.34.5.1053`, **in
  the library, full text**). A non-spatial, *non-informative* tone synchronised with a visual change
  makes that change pop out of a cluttered dynamic display. The size of it is the part worth quoting:
  search slope **147 ms/item without the tone → 31 ms/item with it**, and *with* the tone the set-size
  effect stopped being significant at all (F(2,10)=1.9, p=.224) — the target went from hunted to seen.
  Tone presence F(1,5)=10.7, p<.05, ηp=.68; Tone × Set Size F(2,10)=12.7, p<.005, ηp=.72.
  Their own controls rule out the two cheap explanations: it is **not general alerting** ("does not
  occur with visual cues") and **not top-down cuing** (it survives synchronising the pip with
  distractors on most trials). **Synchrony is the mechanism**, so the pip must fire *with* the line,
  not near it. Salience for **zero pixels** — the right currency for a horror HUD.

### §3.4.1 Tooltips are for controls, never for readouts

`rows::Row::label()` has reserved a tooltip hook since the row model landed, and it is still
unimplemented **on purpose**. §1.4's rule is that a row states its instruction *inline*
(`RAISE OBSERVATION  ≥ 0.50  now 0.10`), and Llanos & Jørgensen 2011 is explicit that information
which is critical or continuously gauged is the wrong thing to minimise. A containment clause behind a
hover would undo both at once.

A *verb* is the opposite case: its meaning is learned once and then never needed again, which is
precisely what a hover is for. So `verb_bar::Verb::hint` explains the five verbs, and nothing else in
the game carries a tooltip.

---

## §3.4.2 Spatial orientation — the map is a verb

**Director's decision, 2026-07-29:** an extraction beacon and off-screen bearings *always*; a minimap
**only while an Engineer sensor drone is live**. The three surfaces are `containment::extraction`'s
light column, `ui::offscreen`, and `ui::minimap` gated on `crate::sensor`.

Say the evidence honestly first: **there is no study isolating minimap presence against immersion or
performance, in any genre.** Battlefield's "hardcore" servers remove it as a *difficulty* measure and
Iacovides et al. speculate in discussion that the effect is really immersion-via-fewer-distractors, but
that is a speculation, not a result. The design below is reasoned, and the reasoning is what has to be
preserved if someone changes it.

- **The gate, not the map, is the design.** McCall et al. 2022 (`10.3758/s13428-022-02002-3`) separate
  **spatial uncertainty** from temporal unpredictability as its own axis of ambiguity, and a permanent
  minimap deletes that axis. Delatorre et al. 2019 (`10.1109/access.2019.2924200`) put a number on the
  middle ground: the optimum is *"randomly providing the **minimal** amount of information that still
  allows the player to counteract the threat"* — not none (frustration), not a radar (tension
  collapse). A map you switch on for seconds is that middle.
- **The cooldown must outlast the duration.** Otherwise the player chains drones into the permanent
  minimap this rule exists to prevent, and it looks like a balance tweak rather than the deletion of a
  horror axis. `sensor::the_cooldown_outlasts_the_reveal` is the guard.
- **The minimap never draws a creature.** The fog already withholds enemies outside live line of sight
  — `fog::visible_at` is what `laser::fire_laser` targets through — and enemy blips would hand back
  exactly what the fog exists to take away, while claiming to be a spatial aid.
- **It reveals topology you have walked, not topology that exists.** Gated on `fog::seen_at`, bounded
  by `sensor::SENSOR_RADIUS`. A sensor is not a wallhack.
- **Prefer lighting to markers for the fixed destination.** Marples 2017 (PhD, Huddersfield) derives
  the thresholds at which guidance lighting begins to bias a route and finds a usable window *below*
  the level at which players notice being guided — which is what a horror game wants. Hence the
  extraction beacon is a light column, and the HUD marker is only a bearing for when it is off-screen.
- **An edge marker is a colour blob, not an icon.** Rosenholtz 2016 — the periphery encodes summary
  statistics, so detail out there is wasted pixels — and its pulse sits at the *bottom* of
  Lewandowska et al.'s effective band, because a permanent marker is the extended-usage case by
  definition.

The prior reading in §6 (*"in a WFC dungeon with limited sightlines, connectors and navigational cues
do work no minimap can"*) still stands and is not in tension with this: the argument was never that the
information is worthless, it was that **free continuous access is the wrong price**.

---

## §3.5 Controls

The section this document did not have, and whose absence was load-bearing: key allocation used to
live in **five hand-written prose censuses** (`selection.rs`, `site/review.rs`,
`knowledge/records.rs`, `antagonist.rs`, `ui/research_hud.rs`), all five of which had drifted to the
same wrong answer — every one named `T` as taken, long after the `T` hotkey was deleted.

**Every binding is data, in `src/input/`.** `Action` is the census; `KeyBindings` is the live table
(a fixed array indexed by `Action::index`, not a map — same reasoning as `ui::layout::HudRegions`);
`Actions` is the read side, and taking it instead of `Res<ButtonInput<KeyCode>>` is what applies the
focus guard. `the_key_space_has_no_collisions` is what keeps the census true.

**Some collisions are legal, so contexts are part of the model.** `W` is both "pan forward" and "menu
up"; the two can never be live together. Each action declares a `Context`, and the collision test
asks whether two contexts can be live *simultaneously* (`Context::overlaps`) rather than applying a
flat uniqueness rule that would force a worse binding. `Context::Dev` deliberately overlaps every
play context — a dev key shadowing a player key is exactly the bug being hunted.

**The focus guard has one home.** While a menu button holds `InputFocus` or the dev note box is
taking raw text, the keyboard belongs to that, and every non-`Menu` action is suppressed.
`research_room::editor` found this the hard way (one `Space` both clicked the focused palette button
and toggled the pause) and grew a local copy; that copy is gone.

**Input and Control are two Access categories, not one.** Power/Cairns et al. 2019 separate *Input*
options (which key) from *Control* options (hold vs toggle, how many actions a task needs). §4.1's
Access/Challenge split applies to both, and neither may be gated by difficulty.

**Immediately-issuable orders stay at ~3–4.** Itthipuripat et al. 2018
(`10.1523/jneurosci.0440-18.2018`) isolate the mechanism behind Hick's-law slowing while controlling
difficulty and attention: added choices leave *sensory* processing untouched and instead **raise the
decision threshold**. More options cost commitment time, not perception — under horror time pressure
that reads as freezing. Anything past the first few belongs behind a deliberate mode.

**A vocabulary of ~12 is learnable to recall in ~30 minutes.** Zheng et al. 2018, *M3 Gesture Menu*
(`10.1145/3173574.3173823`) demonstrated recall-based execution of a dozen commands after three
ten-minute sessions, and found the **grid/gesture** form beats a radial one past ~8 items. Wentzel et
al. 2024 (`10.1109/tvcg.2024.3420236`) split it further: flat menus favour direct pointing,
*hierarchical* ones favour marking. Samp 2011 adds that radial's cost is paid at first sight — so fix
item positions permanently and never reorder by recency.

**Offering a fast path beside a slow one does not work on its own.** Cockburn, Gutwin, Scarr &
Malacria 2014 (`10.1145/2659796`) document the intermodal-transition failure: users plateau on the
slow method and never switch, because no single moment hurts enough to justify the switching cost.
The fix is a *bridge* where the novice path rehearses the expert path (Kurtenbach & Buxton 1994's
marking menus are the canonical one). Practically: a control group that binds invisibly is a control
group nobody binds twice — the readout has to show it happened. `hud::RosterChipTag` is that readout.

**The order set is small because the mode vocabulary is gated on applicability, not preference.** This
review set out to expose `ai::utility::Mode`'s squad actions as a twelve-item order wheel, and reading
the repertoires killed it. Every role behaviour in `squad_ai::role` is `gate(Fact::…)` on whether the
action is *possible*: `Ward` needs `PhotophobeBearingKnown`, `TendWounded` needs `AllyDownNearby`,
`Examine` needs `HasUnexaminedNearby`. Ordering one with its gate off does **nothing at all** — and a
control that silently does nothing is the "magic results that are hard to debug" this project's first
rule exists to prevent. (`Suppress` and `DeploySensor` are not even authored into a repertoire; the
real count was never twelve.)

Exactly one genuine *preference* survives that filter, and it was a real gap: the Gunman has
`Overwatch` at rank 4 and `Engage` at rank 2 **on the same gate**, so gunmen always held and the
player could not ask them to close. `squad::PushOrder` is that ask, and with `HOLD FIRE` and the move
order it makes three immediately-issuable orders — which is exactly Itthipuripat et al.'s budget above,
arrived at from the other direction.

**A player order overrides the decision; it does not become a brain parameter.** The tidy
implementation is a consideration on `Overwatch` reading a new `Fact`, and it would even be
bit-identical headless (multiplying a score by exactly `1.0`). But `squad_ai::genome` encodes a
repertoire by **walking its considerations in order**, so one more grows the Gunman's genome by three
params and invalidates every archived behaviour elite. So the override sits at the decision seam in
`squad_ai::perception`, next to — and for the same stated reason as — the `MoveOrder` branch that
already overrides the AI's movement goal. Verified: the shipped golden and
`deterministic_core_is_bit_identical_across_many_builds` both unmoved.

**Budget the onboarding against 23%.** Iacovides et al. 2015 dropped **7 of 31** screened
participants — all self-reported FPS players — for "obviously struggling with the controls" inside 20
minutes. `ui::controls_screen` exists because the game shipped ~26 player bindings of which 6 were
stated anywhere; it is reachable from the **title** as well as the pause menu, because a player who
cannot work the controls has not started a run yet.

**The controls screen lists what the registry cannot see.** Mouse buttons are not keys, and the
digit row is one mechanism with nine slots rather than nine rebindable actions. A screen generated
purely from `Action` would omit the two most-used inputs in the game, so `UNBOUND_LINES` states them
explicitly and a test asserts they are there.

---

## §4 The four lenses

Every UI change is checked against four lenses. The first two are player-facing; the last two are
this codebase's own invariants.

### §4.1 Presentation lens — Access vs Challenge

Power, Cairns, Barlet & Haynes 2019 (`10.1016_j.ijhcs.2019.06.010`) split options into **Access**
(Input, Control, Presentation, Output) and **Challenge** (Performance, Training, Progress, Social,
Moderation). Their brief: *"Games are meant to be difficult but not difficult to access."*

The settings menu is grouped this way, and `settings_menu::is_access` pins which group each setting
belongs to. Text scale, HUD scale, and reduce-flashing are **Access** and must never be gated by
difficulty. Boss bar and roster density are **Challenge**.

### §4.2 Operability lens

Everything reachable by mouse is reachable by keyboard and vice versa. Menu keyboard navigation is
registered **once, globally** in `UiPlugin`, so a new screen gets it by putting a `TabGroup` on its
root and cannot forget it. Verb chips are clickable *and* keyed, and each chip states its key.

### §4.3 Determinism lens

- `Update` / `OnEnter` / `OnExit` only. **Never `FixedUpdate`.** Asserted by
  `tests/replay.rs::ui_never_leaks_into_deterministic_core`.
- Never gate a gameplay plugin on `in_state(AppState::*)` — the harness has no `AppState`.
- `ui::state::sync_sim_blocked` is the single writer of `SimBlocked`.
- UI may emit an **input intent**, but must not write sim state. The clickable verb bar sends
  `selection::ArmRequest`; `arm_tool_input` stays the single writer of `ArmedTool`.
- `tests/determinism_lint.rs` scans `src/ui/` — any `sort*` needs `sort_total!`,
  `util::sort_value_canonical`, or a `// SORT-OK: <why>` comment.

### §4.4 Evolution lens — what the RL/QD search may touch

**No UI value may become a genome gene.** `docs/animation.md` carves the same exemption for the
cosmetic animation layer and states the reason: everything invisible to `snapshot_hash` "by
construction" would be a knob the search turns forever with the fitness never moving. UI meets that
test identically.

The one legitimate seam is §4.1's split: **Challenge** options describe the game and are fair game
for the search; **Access** options describe the *player* and are not — evolving them would be
optimising against whoever is sitting at the keyboard.

---

## §5 Bevy 0.19 mechanics and traps

Pinned to the lockfile. `bevy_ui`, `bevy_ui_render`, `bevy_ui_widgets`, `bevy_text`,
`bevy_input_focus`, `bevy_picking` are all in the build graph. `bevy_feathers` is in the lockfile but
**not** in the graph, and its visuals are Bevy's editor skin — do not adopt them.

**Do not add a UI framework crate.** As of 2026-07-28 none of `bevy_lunex`, `sickle_ui`,
`bevy_cobweb_ui`, or `woodpecker_ui` has a 0.19 release; sickle_ui's repo 404s and bevy_cobweb_ui is
archived. Stock `bevy_ui` + `bevy_ui_widgets` is both the low-risk path and the one that matches the
one-path-per-feature rule.

### Available and used here

| Capability | Where |
|---|---|
| `Display::Grid` + `RepeatedGridTrack::flex` | `layout.rs` — the 3×3 region frame |
| `ScrollArea` + `Overflow::scroll_y()` | `site_hud.rs`, `research_hud.rs` |
| `Button` + `On<Activate>` observers | `widgets.rs`, `verb_bar.rs` |
| `FontSize::Rem` + `RemSize` | `widgets.rs`, `theme.rs` |
| `UiScale` | `theme.rs` |
| `Node.border_radius` (a **field** since 0.18, not a component) | `verb_bar.rs` |
| per-edge `BorderColor` | `widgets::border_all` |

### Traps

1. **`GridPlacement::{start, end, span}` panic on `0`** and have no `try_` variant. This codebase
   forbids panics — `layout.rs` uses grid **auto-flow by spawn order** instead, which has no panic
   surface. If you ever need explicit placement from runtime data, guard it.
2. **A missing `Res<T>` panics the system in 0.19** rather than skipping it. Any resource a UI system
   reads must either be `init_resource`d by the plugin that registers the reader, or taken as
   `Option<Res<T>>`. The UI-liveness test builds a bare `App` with `UiPlugin` alone and will find
   this immediately.
3. **`UiScale` scales `Val::Px` only** — not `Percent`/`Vw`/`Vh`. That is the desired behaviour here
   (panels keep their proportions, their chrome grows) but it is not obvious.
4. **The embedded default font is a 95-codepoint subset.** `Handle::<Font>::default()` resolves to
   Bevy's `FiraMono-subset.ttf`, which is essentially bare ASCII — every `▓`, `—`, `…`, `◢`, `•`,
   `→`, `·` in UI copy renders as tofu under it. Always use `FontAssets`, which loads the full
   `assets/fonts/FiraMono-Regular.ttf` (1350 codepoints). **Verify any new glyph against that face**
   — `✓` (U+2713), `▶`/`▸` (U+25B6/25B8), `⚠` (U+26A0) and `★` (U+2605) are *not* in it.
   `ui::theme::glyph` holds the checked set.

   **`emerge-mapper` deviates deliberately, and this is the record of it.** It is a separate binary, so
   instead of threading a `FontAssets` resource it overwrites `AssetId::default()` with the full face
   at startup (`crates/emerge-mapper/src/main.rs`) — the same font, the opposite mechanism. The rule
   above exists because a call site reaching for `Handle::default()` got the subset; there, the default
   **is** the shipped face, so there is no second thing to reach for and no site that can forget. One
   binary, one face, one place it is decided. Do not copy this into the game, which has more than one
   font and a real asset lifecycle.
5. **UI draws after post-processing**, so the VHS pass cannot reach it and HUD text stays crisp.
   Keep it that way. UI-side CRT would need a `UiMaterial` on a fullscreen node, and §1.2/§1.3 argue
   against it.
6. **`ImageNode::default()` is an invisible 1×1 transparent texture.** Always `ImageNode::new(h)`.
7. **`Pickable::IGNORE` is per-entity.** A full-screen container needs it or it eats every world
   click; its children stay pickable, which is what lets panels inside the frame have hit targets.
8. **A UI node sized only by its text is 7 logical px tall when that text starts empty.** State a
   `min_height` on anything clickable — a laid-out field measured 7 px is not a target anyone can hit
   on purpose. Found in `emerge-mapper`'s subgrid division fields.
9. **`bevy_ui_widgets::Button` fires `Activate` on click**, and the observer pattern is
   `On<Activate>` + `Query<&Marker>` + `marker.get(activate.entity)`.
10. **Layer order lives in `theme.rs`.** `Z_HUD` < `Z_PANEL` < `Z_BLOOD_LENS` < `Z_MENU_DIM` <
   `Z_MENU`, asserted by a test. Do not spell a layer as arithmetic on another one — seven panels
   used `Z_MENU - 1` and ended up *above* the pause scrim.

### Coming at 0.20 — do not write code that makes these worse

- `BorderRadius` fields become `Val2` and its constructors **lose `const`**. Do not introduce a
  `const BorderRadius` in `theme.rs`.
- `ui::Interaction` and `ui::widget::Button` are deprecated in favour of
  `ui_widgets::Button` + `picking::hover::Hovered` + `On<Activate>`. `src/ui/` is already on the new
  pattern; `perf_hud.rs` and `blood_lens.rs` still reference `Interaction`.
- `FocusCause` gains an `Auto` variant — breaks exhaustive matches.

---

## §6 Audio (documented, not implemented)

Not in scope for the current pass. Recorded here so the rules exist before the audio overhaul lands.

- **Every cue must clear 70% blind identification.** Garzonis, Jones, Jay & O'Neill 2009
  (`10.1145_1518701.1518932`) rejected an auditory icon scoring 64% and set the rule: "sound-source
  identification rates have to be higher than 70% (across a convenient population sample)". Run a
  10-person listening test per cue; below 70%, redesign the metaphor rather than adding a text toast
  to compensate.
- **Real-world metaphor sounds, not abstract musical motifs.** Same paper: untrained identification
  of 20 stimuli averaged 10.65 (SD 2.85) for auditory icons vs 2.00 (SD 1.12) for earcons,
  t(16)=11.007, p<0.001; after a week of training, 15.07 vs 4.20. Earcons never catch up and are
  disliked. Their rule: **"Least frequent notifications have the greatest need to be based on
  meaningful metaphors"** — so a Keter-class breach gets the *most* concrete sound design, because
  the player has had no rehearsal.
- **Audit the bank by role.** Grimshaw & Schott 2007 (`10.26503_dl.v2007i1.313`) split game audio
  into perceptual *sureties* and *surprises*, the latter into **attractors** (invite an action),
  **connectors** (aid orientation), and **retainers** (encourage lingering), plus **navigational
  listening** as a fourth mode. In a WFC dungeon with limited sightlines, connectors and navigational
  cues do work no minimap can.
- **Anything conveyed only by audio needs a visual equivalent.** Nacke, Grimshaw & Lindley 2010
  (`10.1016_j.intcom.2010.04.005`) found a significant main effect of sound on **all seven** GEQ
  dimensions, so muting is a real accessibility loss, not a preference.

---

## §7 Bibliography

### In the home-still library

| Stem | Paper |
|---|---|
| `10.1145_2793107.2793120` | Iacovides, Cox, Kennedy, Cairns & Jennett 2015, *Removing the HUD*, CHI PLAY |
| `10.26503_dl.v2011i1.514` | Llanos & Jørgensen 2011, *Do Players Prefer Integrated User Interfaces?*, DiGRA |
| `10.1177_0018720816669646` | Phan, Keebler & Chaparro 2016, *The GUESS*, Human Factors |
| `10.1016_j.ijhcs.2019.06.010` | Power, Cairns, Barlet & Haynes 2019, *Future design of accessibility in games*, IJHCS |
| `10.1186_s12911-017-0430-8` | Ancker et al. 2017, *Alert fatigue*, BMC Med Inform Decis Mak |
| `10.3389_fpsyg.2022.924953` | Andersen, Miller, Kiverstein & Deterding 2022, *Mastering uncertainty*, Front. Psychol. |
| `10.1038_s41598-022-16284-2` | Lewandowska, Dziśko & Jankowski 2022, *Contrast, habituation and sensitisation in peripheral GUI areas*, Sci Rep |
| `10.1109_21.156574` | Vicente & Rasmussen 1992, *Ecological Interface Design*, IEEE SMC — **downloaded, not yet converted** |
| `10.1145_1518701.1518932` | Garzonis, Jones, Jay & O'Neill 2009, *Auditory icon and earcon notifications*, CHI |
| `10.26503_dl.v2007i1.313` | Grimshaw & Schott 2007, *Situating Gaming as a Sonic Experience*, DiGRA |
| `10.1016_j.intcom.2010.04.005` | Nacke, Grimshaw & Lindley 2010, *More than a feeling*, Interacting with Computers |
| `fdg2014_fdg2014_wip_14` | Nordin, Cairns, Hudson, Alonso & Calvillo Gámez 2014, *The Effect of Surroundings on Gaming Experience*, FDG |
| `10.1145_3235765.3235790` | Green et al. 2018, *AtDELFI*, FDG |
| `10.1109_CCNC.2016.7444811` | Salomoni et al. 2016, *Diegetic game interface with Oculus Rift*, IEEE CCNC |
| `10.1109_tvcg.2009.113` | Machado, Oliveira & Fernandes 2009, *CVD simulation*, IEEE TVCG — **record page only** |

### Cited above, surfaced by the 2026-07-29 UI/controls review

**Ingest status, measured 2026-07-29 — and be precise about it, because the note that used to stand
here was false for some time and nobody noticed.**

- **Downloaded, converted, indexed, full text in hand:** `10.1037/0096-1523.34.5.1053` (pip and pop,
  24 chunks). Reading it upgraded the claim materially — §3.4's bullet now carries the real search
  slopes and both of the paper's own controls, where the abstract had supported only "makes a change
  pop out".
- **Downloaded and catalogued, conversion failing:** `10.3758/s13423-020-01859-9`,
  `10.1109/access.2019.2924200`, `10.26503/dl.v2018i3.1051`, `10.1109/tvcg.2024.3420236`,
  `10.1080/10447318.2023.2210880`. The PDFs are on disk; `scribe_convert` returns *"olmocr did not
  write the expected file"* for each. The scribe instances both report `healthy`, so this is the
  backend writing no output rather than a queue — the same **service-ownership** question this section
  has recorded before, not a code change. Retrying is cheap once it is resolved.
- **No open-access PDF for the DOI at all**, needs a manual fetch: Cockburn et al.
  (`10.1145/2659796`), the M3 gesture menu (`10.1145/3173574.3173823`), both Misztal papers,
  Rosenholtz (`10.1146/annurev-vision-082114-035733`), van den Berg (`10.1167/9.4.24`), Itthipuripat
  (`10.1523/jneurosci.0440-18.2018`).

Every claim attributed to anything in the last two groups — in §1.3, §3.4, §3.4.1, §3.4.2 or §3.5 —
is sourced from its **abstract or published results summary, not from full text**, and the module docs
in `src/` that cite them say so at the site.

| DOI | Paper | Fills |
|---|---|---|
| `10.1145/3410404.3414238` | Misztal, Carbonell & Schild 2020, *Visual Delegates*, CHI PLAY | 67 catalogued ways games render a character's non-visual sensation — **the** reference for a body-horror game |
| `10.1145/3491102.3501885` | Misztal & Schild 2022, *VD-frame*, CHI | the evaluation framework for the above |
| `10.1145/2659796` | Cockburn, Gutwin, Scarr & Malacria 2014, *Novice to Expert Transitions*, ACM Comput. Surv. | §3.5's intermodal-transition failure |
| `10.1145/3173574.3173823` | Zheng, Bi, Li, Li & Zhai 2018, *M3 Gesture Menu*, CHI | the 12-commands-in-30-minutes budget |
| `10.1145/191666.191759` | Kurtenbach & Buxton 1994, *Marking menus*, CHI | novice use as expert rehearsal |
| `10.1109/tvcg.2024.3420236` | Wentzel et al. 2024, *VR Menu Archetypes*, IEEE TVCG | flat → direct pointing, hierarchical → marking |
| `10.1523/jneurosci.0440-18.2018` | Itthipuripat et al. 2018, *J. Neurosci.* | choice count raises the **decision threshold**, not perceptual load |
| `10.3758/s13423-020-01859-9` | Wolfe 2021, *Guided Search 6.0*, Psychon Bull Rev | which features guide attention (§1.3's palette argument) |
| `10.1146/annurev-vision-082114-035733` | Rosenholtz 2016, *Peripheral Vision*, Annu. Rev. Vis. Sci. | the periphery encodes summary statistics |
| `10.1167/9.4.24` | van den Berg, Cornelissen & Roerdink 2009, *J. Vision* | clutter is **crowding**, not element count — spacing ≈ 0.5 × eccentricity |
| `10.1037/0096-1523.34.5.1053` | Van der Burg et al. 2008, *Pip and pop*, JEP:HPP | a 20 ms tick makes a visual change pop out — salience for zero pixels |
| `10.3758/s13428-022-02002-3` | McCall et al. 2022, *The Underwood Project*, Behav Res Methods | **in the library**; dread from withheld response-mapping, and the always-on-soundtrack finding |
| `10.1109/access.2019.2924200` | Delatorre, León, Salguero & Tapscott 2019, *IEEE Access* | the threat-information optimum: minimal, stochastic, still actionable |
| `10.26503/dl.v2018i3.1051` | Boonen & Mieritz 2018, *Paralysing Fear*, DiGRA | agency parameters in horror |
| `10.1109/gem.2015.7377211` | Peacocke, Teather, Carette & MacKenzie 2015, IEEE GEM | the diegetic *number* won on precision + gaze co-location; diegetic *bullets* did not |
| `10.1145/3102071.3106358` | Yang et al. 2017, FDG | **in the library**; more information *improved* performance — "reduce clutter" is not free |
| `10.1080/10447318.2023.2210880` | Caroux & Pujol 2023, *IJHCI* meta-analysis | control mode showed **no** significant effect on enjoyment |
| — | Marples 2017, *Intrinsic perceptual cues on navigation*, PhD, Huddersfield | thresholds at which guidance **lighting** steers a route below conscious notice (the extraction beacon) |
| `10.1371/journal.pone.0075129` | Thompson, Blair, Chen & Henrey 2013, *PLoS ONE* | which skill variable predicts league **changes with league** |

**Three claims this project must stop making.** The corpus does **not** establish that diegetic UI
raises immersion: Iacovides et al. 2015 found it helps experts only, on two subscales; Llanos &
Jørgensen 2011 found it helps **nobody**; Peacocke et al.'s winning "diegetic" display is confounded
with precision and gaze position; and Fagerholt & Lorentzon 2009 — the taxonomy everyone cites — is
an unvalidated MSc thesis whose own position is a middle ground, and which never measured immersion.
`src/psi_vision.rs` and `src/dialogue/mod.rs` both currently cite that literature as if it settled
the question. The design choices are fine; the justification should say **contested**.

Likewise there is **no study isolating minimap presence/absence** against immersion or performance in
any genre, and RTS/squad *input* design is essentially absent from peer review — no work on tactical
pause, control groups, box-select or hybrid schemes. Where this document reasons about those, it is
inference, and `src/selection.rs`'s header says so in as many words.

### Wanted, not yet in the library

The library's classical human-factors shelf is empty. Each of these fills a load-bearing gap above;
all were attempted on 2026-07-28 and blocked (see the note below).

| DOI | Paper | Fills |
|---|---|---|
| `10.1518/001872095779049543` | Endsley 1995, *Toward a Theory of Situation Awareness* | what a tactical display is *for* (§1.1) |
| `10.1109/tvcg.2011.127` | Healey & Enns 2012, *Attention and Visual Memory in Visualization* | preattentive channels and their interference ordering (§1.3) |
| `10.1145/1077246.1077253` | Sweetser & Wyeth 2005, *GameFlow* | validated on two **RTS** games — the closest genre match |
| `10.1080/01621459.1984.10478080` | Cleveland & McGill 1984, *Graphical Perception* | the accuracy ordering §1.3 leans on |
| `10.1016/j.ijhcs.2008.04.004` | Jennett et al. 2008, *Measuring and Defining Immersion* | the IEQ source; four held papers *use* it |
| `10.1145/1357054.1357282` | Pinelle, Wong & Stach 2008, *Heuristic Evaluation for Games* | a game-specific heuristic set |
| — | Fagerholt & Lorentzon 2009, *Beyond the HUD* (Chalmers MSc) | origin of the diegetic/non-diegetic/spatial/meta taxonomy |

> **Ingest is no longer blocked — measured 2026-07-29.** The note that stood here said
> `hs-serve-olmocr-vllm.service` was crash-looping on `EADDRINUSE` for port 8081 and that PDFs hung
> in conversion. `system_status` now reports **both scribe instances healthy** (one idle with 12 free
> slots, one actively converting), distill up on CUDA with **8,150 embedded documents / 247k chunks**,
> and conversions completing in 50–230 s. `pipeline_drift` is 130 against a threshold of 3, which is
> a backlog, not a stall. **The papers below are fetchable; the blocker was resolved and this note
> was not updated.**
>
> Still true: `paper_download` never consults CORE even when `paper_search` returns CORE URLs in the
> same response, which is what blocked Healey & Enns and Sweetser & Wyeth specifically.

---

## Where things live

| Concern | File |
|---|---|
| Design tokens, hazard ramp, chroma ceiling, glyph set, layers, scale wiring | `src/ui/theme.rs` |
| The keyboard registry — every binding, its context, the collision test | `src/input/` |
| The key list shown to the player | `src/ui/controls_screen.rs` |
| Selection, control groups, order queue | `src/selection.rs` |
| Off-screen bearings (extraction, selected operatives) | `src/ui/offscreen.rs` |
| The sensor-gated minimap, and the drone that gates it | `src/ui/minimap.rs`, `src/sensor.rs` |
| The one-line event budget + its emitters | `src/ui/event_line.rs` |
| The row model + renderer | `src/ui/rows.rs` |
| The 3×3 region frame | `src/ui/layout.rs` |
| Shared widgets, menu focus/keyboard nav | `src/ui/widgets.rs` |
| Screen state machine, the single `SimBlocked` writer | `src/ui/state.rs` |
| In-game panels | `src/ui/{hud, containment_hud, verb_bar, briefing}.rs` |
| Site panels | `src/ui/{site_hud, research_hud}.rs`, `src/knowledge/records.rs`, `src/site/review.rs` |
| Menus | `src/ui/{title, pause, settings_menu, boot, warmup, debrief}.rs` |
| World colours (**not** UI) | `src/palette.rs` |
| Art direction | `docs/lore/2026-07-12-scp-color-language.md` |
