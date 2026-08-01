# The acoustic program — making sound worth evolving

*2026-08-01. Design for FVS-B-10 and its successors, and the ruling on FVS-N-28's descriptor question.*

---

## 0. The finding this starts from

The audio archive collapsed to **3 of 64 cells**. Twice — the 2026-07-28 descriptor sweep measured
exactly `3/64`, and the 2026-08-01 bake (2 h 54 m, 24 islands, every island producing an elite)
measured `3/64` again, across an intervening genome-length change (`audio_genome::N` 15 → 16). Two
independent bakes landing on the same three cells is structural.

**The mechanical cause, read off the code rather than argued:** `swarm_descriptor` bins on

- `aggression` = share of decisions in `{Latch, Rally, Muster, Chase}`
- `persistence` = 1 − share of `Flee`

The behaviour the acoustic system exists to produce is **`Mode::Investigate`** — a creature leaving
its business to walk toward a noise. It is in neither set. A creature dragged across the map by
gunfire and a creature standing still are, to this descriptor, indistinguishable.

**But changing the descriptor is not the fix, and that is the point of this document.** The reason
there is nothing good to measure is that there is nothing for sound to be *good at*. The channels
propagate and are perceived; no player verb reads them. Optimising a subsystem the player cannot
perceive or act on is the same error as a path the player cannot tell apart — one layer down.

So: **stop evolving audio until it has a game attached.** Build that game in four stages, then let
the descriptor fall out of it.

---

## 1. What the corpus says to build toward

Four in-corpus sources, and each one points at a different stage below.

**Grimshaw & Schott 2007, *Situating Gaming as a Sonic Experience: The acoustic ecology of
First-Person Shooters*** (`10.26503/dl.v2007i1.313`). Two ideas do the work here. First, sounds are
classified by *function* — **attractors** invite the player to act, **connectors** aid orientation,
**retainers** encourage lingering — and they add a fourth listening mode of their own,
**navigational listening**: following a sound to its source. Second, and more useful for a game with
stigmergy channels: one actor's sounds "morph other players' soundscapes and so provide **new
affordances**". Sound is a thing you *use*, not only a thing you leak. They also name **volatile vs
predictable** soundscapes as a readable signal of activity.

**The tension study** (`10.1016/j.intcom.2010.04.005`). In an FPS "hunter and hunted" scenario,
tension was *lowest* with music and sound both on, and *higher* with music but no sound — players
"lacked auditory feedback and were distracted", suffering "the perceptual mismatch between the world
that the eyes see and the world that the ears hear". **Informative sound cues lower tension; their
absence raises it.** So the acoustic layer's job is information, and music must never mask it.

**Kennedy et al. 2015, *Removing the HUD*** (`10.1145/2793107.2793120`). Non-diegetic elements reduce
player involvement. Every anomaly this game adds grows the containment HUD; sound is the way to stop
that curve.

**Kaushik 2025, *Procedural Music Generation in Video Games***
(`10.36948/ijfmr.2025.v07i02.39384`). Names genetic algorithms among its four generation techniques,
and frames the central craft problem as **"balancing predictability and novelty"**. It also argues for
**feedback loops** where game state drives music *and* music drives player action.

---

## 2. Stage 1 — noise as a two-way resource

**The smallest change that turns an existing subsystem into a game.**

Today squad noise is a pure cost: you emit it, creatures come. Grimshaw & Schott's affordance point
inverts that — noise you *choose* to make somewhere else is a tool. A thrown noisemaker, or a
deliberate shot into a far wall, deposits into `NOISE_SWARM` and pulls the swarm off the extraction
route.

**Why this one first:** the machinery is already built and unused. `NOISE_SWARM` propagates,
`crab_draw_to_din` scales the pull, `investigate_threshold` gates it, and `Mode::Investigate` already
exists in the brain. Nothing new is needed in the AI at all — only a way for the player to write into
the channel deliberately.

It also makes the *quiet* half meaningful. Noise discipline is only a decision if noise is sometimes
worth spending; a pure cost is not a trade-off, it is a tax.

**Evolvable knobs, each with a causal path to a descriptor axis:** lure strength (how much it
deposits), dwell (how long before it evaporates below threshold), and **habituation** — how quickly a
swarm stops answering a repeated trick. Habituation is the interesting one: it is what stops the verb
being a solved button.

**Constraints.** `FixedUpdate`, pinned. It must **not** add a `Mode` — `MODE_COUNT` sets
`NeuralPolicy::WEIGHT_COUNT`, and the policy archive baked 2026-08-01 would be invalidated *by width*.
`Investigate` already exists and is the honest model. Deposits need a stable total key over the
emitting entity, per the determinism rules.

## 3. Stage 2 — audible slop

**The one that is uniquely this game.**

SCP-9191 is a generator of bad copies. Kaushik's framing gives the mechanism for free: **slop is what
happens when novelty is turned down too far.** So the things the generator makes can be scored
procedurally with *deliberately degraded* novelty — loops that repeat a beat too soon, near-identical
variations, a motif that never develops.

The player learns to hear *"this was made by the generator"* before seeing it. That is simultaneously
a mechanic (identify slop by ear), a thematic statement (the antagonist's defect is audible, not
narrated), and a real evolvable space (how degraded; how detectable).

**First host: the watch feed** (`src/broadcast.rs`, FVS-C-7). A screen that churns out crabs for as
long as anyone looks at it should *sound* like it is churning. Its emissions are the natural place for
a deliberately-too-repetitive motif, and it already has a containment rule the sound can telegraph.

## 4. Stage 3 — ambience as the containment readout

The containment HUD currently names the channel that is failing. Per *Removing the HUD*, that is
involvement spent on a panel; per the tension study, the sound layer is where information belongs.

So: the room's ambience tightens as a hold slips. You learn to **hear** whether containment is
working, and the HUD can shrink rather than growing with every anomaly added.

**This is a real difficulty dial, not decoration** — cue legibility and lead time. A legible
soundscape is an easier level, which is exactly the kind of variation a quality-diversity archive
should hold. It is Grimshaw & Schott's *connector* function, made mechanical.

Sequenced third because it is as much a UI-replacement project as an audio one, and it wants stage 1's
verb to exist so that "the sound told me" has something to lead to.

## 5. Stage 4 — music driven by the simulation

Adaptive layering driven by actual squad `FEAR`/`MORALE` and containment progress rather than scripted
combat states — Kaushik's feedback loop, with the sim as the input parameter.

**One hard constraint from the tension study:** music must **duck under** the informative layer. The
worst measured condition was music-on/sound-off; a score that masks the cues stages 1–3 establish
would actively undo them.

Least new gameplay of the four, so it goes last. Nothing here is a verb.

---

## 6. The descriptor, once any of this exists

Both axes are **behavioural outcomes, not genome readouts** — which is the property FVS-N-21 and the
descriptor-degeneracy watchlist exist to protect. Binning on "how loud is it" would fill the archive
and mean nothing, because it would be reading the genome back to itself.

- **Axis 1 — drawn by sound.** Share of creature decisions that are `Mode::Investigate`. **Free
  today**: `trace.decisions` already carries every creature's mode, so this is a one-line descriptor
  change with no new instrumentation. Jointly moved by loudness, propagation, `crab_draw_to_din` and
  `investigate_threshold` — i.e. by the actual genome, but by no single gene.
- **Axis 2 — heard before seen.** Fraction of first squad↔creature contacts where the creature
  arrived via `Investigate` rather than `Chase`. Needs one new trace field. This is the tension
  study's finding operationalised: how much threat information reaches the player by ear before eye.

After stage 1, axis 2 sharpens further into *"did the squad's noise discipline change who found
them"*, which is the thing the player is actually deciding.

---

## 7. Ruling

1. **Do not re-bake audio** until the descriptor changes. Re-running produces 3 cells again; that is
   measured, not predicted.
2. **Do not change the descriptor yet either.** Land stage 1 first, so the axes are measuring a game
   rather than machinery.
3. `assets/config/elites_audio.candidate.ron` from the 2026-08-01 bake **should not ship**. A 3-cell
   archive gives the director three acoustic worlds. The bake was not wasted — it is the measurement
   that justifies this document.
