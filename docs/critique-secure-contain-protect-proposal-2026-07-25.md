# Critique: "Secure / Contain / Protect" game-loop proposal (2026-07-25)

Targeted concerns on the capture→research→unlock loop proposal, checked against (a) the actual
codebase, (b) the four real `docs/lore` docs, and (c) the `home-still` research corpus the proposal
claims as its grounding. The core thesis — replace "win by wiping the level" with "win by extracting a
contained anomaly," and use that to reunify the shipped systemic game with the inert lore docs — is
sound and I'd keep it. The concerns below are about specific claims and sequencing inside the proposal,
not the thesis.

---

## 1. A cited source doesn't exist

§10 References lists `2026-07-12-scp-monster-roster.md` as a companion doc and attributes to it
"per-anomaly behavior; SCP-2521 and the slop connection (§8)." **There is no such file.**
`docs/lore/` contains exactly five docs (universe, role-taxonomy, equipment-taxonomy, color-language,
almond-water) and none of them mention SCP-2521. This citation should be removed or, if it's
describing a doc you intend to write, marked clearly as "not yet written" rather than cited as if it
already grounds the design.

**This also produces a canon contradiction worth flagging directly:** the game's actual, already-shipped
origin story for "slop" is in `src/lib.rs`'s own module doc — *"deliberately ugly, uncanny-valley
monsters churned out by SCP-9191, a rogue monster-generating AI."* The proposal's open question 11.2
("How does the slop antagonist enter the loop?") doesn't reference SCP-9191 at all, and the phantom
citation implies a different number (SCP-2521) is the "slop connection." Before any research/unlock
content is authored around "the slop," reconcile this with the code's existing commitment — don't let
two different in-universe explanations for the same antagonist exist between a design doc and `lib.rs`.

---

## 2. The Tier-1 example table violates the project's own documented anti-pattern

The proposal's core "containment vector" table leads with **SCP-173**, and also uses **SCP-096**. Both
are treated as if they're already part of this game's roster. Neither is — there's no `scp173`/`scp096`
code, asset, or lore mention anywhere in the repo.

This isn't just a scope gap, it's a direct contradiction of guidance already sitting in this project's
own corpus:
- `docs/lore/2026-07-12-scp-role-taxonomy.md` §14, "Amateur tells — do not do these": **"Leading with
  SCP-173."**
- `docs/lore/2026-07-12-scp-universe.md`: *"Containment Breach nailed atmosphere... but is a narrow
  slice (one site, classic SCPs 173/106/096/079)... Fans complain when games over-rely on the same five
  'greatest hits' SCPs."* And separately: *"SCP-173 as the mascot/first thing shown"* is listed as a
  common amateur tell.

The proposal does exactly the thing its own source material warns against, and leads its pitch with it.
Recommend re-deriving the Tier-1 table from the roster that actually exists and already has a
containment-shaped identity: SCP-999 (befriend), SCP-1048 (out-watch the original), SCP-150 (parasite
cure/extract), crabs (nest), SCP-610 (quarantine) — all five already appear correctly in the per-anomaly
table's lower rows and in §9's build order. Cut 173/096 or explicitly label them "future greatest-hits
anomalies, added after the bespoke roster is proven" — don't open the pitch with them.

---

## 3. SCP-173's containment vector is claimed as "already modeled" — it isn't

The table says 173's containment ("keep it observed while an anchor suppresses mobility... look away =
it moves") reuses the existing `ATTENTION` gaze-pheromone, "native co-op tension" for free. Checked
against `src/ai/field.rs` per the codebase review: `ATTENTION` is a **decaying, diffusing scalar field
over grid cells**, deposited from the fog-of-war gaze footprint — it answers "how much has this area
been looked at recently," not "is this specific tracked entity under continuous, zero-frame-gap
observation right now," which is what 173's canonical mechanic actually requires. Those are different
primitives: one is ambient/spatial and tolerant of decay, the other is a hard per-entity boolean with no
tolerance for a single dropped frame. Reusing `ATTENTION` gets you a *softer, laggier* version of the
mechanic, not the real one — that's a legitimate design choice, but the proposal presents it as a free
reuse when it's actually a redesign of what the channel means for at least one consumer. Same caveat
applies to 096's "face must not be seen" — it needs a directional/facing check against a specific
entity, not a density read.

If 173/096 stay in scope at all, call out explicitly that they need a new **per-entity continuous-watch
state** distinct from the ambient field, not just a sign-flip on an existing read — that's new
engineering, not new data.

---

## 4. "One clean verb" is oversold — the per-anomaly table already has three different verbs

The thesis is "the game's verb is Contain," and §2 pitches the Portable Spatial Containment Device
("throw a sphere, take it alive") as the unifying mechanic. But the anomaly table itself assigns:
- **Sphere-throw** capture (999, 1048, arguably 150's hosts)
- **Area quarantine boundary** (610 — a field-gradient wall, not a thrown object)
- **Nest destruction** (crabs — sealing/capping a structure, not capturing a creature at all)

Nest-sealing in particular isn't a capture verb — it's the existing kill-for-no-yield path applied to a
structure instead of a body. That's fine as a design choice, but it undercuts the "one verb, thrown
sphere" framing in §2's pitch. Recommend either (a) naming these as three distinct containment
*archetypes* up front (single-target, area-denial, source-elimination) rather than implying one item
does it all, or (b) accepting the throw-sphere item literally doesn't apply to swarms/outbreaks and
saying so.

---

## 5. Build-order step 1 isn't independently shippable, contrary to how it's framed

§9 states step 1 ("wire the zero-unit wipe to `GameOver`, stub 'target contained' to `Victory`") "alone
makes it a game." But a stubbed Victory condition for "target contained" is meaningless until *something
in the game can actually contain a target* — which is step 2. As written, step 1 can be merged and
compiled alone, but it can't be *played* as a resolving loop until step 2 lands; the two are one unit of
work, not sequential independent ships. Either fold them into a single first milestone, or make step 1
literally standalone by picking a placeholder win condition that already exists in the sim today (e.g.
"survive N minutes" using the existing time/wave systems) — something to test the state-machine plumbing
before the capture mechanic exists at all.

---

## 6. The "live curriculum director" step understates what's actually missing

§9 step 6 and §4's framing both describe wiring POET/QD live as though it's mostly a matter of exposing
the existing offline pipeline. Per the codebase review, three things make this considerably larger than
implied:
- The current elite-overlay path (`elite_overlay.rs`) is a **static, boot-time, env-var-driven config
  swap** — there is no live, in-session archive-selection code anywhere in the shipped game. "Feed the
  player's capability estimate to the archive selector" is a new runtime system, not a wiring task.
- The RL policy archive is **currently stale and rejected** (Mode alphabet grew 25→29 with SCP-1048) and
  needs a multi-hour retrain before it's even valid offline, let alone live.
- CMA-MAE — the higher-quality MAP-Elites emitter — is implemented and unit-tested but **unreachable
  from any `train` subcommand**, i.e. dead code today. A "live director" built on the weaker, currently-
  reachable emitter should say so rather than implicitly assuming the better one is available.

None of this invalidates the idea — it's a genuinely good differentiator — but it belongs later in the
build order than step 6 implies, and should be scoped as "build a new live selection system," not
"connect the existing one."

## 7. The fitness function doesn't automatically transfer to the new goal

§3 asserts "the offline fitness function and the player's felt fun are the same quantity — wire them
together," pointing at `surprise::fitness = W·S·L`. But the QD pipeline's actual fitness today scores
survival/"squad not wiped" and behavioral surprise — it has no concept of *containment quality* or
*specimen yield*, which is what the new research economy needs to reward. Making captures (not kills)
the valuable outcome means the offline fitness function itself needs new terms, not just a relabeling of
what it already measures — otherwise the QD search keeps optimizing for combat encounters that are
*surprising to watch get killed*, which can directly conflict with tuning captures to be
learnable/satisfying. Worth naming as its own design/engineering task rather than assuming §3's
existing quantity carries over for free.

---

## 8. Citation hygiene: verified against `home-still`, mostly solid, two issues

I checked all four cited papers against the actual `home-still` corpus (not just trusting the reference
list):

- **Vansteenkiste & Ryan (2013)**, DOI `10.1037/a0032359` — in the corpus, fully indexed, and the
  proposal's characterization of need-satisfaction/need-frustration is accurate.
- **Oudeyer & Kaplan (2007)**, DOI `10.3389/neuro.12.006.2007` — in the corpus. The specific claim
  ("must group comparable situations into regions... don't reward moving from the unpredictable-leaf to
  the blank-wall") is a faithful paraphrase of the paper's actual leaf-in-wind/white-wall example. Good.
- **Rietveld, Miller & Kiverstein (2017)**, DOI `10.1007/s11229-017-1583-9` — in the corpus, and the
  rate-of-error-reduction hedonic-value claim is accurately represented. I could not independently
  locate the *exact* quoted sentence ("even large instantaneous errors could be experienced as positive
  as long as...") in the chunks retrieved — the surrounding argument is right, but if that's meant to be
  a direct quote, double check it against the PDF before it goes in a doc as quoted text rather than
  paraphrase.
- **Ryan & Deci (2000)**, DOI `10.1207/s15327965pli1104_01` — **not found anywhere in the local
  `home-still` catalog.** It's referenced *inside* the Vansteenkiste & Ryan 2013 paper (which is
  downloaded), but the 2000 paper itself was never separately downloaded/converted/indexed here. The
  citation is almost certainly correct on its merits (it's one of the most-cited papers in the field),
  but the proposal's blanket framing — *"Grounding: the home-still research corpus"* — overstates how
  much of this is actually sourced from the corpus versus general knowledge. Recommend either running
  `paper_download` on that DOI so it's genuinely in the corpus, or being explicit in the doc about which
  citations are corpus-verified vs. cited from general training knowledge.

**Section-number drift** (minor, but worth a pass before this becomes a reference doc): "Portable
Spatial Containment Device (equipment doc §10.4)" — the device is actually in the equipment doc's **§10
Armaments**; §10.4 is a section number from the *role-taxonomy* doc (Xenobiologist), not the equipment
doc — looks like the two documents' section numbers got crossed. Similarly "color doc's Type-Magenta kit
(§6)" — Type Magenta is described across color-doc §2–§3, not §6 (§6 is "Recommended color system for
foundation_vs_slop," a different section). Worth a citation pass before this ships as a reference doc
other designers will follow section numbers into.

---

## Bottom line

The loop concept is good and directly answers the state review's biggest gap. Before it becomes a
decision record: (1) pull the phantom citation and reconcile the slop-origin question with the
already-shipped SCP-9191 explanation in `lib.rs`; (2) rebuild the flagship example table from the
roster that actually exists instead of leading with the two anomalies the project's own lore doc says
not to lead with; (3) be explicit that SCP-173/096-style containment needs new per-entity engineering,
not a reuse of the ambient `ATTENTION` field; (4) treat the live-QD-director as its own large build, not
a wiring step; and (5) fix the two crossed section-number citations. None of these kill the proposal —
they're the gap between "a strong pitch" and "a decision record an engineer can start building from."
