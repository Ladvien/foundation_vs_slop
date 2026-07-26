# The Site, the ASYNC Door, and Operative Knowledge

**Status:** design, not implemented. Written 2026-07-26 at the Director's request, before code.
**Supersedes/absorbs:** FVS-G-1 (persistent Site), FVS-D-4 (Site↔specimen), FVS-G-3 (roguelite boundary),
and replaces the "squad levelling" idea that would have violated FVS-F-2.

---

## 1. What this is

Three things that turn out to be one thing:

1. **A persistent hub** — the Site — that the player returns to between expeditions.
2. **A portal** — the ASYNC door — that is the only way out into the field.
3. **A progression system that is not levelling**: operatives accumulate *knowledge*, knowledge
   propagates between them, and knowledge changes how they behave.

The third is the reason to build the first two. A hub with no economy of its own is a menu with
geometry. What makes the Site worth walking around is that it is where knowledge is *written down*,
*read*, and *argued about* — and where the O5 Council decides what the Director has earned.

---

## 2. The Site

### 2.1 Why it is hand-authored, not generated

**The Site's geometry must be fixed.** A hub you return to after every expedition has to be learnable —
you should know where the specimen cells are without reading a sign. Procedural generation actively
fights that, and every roguelite hub that works (the House of Hades, the Prisoners' Quarters) is
hand-built.

There is also a mechanical reason. `Dungeon` is a single global resource that FVS-A-5 now regenerates
per run. Two *procedural* worlds means two of those and a great deal of ambiguity about which one a
system means. Hand-authored Site geometry sidesteps the question entirely: the Site is entities, not a
`Dungeon`.

### 2.2 The ASYNC door

**The portal already exists in code.** FVS-A-5 built `RunState::Idle ↔ Active`: entering a run
generates a fresh world from an advanced `RunSeed`; leaving tears the run entities down via
`session::run_scoped()`. The ASYNC door is that transition with a body. Nothing new is needed in the
state machine — the door is a trigger volume that calls `NextState::set(RunState::Active)`.

That also means the Site is simply **what exists while `Idle`**, and the "exempt the Site from teardown"
half of FVS-A-4 is free: Site entities just never carry `run_scoped()`.

### 2.3 Lore

The Site was chosen for one reason: **it has the ASYNC door**, a stable anomalous aperture onto the
Backrooms. The Foundation did not build the door; it built a Site around it, because a door that opens
onto somewhere the ordinary world does not reach is exactly the thing you contain by *surrounding*.

This does real work beyond flavour:

* It explains why an MTF stages from here rather than anywhere else.
* It explains the Backrooms aesthetic of the expedition levels — already in the assets
  (`almond_water_backrooms`, the wallpaper/carpet textures) and already researched in
  `docs/lore/2026-07-13-backrooms-almond-water.md`.
* **It makes the "each seed is a Branch universe" framing spatial.** The backlog already calls every
  procedural seed a Branch. A hub with *doors* makes that literal instead of abstract — and additional
  doors later are the metaphor completing itself, not scope creep.
* It is on-theme for SCP-9191, an out-of-control *generator*: a Site whose defining feature is an
  aperture onto endlessly generated space.

**The Site is SITE-67** (Director's decision, 2026-07-26). Outside the heavily-established set
(19/17/45/64), so the ASYNC door is ours rather than retrofitted onto someone else's canon. `BACKLOG.md`
§7's standing "re-verify SCP canon before shipping copy" note still applies to any *quoted* canon; the
number itself is ours to define.

### 2.4 What is in it

| Area | Purpose | Depends on |
|---|---|---|
| **The ASYNC door** | Leave on an expedition. The only exit. | FVS-A-5 (done) |
| **Containment wing** | Captured specimens, visibly held, one cell each. | FVS-D-4 |
| **Research wing** | Run experiments on specimens; the Thaumiel tree. | Push 4, FVS-F-1..3 |
| **Records office** | Read and write reports — where knowledge propagates *between* runs. | §3 |
| **Requisition** | Spend the O5 budget on consumables. | §4 |
| **Briefing room** | The O5 performance review; the Director's standing. | §4 |

---

## 3. Operative knowledge — the progression system

> The Director's framing: *"two squad members are doing field research, one tells the others about
> SCP-1048 being cuddly but its variants being killers. That squad member gets 'Knows 1048 variants are
> killers.' Then later that squad member could tell others, write it as hearsay in a report, but
> ultimately, if the squad member runs into 1048 variants, it gets more anxiety and higher reaction
> times."*

This is the right system for this game, and it is **not** levelling. It is worth being precise about
why, because the difference is the whole point of FVS-F-2.

### 3.1 Knowledge is not a stat

A level is a scalar that makes an operative better at everything, everywhere, forever. A *belief* is a
proposition about a **kind of thing** that only does anything when that kind of thing is present. It is
contextual, it is legible ("Okafor knows 1048-A is lethal"), it can be **wrong**, and it can be
*transmitted*. None of that is true of a number going up.

### 3.2 The model

Deliberately separate from the existing `ai::brain::Fact`, which is **perception** — "is this true right
now, for me, sensorily". This is **lore**: "I believe this about a class of thing, and here is where I
got it."

```
Belief {
    subject:    Subject,       // SCP-1048-A, the crab swarm, almond water, ...
    claim:      Claim,         // Lethal, Harmless, CapturableBy(rule-hint), Contagious, ...
    confidence: f32,           // [0,1]
    provenance: Provenance,    // how it was acquired — see below
    acquired:   u64,           // run tick, for recency
}

Provenance { Firsthand, Witnessed, Told { from: SquadMember }, Read { report: ReportId } }
```

**Absence of a belief is not a low-confidence belief.** This is the one modelling point I would not
compromise on, and the corpus has the argument: *"not knowing the chance of mutually exclusive events
and knowing the chance to be equal are two quite different states of knowledge"* (Fisher, quoted in
W3014596384 in the local corpus, on why a single probability distribution cannot represent ignorance).
So an operative who has never met SCP-1048 has **no** `Belief` for it — not `confidence: 0.5`. The
representation is `Option`, and "unknown" is a distinct behavioural state from "unsure".

### 3.3 Acquisition, in order of reliability

| Provenance | How | Confidence |
|---|---|---|
| **Firsthand** | It happened *to me* — I was struck by 1048-A. | Highest |
| **Witnessed** | I saw it happen to someone else. | High |
| **Told** | Another operative told me in the field. | Medium, decays with each retelling |
| **Read** | I read it in a report at the Site. | Lowest, and **persists across runs** |

`Told` rides the **existing dialogue system** (`src/dialogue/`, and `squad_ai::dialogue`'s
`MemoryStream`, already grounded in Park et al.'s generative agents). That is not a coincidence — the
dialogue layer currently has one authored conversation on a dev hotkey (FVS-K-3) and no reason to exist.
This gives it one: **conversation is the transmission medium**, so authoring conversations becomes
gameplay-load-bearing rather than decoration.

`Read` is the cross-run channel. An operative who dies takes their firsthand knowledge with them — but
if they filed a report, the *next* squad can read it. That is the roguelite meta-progression, and it is
diegetic rather than a stat carried over.

### 3.4 What knowledge *does* — and why it must cut both ways

The Director's instinct is right and worth protecting: **knowing that 1048-A is lethal makes an
operative more afraid of it.** Knowledge is not a pure buff.

* **Cost:** when the subject is in perception, a high-confidence `Lethal` belief raises FEAR gain for
  that operative. A frightened operative flees sooner, aims worse, and breaks a containment hold.
* **Benefit:** knowledge is what makes *containment* possible at all. You cannot drive an anomaly into
  a basin you do not know the shape of. Concretely: an operative with a `CapturableBy` belief can read
  that anomaly's rule clauses in the containment HUD (FVS-L-1 already renders per-clause state); without
  it, the clauses show as unknown.

That asymmetry is the game's thesis in one mechanic — **the Foundation's tragedy is that understanding a
thing is what makes it frightening, and also the only way to contain it.** It is the same trade the
research economy already encodes (Push 4: research reduces uncertainty about hidden parameters), pushed
down onto the individual operative.

### 3.5 The best part: false belief is an attack surface

Hearsay can be **wrong**, and it propagates anyway. A `Told` belief that started as a misreading, or a
report filed by a frightened operative, spreads through the squad and gets acted on.

The corpus has the mechanism: *Secrets and Misperceptions: The Creation of Self-Fulfilling Illusions*
(Sociological Science 2014, DOI 10.15195/v1.a26) — pluralistic ignorance, where a false belief survives
because everyone assumes everyone else knows better.

**And this is what the antagonist is for.** SCP-9191 generates *slop*. Slop is not only ugly monsters —
it is **plausible garbage**, which is exactly what a false report is. Giving 9191 a way to seed
misinformation into the squad's belief network makes the endgame theme ("restoring curation/quality
against an out-of-control generator") a *mechanic* the player fights, not a narrative frame around
unrelated combat. The counter-play is Foundation-shaped: **verify firsthand, and curate the records.**

This is the strongest idea in this document and I would build toward it deliberately.

### 3.6 Determinism and evolution — the constraints this must respect

* **Beliefs affect the pinned core** (they modulate FEAR, which feeds Think → movement → hashed
  `Transform`). So they are simulation state, not cosmetic.
* **The belief set must be a value field on a component present from spawn** — never a marker toggled
  on acquisition. `crate::scp1048`'s rule: *"a flipped marker would split the hashed archetype and make
  ECS iteration order run-dependent."* The `Containment` component (FVS-B-2) is the pattern to copy.
* **Propagation must be order-independent or canonically sorted.** "A tells B" over a query is a
  *pick*, so it needs a stable total key — `SquadMember` is the one every other site uses. See
  `tests/determinism_lint.rs`.
* **It must reach RL/QD** (CLAUDE.md). The evolvable knobs are propagation rate, confidence decay per
  retelling, the FEAR gain per unit of confidence, and the firsthand/hearsay reliability split. A
  natural QD descriptor falls out: **epistemic spread** — how concentrated or distributed knowledge is
  across the squad — which is a genuinely new behavioural axis and may help the archive-collapse problem
  recorded in the RL notes.
* **Expect a golden re-pin.** Any new `FixedUpdate` system permutes the schedule; this is well
  documented in `tests/replay.rs`.

---

## 4. The economy: the O5 budget

> The Director's framing: *"All basic resources purchased. Medkits, ammo. Budget decided by O5 council,
> based on the director (the player's) performance."*

This is better than a scavenged-currency system, for a reason worth stating: it is **diegetic and
top-down**. The player is not a looter; they are a Director being *assessed*. Losing an operative is not
just a tactical loss, it is a line in a performance review.

* **Budget is granted, not earned per-item.** After each expedition the O5 Council issues an allowance.
* **The rating reads the same metrics the QD fitness already computes** — survivors, containment yield,
  time, breaches. That is deliberate: FVS-I-1 has to add containment/yield terms to the fitness anyway,
  and the O5 review is the *player-facing face of the same numbers*. One source of truth for "how did
  that expedition go", surfaced twice.
* **Budget buys consumables only** — medkits, ammo, field equipment. It must **not** buy capabilities;
  those come from research (FVS-F-2: unlocks grant verbs, never numbers). Keeping the two economies
  disjoint by *kind* is what stops the soft currency from eating the research loop.

**Open risk:** a performance-rated allowance can death-spiral — a bad run yields a small budget, which
causes a worse run. Needs a floor, and probably an explicit "the Council is displeased but you are not
relieved of command" band. Worth designing before building.

---

## 5. Staging

Each stage is independently playable. Do not start the next until the previous one is green.

| Stage | Delivers | Notes |
|---|---|---|
| **1. Site + door** | Hand-authored Site; ASYNC door drives `RunState`; specimens visibly held. | The loop closes: expedition → capture → return → *see it in a cell*. Uses A-5's machinery; no new state. |
| **2. Records + beliefs (firsthand only)** | `Belief` on operatives, firsthand acquisition, the FEAR cost, the containment-HUD benefit. | The mechanic proves out with **no propagation** — simplest thing that can show the trade. |
| **3. Propagation** | `Told` via dialogue, `Written`/`Read` via reports at the Site. | Gives `src/dialogue/` a job. Cross-run persistence needs FVS-G-2 (save/load). |
| **4. O5 budget + requisition** | Performance rating, allowance, consumables. | Reads FVS-I-1's fitness terms; do it after those exist rather than duplicating them. |
| **5. False belief / 9191 seeding** | Misinformation as an attack surface; curation as counter-play. | The payoff. Needs 2–4 in place first. |
| **6. More doors** | The multidimensional hub. | Pure upside once the first door works. |

Research/Thaumiel tree (Push 4, F-1..F-3) slots between 2 and 4 and is unchanged by this document.

---

## 6. Decisions — settled 2026-07-26

1. **Site number: 67.** See §2.3.
2. **Operatives PERSIST across runs**, and carry their knowledge with them. (I had leaned the other way
   — mortal operatives, immortal reports — so the consequences are worth stating plainly rather than
   quietly inherited:)
   * **Losing an operative is now a permanent loss of everything they knew.** That is a real stake, and
     the right one for this game, but it means death has to be *rare and legible* rather than routine
     attrition. If a wipe is common, the meta-loop resets constantly and the knowledge system never
     compounds.
   * **Reports become insurance, not the only memory.** A veteran who files what they learn is hedging
     against their own death. That is a good voluntary action to offer the player — and it makes the
     records office a *choice* ("spend the time writing it up?") rather than mandatory bookkeeping.
   * **Veterans diverge.** Two operatives who survived different expeditions know different things, so
     squad selection becomes a real decision: take the one who knows this anomaly, or the one who is not
     yet afraid of it. This is where persistence pays off, and it argues for the roster screen in (4).
   * **Watch for veteran lock-in:** if one operative accumulates everything, the player will always pick
     them and the others rot. A counter-pressure is needed — fatigue, assignment limits, or simply that
     fear accumulates alongside knowledge and a veteran is the *most* afraid.
3. **The budget floor: open.** Still needs a number, and it is safe to defer — the floor only matters
   once the O5 review exists (stage 4).
4. **The player CAN see an operative's beliefs** (subject to change). So the roster screen is in scope:
   "Okafor — SCP-1048-A is lethal (firsthand, high confidence)". This is what makes a *false* belief
   spreading something the player can notice and act on, which stage 5 depends on.

---

## 7. Assets — audited against the library 2026-07-26

The Ozea Studio "Ultimate SciFi Asset Library" (`models/scifi/ozea_ultimate_library/`, 37 packs, 411
distinct meshes) covers **most of the Site**. License permits commercial use and modification;
redistributing the pack itself does not — fine for shipping inside a game.

### Already covered — build the Site from these

| Need | Have | Where |
|---|---|---|
| **Site interior kit** | `Floor_Plain`, `Floor_Grate`, `Ceiling_Corner_In/Out`, `Ceiling_Slope`, `Railing`, `Barrier_02/03`, `Double_Block`, + 69 wall/panel meshes | **HS series** is effectively a containment-facility kit; A series is structural/corridor |
| **Containment cells** | `Cellule`, `Cellule_Compact`, `Cryogenic_Stasis_Chamber`, `Botanical_SpecimenChamber_Tall`, `Aquatic_GrowthPod`, `MedPod_Treatment_Bed` | HS + D/F series |
| **Research wing** | `Analysis_Tube_Rack`, `Medical_Vial`, `Medical_Injector`, `Medical_Roling_Cart`, `MedCabinet`, `Medical_Storage_Shelf`, `Botanical_HydroponicStation` | D/F series |
| **Records office** | `Control_Pannel`, `Data_Unit`, `Medical_Data_Tablet`, + 24 desk/console/terminal/screen meshes | F series (furniture/interior) |
| **Requisition** | `Medical_Storage_Crate`, `Caisse`, + 18 crate/locker/shelf/cabinet meshes | D/HS series |
| **Lighting** | `Floor_Light_01/02/03` + 12 light/emissive meshes | **E series** is lighting/emissives |
| **Door frames** | `Door_Single_V1–V3`, `Door_Double_V1–V3 L/R`, `DoorFrame_Single/Double`, `DoorSign_Room` | B series |

### Still needed — in order

1. **An FBX/OBJ → GLB conversion pipeline. This is the blocker for everything above.** The library ships
   `.fbx` + `.obj` + `.blend` and **zero `.glb`**; all 194 `.glb` on the share are mushrooms and SCP
   characters. `docs/artist_guide.md` §3 is a hard requirement — glTF 2.0 binary only, no FBX. Blender is
   available on this host with a working headless pipeline, so this is a scripted batch job, not manual
   work. Nothing else on this list matters until it exists.
2. **The ASYNC door effect** — and the good news is this is a **shader**, not a model. The heavy frame
   already exists (`DoorFrame_Double`); what does not exist is the *aperture*: the volume inside the
   frame that visibly is-not-a-room. That is exactly the kind of thing this project already does well
   (17 authored `.wgsl`, a shared noise library, the psi-vision and VHS passes). It is the game's
   signature image and worth real effort.
3. **SCP-610 `.glb`** — still the single biggest *content* blocker overall (FVS-C-1), unrelated to the
   Site. Only a Blender generator exists.
4. **Capture device + throw VFX** — the core verb has no visual at all: canister, arc/trail, and a
   containment field volume that reads as "this thing is being held".
5. **Containment-progress state on the creature** — a shader/particle state readable across the room, so
   the player is not forced to watch the HUD.
6. **Quarantine boundary effect** — area-denial bounds a *region*; it needs a visible perimeter where
   inside/outside is unambiguous.
7. **Specimen "held" idles** — animation, not modelling; 999, 1048 and 150 already have rigs.
8. **Nest-capping seal** — archetype 3 currently succeeds silently.

### Housekeeping found during the audit

* **~700 macOS resource-fork files (`._*`) are interleaved through the library** and will confuse any
  batch importer — every `find` for `.fbx` returns them as phantom duplicates. Strip them before writing
  the conversion pipeline.
* Note the pattern to keep: nothing here should be copied into `assets/` wholesale. The share is the
  *library*; `assets/` holds only what the game loads, converted and named for its use (see
  `docs/artist_guide.md` §2).

---

## 8. Why I think this is the right direction

The Site is not a menu with geometry. It is where the game's three loops finally touch: you *capture* in
the field, you *learn* in the records office, and what you learn changes who your operatives are the next
time they go out. The knowledge system is what makes the hub a place rather than a screen — and it turns
the antagonist from a monster generator into something that can attack the squad's *understanding*,
which is a far more interesting fight for an organisation whose entire purpose is knowing things
accurately.
