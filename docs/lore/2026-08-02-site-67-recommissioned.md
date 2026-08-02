# Site-67, Recommissioned
### The Site's own history, its staff roster, and the canon each is grounded in

**Status:** Design decision record. Unlike the other files in `docs/lore/`, this one *decides* things —
it is the fiction the game ships, not a survey of what the mythos contains.
**Companion to:** `docs/2026-07-26-site-hub-and-operative-knowledge.md` §2.3 (which this amends), and
`2026-07-12-scp-role-taxonomy.md` (which supplies every title and clearance used below).
**Canon posture:** Site-67 has **one** archived canon entry and **no** developed article. The entry is
adopted verbatim; everything after it is ours.
**Licensing:** SCP Wiki text is **CC BY-SA 3.0**. The quotation in §1 is attributed at §7 and any
derived text shipped in-game inherits share-alike. See the licensing guide before shipping copy.

---

## 0. Confidence legend

Same markers as the other lore files, so a reader can tell inherited canon from invention at a glance.

| Marker | Meaning | Guidance |
|---|---|---|
| **●** | Canon — attested on `scp-wiki.wikidot.com` | Use verbatim; do not contradict |
| **○** | Canon-adjacent — a structure canon supplies, applied by us | Safe, but ours to shape |
| **✦** | Invented for this project | Must stay internally consistent |

---

## 1. The entry we adopted

● From the **archived** Secure Facilities Locations list:

> **Site-67:** Small facility located in the North-East of England, built to contain high risk SCPs
> from the surrounding area, abandoned after SCP-███ breached containment. See Psychiatric Evaluation
> Log 47721 for details.

That is the whole of it. There is no Site-67 article, no floor plan, no named staff, and no stated
date. A **separate and unrelated** proposal exists in the wiki's critique forum — a Site-67 beneath
the Montreal Tower doing extrasolar early warning — but it is a draft seeking greenlight, not canon,
and this project does not use it. If it ever lands, the two coexist the way the Foundationverse
always handles collisions: overlapping canons, no arbiter.

### Why adopt it rather than start clean

The design doc chose the number 67 precisely because it was unclaimed, so adopting the entry needs to
pay for itself. It pays three times:

1. **It explains the building.** ✦ Half of Site-67 is bare floor — the research wing is twelve by ten
   cells with a decal in it, and the containment wing's cells are glass fronts with nothing behind
   them until you fill them. A facility *reopened* around an aperture nobody planned for is a fiction
   in which bare floor is correct. A continuously-operating Site with empty rooms is just unfinished
   authoring.

2. **It hands SCP-9191 a document to forge.** `src/antagonist.rs` already files reports **nobody
   wrote** (`PHANTOM_AUTHOR`, `src/knowledge/records.rs:142`), and the counter-play is going and
   looking at the thing yourself. A Site whose own history contains a redacted anomaly and a
   psychiatric evaluation log it cannot produce is the ideal substrate: the first unverifiable
   document in the archive was there before the player arrived.

3. **It costs the ASYNC door nothing.** ○ The door stays ours. The design doc's rule is unchanged and
   is restated here because it is the load-bearing sentence of the whole hub:

   > The Foundation did not build the door; it built a Site around it, because a door that opens onto
   > somewhere the ordinary world does not reach is exactly the thing you contain by *surrounding*.

   Recommissioning adds one clause in front of it: **and then reopened that Site when the door
   appeared.** The building predates us; the aperture does not.

### The shape of the history

✦ Deliberately thin. Enough to hang signage and dialogue on, not enough to become a second setting:

- Site-67 was commissioned as a **small** regional containment facility in the North-East of England,
  and it was never a Site-19. Small is canon and small is useful — it is why the player knows every
  room.
- It was **abandoned** after a breach. The anomaly is `SCP-███` in every document the player can
  read, and it stays that way. ⚠️ **Do not name it.** The redaction is the asset; a named monster in
  the basement is a different game and an obligation we would then owe the player.
- **Psychiatric Evaluation Log 47721** is referenced by the facility record and is **not in the
  archive**. It is the one document the records office cannot produce. This is canon-supplied — the
  entry itself points at a log and the wiki never wrote it.
- It was **recommissioned** when the ASYNC aperture was detected, because an aperture onto endlessly
  generated space is contained by surrounding it, and there was already a building around that spot.
- ✦ The current staff have been here **months, not decades.** Nobody on site was here for the breach.
  That is why they are willing to work next to it, and it is why the Archivist cannot tell you what
  47721 said.

---

## 2. Clearance and class — the two axes the Site displays

Both are ● canon and both are quoted from the official Security Clearance Levels page. The Site's
wall placards (`§5c` of the implementation plan) carry the clearance of the wing they label, so these
have to be right.

### Security clearance — a *ceiling on information*, never a rank

| Level | Name | Canon description |
|---|---|---|
| 0 | For Official Use Only | non-essential personnel with no need to access information regarding anomalous objects |
| 1 | Confidential | working in proximity to, but with no access to, anomalies in containment |
| 2 | Restricted | security and research personnel requiring **direct access to information** on contained anomalies |
| 3 | Secret | senior personnel requiring source, recovery circumstances, and long-term planning |
| 4 | Top Secret | senior administration; site-wide and regional intelligence |
| 5 | Thaumiel | highest-ranking administration; effectively unlimited access |

⚠️ **`docs/lore/2026-07-12-scp-role-taxonomy.md` §14 names two amateur tells that live right here:**
treating Level 5 as the top of an XP ladder, and confusing clearance with rank. Clearance is a
ceiling; someone still approves each read. Site-67 has no Level 5 personnel and should never grow one
— **Thaumiel** in this game is the *research tree*, and the collision of names is the good kind.

### Personnel class — *proximity permission*, orthogonal to clearance

| Class | Canon description |
|---|---|
| A | essential to strategic operations; **prohibited** from direct anomaly access |
| B | essential to local operations; only quarantined anomalies cleared of mind-affecting effects |
| C | direct access to most non-hostile anomalies; quarantined if exposed |
| D | expendable personnel used to handle extremely hazardous anomalies, drawn from prison populations |
| E | provisional — a field operative **exposed** during initial containment, pending screening |

**Class E is not a job. It is a debuff**, and this game already has the mechanic it belongs to: an
operative who has met an anomaly firsthand carries `Belief` and FEAR that one who has only heard of
it does not. Class E is the Foundation's name for that state. ○ Worth wiring into the roster screen
eventually; not in scope for the current plan.

---

## 3. The staff roster

Nine staff, in the wings where they belong. Every title is ● from the taxonomy doc's §5 — the
canonical eight plus the two specialist tracks canon names explicitly. Names are ✦.

| # | Name | Title | Clr | Class | Post | Rig |
|---|---|---|---|---|---|---|
| 1 | Dr. Halvorsen | **Senior Researcher** | 3 | C | Research | `scientist` |
| 2 | Dr. Amara Beck | **Researcher** | 2 | C | Research | `researcher` |
| 3 | Ito | **Containment Specialist** | 2 | C | Containment | `cipher_hazmat` |
| 4 | Brennan | **Containment Specialist** | 2 | C | Containment | `fieldop` |
| 5 | Nowak | **Security Officer** | 1 | B | Monitoring | `cipher_field` |
| 6 | Sgt. Achebe | **Tactical Response Officer** | 2 | B | Monitoring | `makarov` |
| 7 | Farrow | **Archivist** (RAISA) | 2 | B | Records | `cipher_standard` |
| 8 | Dr. Lindqvist | **Paratherapist** | 2 | C | Activities | `cipher_senior` |
| 9 | Duyen | **Logistics** | 0 | C | Galley | `fieldop` |

### The three combat tiers, and why all three are on site

The taxonomy doc states it as a canon analogy: **Guards are military police, Response Teams are
combat infantry, MTFs are special operations.** Collapsing them into one "soldier" is a named
amateur tell — and this game is otherwise at risk of exactly that, because the only armed people the
player has ever seen are the five MTF operatives they send through the door.

So the Site carries one of each, and the difference is legible in what they do:

- **Nowak, Security Officer** (● *"in a breach, their job is to call for backup and evacuate
  civilians — not to fight"*) — walks the containment wing, escorts D-Class, carries a sidearm and
  is not expected to use it.
- **Sgt. Achebe, Tactical Response Officer** (● the SWAT tier; escorts containment teams) — the
  middle tier everyone forgets. Posted at Monitoring, which is the room that watches the cells.
- **The player's five** are ● MTF Operatives, and they are the only people at Site-67 who go through
  the door.

### The two that touch shipped mechanics

Seven of the nine dress a room. Two of them stand where the game already has a system, and those are
the two worth the authoring:

- **Farrow, Archivist.** ● RAISA — *"Records and information security. Decides what gets redacted.
  Those `[REDACTED]` blocks have an author."* Farrow is posted in the records office, which is where
  SCP-9191's unattributed reports appear, and is the person who **cannot produce Log 47721**. The
  antagonist's attack surface now has a human standing on it.
- **Dr. Lindqvist, Paratherapist.** ● canon term — *"therapy for people who have seen things."* Posted
  in the activities room, which the design doc describes as *"the room that offsets what the field
  does to people"*.

  ⚠️ **This entry originally claimed operatives carry FEAR between expeditions. They did not** —
  `Drives` is run-scoped and absent from `SaveGame`, so fear reset to zero every expedition and there
  was nothing here for a paratherapist to treat. Only *beliefs* persisted. The claim was corrected on
  2026-08-02 by making it true rather than by deleting it: `knowledge::Knowledge::strain` persists in
  `SquadKnowledge`, accrues per expedition survived, raises the operative's **FEAR floor** in the
  field, and is spent in her room. It is the design doc §6.2 counter-pressure to veteran lock-in —
  *"fear accumulates alongside knowledge and a veteran is the most afraid"* — and her second verb, a
  **deep debrief**, is §3.4's trade as a button: she can talk an operative down from a `Lethal`
  belief, and it costs them the belief.

⚠️ **There is no Site Director in this roster, and there must not be.** The design doc §6 settles that
**the player is the Director of Site-67**. A Site Director NPC would be the player's own chair with
someone else in it.

### Support staff are not optional texture

○ Duyen is Level 0 and works in the galley, and the taxonomy doc is explicit about why that entry
exists: *"Do not skip these. They are what make a site feel like a workplace rather than a
dungeon."* Canon runs a motor pool, a cafeteria, HR and Accounting and plays all of them straight.

---

## 4. D-Class at Site-67

● Canon: expendable personnel drawn from prison populations, used for extremely hazardous anomalies,
and — per `2026-07-12-scp-color-language.md` §4 — **orange** is one of the few colours in the mythos
that genuinely carries meaning.

The Director's call is that they are **visible but segregated**: their own block, reachable only by
walking the length of the containment wing and out through Monitoring. The escort route *is* the
segregation, and it is the canonically right adjacency — D-Class are walked to test chambers, not
housed among staff. The living wing sits at the opposite corner of the building.

**Why they are in the game at all.** The taxonomy doc's load-bearing list has one entry that is not a
fact but a posture: *"D-Class are expendable; the Foundation is morally grey."* Making the Foundation
unambiguously heroic is a named amateur tell. This game currently has no image that argues against
heroism — you contain rather than kill, you bring specimens home, your Council is stern but never
relieves you. A block of people in orange, escorted past the cells by a guard, is the cheapest honest
counterweight available, and it costs no mechanic to say it.

⚠️ **Open, and the Director's to settle: the D-Block has no verb yet.** The repo's named process risk
is shipping a room with no verb in it. Walking past something that means something is arguably the
containment wing's verb already, but that should be a decision rather than an oversight.

⚠️ **Asset gap.** None of the eleven characters in `scp_characters` is a D-Class. CIPHER is a
researcher with four duty outfits; `fieldop`'s jumpsuit is a field uniform. This needs one new
archetype in the asset project — the garment library already has the jumpsuit; the work is a config
and an orange material.

---

## 5. What this fiction must not do

Carried forward from `2026-07-12-scp-role-taxonomy.md` §14 and §13, filtered to the ones this
document could plausibly break:

- **Do not name the anomaly that closed the Site.** `SCP-███` stays redacted in every readable.
- **Do not collapse the three combat tiers.** They are on site partly to prevent this.
- **Do not treat clearance as rank** or Level 5 as an achievement.
- **Do not make the Foundation heroic.** §4 exists for this reason.
- **Do not use GOC vocabulary** (Type Green, Type Blue) as Foundation speech. It belongs to the rival
  organisation that kills what we would cage.
- **Do not import the Montreal proposal** (§1). It is a forum draft, not canon.
- **Remember most anomalies are boring.** The mundane texture is half the charm, which is the whole
  argument for Duyen in the galley.

---

## 6. What this changes elsewhere

- `docs/2026-07-26-site-hub-and-operative-knowledge.md` **§2.3 is amended** in the same commit: the
  Site was not merely *chosen*, it was *reopened*.
- `BACKLOG.md` §7's standing note — *re-verify SCP canon before shipping copy* — is satisfied for the
  quotation in §1 as of **2026-08-02** by direct fetch of the archived facilities page. It still
  applies to any further canon quoted into shipped copy.
- The staff roster in §3 is the source for `assets/site/staff.ron`. ⚠️ **That file becomes
  append-only** once anything indexes it — reordering it silently rewrites who is who.

---

## 7. References

All SCP Wiki content is **CC BY-SA 3.0**; attribution required, share-alike is viral for derived
content.

| Used for | URL | Fetched |
|---|---|---|
| **The Site-67 entry (§1)** | `https://scp-wiki.wikidot.com/archived:secure-facilities-locations` | 2026-08-02 |
| Clearance levels, personnel classes, the official eight titles (§2, §3) | `https://scp-wiki.wikidot.com/security-clearance-levels` | 2026-08-02 |
| Departments, RAISA (§3) | `https://scp-wiki.wikidot.com/departments` | via role taxonomy |
| Paratherapist, specialist tracks (§3) | `https://scp-wiki.wikidot.com/personnel-and-character-dossier` | 2026-08-02 |
| Licensing | `https://scp-wiki.wikidot.com/licensing-guide` | — |

Not used, recorded so nobody re-finds it and assumes it is canon: the *Protected Site 67 —
Extrasolar Observation and Detection Site* critique-forum draft.
