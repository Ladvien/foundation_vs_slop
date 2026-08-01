# Two live layers — the expedition keeps ticking while you are at the Site

*2026-08-01. Director's call: the Site becomes a place you **risk** visiting. Design only; nothing
below is implemented.*

---

## 1. The good news: the architecture already separates these

The instinct is that this is a deep change. It is mostly not, because the two concepts are already
distinct in this codebase and were kept that way on purpose:

| | owns | today |
|---|---|---|
| `session::RunState` | **the simulation** — is an expedition alive and ticking | `Idle` / `Active` |
| `ui::state::AppState` | **the screen** — where the player is looking | `Title` / `InGame` / `Site` / `Debrief` / … |

Evidence this was deliberate, not luck:

- `RunPhase` is a SubState sourced on **`RunState::Active`, not `AppState::InGame`** — and
  `session/mod.rs:129` says why: "the containment systems that will read it must run headless."
- `ui::containment_hud` gates on the two **independently**:
  `.run_if(in_state(AppState::InGame)).distributive_run_if(in_state(RunState::Active))`.
- `AppState` is asserted absent from the deterministic core by
  `replay::ui_never_leaks_into_deterministic_core`.

So "the sim is running" and "the player is watching it" are already different questions. **What
couples them is not the architecture — it is two UI handlers** (`ui/debrief.rs:284` and
`ui/pause.rs:141`) that set `RunState::Idle` at the same moment they send the player to the Site.

## 2. The change, stated minimally

**Leaving for the Site must stop meaning "end the run".**

- **Visit** (new): `AppState::Site` while `RunState` stays **`Active`**. The expedition keeps ticking,
  unattended. This is the flip.
- **End** (existing): `RunState::Idle`, which despawns the expedition via `run_scoped()` and advances
  the seed. Reached by extraction, wipe, or an explicit *abandon*.

Everything gated `in_state(RunState::Active)` then keeps running while the player is at the Site,
which is the entire feature — and it is one condition already written on hundreds of systems.

## 3. What actually has to change

1. **Split the two verbs in the UI.** `RETURN TO SITE` becomes a *visit*; a separate, deliberate
   `ABANDON EXPEDITION` sets `RunState::Idle`. Today one button does both, silently.
2. **`advance_to_next_world` must not fire on a visit.** It runs `OnExit(RunState::Active)` and
   advances the seed — correct for ending a run, catastrophic for a visit (you would return to a
   *different world*). Since visits no longer leave `Active`, this is free — but it is the single
   most dangerous thing to get wrong, so it wants a test that names it.
3. **The camera.** It is deliberately not `run_scoped()`, so it survives; it needs to travel to the
   Site and back to the squad. `site::visuals::focus_camera_on_site` already does half of this.
4. **The squad, unattended.** This is the design question, not a technical one — see §4.
5. **Time.** `Time<Virtual>` must keep advancing at the Site. Check `ui::pause` does not zero it on
   the way there; pausing is currently reachable from the same screens.

## 4. The design question that decides whether this is good

**What does the squad do while nobody is watching?**

Three answers, and they are three different games:

- **Hold** — they stop where they stand. Safest, and it makes the Site nearly free to visit, which
  undercuts the whole "risk" premise.
- **Continue** — they keep executing the last order (advance, hold, extract). The Site costs you
  *control*, not safety. This is the Dungeon Keeper reading.
- **Autonomous** — the squad AI drives them fully. The Site costs you control AND they may do
  something you would not have chosen. Highest tension, and this is the one the RL policy archive
  baked on 2026-08-01 could actually supply — it is a trained squad controller with nothing driving
  it in play.

**Recommendation: Continue, with Autonomous as the follow-on.** Continue is legible — the player can
predict the cost of leaving — and it does not depend on the policy archive being good. Autonomous is
where this becomes special, and it finally gives the policy archive a job in the shipped game.

## 5. What this breaks, and what it makes possible

**Breaks:** every test and screen that assumes `AppState::Site` implies no live expedition. The
biggest is `persist` — it saves `OnEnter(AppState::Site)`, which would now fire mid-run and snapshot
a *live* expedition into the campaign save. That must move to run-end, not screen-change.

**Makes possible**, and this is why it is worth it:

- The Site's requisition and research stop being between-run bookkeeping and become **decisions under
  fire** — you buy a consumable knowing the squad is exposed while you shop.
- The O5 allowance gains real texture: time spent at the Site is time the expedition is unsupervised.
- The watch feed becomes genuinely nasty. It generates *while watched* — and leaving the squad
  standing in front of one while you visit the Site is a mistake the player makes exactly once.

## 6. Suggested order

1. Split visit-vs-abandon in the UI, with a test that a **visit preserves the run seed** and an
   **abandon advances it**. Do this first; it is the whole correctness risk.
2. Move `persist`'s save trigger off `OnEnter(AppState::Site)` to run-end.
3. Camera travel + a way back (the ASYNC door already exists as fiction and geometry).
4. Squad behaviour = **Continue**.
5. Only then consider Autonomous, wired to `FVS_POLICY_ELITE`.
