# SCP Foundation Role Taxonomy
### Reference for agents designing playable and non-playable roles

**Status:** Research reference. Not a design decision record.
**Scope:** Personnel roles only. See sibling docs for anomalies, factions, and paratech systems.
**Canon posture:** Tight, not religious. Faithful to shared vocabulary; free to invent within it.
**Game context:** Multiversal scope. Party roles: combat specialist, researcher, psionic, xenobiologist.

---

## 0. How to use this document

If you are an agent generating role definitions, character sheets, class trees, or NPC rosters:

1. **Read §1 first.** The five-axis model is the load-bearing idea. A role is not a rank; it is a coordinate.
2. **Respect the canon confidence markers** (§2). `★` and `●` items are shared vocabulary — misusing them is the difference between a game that reads as authentic and one that reads as fan-fiction. `○` items are yours to shape.
3. **Never invent a new clearance level or personnel class.** Those two axes are closed sets. Everything else is open.
4. **Prefer `Specialist` for anything that doesn't fit.** It is the canonical escape hatch and it is used exactly this way in canon.
5. **Use the schema in §11** as the source of truth for data structures.

---

## 1. The core model: five orthogonal axes

The Foundation has **no unified rank ladder.** The wiki says so explicitly and warns authors off treating clearance as rank. Compartmentalization is the point: a private can hold Level 4 clearance for one operation while a colonel sits at Level 2 because he has no need to know.

This is a gift. It means a character is a **coordinate in a five-dimensional space**, not a position on a ladder.

| Axis | Question | Closed set? | Game analogue |
|---|---|---|---|
| **Clearance Level** (0–5) | What may you *know*? | **Yes — do not extend** | Information unlocks |
| **Personnel Class** (A–E) | How close may you *get*? | **Yes — do not extend** | Risk & permission state |
| **Staff Title** | What do you *do*? | No | Class |
| **Department / Division** | What is your *domain*? | No | Specialization / skill tree |
| **MTF Assignment** | Where are you *deployed*? | No | Party / squad |

Design consequence: **you can move a character along one axis without moving them along the others.** Promote a researcher's clearance without changing their job. Flag an operative Class E without demoting them. That decoupling is where the drama lives.

---

## 2. Canon confidence legend

Applied to every role in this document.

| Marker | Meaning | Agent guidance |
|---|---|---|
| **★** | On the official Staff Titles list (`security-clearance-levels`). There are exactly **eight**. | Use the canonical name verbatim. Do not rename. |
| **●** | Strong canon by heavy, consistent usage across many articles. | Safe to use. Keep the established meaning. |
| **○** | Niche, author-specific, or a reasonable extrapolation from canon. | Free to define, rename, or discard. |
| **✦** | Invented for this project. Not canon. | Must be internally consistent. Flag in-game if it matters. |

**Important:** the official list of staff titles is *tiny*. Eight entries. Everything else in this document is emergent from usage. That is not a gap in the research — it is how the setting works, and it is deliberate.

---

## 3. Axis 1 — Clearance Levels

Clearance is a **ceiling on information**, not an entitlement to it. Access is need-to-know, granted per-project at the discretion of a **Disclosure Officer** (see §6, and read the design note — this role is badly underused).

| Level | Name | Typical holders |
|---|---|---|
| 0 | For Official Use Only | Clerical, logistics, janitorial at facilities holding no anomalies |
| 1 | Confidential | Support staff working *near* anomalies with no access to them |
| 2 | Restricted | **Most researchers, field agents, containment specialists** |
| 3 | Secret | **Senior researchers, project managers, security officers, response teams, MTF operatives** |
| 4 | Top Secret | Site Directors, Security Directors, MTF Commanders |
| 5 | Thaumiel | O5 Council and hand-picked staff |

**Special case — Δ (Delta) clearance.** RCT-Δt (Temporal Anomalies) operates outside the normal clearance structure with its own designation. Canonically, their credentials are *not reliably recognized* by other departments. This is free narrative friction for a multiversal game — an agent from a department that has not been founded yet, holding a badge nobody's scanner accepts.

**Do not:**
- Treat Level 5 as "max level" in a progression system. It is a compartmentalization tier, not XP.
- Assume high clearance implies high rank or vice versa.

---

## 4. Axis 2 — Personnel Classes

Assigned by **proximity to danger.** Fully orthogonal to clearance.

| Class | Meaning | Game reading |
|---|---|---|
| **A** | Strategically essential. **Never** permitted near an anomaly. Evacuated first. All O5s are Class A. | Cannot enter the play space. A constraint, not a buff. |
| **B** | Locally essential. Only quarantine-cleared anomalies. Evacuated on breach. | Restricted access |
| **C** | Direct access to most non-hostile anomalies. Quarantined if exposed to memetics. | **The default for playable characters** |
| **D** | **Expendable.** Recruited from death-row inmates worldwide. Monthly amnestics or termination. Terminated immediately in a catastrophic site event. Protocol 12 permits recruitment from political prisoners and refugees under duress. | See below |
| **E** | **Provisional, post-exposure.** Applied to field and containment staff exposed during initial containment of a *new* anomaly. Quarantined, monitored for changes in behavior, personality, or physiology. Cleared only after full debriefing. | **See below — this is the best mechanic in the setting** |

### Design note: Class E is not a job. It is a debuff.

You did your job correctly. You were the first one through the door, which is what you were paid for. You are now flagged, quarantined, and — until a psychiatrist signs off — not entirely trusted to be yourself.

For a party-based game this is nearly perfect: a status that is **earned by competent play**, applies to a *character* rather than an *encounter*, persists across missions, and creates paranoia among teammates without requiring any actual betrayal mechanic. Consider making it visible to other party members but not to the afflicted player.

### Design note: Class D and the termination policy

The monthly-termination policy is **contested in-universe.** The wiki itself suggests it may be deliberate disinformation intended to stop researchers forming attachments to D-Class, and that the modern reading is amnestics plus transfer, with termination reserved for the contaminated or the broken.

Do not resolve this ambiguity. The ambiguity *is* the content.

---

## 5. Axis 3 — Staff Titles

### 5.1 The official eight

These are the only titles on the canonical list. Use them verbatim.

- **★ Researcher** — the scientific branch. Specialties explicitly span from chemistry and botany to theoretical physics and *xenobiology*.
- **★ Containment Specialist** — engineers and technicians. Two distinct jobs: field teams that establish *initial* containment and transport, and engineers who design and maintain cells. **Not combat personnel.** Canon jokes that they carry a pipe wrench.
- **★ Security Officer** ("Guard") — recruited from military, law enforcement, corrections. Handles *information* security as well as physical. **In a breach, their job is to call for backup and evacuate civilians — not to fight.**
- **★ Tactical Response Officer** — the SWAT tier. Heavy weapons, real armor, escorts containment teams, defends against hostile Groups of Interest.
- **★ Field Agent** — the eyes and ears. Two flavors: *embedded* (undercover in police, EMS, regulators; typically unarmed; job is to notice and phone it in) and *investigator* (a suit; may carry a sidearm; job is to confirm and call for backup). **Neither is equipped to fight an anomaly.**
- **★ Mobile Task Force Operative** — special forces. Veterans drawn from across the Foundation.
- **★ Site Director** — top of a facility. Level 4. All department directors report to them; they report to O5.
- **★ O5 Council Member** — thirteen Overseers, `O5-1` through `O5-13`. Identities classified. Always Class A.

**The three-tier combat distinction is load-bearing.** Canon states it as an analogy: Guards are military police, Response Teams are combat infantry, MTFs are special operations. Collapsing these into one "soldier" class is a recognizable amateur tell.

### 5.2 Research track

- ● **Assistant Researcher** / ● **Junior Researcher** — new, or not yet trusted with judgment calls
- ● **Researcher** — years to a decade of experience; typically owns one *aspect* of one anomaly
- ● **Senior Researcher** — leads teams; canon describes them as rare
- ● **Lead Researcher** / ● **Project Lead** — owns an SCP end to end
- ● **Doctor ("Dr.")** — used constantly in canon. Functionally a Researcher with a doctorate. **Not a rank.**
- ● **Department Head / Director** — e.g. the RAISA Director
- ○ **Chair** — Site-43 uses titles like "Chair of Archives and Revision"

### 5.3 Security and combat track

- ● **Guard** → ● **Sergeant** → ● **Security Chief** → ● **Director of Security** (Level 4)
- ● **Response Team member** — the middle tier everyone forgets exists. Do not forget it.
- ● **MTF Operative** → ● **MTF Commander (MTFC)** (Level 4)
- ○ **Decommissioning specialist** — destroys anomalies for a living. Works under the Decommissioning Department, which reviews petitions to terminate anomalies when containment is deemed unsustainable.
- ○ **Recovery / Extraction specialist**

### 5.4 Field and intelligence

- ● **Agent** / ● **Senior Agent**
- ● **Embedded Agent** — inside police departments, EMS, coroners' offices, regulators
- ● **Investigator** — confirms, doesn't fight
- ● **Analyst** — Department of Analytics; runs the WATCHDOG monitoring network
- ● **Amnestics technician** — the one actually holding the syringe
- ● **Cleaner / cover-up operative** — MTF Gamma-5 "Red Herrings", Disinformation Bureau
- ○ **Liaison** — to Groups of Interest, governments, Nexuses

### 5.5 Specialist tracks

**`Specialist` is the canonical catch-all** for a skillset that does not fit the org chart. Use it liberally; canon does.

- ● **Psionics Specialist** — **canon.** Exemplar: Specialist Samara Maclear, a former field agent employed for her clairvoyance and consulted across departments on psionic anomalies.
- ● **Thaumaturge** — Alchemy Department. Learned, ritual, EVE-fuelled. **Not a psionic.** See §10.
- ● **Memeticist** / ● **Antimemeticist** — the latter runs on mnestics and cannot reliably remember their own job
- ● **Xenobiologist** — named explicitly in the canonical clearance document
- ● **Parazoologist** / ● **Cryptozoologist**
- ● **Pataphysicist** — Site-87, Sloth's Pit. Narrative anomalies.
- ● **Chaplain / Tactical Theologian** — Akiva radiation, relics, exorcism
- ● **Archivist / Librarian** — RAISA, Archival Division
- ● **Paratherapist** — canon term. Therapy for people who have seen things.
- ○ **Reality Anchor Engineer** — Scranton hardware
- ○ **Kant Counter Technician** / ○ **Hume Field Operator**
- ○ **Cognitohazard Specialist** — MTF Eta-10 "See No Evil". Works through mirrors and cameras because looking directly is fatal.
- ○ **Eigenweapons / Paraweapons Engineer** — Department of Anomalous Weapons Development

### 5.6 Multiversal roles — **priority for this project**

- ● **Reality Liaison** — **canon.** Exemplar: Alex Thorley, Reality Liaison for the Department of Unreality, a department that does not exist. Constantly transferred between sites. Nobody can articulate what the job requires; Thorley appears uniquely qualified for it.
- ● **RCT-Δt Operative** — Temporal Anomalies. Holds Δ clearance. Canonically, the department **has not yet been founded**, so operatives from the past, future, or alternate timelines struggle to get their credentials recognized.
- ● **Multi-U Expedition Member** — Department of Multi-Universal Affairs. Operating doctrine: *observe, do not interact*. The canonical framing is that in another universe, **you** are the anomaly.
- ○ **Nexus Liaison / Free Port Attaché** — Three Portlands, Backdoor SoHo. Diplomatic postings to places where the Veil does not apply.
- ○ **Ways-walker** — ritual traversal rather than technological. The Serpent's Hand does this; a Foundation equivalent is a natural invention.
- ○ **Lampeter station staff** — SCP-7005 is a decaying multidimensional transport network. The Foundation controls several hundred stations. Canon never names the people who *man* them. **This is free real estate for a multiversal game** — an entire職 class with a canonical justification and no canonical name.

### 5.7 Administration and oversight

- ● **The Administrator** — may not exist
- ★ **O5-1 … O5-13** — always Class A; never touch an anomaly
- ● **Factotum** — canon term for O5 personal assistants **and body doubles**
- ● **Disclosure Officer** — decides, per project, what you are permitted to know. **See design note below.**
- ● **Director of Task Forces** — commissions and dissolves MTFs
- ● **Ethics Committee member** — decides what is *ethical*. O5 decides what is *safe*. These are different questions and the friction is deliberate.
- ● **Senior Legal Consultant** — canon (Sheldon Katz, whose legal acumen is respected by demonic entities)
- ○ **Assistant Director** / ○ **Regional Command staff**

#### Design note: the Disclosure Officer

This is the most under-exploited role in the entire mythos.

Clearance is a ceiling. Someone still has to approve each read. That someone is a person with a name, a desk, and an opinion.

In a game where the party is chasing a truth across universes, **an NPC who can simply say no is worth more than any monster.** They cannot be shot. They cannot be out-run. They are not evil. They are correct, by policy, and they will be correct again next week.

### 5.8 Support and texture

Do not skip these. They are what make a site feel like a workplace rather than a dungeon.

- ● Janitorial, clerical, logistics (Level 0–1)
- ● Medical staff — **the people who declare you Class E**
- ● IT / SCiPNET administration
- ● **Acroamatic Abatement** — anomalous waste disposal. Somebody does this. Canon has a whole department for it.
- ○ Motor pool, cafeteria, HR, Accounting — canon has all of these and plays them straight

---

## 6. Axis 4 — Departments

Not exhaustive. Canon lists 300+. These are the ones that bear on role design.

| Department | Relevance |
|---|---|
| **Anomalous Weapons Development (AWD)** | Designs para- and eigenweapons for MTFs and site security. Grew out of a 7th Occult War think-tank. Its reputation is currently poor and it is trying to rebuild trust. → **Combat specialist** |
| **Department of Parazoology** | Anomalous animals. Subdivisions include Aquatic Anomalies, Apiary, Cetacean Studies, and Architectural Zoology (living buildings). → **Xenobiologist** |
| **Cryptozoology Division** | Cryptids. Field research, lab testing, containment of parafauna. HQ Site-44. → **Xenobiologist, field flavor** |
| **Dept. of Astronomy / Extraterrestrial Affairs** | Subdivisions include Exoarchaeology, Exo-Linguistics, Xenohistory, Xenozoology. → **Xenobiologist, actual-*xeno* flavor** |
| **Pataphysics Department** | Site-87. Narrative anomalies; the proposition that reality is fictional. → **Multiversal** |
| **Dept. of Multi-Universal Affairs** | Alternate universes. → **Multiversal** |
| **Dept. of Interdimensional Logistics** | Runs SCP-7005 "Lampeter". Underfunded. → **Multiversal** |
| **Temporal Anomalies Dept. (RCT-Δt)** | Δ clearance. Outside the normal command structure. Not yet founded. → **Multiversal** |
| **Antimemetics Division** | Ideas that erase themselves. Staff run on mnestics. → **Highest mechanical ceiling in the setting** |
| **Alchemy Department** | Aetheric manipulation. A division of the Department of Science. → **The wizard** |
| **Dept. of Tactical Theology** | Relics, miracles, cults. Staffed by theologians, clerics, and sceptics. HQ Reliquary Area-27. → **The cleric** |
| **AIAD** | Builds `.aic` — Artificially Intelligent Conscripts — that run facilities and deploy on missions. → **Party AI / hacker** |
| **Ethics Committee** | Watches, evaluates, passes judgement |
| **RAISA** | Records and information security. Decides what gets redacted. Those `[REDACTED]` blocks have an author. |
| **Disinformation Bureau** | Amnestics, memory fabrication, cover stories |

---

## 7. Axis 5 — MTF Assignment

**Naming convention:** `MTF <Greek letter>-<number> ("<Nickname>")`

**The Greek letter carries no meaning.** It is decorative. Numbers repeat across letters. You are free to invent units.

**Organization:** each MTF is structured to suit its purpose. Combat units follow military hierarchy; small units may have informal or frankly strange chains of command. Topped by an MTF Commander. Size ranges from battalion-strength to fewer than a dozen. In the field they pose as police, EMS, or military.

**Lifecycle:** commissioned by the Director of Task Forces, often with O5 approval. **Deactivated when the job is done, or if casualties render them non-viable.** A task force can be disbanded out from under the party. That is canon, and it is a plot.

### Units relevant to this project

| MTF | Mission | Fits |
|---|---|---|
| **Tau-5 "Samsara"** | Immortal cyborg clones grown from the flesh of a dead god. Esoteric and experimental weaponry against **thaumaturgic and psionic threats**. | **Nearly your exact party already.** Combat + psionic in one unit. |
| **Lambda-9 "Mind over Matter"** | Investigates, contains, and sometimes *terminates* psionic phenomena. **Some members are themselves psionic.** | **Psionic — the canonical home** |
| **Delta-3 "Solomon's Hand"** | Test case for the **Special Asset Task Force Program** — an initiative to employ people with paranormal abilities as field agents. Dissolved 1990 after losing its primary Special Asset Agent. | Psionic — the *precedent*, and a ready-made tragic backstory |
| **Alpha-9 "Last Hope"** | Trains and deploys **humanoid SCP objects** in the field | Psionic, if yours is asset rather than employee |
| **Nu-7 "Hammer Down"** | Heavy assault | Combat specialist |
| **Epsilon-11 "Nine-Tailed Fox"** | Internal security. Deployed when site protocols fail and multiple breaches are imminent. | Combat specialist — the iconic one |
| **Gamma-4 "Green Stags"** | Forested and nature anomalies; ecology, tracking. Operates on behalf of the Cryptozoology Division. | Xenobiologist |
| **Theta-4 "Gardeners"** | Plant and plant-like anomalies; widespread infestations | Xenobiologist |
| **Eta-5 "Jäeger Bombers"** | Large-Scale Aggressors — entities over 30m | Xenobiologist, kaiju mode |
| **Beta-7 "Maz Hatters"** | Extreme biological, chemical, radiological hazards; pandemic contingencies | Xenobiologist, outbreak mode |
| **Eta-13 "Gulliver's Tourists"** | Created to journey into a vast extra-dimensional tunnel network known as "The Gate" | **Multiversal** |
| **Beta-777 "Hecate's Spear"** | Thaumaturgical ritual analysis, countermeasures, and thaumaturgical **combat** | The wizard |
| **Eta-10 "See No Evil"** | Visual cognitohazards; anomalies requiring indirect observation | Party-wide mechanic bait |
| **Kappa-10 "Skynet"** | Cyber-anomalies, using AICs alongside researchers | The hacker |
| **Alpha-1 "Red Right Hand"** | Reports directly to O5. Best and most loyal. Everything about them is Level 5. | Antagonist / endgame |
| **Rēsh-1 "Seat of Consciousness"** | Reports to the **Administrator**, as a deliberate counterweight to Alpha-1 | Faction politics |

---

## 8. Non-staff human categories

- **★ D-Class** — expendable test subjects. See §4.
- **★ Class E** — a *status*, not a job. See §4.
- **● Person of Interest (PoI)** — anomalous civilians the Foundation watches rather than contains
- **● Special Asset Agent** — canon. People with paranormal abilities employed as field agents (MTF Delta-3). The program was dissolved in 1990.

---

## 9. Non-human personnel

- **● `.aic`** — Artificially Intelligent Conscript. Runs facilities, deploys on missions. Canon examples: 8-Ball, Glacon. A squadmate who is software.
- **● Humanoid SCP Asset** — MTF Alpha-9 "Last Hope" deploys *contained anomalies* as operatives. They have designations, not just names.
- **○ Anomalous staff** — canon has plenty. The Anomalous Entity Engagement Division exists specifically to make this humane.

---

## 10. The four party roles — full specification

### 10.1 Combat specialist

| Field | Value |
|---|---|
| Canonical title | **Tactical Response Officer** (site-bound) or **MTF Operative** (deployable) |
| Common error | Calling them a Security Officer. That is the tier *below* — a guard, not a soldier. |
| Clearance | 3 |
| Class | C |
| Department | Anomalous Weapons Development |
| MTF | Nu-7, Epsilon-11, Tau-5 |
| Mechanical hooks | Eigenweapons and paraweapons; Scranton Reality Anchor deployment; the fact that AWD's reputation is currently in the toilet and their gear is not entirely trusted |

### 10.2 Researcher

| Field | Value |
|---|---|
| Canonical title | **Researcher** → **Senior Researcher** |
| Clearance | 2 → 3 |
| Class | B or C |
| Department | Any — this is the skill-tree choice |
| Mechanical hooks | Kant counter and Hume readouts; amnestic administration; containment-class assessment. The **"Locked Box Test"** — imagine the object in a locked box, ask what happens if you leave it alone — is canon's own heuristic for classification and would make an excellent minigame. |

### 10.3 Psionic

| Field | Value |
|---|---|
| Canonical title | **Psionics Specialist** |
| Clearance | 2–3 |
| Class | C — **or the character *is* the anomaly** |
| MTF | **Lambda-9** (canonical home), Delta-3 (the precedent), Alpha-9 (if asset), Tau-5 |

**Canon gives two framings, and choosing between them is your game's thesis:**

1. **Specialist** — an employee with an unusual résumé. Clocks in. Gets a badge. Consults. Goes home.
2. **Special Asset / Alpha-9** — a contained anomaly the Foundation has decided to *point at things.* Has a designation. Personnel Class rules apply *to* them, not just around them.

Option 2 is the more interesting character by a wide margin. It makes every other party member's clearance and class rules load-bearing on a person they eat lunch with.

**Keep psionics mechanically distinct from thaumaturgy.** Fans police this line:

| System | Source | Fuel | Learnable? |
|---|---|---|---|
| **Psionics** | Innate. Mind and signal. | — (or a personal reserve) | No |
| **Thaumaturgy** | Learned. Ritual and rule-bound. | EVE | **Yes, by anyone with skill** |
| **Reality-bending** | Innate. Alters reality itself. | Personal Hume differential | No |

Three systems. Three resources. Do not merge them.

### 10.4 Xenobiologist

| Field | Value |
|---|---|
| Canonical title | **Researcher**, specialty xenobiology — named explicitly in the canonical clearance document |
| Clearance | 2 → 3 |
| Class | C |
| Department | Parazoology / Cryptozoology Division / Extraterrestrial Affairs |
| MTF | Gamma-4, Theta-4, Beta-4, Eta-5, Beta-7 |
| Dark mirror | **Sarkicism.** Flesh-crafting, carnomancy, biological immortality, chimeric thralls. A Karcist is what your xenobiologist becomes if they stop asking permission. |

**Multiversal note:** in a game that visits other universes, the xenobiologist has the most naturally escalating job in the party — every world is a new phylogeny. Consider a cataloguing/collection system as their core loop.

---

## 11. Machine-readable schema

Use this as the source of truth for role data structures.

```yaml
# role.schema.yaml
Role:
  id: string                    # snake_case, stable, e.g. psionics_specialist
  display_name: string
  canon_confidence: enum        # OFFICIAL | STRONG | NICHE | INVENTED
  canon_source: string?         # URL. Required unless canon_confidence == INVENTED.

  # --- The five axes ---
  clearance:
    typical: int                # 0..5
    range: [int, int]
    special: string?            # e.g. "delta"  (RCT-Δt only)
  personnel_class:
    typical: enum               # A | B | C | D | E
    permitted: [enum]
  track: enum                   # RESEARCH | SECURITY | FIELD | SPECIALIST
                                # | MULTIVERSAL | ADMIN | SUPPORT | NON_HUMAN
  departments: [string]         # department ids; may be empty
  mtf_eligible: [string]        # mtf ids; may be empty

  # --- Play ---
  playable: bool
  combat_tier: enum             # NONE | SIDEARM | RESPONSE | SPECIAL_OPS
  power_system: enum?           # PSIONIC | THAUMATURGIC | REALITY_BENDING
                                # | THEURGIC | NONE
  resources: [string]           # meter ids: hume | eve | akiva | mnestic ...

  progression:
    grades: [string]            # ordered, e.g.
                                # [assistant_researcher, researcher,
                                #  senior_researcher, lead_researcher]

  notes: string?
```

```yaml
# Worked example
- id: psionics_specialist
  display_name: Psionics Specialist
  canon_confidence: STRONG
  canon_source: https://scp-wiki.wikidot.com/personnel-and-character-dossier
  clearance:   { typical: 3, range: [2, 3] }
  personnel_class: { typical: C, permitted: [C, E] }
  track: SPECIALIST
  departments: []
  mtf_eligible: [lambda_9, delta_3, alpha_9, tau_5]
  playable: true
  combat_tier: SIDEARM
  power_system: PSIONIC
  resources: [psi]
  progression:
    grades: [consultant, specialist, senior_specialist]
  notes: >
    Canon exemplar: Specialist Samara Maclear, former field agent,
    employed for clairvoyance, consulted across departments.
    Alternate framing: Special Asset (see MTF Delta-3) — the character
    is itself the anomaly. Choose one; do not blend.

- id: tactical_response_officer
  display_name: Tactical Response Officer
  canon_confidence: OFFICIAL
  canon_source: https://scp-wiki.wikidot.com/security-clearance-levels
  clearance:   { typical: 3, range: [2, 3] }
  personnel_class: { typical: C, permitted: [C, E] }
  track: SECURITY
  departments: [anomalous_weapons_development]
  mtf_eligible: [nu_7, epsilon_11, tau_5]
  playable: true
  combat_tier: RESPONSE
  power_system: NONE
  resources: []
  progression:
    grades: [response_officer, team_lead, security_chief, director_of_security]
  notes: >
    NOT a Security Officer — that is the tier below (a guard).
    Three-tier combat distinction is load-bearing canon:
    Guard : Response Team : MTF  ::  MP : infantry : special forces.
```

---

## 12. Antagonist and other-faction roles

For the NPC roster. Each faction has its own vocabulary; using it correctly is cheap authenticity.

| Faction | Roles | One-line posture |
|---|---|---|
| **Global Occult Coalition (GOC)** | Assessment Team, Strike Team ("Orange Suits"), PHYSICS Division, PSYCHE Division, Council of 108 | UN-backed. They **destroy**; you contain. |
| **Chaos Insurgency** | Cell operative; "The Engineer" | Foundation splinter. Weaponizes what you'd cage. |
| **Serpent's Hand** | Member | Anti-containment. Calls you "the Jailors." Travels by Ways. |
| **Wanderers' Library** | **Librarian** | **Not** Serpent's Hand. The Library's own staff. Strict. |
| **Sarkicism / Nälkä** | **Karcist** (practitioner), **Klavigar** (the four apostles), **Halkost** (flesh-thralls) | Your xenobiologist's dark mirror |
| **Marshall, Carter & Dark** | Sales Representative, Procurement Agent | Wields money and lawyers, not guns |
| **Are We Cool Yet?** | **Anartist** | Makes anomalous art. Wants an audience. |
| **Church of the Broken God** | Cogwork Orthodoxy clergy ("tickers") vs. Maxwellists ("hummers") | Schismatic techno-religion |
| **Unusual Incidents Unit (UIU)** | FBI Special Agent, paranormal division | Enforces US law in Three Portlands |

---

## 13. Load-bearing vs. inventable

| Get this right | Invent freely |
|---|---|
| Object classes measure **containment difficulty**, not danger | New SCPs, new sites |
| The three-tier combat distinction (Guard / Response / MTF) | New Mobile Task Forces |
| Clearance ≠ rank | New departments |
| Psionics ≠ thaumaturgy ≠ reality-bending | Universe designations |
| The Veil and amnestic logic | Your party's specific characters |
| GOC destroys; CI terrorizes; Serpent's Hand liberates | Cosmology and origins (SCP-001 is *deliberately* contradictory) |
| D-Class are expendable; the Foundation is morally grey | Narrative-layer / conceptual-tier material — skip it |
| "Secure, Contain, Protect" | Names for the unnamed (e.g. Lampeter station staff) |

---

## 14. Amateur tells — do not do these

- Calling Keter "the most dangerous class." It means *hard to contain*. A doomsday device that never activates is Safe; a teleporting kitten is Keter.
- Collapsing Guards, Response Teams, and MTFs into one "soldier."
- Treating Level 5 as the top of an XP ladder.
- Using the GOC's threat taxonomy (Type Green, Type Blue) as Foundation vocabulary. **It is not.** It belongs to the rival organisation that kills what you would cage. Using it makes your Foundation sound like the GOC — which may be a deliberate choice, but should be one.
- Making the Foundation unambiguously heroic.
- Leading with SCP-173.
- Treating the Greek letter in an MTF designation as meaningful.
- Merging magic and reality-bending.
- Forgetting that most anomalies are boring. The mundane texture is half the charm.

---

## 15. References

All SCP Wiki content is **CC BY-SA 3.0**. Attribution is required and share-alike is viral for derived content. See the licensing guide before shipping.

### Primary — official resource pages

| Topic | URL |
|---|---|
| **Security Clearance Levels** (the canonical staff-title list — start here) | https://scp-wiki.wikidot.com/security-clearance-levels |
| Mobile Task Forces | https://scp-wiki.wikidot.com/task-forces |
| Complete MTF list | https://scp-wiki.wikidot.com/task-forces-complete-list |
| Foundation Departments | https://scp-wiki.wikidot.com/departments |
| Semi-comprehensive department list (300+) | https://scp-wiki.wikidot.com/departments-complete-list |
| **Personnel and Character Dossier** (source for Psionics Specialist, Reality Liaison) | https://scp-wiki.wikidot.com/personnel-and-character-dossier |
| Object Classes | https://scp-wiki.wikidot.com/object-classes |
| Groups of Interest | https://scp-wiki.wikidot.com/groups-of-interest |
| Secure Facilities Locations | https://scp-wiki.wikidot.com/secure-facilities-locations |
| Canon Hub | https://scp-wiki.wikidot.com/canon-hub |
| **Licensing Guide** | https://scp-wiki.wikidot.com/licensing-guide |

### Primary — multiversal

| Topic | URL |
|---|---|
| Temporal Anomalies Dept. / RCT-Δt | https://scp-wiki.wikidot.com/welcome-to-delta-t |
| SCP-7005 "Lampeter" | https://scp-wiki.wikidot.com/scp-7005 |
| Pataphysics — Site-87 hub | https://scp-wiki.wikidot.com/the-s-c-plastics-hub |
| Unreality Department | https://scp-wiki.wikidot.com/unreality-hub |
| Antimemetics Division | https://scp-wiki.wikidot.com/antimemetics-division-hub |

### Primary — faction / role flavor

| Topic | URL |
|---|---|
| GOC humanoid threat guide (**their** Type Green/Blue taxonomy) | https://scp-wiki.wikidot.com/goc-supplemental-humanoid-guide |
| Anomalous Weapons Development | https://scp-wiki.wikidot.com/awd-hub |
| Cryptozoology Division | https://scp-wiki.wikidot.com/cryptozoology-division-hub |
| Tactical Theology | https://scp-wiki.wikidot.com/tactical-theology-hub |
| AIAD (`.aic`) | https://scp-wiki.wikidot.com/aiad-homescreen |
| Decommissioning Department | https://scp-wiki.wikidot.com/decom-dept-hub |
| Ethics Committee orientation | https://scp-wiki.wikidot.com/ethics-committee-orientation |

### Source-quality warning

Fan wikis (`*.fandom.com`, `scpdb.miraheze.org`, TV Tropes) are **community syntheses**, not canon. They are useful for orientation and frequently wrong on detail. Anything load-bearing must be verified against the primary article on `scp-wiki.wikidot.com` before it ships.

---

## 16. Open questions for the designer

1. **Is the psionic an employee or an asset?** (§10.3) This is the single highest-leverage unresolved decision in the role design. It changes what the other three characters *are* to each other.
2. **Does the party belong to an existing MTF, or a new one?** Tau-5 "Samsara" is uncannily close to the stated party composition — combat plus psionic, against thaumaturgic and psionic threats. Adopting it buys canon weight; inventing one buys freedom. Adopting it and then having it **dissolved** buys both.
3. **Which multiversal framework is the spine?** Lampeter (infrastructure, decay, logistics), Multi-U (expeditions, prime directive), Ways (ritual, low-tech, Library-adjacent), or RCT-Δt (time rather than space). They compose, but one should lead.
4. **Is there a Disclosure Officer?** Please let there be a Disclosure Officer.
