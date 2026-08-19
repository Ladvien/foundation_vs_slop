# The editor's stated design, checked against the corpus

**Prepared 2026-08-17.** The brief was seven claims about what `emerge-mapper` should be: shortcut keys as
first-class citizens, extreme ease and intuitiveness, **colour guiding more than explicit messages**, reduced
noise, an intuitive flow over composable data entities, a theme of *kit bashing electronically*, and support
for WFC composing pieces at several levels. This document searches home-still on each axis and reports what
the literature says, what this repo already settled, and what is genuinely new.

**This is a menu, not a decision.** Every design implication below is written as an option with its cost.

Corpus at time of search: 9,255 embedded documents, 284,528 chunks, `bge-m3` on CUDA, all healthy.

---

## 0. What is already settled — do not re-litigate these

Four of the seven axes have prior work in this repo. Read these before proposing anything:

| Axis | Where it was settled | The short version |
|---|---|---|
| Shortcut keys / novice→expert | `docs/2026-08-15-usability-handoff.md`, `docs/research/2026-08-15-chooser-plan-vetting.md` | ExposeHK (`10.1145/2470654.2470735`) is indexed and read; the always-on hint line was chosen over an overlay because a keyboard-only editor has nothing for an overlay to attach to; per-row keys must be **stable per item, not per position**. |
| Composition levels | `docs/2026-08-09-unified-composition.md`, `docs/research/2026-08-09-grid-composition-corpus-check.md` | Lattice composition: a bounded composition **is** a tile. CGA absolute/relative split values, the corner-vs-edge token question (FVS-R-11), nest-at-authoring vs compose-at-stamp. |
| Solver choice / encodings | `docs/research/2026-08-10-pcg-solver-corpus.md`, `2026-08-10-constraint-encodings.md`, `2026-08-10-solver-choice.md` | Karth & Smith both papers, Sturgeon, ASP-as-design-space, N-WFC, snappable meshes, design-level constraints — all already cited. |
| Game-side colour and attention | `docs/ui.md` §1.3, §3.4 | Wolfe GS6, Rosenholtz, crowding, the alert budget, *"make color mean deviation, not danger"*. **Written for the game HUD, not for the editor** — that gap is §3 below. |

Everything below either extends these or covers an axis they do not.

---

## 1. Five papers in the library that no document here has cited

Verified absent from `docs/` by DOI grep on 2026-08-17.

### 1.1 Alexander's properties as a *layer* of rules — `10.1145/3337722.3341839`

Sandhu & McCoy, *A Framework for Integrating Architectural Design Patterns into PCG*, FDG '19.

They take three of Christopher Alexander's fifteen properties of wholeness — **strong centers, boundaries,
contrast** — and argue they belong in a WFC rule set *above* the domain-specific rules:

> *"we are trying to use higher level rules for higher level properties such as strong centers, boundaries,
> and contrast… The goal here is to try and decouple them so the more domain-specific rules can be swapped
> out while higher level rules remain the same."*

Two things this gives us that the already-cited WFC papers do not. First, a **reuse story for rule sets** —
the thing this project keeps paying for per-kit. Second, and more surprising, their worked example of
*contrast* is a colour argument about a UI, not about geometry:

> *"By having a dull blue background, the red and green squares can stand out, even more, thus drawing the
> player's eye to more important parts on the map."*

So the same property that organises the generated content organises the display of it. That is the cleanest
bridge in the corpus between the brief's "colours guide" and its "compose at different levels".

Honest caveat, theirs not mine: *"A fundamental limitation of this work is proving that these properties will
hold for all domain spaces."* It is a four-page position paper with no evaluation.

### 1.2 Content chunks — `10.1145/3337722.3341848`

Balint & Bidarra, *A generalized semantic representation for procedural generation of rooms*, FDG '19. About
*room* generation, but the finding is about **where semantics live**:

> *"Using a content chunk, semantics for an object are now contained within a single node, instead of being
> distributed over the graph."*

The two semantics they name are exactly the two this editor's kit vocabulary lacks a word for, and their own
examples are the clearest statement of them:

- **Abstraction** — *"the selection of one object to generate from a possible group"*; *"generic seat instead of
  chair"*
- **Replication** — *"the generation of multiple objects given a single selection"*; *"cups instead of cup"*

They claim the representation *"subsumes previous models as particular cases"* — so it is an extension, not a
competing schema.

And the consequence they measure is a **loss of expressivity you cannot see**: they distinguish a motif from
its *visible distribution*, and note that *"the expressive power of a room generation system is hampered by
both the representation it uses and its generation system"* — object properties *"can be lost in the motif,
as well as never sampled by the generator."*

This is directly usable. `docs/research/2026-08-10-expressive-range.md` measures what the generator produces;
this says the ceiling may be in the schema rather than the search, and that the two are separable.

### 1.3 Noncommand interfaces — `10.1145/255950.153582`

Nielsen, *Noncommand user interfaces*, CACM. Old (1993) and mostly about a future that did not arrive, but one
passage is the strongest thing in the corpus on the brief's "reduce noise" axis, because it identifies
turn-taking itself as the noise source:

> *"Many next-generation interfaces will abandon turn-taking because they will have no well-defined transition
> points where the user would stop and wait for a response."*

> *"Dynamic queries where the system works in parallel with the user without waiting for the user to finish
> specifying a query were **52% faster** than a traditional database on one test and were much preferred by
> users."*

An editor that answers *while* you are choosing does not need a message telling you the answer.

### 1.4 WorldBrush — `10.1145/2766975`

Emilien, Vimont, Cani, Poulin & Benes, SIGGRAPH 2015. The palette metaphor, done properly:

> *"selected regions of procedurally and manually constructed example scenes are analyzed, and their parameters
> are stored as distributions in a palette, **similar to colors on a painter's palette**. These distributions can
> then be interactively applied with brushes and combined in various ways, like in painting systems."*

They extend *"brushes and pipette tools, copy-paste, move, stretch, color blend and gradient"* to world
authoring, and name the failure they are avoiding: *"the user must ensure scene consistency, which potentially
breaks the artistic flow."*

Relevant here because the repo already has a Shift+B area-clone stamp and a weight-brush idea from N-WFC, and
this is the paper that says the whole verb set of a paint program is available, with a **pipette** — sample the
statistics of a region you like, then paint with them — as the operation nobody in this project has proposed.

### 1.5 Measuring the quality of the *authoring pipeline* — `10.1145/3235765.3235821`

van Rozen & Heijn, *Measuring Quality of Grammars for Procedural Level Generation*, FDG '18. Everything else in
the corpus measures generator output; this instruments the rules:

> *"it is hard to predict how each grammar rule impacts the overall level quality, and tool support is lacking"*

> *"**A lack of direct manipulation compromises the ability of designers to isolate and improve level
> qualities**, e.g., when authoring bridges, forests or paths."*

Two instruments: **MAD** (Metric of Added Detail — does this rule add or remove detail relative to its phase in
the pipeline?) and **SAnR** (Specification Analysis Reporting — express level properties, then watch how they
evolve across a generation history). Their result is deliberately modest: *"problematic rules tend to break SAnR
properties and that MAD intuitively raises flags."*

The transfer: this editor's rules are adjacency tokens and mount classes, not rewrite rules, but "which
authored thing made the output worse" is the same question, and it is currently unanswerable here.

---

## 2. Shortcut keys as first-class citizens — what the corpus adds to what we knew

ExposeHK is read and already drives the hint-line decision. Three things in it that the prior docs did not pull
out, all bearing on the *kit tab and arrow work on this branch*:

**The efficiency argument is three separate mechanisms, and only two are about the hands.** Hotkeys win because
the hands are already on the keyboard, because they skip the pointer round-trip from workspace to widget and
back, *and* because *"they allow a wide range of commands to be selected with a single key combination, thus
removing the need to traverse a menu or tab hierarchy."* The third is the one a tabbed editor can lose by
accident: a key that means different things per tab has re-introduced the hierarchy it was meant to flatten.

**Depth costs experts specifically.** *"Theoretical and empirical results tend to show that selection time
increases with the number of menu levels for experts"*, and Miller et al.'s finding that *"it is difficult to
chunk a multiple-keys sequence into one single cognitive unit when using Alt-Key navigation"* is an argument
against any prefix-key scheme (`g` then `t`) that this editor might reach for as it runs out of letters.

**The clutter objection to showing everything is answered empirically.** On presenting all bindings at once:

> *"while [it] may give an initial impression of visual clutter, studies suggest that similar methods of
> parallel presentation can improve pointer-based selection performance and reduce visual search times because
> **rapid eye saccades can replace comparatively slow pointer-based manipulation**."*

That is a direct licence for a dense always-on hint line, from the paper the hint line was derived from.

**The trap already recorded, restated because it is the one that recurs:** *"Traditional hotkey methods require
users to discover hotkeys using a non-hotkey modality (pointing), and consequently users rehearse pointing, not
hotkey use."* Any mouse path added to this editor for discoverability is training the wrong thing.

Still not in the library, still the single most relevant paper, still paywalled: Cockburn, Gutwin, Scarr &
Malacria 2014, *Supporting Novice to Expert Transitions in User Interfaces*, `10.1145/2659796`.

---

## 3. Colour instead of messages — the axis with the least prior work here

`docs/ui.md` §1.3 covers this for the **game**. The editor is a different problem: its displays are novel,
dense, and read for minutes at a time rather than glanced at under threat. Four corpus findings that change the
answer.

**Postattentive amnesia says familiarity does not help you search.** Healey & Enns, `10.1109/tvcg.2011.127`,
reporting Wolfe: previewing a display before being told what to look for *"provided no advantage. Postattentive
search was as slow (or slower) than the traditional search."* Their conclusion is written for exactly our case:

> *"In most cases, visualization displays are novel, and their contents cannot be committed to LTM. If studying
> a display offers no assistance in searching for specific data values, then **preattentive methods that draw
> attention to areas of potential interest are critical** for efficient data exploration."*

An editor panel the author has seen a thousand times still does not search itself. Colour is not a courtesy for
newcomers; it is the only channel that shortens search for the expert too.

**Guidance is as much about rejection as attraction.** Wolfe, GS6, `10.3758/s13423-020-01859-9`: *"guidance
could be as much about rejecting distractors as it is about guiding toward targets."* Two design consequences
pull in opposite directions and both are defensible: colour the *few* things that need attention, or colour the
*many* things that do not so they can be dismissed as a set. Worth deciding deliberately rather than per-panel.

**The measured industry practice is fault-colouring, and it is one colour.** Game AI Pro 1 ch. 33, on their
query-authoring editor:

> *"we colored with yellow (as opposed to everything else being in dark colors) everything that was incorrectly
> set up or was missing some values. **This way we instantly knew where a given query was broken at the very
> first glance.**"*

Paired with their labelling goal — *"even an untrained person could more or less tell what a given query will
generate just by looking at it"* — this is the whole brief in two sentences, from a shipped tool. Note the
structure: **one saturated hue on a dark field, meaning "not yet valid"**. Not a palette. Not semantic
categories. One colour for one predicate.

**And the constraint that makes colour-only wrong as an absolute rule.** WCAG 1.4.1, catalogued in
`10.48550/arXiv.2507.19549` at severity *Serious*: *"Visual information is conveyed using color alone without
additional indicators like text, shape, or pattern, making it inaccessible to users with color vision
deficiencies."* The brief says colour should guide **more than** explicit messages, which is compatible with
this — colour carries the *salience*, a glyph or a shape carries the *identity*. Colour as the only carrier of
identity is a defect for any reader with a colour vision deficiency. (Prevalence is not sourced from this
corpus; the WCAG severity is.)

**And the measured ceiling on how loud a peripheral cue should be.** Lewandowska, Dziśko & Jankowski 2022,
`10.1038/s41598-022-16284-2`, on stimuli in the screen periphery — *"a high visual intensity is not necessarily
needed for the best impact. A **medium contrast level**, a horizontal or vertical display localization, and a
**flashing frequency of 2 Hz** are sufficient to obtain the best visibility in the peripheral area."* High
contrast is reserved for *"critical alerts and the need for short-term intensive stimuli"*, and *"if it is not a
continuous operation"* — because for longer use, high-intensity cues *"can cause unnecessary irritation or even
cognitive load"*. That is a habituation budget for editor chrome, and it argues for a **quiet** status colour
plus one loud one held in reserve.

(Their paper also summarises earlier work on hue sensitivity falling faster for red→green than yellow→blue in the
periphery — that finding is cited by them, not measured by them, and the sources are not in this library.)

**The deepest framing, already cited in this repo for other reasons.** Vicente & Rasmussen, *Ecological Interface
Design*, `10.1109/21.156574`. Their skills/rules/knowledge ladder says the interface decides which kind of
cognition you get: skill-based behaviour *"can only be activated when information is presented in the form of
time-space signals"*, rule-based is *"triggered by familiar perceptual forms (signs)"*, and knowledge-based needs
*"meaningful relational structures (symbols)"*. Text is a symbol; it recruits the slowest level. A colour is a
sign, a moving highlight under the cursor is a signal. *"To support interaction via time-space signals, the
operator should be able to act directly on the display."* This is the theory behind the brief's whole colour
claim, and it is a stronger argument than "colour is prettier than text".

---

## 4. Reducing noise — the case against messages, sourced

**Repeated uninformative alerts are worse than no alerts, and the mechanism is measured.** Ancker et al.,
`10.1186/s12911-017-0430-8` (already cited in `docs/ui.md` and the animation-editor doc, and it applies here
verbatim): *"alert fatigue is connected to complexity of work and proportion of repeated (and likely
uninformative) alerts."* The killer detail for an editor that notifies on every action:

> *"repeated alerts caused additional cognitive overload because of the need to review and dismiss them, **even
> if the clinician tended to dismiss them without reading them**."*

The cost is paid on dismissal, not on reading. A notice that is never read is not free.

**Then the positive form.** Nielsen's dynamic-queries result above (52% faster, no turn-taking), and from
Marschner's *Fundamentals of Computer Graphics* ch. 27: *"Low-latency visual feedback allows users to explore
more fluidly, for example by showing more detail when the cursor simply hovers over an object rather than
requiring the user to explicitly click."* Plus Shneiderman's mantra, which is a noise budget by another name —
*"Overview first, zoom and filter, details on demand"*.

**Sentient Sketchbook is the worked example of replacing messages with a continuous readout.** `fdg2014_paper_37`:
level quality appears as *"colored horizontal bars ranging from 0% to 100%"*, recomputed live, because
*"evaluating navigational and topological properties of the user's level is lightweight and can be performed on
real-time with every user interaction."* No modal. No message. A bar that is the wrong length.

**The one thing the corpus insists must stay explicit.** Lai et al., `10.1145/3402942.3402946`, note that
designers are *"mainly interested on why a map is 'deemed unplayable by the AI agent'"*, and the PCG book's
mixed-initiative chapter poses the conflict question without answering it: when the human states contradictory
desires, *"Should it simply provide an error message? Should it randomly choose which desire is more important
for the human? Should it generate several plausible answers and then ask the human to choose which solution is
most reasonable?"* A **refusal needs a reason**; a success does not need an announcement. That asymmetry is the
noise rule, and it matches this project's one-path/fail-loudly stance rather than fighting it.

---

## 5. Kit bashing electronically — the theme has a literal precedent

The brief's metaphor is already implemented in a published technique. Códices, Andrade, Silva & Fachada,
*Procedural generation of 3D maps with snappable meshes*, `10.1109/access.2022.3168832` (cited in
`docs/2026-08-16-collections.md` and the solver corpus, but **for its algorithm, not for its interface**).

Their compatibility model is a connector with **two visible properties**:

- a **pin count** — connectors pair when counts match, or differ within a `pinTolerance`
- a **colour** — a `colorMatrix` of valid combinations, one-way or symmetric, with **white as a wildcard**

And the reason that matters for an editor rather than a generator:

> *"A designer can easily define a passage as **n pins wide or tall**, keeping consistency in the design of the
> layout of the individual pieces being made separately."*

> *"by using the colour matching rules, the pieces developed by one designer can be **grouped in the final
> outputs**, allowing for focused design and prototyping of pieces belonging to specific areas or sections that
> can be seamlessly combined together while keeping a mixed-authorship approach."*

So the colour is not decoration on top of the constraint — **the colour is the constraint**, legible at a glance,
and it doubles as an authorship/zone grouping. That is the brief's "colours guide the user" and its
"electronic kit bashing" turning out to be the same mechanism. This repo's adjacency tokens are already
functionally edge colours (`emerge_core::adjacency`, Wang tiles per `docs/2026-08-09-unified-composition.md`);
what snappable meshes adds is that the token should be **drawn in the colour it is**, and that a wildcard
deserves its own reserved colour.

They also record the comparative usability claim this project should not ignore, since it chose the harder tool:

> *"…than WFC, which several users found difficult to grasp and refactor."*

Their mitigation is explainability rather than simplification — via Zhu et al.'s XAID argument that
*"increasing algorithmic complexity… hinders the designer's understanding and trust about what the algorithm is
doing. Consequently, designers are likely to avoid using such techniques to their full potential or not use
them at all"* — implemented as **a narrated log**: *"the generative process is narrated in the form of a
sequential textual description, in which the algorithm's decisions are explained."*

Note the tension with §4: that is a lot of text. The reconciliation is that the narration is *on demand for a
refusal*, not *on every success* — which is precisely the asymmetry §4 landed on.

**And the WFC-native version of the same idea, already in the corpus.** Karth & Smith 2017,
`10.1145/3102071.3110566` — WFC's own animation is the explanation:

> *"The unresolved information in the images on the left is shown as **the average color value of their possible
> outputs**."*

> *"The concept of working with partial designs is part of what makes the animations derived from
> WaveFunctionCollapse executions so visually stunning — we aren't used to seeing our generators work this way."*

Superposition rendered as blended colour is a progress display, a debug view and an explanation at once, and it
costs no messages. Gumin's own reason, quoted in the same paper: *"I noticed that when humans draw something
they often follow the minimal entropy heuristic themselves. That's why the algorithm is so enjoyable to watch."*

---

## 6. Composable entities and WFC at several levels

Mostly settled — see §0. What the sweep adds:

**Authoring by critique, with negative examples.** Karth & Smith 2019, `10.1145/3337722.3341845` (cited here for
its multi-tile-module footnote; its *interaction* argument is not). The tension it names is this project's own
tile-authoring cost: *"the more design effort expended to produce detailed training examples for shaping a
generator, the lower the return on investment."* Their answer:

> *"we demonstrate how an artist might craft a focused set of additional positive and negative design fragments
> **by critique of the generator's previous outputs**… the goal is [to] define a space of desirable artifacts
> from which the generator may sample."*

Mechanically cheap in our terms: *"any arbitrary adjacency validity function can be substituted here… it can act
as the whitelist for the constraint domains without changing the WFC solver itself."* A **"never this again"**
verb pointed at a generated result is a schema-level feature, not a solver change. They flag the cost honestly:
negative examples come *"at the cost of increased interface complexity"*, and *"One of the reasons that WFC was
rapidly adopted was that artists could create complex constraints by painting a picture. Complicating the
interface removes some of this advantage."*

**Pinning, and showing what is pinned.** From the PCG book on Tanagra's evolution: the user pins geometry by
adding positional constraints, the system *"attempt[s] to minimise the number of required positioning changes
(including never being allowed to move pinned geometry)"*, and — the part that reads as a bug report from the
future — *"Later versions of Tanagra altered the UI to make it clearer what geometry was 'pinned' and what was
not."* Later still they added *"geometry preference toggles… whether or not particular geometry patterns are
preferred or disliked"*, which is §6's critique verb in per-slot form.

**Scale has a documented ceiling.** Merrell & Manocha, `10.1109/tvcg.2010.112` (cited here for its cost
argument, not this): *"Model synthesis algorithms are good at creating geometric detail at a particular scale,
but not at multiple scales"* — they cannot do building-scale and doorknob-scale at once, and grammars do that
better. This is independent support for the split this repo already chose: lattice inside a tile, something else
across the map.

**The invariant nobody should break.** PCG book ch. 11: *"All content that a human can produce using a
mixed-initiative PCG system must be possible for the computer to generate on its own."* If a hand-placed
arrangement cannot be expressed as constraints, the solver can never reproduce, extend or repair it. That is a
testable property of this editor, and it is currently untested.

---

## 7. Two designer-respect frames worth adopting wholesale

Both from `10.1145/3402942.3402946` (Lai, Leymarie & Latham, *Three Pillars of Industry*) — cited in three
handoffs here, never for these:

**Compton's grokloop.** Four steps — build a hypothesis, modify the model, evaluate the result, update the
model — and the line that makes it a design target: *"I found myself wanting a way to say **the speed of learning
depends on how short the loop is**."* This is the brief's "intuitive flow" with a measurable proxy: not clicks
per action, but *seconds from a change to seeing its consequence*.

**Gingold's magic crayons**, quoted approvingly there: tools that *"enable authors to obtain satisfactory results
with a small amount of effort"*, that *"are artistically expressive"*, and that *"are magic because they are
imbued with the power of computation."* The impossible-sounding target they set is *"a tool with the speed of
automation and control of manual labour."*

**Their catalogue of user fatigue is a checklist to test against**: it sets in *"when the designer is forced to
go through too many iterations, if feedback is slow, when there are too many options or the interface requires a
very specific input."* All four are measurable in the headless harness.

And the sharpest correction in the paper, which contradicts a natural reading of "one path, designer decides":

> *"Giving control by allowing the designer to overrule the MI-PCG agent is **not** equal to respecting designer
> control, as this takes away the co-creative human-agent relationship entirely."*

An override button is not the same as control. Tanagra's local-edit-then-adapt is the shape they endorse.

---

## 8. Gaps — searched for, not found

Named because these are the places to stop trusting this document.

- **Cockburn et al. 2014, `10.1145/2659796`** — the novice-to-expert survey. Still paywalled, still the most
  relevant single paper. Unchanged since 2026-08-15.
- **No editor-specific colour-coding study anywhere in the corpus.** Everything in §3 is transferred from
  visualisation (Healey & Enns), vision science (Wolfe, Lewandowska), process control (Vicente & Rasmussen) or a
  single practitioner chapter (Game AI Pro 33). No controlled study of colour coding in a *content-authoring
  tool* surfaced. The §3 recommendations are inferences, and should be labelled as such wherever they land.
- **The HCI notification literature is not here.** Searches for interruption cost, modeless feedback and
  progressive disclosure return audio noise suppression and clinical decision support. Alert fatigue transfers
  well and Nielsen is a lucky hit; there is no Bailey & Konstan, no Iqbal & Horvitz.
- **Nothing on keyboard-driven *modal* editing.** No Vim/Kakoune-style composability, no verb-object grammar
  study, no chord-vs-mode comparison. If the editor's key scheme grows a grammar, the corpus cannot vet it.
- **`abstract_search` is unreliable for this domain.** Four abstract-level queries returned mostly untitled rows
  and off-topic economics papers; `distill_search` with domain vocabulary outperformed it every time. Use chunk
  search here, and expect one reformulation when a term is polysemous ("noise", "clutter").
- **Sandhu & McCoy has no evaluation**, by their own admission, and the Alexander mapping is asserted rather
  than tested. Treat §1.1 as a hypothesis with a good pedigree.

---

## 9. Open, for the author

Ordered by how much they block.

1. **Does colour carry identity or only salience?** §3 says salience-only is defensible and accessible, and that
   colour-as-sole-identity is a WCAG *Serious* defect. The Game AI Pro precedent is one hue meaning "not yet
   valid". A full semantic palette is the other option and costs a legend. This decides the whole visual scheme
   and I have not assumed it.
2. **Should adjacency tokens be drawn as the colours they already are?** §5 says the snappable-meshes connector
   colour *is* the constraint. Ours are tokens on boundary cells. Rendering them as colour is cheap; reserving a
   wildcard colour is a schema decision that interacts with FVS-R-11 (edge vs corner).
3. **Is "never this again" a verb?** §6 says a negative-example critique is a whitelist edit, not a solver
   change — but Karth & Smith warn it costs interface complexity, and this repo's one-path rule means it has to
   be *the* way constraints narrow, not a second way alongside token authoring.
4. **Do we want a pipette?** §1.4's palette-of-distributions has no counterpart here. It is the largest genuinely
   new verb the sweep turned up, and also the largest.
5. **Should a refusal narrate?** §4 and §5 agree on the asymmetry — reasons on refusal, silence on success — but
   snappable meshes' narrated log is a lot of text, and this editor already has a notice channel. Whether the
   narration is a line, a panel or a log is unresolved.
6. **Is "anything the human can place, the solver can generate" a test we want to hold?** §6's invariant is
   checkable in the headless harness and is currently unchecked. It may already be false, and finding out is
   cheap.
