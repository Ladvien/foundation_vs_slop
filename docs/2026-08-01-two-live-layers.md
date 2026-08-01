# Two live layers — the expedition keeps ticking while you are at the Site

*2026-08-01. Director's call: the Site becomes a place you **risk** visiting.*

*Amended 2026-08-01, after checking every claim against HEAD. Three claims in the first draft were
wrong and are corrected in place — see §7 for what changed and why it matters. §4 is now decided.*

*Steps 1–5 of §8 are **implemented**; §8 marks what landed. Step 6 (shipping the policy archive) is
not started and is deliberately on its own schedule.*

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
- `ui::containment_hud` gates on the two **independently** (`ui/containment_hud.rs:56`):
  `.run_if(in_state(AppState::InGame)).distributive_run_if(in_state(RunState::Active))`.
- `AppState` is asserted absent from the deterministic core by
  `replay::ui_never_leaks_into_deterministic_core` (`tests/replay.rs:1139`).
- The camera deliberately outlives a run (`camera.rs:129`: "`DespawnOnExit` would take it").

And it is better than the above, in a way the first draft missed: **both worlds are already resident
in the same `World` every frame.** Site-67's geometry spawns on `Startup`, not
`OnEnter(AppState::Site)` (`site/visuals.rs:100`), and `site/layout.rs:371` *asserts* the layout
origin sits at ≥ (512, 512) — far outside any dungeon. There is no despawn/respawn to orchestrate and
no spatial collision to solve. The Site persists by simply not carrying `run_scoped()`
(`site/mod.rs`, "Why the Site persists for free"). Two live layers is, geometrically, already true.

So "the sim is running" and "the player is watching it" are already different questions. **What
couples them is a single UI handler** — `ui/debrief.rs:284` — which sets `RunState::Idle` at the
moment it sends the player to the Site.

## 2. The change, stated minimally

**Leaving for the Site must stop meaning "end the run".**

- **Visit** (new): `AppState::Site` while `RunState` stays **`Active`**. The expedition keeps ticking,
  unattended. This is the flip.
- **End** (existing): `RunState::Idle`, which despawns the expedition via `run_scoped()` and advances
  the seed. Reached by extraction, wipe, or an explicit *abandon*.

Everything gated `in_state(RunState::Active)` then keeps running while the player is at the Site,
which is the entire feature — and it is one condition already written on hundreds of systems.

## 3. What actually has to change

**1. A visit affordance has to be built — it cannot be a rename.** This is the correction that most
changes the shape of the work. There is **no mid-run path to the Site today at all**:

- `ui/debrief.rs:284` (`RETURN TO SITE`) is only reachable after `RunOutcome` resolves — the
  `Victory`/`GameOver` → `Debrief` chain. By the time that button exists the expedition is *over*, so
  it cannot "become a visit"; there is nothing left to leave running.
- `ui/pause.rs:141` (`QUIT TO TITLE`) goes to `AppState::Title`, **not** the Site. It is an
  abandon-to-title.

So a visit is a genuinely new `InGame → Site` transition on a live, unresolved run, and it needs a
new affordance. `MenuState` is a substate of `AppState::InGame` (`ui/state.rs:53`), so the pause menu
is the natural home for it — but note the corollary: **there is no pause menu at the Site**, so
`ABANDON EXPEDITION` must live either in that same in-game pause menu or on the Site HUD.

**2. The way back is currently blocked by an explicit guard.** `enter_the_door`
(`site/visuals.rs:292`) early-returns unless `RunState == Idle` (`:301`) and sets `AppState::Warmup`
(`:313`). During a visit `RunState` is `Active`, so the ASYNC door is **inert and the player is
stranded at the Site**. It has to become a match on run state, not a guard:

- `Idle` → begin an expedition: `RunState::Active`, `AppState::Warmup` (today's path, unchanged).
- `Active` → return to the live one: `AppState::InGame` directly, **not** `Warmup` — `Warmup` waits
  on `MoldWarm` and would re-gate an already-built world.

One `match`, two arms, no bool and no fallback branch.

**3. `advance_to_next_world` must not fire on a visit.** It runs `OnExit(RunState::Active)`
(`session/mod.rs:278`, `:364`) and advances the seed — correct for ending a run, catastrophic for a
visit (you would return to a *different world*). Since visits no longer leave `Active`, this is free
— but it is the single most dangerous thing to get wrong, so it wants a test that names it.

**4. `persist` snapshots on the wrong event.** It saves `OnEnter(AppState::Site)`
(`persist.rs:560`), which would now fire mid-run and write a *live* expedition into the campaign
save. That trigger must move to run-end.

**5. The camera.** It survives by design (`camera.rs:129`); it needs to travel to the Site and back
to the squad. `site::visuals::focus_camera_on_site` already does half of this.

**6. Time is already correct — do not touch it.** `should_freeze` returns `false` for
`AppState::Site` (`ui/state.rs:137`), so `Time<Virtual>` keeps advancing there today, and pause is
not even reachable from the Site (`MenuState` is an `InGame` substate). This item is closed before it
starts. Its *justification comment* is what needs fixing — see §6.

**7. The squad, unattended.** Decided — see §4.

## 4. What the squad does while nobody is watching — decided

**Director's call: they continue the standing order, and act autonomously within it, driven by the
policy archive.**

The first draft framed this as three competing implementations (Hold / Continue / Autonomous). That
framing was wrong. `unit_movement` (`squad.rs:1009`) already arbitrates exactly this way:

> "The preferred velocity comes from *either* an authoritative player `MoveOrder` (flow-field steer,
> the original path — unchanged) *or*, for an order-less unit, the squad AI's `DesiredMove` goal."

A `MoveOrder` is a component on the unit, removed only on **arrival** (`squad.rs:1196`) — nothing
about an `AppState` change touches it. So across a visit:

- the standing order **persists and keeps being executed** — Continue is free;
- the moment it is consumed, the unit falls through to `ActivePolicy` — Autonomous is free.

Neither is new machinery. The three-way choice was really a question about what to *suppress*, and
"Hold" is the only one that would have required writing code.

That leaves the one part that is real work: **"driven by the policy archive."** `ActivePolicy`
defaults to `UtilityPolicy` — the hand-authored dual-utility role brain (`squad_ai/policy.rs:25-28`).
The neuroevolved `NeuralPolicy` is reachable **only** through the `FVS_POLICY_ELITE` env overlay
(`lib.rs:185-198`, `elite_overlay.rs:46`/`:304`). Making the baked archive the shipped controller is
the decision, and it carries two consequences worth stating up front:

- **It re-pins the replay goldens.** `ActivePolicy` is read inside the pinned core
  (`squad_ai/perception.rs:114`), so swapping the default changes squad behaviour in every golden.
  Expected and routine, but it is a re-bake, not a free swap.
- **The archive is invalidated by width.** Adding a `Mode` changes `NeuralPolicy::WEIGHT_COUNT` and
  invalidates every baked archive (`broadcast.rs:21`). Shipping the archive means that constraint
  becomes a shipping constraint, not just a training one.

### Why this shape, and not a monolithic autonomous squad

This is the Killzone 3 arrangement, and worth naming because the precedent is load-bearing rather
than decorative. Straatman et al., *"Hierarchical AI for Multiplayer Bots in Killzone 3"* (Game AI
Pro, ch. 29; local: `papers/ga/gameaipro1-ch29-hierarchical-ai-multiplayer-bots-killzone-3.pdf`)
describe a three-layer hierarchy in which orders flow down and information flows up:

> "The bot AI **follows squad orders, but has freedom in how to execute those orders.**"

That is precisely the split above: the player's `MoveOrder` is the order layer, and the policy is the
execution layer. It is why the design stays legible — the player can predict the *cost* of leaving,
because the goal they set is still the goal — while the moment-to-moment behaviour is something they
did not choose. Killzone also constrains individual freedom to a corridor produced by the squad
planner (ch. 29 §29.5); our equivalent constraint is that the policy only ever selects from the
**role's** `behaviors` slice, so autonomy is bounded by role rather than by geometry.

The archive that supplies that execution layer is a MAP-Elites repertoire, not a single tuned
controller — the diversity is the point, and it is what makes an unattended squad read as *a squad
with habits* rather than one script (Gravina, Khalifa, Liapis, Togelius & Yannakakis,
*"Procedural Content Generation through Quality Diversity"*, IEEE CIG 2019,
doi:10.1109/CIG.2019.8848053 — the QD-as-repertoire framing this engine already implements). Its
weights come from gradient-free policy search (Salimans et al. 2017, *"Evolution Strategies as a
Scalable Alternative to Reinforcement Learning"*, arXiv:1703.03864), already cited at
`squad_ai/policy.rs`.

This finally gives the policy archive a job in the shipped game. Today it is a trained squad
controller with nothing driving it in play.

## 5. What this breaks, and what it makes possible

**Breaks:** every test and screen that assumes `AppState::Site` implies no live expedition. `persist`
(§3.4) is one. But the draft named the wrong biggest, and the real one is a whole *class*:

> **Anything gated on `RunState::Active` alone now runs while the player is standing in the hub.**

`RunState::Active` used to be a serviceable proxy for "the player is watching the expedition." A visit
severs those two meanings, and everything that leaned on the conflation breaks silently — no error, no
log line. The instances found while building it:

| Site | What it did during a visit |
|---|---|
| `selection.rs:318` — all order-issuing input | Right-click marched the squad toward a Site-space ray's `y = 0` hit, 512+ units outside the map. One left-click both walked the Site avatar and re-selected the squad. Every armed verb still threw — you could deploy a capture device from inside Site-67. |
| `selection.rs` — `draw_selection_rings` / `update_cursor` | Selection rings on an off-screen squad, and an armed-tool cursor for a verb that cannot fire. |
| `camera.rs` — `Action::CameraRecenter` | Glided the camera back to the dungeon with the Site HUD up — a way to supervise the squad you are supposed to have left unattended. It no-opped before the change only because `SquadAnchor` was invalid with no run live. |

**The fix has to be a resource, not a run condition**, and this is the constraint that shapes it:
`ui/state.rs` and `ui/mod.rs` both forbid gating gameplay on `in_state(AppState::InGame)`, because the
harness never registers `AppState` and the world must keep ticking under the boot and title screens.
So it follows `SimBlocked`'s shape exactly — `time_control::OrdersBlocked`, one writer
(`ui::state::sync_order_block`), inert `false` headless, asserted so by
`replay::ui_never_leaks_into_deterministic_core`.

It has to be *separate* from `SimBlocked`, too. Those two meant the same thing until now; at the Site
they diverge and must: the sim keeps running (that exposure is the feature) while the mouse stops
reaching the squad. Collapsing them back into one either freezes the unattended squad or lets a
right-click command it from another building.

The rest are §6's comments and one pinned unit test.

**Opens, unexpectedly:** `site/mod.rs`'s "The constraint that decides squad presence" argues real
squad `Unit`s cannot stand at the Site because `unit_movement` and `fog::update_los` take
`Res<Dungeon>`, which while `Idle` is absent or stale. **During a visit `Dungeon` is live.** That
constraint relaxes for exactly the duration of a visit. Not something to build on yet, but it is a
door opening rather than a thing breaking, and it should be recorded before someone re-derives the
old reasoning.

**Makes possible**, and this is why it is worth it:

- The Site's requisition and research stop being between-run bookkeeping and become **decisions under
  fire** — you buy a consumable knowing the squad is exposed while you shop.
- The O5 allowance gains real texture: time spent at the Site is time the expedition is unsupervised.
- The watch feed becomes genuinely nasty. It generates *while watched* — and leaving the squad
  standing in front of one while you visit the Site is a mistake the player makes exactly once.

## 6. Comments that become false

Four justification comments reason from "at the Site, `RunState` is `Idle`". Each is correct today
and misleading the moment a visit exists; they will be re-derived wrongly if left:

| Site | Says |
|---|---|
| `ui/state.rs:137` | "There is no expedition running to freeze anyway: `RunState` is `Idle` here" |
| `ui/state.rs` (test `arming_region_capture_freezes_the_sim`) | asserts the Site-never-freezes rule on that same rationale |
| `ui/research_hud.rs:53` | "SITE … is only ever entered by `RETURN TO SITE` — and that sets `RunState::Idle`" |
| `site/nav.rs:7` | "while `RunState::Idle` that resource is **absent** … or **stale**" |
| `site/mod.rs` | the squad-presence constraint (§5) |

The *behaviour* at each site stays right. Only the reasoning dies.

## 7. What the first draft got wrong

Recorded so the correction is not silently absorbed:

1. **"Two UI handlers send the player to the Site."** Only one does. `ui/pause.rs:141` is `QUIT TO
   TITLE` → `AppState::Title`.
2. **"`RETURN TO SITE` becomes a visit."** It cannot — it is post-resolution. The visit is a new
   `InGame → Site` transition that does not exist, which makes §8.1 a build, not a split.
3. **"The ASYNC door already exists as fiction and geometry."** True, and it is *guarded shut*
   against precisely this case (`site/visuals.rs:301`). Not free; a required change.

It also under-sold §1 (both worlds already coexist at a ≥512 offset) and over-sold §3's time item
(already satisfied). The pattern is that the draft reasoned from module docs rather than from the
system registrations — the docs describe the design, the registrations describe HEAD.

## 8. Order, and what landed

1. ✅ **The visit affordance and the door's return arm.** `input::Action::VisitSite` (default `O`,
   rebindable, `Context::InGame`) sets `AppState::Site` and touches nothing else
   (`site::visuals::leave_for_the_site`). `enter_the_door` became a two-arm `match`: `Idle` starts an
   expedition via `Warmup`, `Active` returns to `InGame` directly. `ABANDON EXPEDITION` is a separate
   pause-menu button; `QUIT TO TITLE` is unchanged. Pinned by
   `session::a_visit_preserves_the_run_seed_and_an_abandon_advances_it`.
2. ✅ **`persist` moved to run-end.** `OnExit(RunState::Active)`, ordered
   `.after(session::RunEnd::AdvanceSeed)` — a new named set, because `SaveGame::run_seed` must be the
   universe the player resumes *into*. That ordering previously held by accident, via the extra state
   transition. Side benefit: quit-to-title now saves, which it never did.
3. ✅ **Camera travel.** `return_camera_to_squad` on `OnEnter(AppState::InGame)`, snapping (not
   gliding) to `SquadAnchor` — the Site is 512+ units away, so a glide would be a crawl.
4. ✅ **The §6 comments**, in the same change as the behaviour that invalidated them. Plus one the
   draft missed: `lib.rs`'s note that save/load is harness-safe *because* it keys off
   `AppState::Site`. That reason is gone; the registration is now the only guard, and the comment
   says so.
5. ✅ **Squad behaviour: nothing to build**, as §4 predicted — but the invariant it rests on is now
   enforced. `tests/squad_runs_unattended.rs` is a source lint: no file under `src/squad.rs` or
   `src/squad_ai/` may mention `AppState`. A gate there would freeze the unattended squad, an
   `OnExit(AppState::InGame)` cleanup would drop the standing order, and **neither is visible to the
   replay suite** — `ui_never_leaks_into_deterministic_core` asserts `AppState` is *absent* headless,
   so a stray condition is never satisfied and a stray `OnExit` never fires. Green suite, broken game.
6. ✅ **The `RunState::Active`-alone class**, found by asking what actually switches when the player
   changes area (§5's table): order input, selection cosmetics and camera-recenter all gated on the
   new `time_control::OrdersBlocked`.
7. ⬜ **Not yet verified in a running game.** Everything above is type-checked and covered by the
   GPU-free suite; nobody has pressed `O` and looked. The area switch is exactly the kind of
   cross-layer behaviour unit tests cannot see.
8. ⬜ Separately, and on its own schedule: promote the policy archive from `FVS_POLICY_ELITE` overlay
   to the shipped `ActivePolicy` default, and re-pin the goldens.

### Found on the way in, and fixed

`Action::ArmLure` was declared on the enum but **missing from `Action::ALL`**, and `Action::index()`
ended in `unwrap_or(0)` — so the lure silently took `CameraPanForward`'s slot. It was really bound to
`W`, absent from the controls screen, and impossible to rebind or persist. That also hid a second
bug: its default `V` collided with `DeploySensor`'s, invisible to `the_key_space_has_no_collisions`
because that test iterates `ALL` too. `index()` is now an exhaustive `match`, so a new variant is a
compile error rather than a wrong answer, and the density test's assertion stopped being tautological.
The lure keeps `V`; `DeploySensor` moved to `Y` — not the equally-free `T`, which three fixtures
rebind onto.
