# FVS-B-10 — noise discipline: researched options

**Prepared 2026-07-30. This is a menu, not a decision.** B-10 is an L-sized design item and the choice
of verb is a taste call; what follows is the literature, what the repo already has, and four options
costed against both. Pick one (or a subset) and I'll build it.

---

## 1. What already exists — more than the item implies

| Piece | Where | State |
|---|---|---|
| `NOISE_SQUAD` channel | fed by fire, bolt impacts, unit death (`squad.rs:932`) | propagating |
| `NOISE_SWARM` channel | fed by crab death squelch, SCP-610 drone | propagating |
| Propagation + repulsion gains | `audio_tuning.rs:29,31,63,65` | **evolvable** (`audio_genome`, N=16) |
| Perception of din | `unit_fear_of_din`, `crab_fear_of_din`, `investigate_threshold` | wired |
| Aim point at loudest din | `ai/brain.rs:130` | wired |
| One player-facing consumer | SCP-610's containment rule caps `NOISE_SQUAD` at 0.20 | shipped (K-1) |
| A latched **stance** verb precedent | `Verb::HoldFire` — explicitly *not* an `ArmedTool` (`ui/verb_bar.rs:75-81`) | shipped |

The verb bar currently holds six: `Device, Quarantine, Cap, HoldFire, Sensor, Push`.

**One architectural thing is already right, and the literature says so.** Mafia III's AI hearing engine
is "completely independent of the audio engine" — sounds that matter to AI are registered with a
separate hearing system, not inferred from playback (*Game AI Pro 4*, ch. 16). This repo does exactly
that: `sim_harness.rs:282` records that the acoustic model the audio search evolves is the
`NOISE_SQUAD`/`NOISE_SWARM` stigmergy channels, **not** `src/audio.rs` playback. That separation is why
the sim stays deterministic while audio is windowed-only. **Do not "unify" them.**

---

## 2. What the literature actually recommends

**Perception should be graduated, and its speed a product of factors.** Mafia III does not aggro on
detection; a stimulus starts a *recognition* process whose time is `t = 1 / ∏ fᵢ` over distance, target
speed, lighting, and the angle between the NPC's facing and the event. The design consequence: a player
gets a window in which to *undo* a mistake, which is what makes stealth feel fair rather than punitive.

**Stimulus strength should be tiered per event type, and shaped over time.** Crytek's Target Tracks
gives every stimulus an ADSR envelope, with peaks balanced across types — footsteps peak at 25, weapon
sounds at 50, direct line-of-sight at 100 (*Game AI Pro 1*, ch. 31). Decay exists specifically so a
*fresh* stimulus can outrank one that has been sustained a while.

> **The repo is already 80% of the way to this and nobody framed it that way.** A stigmergy channel
> with a `deposit` rate and an `evaporate` rate **is** an ADSR envelope — attack and release — except
> it is *spatial* as well as temporal, which is strictly more than Crytek had. What is missing is the
> deliberate **tiering of peaks by event type**: right now a footstep, a shot and a death all deposit
> into `NOISE_SQUAD` at rates that were set independently rather than balanced against each other.
> That is a cheap, high-value fix regardless of which option below is chosen.

**Sound is how a player learns about events off-screen.** Grimshaw & Schott's acoustic-ecology account
of FPS play (DOI 10.26503/dl.v2007i1.313) describes the *telediegetic* case: enemies tracking a
teammate by footsteps and gunfire, with the player meeting the consequences later. In an isometric
squad game with fog-of-war, the din field is the natural carrier of exactly that.

**In horror specifically, agency is the parameter being tuned.** Boonen & Mieritz, *Paralysing Fear:
Player Agency Parameters in Horror Games* (DOI 10.26503/dl.v2018i3.1051). Noise discipline is a lever
that *reduces* moment-to-moment agency (you move slower) to *buy* strategic agency (you choose when to
be seen) — which is the trade worth being deliberate about, and the reason a HUD readout matters: an
invisible constraint reads as the game cheating.

---

## 3. The options

### A — `MOVE QUIET`: a second latched stance · **S/M · recommended starting point**

A seventh verb, shaped exactly like `HOLD FIRE`: latched, no aiming, no economy. While active, units
move at a reduced speed multiplier and deposit proportionally less `NOISE_SQUAD`.

*For:* smallest diff with a proven shape — `verb_bar.rs` already documents `HoldFire` as the precedent
for a non-`ArmedTool` chip, and `sensor.rs:34` records that precedent being reused once already. It
makes the existing channel a player choice without inventing a subsystem. Pairs naturally with the
SCP-610 rule that already caps `NOISE_SQUAD` at 0.20 — the cap stops being a thing that happens *to*
you and becomes a thing you *manage*.
*Against:* one more always-visible chip on a six-verb bar. Speed/quiet is the most obvious possible
trade, so it is safe rather than interesting.
*Evolvable:* the speed multiplier and the deposit scale are two new `audio_genome` knobs — and unlike
I-7/I-8, they plausibly move `swarm_descriptor`'s aggression axis, because quiet squads get engaged
less. (Worth checking against `docs/descriptor_audit.md` §4 before landing.)

### B — A din channel in the HUD · **S · complements any other option**

The containment HUD already names channels, so the vocabulary exists. Show `NOISE_SQUAD` at the squad's
position as a meter, with the SCP-610 threshold marked.

*For:* the cheapest thing on this list and the one the literature most directly supports — an invisible
constraint reads as unfairness (Boonen & Mieritz), and B-10's own complaint is *legibility*, not
mechanics. Arguably a prerequisite for A, C and D rather than an alternative to them.
*Against:* it is a readout, not a verb, so on its own it does not answer "no player-facing verb reads
them".

### C — Graduated investigation (the Mafia III model) · **M/L**

Crabs currently cross `investigate_threshold` and commit. Instead, accumulate a per-crab recognition
value whose rate is a product of factors (din magnitude, distance, whether the squad is moving), so a
squad that goes quiet *before* the meter fills is never investigated.

*For:* the deepest change to how noise *feels* — it introduces the recoverable-mistake window that
makes stealth fair, and it is the mechanism the literature is most specific about.
*Against:* new per-agent state in `FixedUpdate`, so it touches the pinned core and **will move
goldens** — a deliberate measure-and-re-pin, not a drive-by. Needs a stable total order over crabs.

### D — Tier the deposit peaks by event type · **S · do this regardless**

Balance `NOISE_SQUAD` deposits so footstep < bolt impact < shot < unit death form a deliberate ladder,
the way Crytek balances 25/50/100 across stimulus types, instead of three rates set independently.

*For:* nearly free, makes every other option legible, and it is the one item here with a named
best practice behind it. Also makes `HOLD FIRE` meaningfully quieter than merely not-shooting.
*Against:* it moves the acoustic balance, so the `audio` archive's one landed bake goes stale — though
that bake is already stale for three other reasons (see H-1) and occupies 3/64 cells.

---

## 4. My recommendation, for what it's worth

**D + B first** (both S, neither touches the pinned core), then **A** if you want the verb. Hold **C**
until N-13 and the golden re-pin question are settled, because it is the only one that moves goldens
and it would be landing on top of a known live leak.

That ordering also front-loads the thing B-10 actually complains about — the acoustic layer being
"machinery without a game attached" is fixed the moment the din is *visible* and *tiered*, before any
new verb exists.

**Not decided here, and not mine to decide:** whether the squad should have a seventh verb at all, and
whether quiet should cost speed or something else (accuracy, fatigue, a per-run resource).
