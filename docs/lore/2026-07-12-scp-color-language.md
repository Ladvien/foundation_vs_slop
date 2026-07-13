# SCP Color Language
### Reference for agents designing visual identity, factions, threat readouts, and damage types

**Status:** Research reference. Not a design decision record.
**Companion to:** `2026-07-12-scp-role-taxonomy.md`, `2026-07-12-scp-equipment-taxonomy.md`
**Canon posture:** Tight, not religious.

---

## 0. The answer, up front

**The Foundation has no color language. That is the point.**

Its visual identity is deliberately anti-color: black redaction bars, manila folders, clinical white, grayscale photographs, the black-and-white containment sigil. The aesthetic *is* bureaucratic suppression. Adding a Foundation house palette would be the single most tone-deaf thing you could do.

**Color in the SCP universe belongs to everyone else.** The rival that kills instead of contains has an elaborate color taxonomy. The prisoners wear orange. The anomalies are the things with hue.

That asymmetry is your entire visual thesis, and it's free:

> **Grayscale is contained. Color is anomalous. Saturation is a readout.**

In a multiversal game this is enormous. How *colorful* a universe looks can be a diegetic instrument — a Hume meter you don't have to explain. Baseline reality is desaturated. Deviation blooms.

There are exactly **three** real color/light systems in canon. Everything else is per-faction aesthetics and per-author invention.

---

## 1. System One — the Foundation's ACS: it isn't color, it's **luminosity**

The Anomaly Classification System's **Disruption Class** is the Foundation's only official scale in this territory, and the wiki's own footnote is explicit: the names are all degrees of **light or illumination**, and the higher the class, the farther the light spreads from its source.

| Class | Etymology | Meaning | Scope |
|---|---|---|---|
| **Dark** | (baseline — chosen the same way "Safe" was) | Inert unless interacted with. Trivial to clean up. | One person |
| **Vlam** | Dutch: *flame* — a candle flame | Affects a handful of people. Relatively simple to neutralize. | A room |
| **Keneq** | Pacific Gulf Yupik: *fire* — a campfire | Spreads fast enough to worry. Moderately difficult to counter. | A city |
| **Ekhi** | Basque: *sun* | Swift, hard to manage. Quite difficult to neutralize. | A country → the world |
| **Amida** | The Buddha of Eternal Light | The Foundation is effectively **declaring war** on it. | The world, possibly the universe |

**This is the best thing in this document.** Your threat scale is *how much light is escaping.* Containment is darkness. A breach is a fire getting brighter. The scale runs candle → campfire → sun → eternal light.

You do not need to invent a threat UI. Canon built you one out of luminance, and luminance is something a game engine already understands.

**Risk Class** (the other new ACS axis — danger to an individual, as opposed to danger to the Veil) is a conventional five-step: **Notice → Caution → Warning → Danger → Critical**. Standard hazard-signage semantics; the community renders it green→yellow→orange→red→black. That one is ordinary. The luminosity scale is not.

⚠ **Adoption caveat.** ACS is official and used on many newer articles, but it is **contested and not universal.** Some respected authors reject it; the Russian and French branches use their own systems. Do not present it as gospel canon. As a *game HUD*, though, it's excellent — and the Disruption/luminosity idea works even if you drop the rest.

> **Canon inconsistency worth knowing:** the ACS guide renumbers clearance levels (Level 1 Unrestricted → Level 6 "Cosmic Top Secret") in a way that **conflicts** with the `security-clearance-levels` page (Level 0 FOUO → Level 5 Thaumiel). Canon has not reconciled these. Pick one. I'd pick the older page — it's the more widely used.

---

## 2. System Two — the GOC's Type designations: **the real color system**

The Global Occult Coalition — the UN-backed rival that destroys what you would contain — classifies anomalous humans by **color codeword**. This is canon, primary-sourced, and actively maintained (last edited June 2026).

**It is not Foundation terminology.** A Foundation character using it sounds like they've been reading enemy manuals. *Which may be exactly what you want.*

### The Types

| Type | Entity | Notes |
|---|---|---|
| **Green** | **Reality Bender** | The famous one. See the progression below. |
| **Blue** | **Thaumatologist** (wizard, witch, sorcerer) | Learned magic |
| **Magenta** | **Psionic** | ← **This is your party member.** |
| **Yellow** | **Polymorph** (shapeshifter) | |
| **Red** | **Regenerator** | |
| **Gray** | **Post-Mortem Reanimation** | Zombies |
| **Cyan** | Spectres and apparitions | |
| **Pink** | Human hybrids | |
| **Black** | Demigod / deity | ○ secondary sources |
| **White** | Congenital anomalous deformity | ○ secondary sources |
| **Beige** | Anomalous dismembered body parts | ○ secondary sources |

### The system is **compositional** — and that's the gift

Hybrids get hyphenated. Canon examples:

- **Yellow-Silver** — a polymorph whose condition is contagious via saliva or broken skin *(i.e. a werewolf)*
- **Yellow-Red** — a polymorph who also regenerates
- **Yellow-Grey** — a polymorph who gained the ability *after dying and reanimating*

**Two colors, one entity.** That is a monster-composition system handed to you fully formed. Design one enemy per color, then let them cross.

### Type Green: the four-phase corruption arc

Canon states that ~99% of reality benders follow this sequence as their power grows:

| Phase | Name | GOC posture |
|---|---|---|
| 1 | **Denial** — refuses to accept what they can do; may self-suppress and stop here | — |
| 2 | **Experimentation** — testing limits, either methodically or in sudden jumps | — |
| 3 | **Stability** — full control; *chooses* not to use it; tries to live normally | Threat Level 1: **monitor, do not engage** |
| 4 | **The Child-God** — obsession, collapsing empathy, megalomania, using people | Threat Level 5: **eliminate immediately** |

That is a **corruption meter with a canonical failure state**, and the tragedy is baked in: Phase 3 is a person choosing to be good, and canon says most of them don't stay there.

### Type Magenta: the psionic classes — **named for moon phases**

| Class | Name | Range | Capability |
|---|---|---|---|
| I | **Mlađak** (new moon) | Immediate vicinity | Usually unaware they're doing it. Often misdiagnosed as mental illness. |
| II | **Padajući** (waning) | A few meters, needs line of sight | Surface thoughts, reflexes. Most materials block them. |
| III | **Rastući** (waxing) | ~2 km, blocked by ~30 cm of material | Multiple targets, hallucinations, crude direct control. **Death releases a psionic "backlash" across their whole area of influence.** |
| IV | **Uštap** (full moon) | ~20 km, **pierces almost all material** | Only two ever confirmed. Wears other people as extensions of their subconscious. **Cannot be rendered unconscious.** Death saturates the region for *months*. |

Note what just happened: **the GOC's psionic scale is also a light/phase scale.** New moon → full moon. The Foundation's threat scale is candle → sun. Two organizations, two taxonomies, and both of them reached for *how much light is in the sky.*

That is not a coincidence you need to explain. It's a motif you should exploit.

### "Code Magenta" — steal this immediately

The GOC's field protocol for an operative who suspects they've been psionically influenced begins: **write your serial number, your Overseer's contact details, and the words "Code Magenta" on your wrist** — because all of that is information the entity is likely to have already altered.

Then: clear your head. Don't dwell on thoughts, they may not be yours. Get rid of your weapons. Don't drive. Don't isolate — go somewhere public and crowded, because it will attack you where no one is watching.

**Ink on skin outranks memory.** That is a horror mechanic, a UI element, and a scene, all at once.

---

## 3. System Three — the GOC's **color-coded loadout**

The Types are matched by color-coded ammunition and color-coded suits. This is a **damage-type / resistance matrix**, and canon even supplies the percentages.

### Ammunition

| Round | Composition | Used against |
|---|---|---|
| **Ochre Bullets** | Cold iron | **Type Blue** — standard anti-thaumaturge |
| **Malachite Bullets** | Mk V "Hopeless Alderwood Iron" — bronze jacket, Alderwood Iron core | **Type Blue**, Response Level 4+ |
| **Black Bullet** | The GOC's current standard | General |
| **MkIII MPHDCA** | Predecessor to the Black Bullet — silver jacket, osmium core, incendiary | **True Polymorphs** |
| **Fuchsia Bullets** | Mk IV — ordinary rounds etched with runes, glyphs, sigils by a thaumatologist ○ | Type-dependent (the runes decide) |
| **TICK rounds** | *Tactical Interrupter of Cognition and Kinetics.* Not a bullet — a device that unfolds mid-flight, hooks into tissue, and delivers continuous shock. Carries a transmitter that can **whitelist friendly targets.** | **Type Magenta** |

**Why TICK is non-lethal is the whole point.** You use it on psionics because *your own squad may be turned against you*, and you need a round that can be fired at a teammate. The whitelist transmitter exists because the GOC expects friendly fire to be the normal case.

That is a canon justification for a "subdue, don't kill" weapon and it comes with built-in dread.

### Suits — an escalation ladder

| Suit | Posture |
|---|---|
| **Grey** | Covert. Nobody sees you. |
| **Bronze** | Anti-thaumaturgic — deployed against Type Blues |
| **White** | Heavy engagement. Strike Team 3399 ("Nil Nisi Bonum") fields modified White Suits treated with a substance that inhibits psionic emission — and rotates its operatives out regularly, because wearing it damages the mind. |
| **Orange** | **Rarely approved.** Only when the Overseer judges that concealment can *still somehow* be maintained. |

**The color of the suit tells you how far things have gone.** Grey means they don't want you to know they're here. Orange means they've stopped caring.

Give the player one look at an orange suit and they'll understand the stakes without a line of dialogue.

### Canonical resistance tables

Canon publishes actual efficacy percentages. Use them directly.

**Type Yellow (Polymorph):**

| Method | Efficacy |
|---|---|
| Destruction of head or heart | 99% |
| Extreme heat or cold | 95% |
| Cold iron | 88% |
| Standard kinetic trauma | 86% |
| Silver | 55% |
| Non-natural metals (e.g. technetium) | 18% |
| Non-silver coinage metals (copper, gold) | 17% |
| Non-silver precious metals (palladium, platinum) | 8% |
| Bones/blood/skin of the form they mimic | 2% |
| Wolfsbane | 1% |

**Type Gray (Reanimated):**

| Method | Efficacy |
|---|---|
| Immolation | 99.9% |
| Destruction of brain | 72% |
| Bodily trauma | 54% |
| Toxins | 3% |
| Other | 2% |

Note the design intelligence here: **silver only works 55% of the time.** Folklore is *mostly wrong*, and the manual says so with a number. Fire and decapitation beat every mystical remedy. That's the Foundation-universe worldview in a table — the occult is real, and it is *less effective than a flamethrower.*

**Type Blue vulnerabilities** (no percentages given, but mechanically rich):
- **Iron and silver** redirect and negate thaumaturgy. Alloys (steel, sterling, electrum) work, but worse than pure metal.
- **Locomotion** — many Blues are anchored to a place. Force them to move. Dragging a rural thaumaturge into a city can *kill them*, because urban aetheric flux is polluted and erratic.
- **Sensory Occlusion** — a straitjacket and a gag defeats any Blue who casts by speech or gesture. Not the ones who cast by *wishing*.

Thaumaturgic "Performances" — the input method — are: **Leitmotif** (speech/song), **Mudra** (gesture), **Runic** (symbols), **Wish** (pure mental effort), **Ritual** (a combination). Your counter depends on which. That is a rock-paper-scissors system, canon, ready to go.

---

## 4. D-Class orange

The one color every SCP fan will recognize on sight.

D-Class wear **orange jumpsuits**. It is prison-issue, because they *are* prisoners. It is the Foundation's single most legible visual signal, and what it signals is: *this person is expendable, and we dressed them so you'd know at a glance.*

Note the collision. **Orange is also the GOC's stop-hiding suit.** Two organizations, one color, two meanings — expendability and total escalation. If that resonance is useful, use it. If not, ignore it. But know it's there.

⚠ **Keycard colors are game-canon, not wiki-canon.** The color-coded keycard tiers everyone remembers come from *SCP – Containment Breach* and *Secret Laboratory*, not the wiki. You're free to build your own; you're also free to honor the games' convention, which players will read instantly. Just don't cite it as lore.

---

## 5. Faction palettes

Mostly ○ — emergent from the wiki's CSS theme ecosystem and per-article art rather than stated canon. **Treat as strong convention, not law.** Every one of these is yours to reinterpret.

| Faction | Palette | Rationale |
|---|---|---|
| **The Foundation** | Grayscale. Black redaction. Manila. Clinical white. Institutional beige. | *Anti-aesthetic.* The absence is the identity. |
| **GOC / UNGOC** | **UN blue** — they field a specialized version of the UN's blue helmets. Plus the suit ladder (grey/bronze/white/orange). | They're a legitimate international body and they want you to know it. |
| **Chaos Insurgency** | Black, dark red. A broken Foundation sigil. | Defectors. Their brand is *your brand, damaged*. |
| **Church of the Broken God — Cogwork Orthodoxy** | **Brass, bronze, gold, oil.** Clockwork. Steam. | "Tickers." Analog, mechanical, anti-digital. |
| **Church of the Broken God — Maxwellism** | **Electric blue, neon, screen-glow.** | "Hummers." Networked, cybernetic. The schism *is* a palette war. |
| **Sarkicism / Nälkä** | **Crimson, viscera, bone, gold ornament.** Wet. | Flesh-craft. The organic against the mechanical. |
| **Marshall, Carter & Dark** | **Black and gold.** Victorian. Serif. Expensive paper. | They wield money and lawyers. The palette is a boast. |
| **Are We Cool Yet?** | Deliberately incoherent — neon, spray paint, gallery white. | Anartists. A consistent palette would be a failure of the brief. |
| **Dr. Wondertainment** | Primary colors. Toy plastic. Saturated to the point of menace. | The cheerfulness is the threat. |
| **Serpent's Hand** | Ink, parchment, deep green. | Books and snakes. |
| **The Fifth Church** | Starfield black, cosmic violet. | Fifthism. |
| **Anderson Robotics** | Clean tech-startup white and chrome. | Paratech, retailed. |
| **Herman Fuller's Circus** | Circus red-and-white, gaslight, greasepaint. | |

**The CotBG schism is the strongest visual idea on this list.** Two sects of the same religion, one brass and steam, one neon and network, at war over whether God is analog or digital. If you need a faction and you want it to read instantly at a distance, that's the one.

---

## 6. Recommended color system for `foundation_vs_slop`

Synthesizing all of the above into a proposal. **✦ Invented — this is design, not canon.**

### The core rule

> **Desaturation = reality. Saturation = anomaly.**

Bind the global saturation of the render to the **Hume level**. Baseline reality is near-monochrome and slightly warm — like a photocopied document. As Humes drop, color bleeds in.

This gives you, for free:
- A Hume meter nobody needs to be taught to read
- A **Scranton Reality Anchor** that visibly *drains the world of color* when deployed — comforting and awful in equal measure
- A **Scranton-Eamon Reality Sink** that does the opposite, and the player will feel it in their gut before they read the tooltip
- A multiversal traversal readout: **every universe has a color temperature.** Baseline is gray. A Branch universe is tinted. A Floater is *loud.*

### The threat readout

Use the ACS **luminosity** scale, not a color scale. Containment = darkness. The HUD indicator for a live anomaly is *how much light is getting out.*

Dark → Vlam (candle) → Keneq (campfire) → Ekhi (sun) → Amida (the screen cannot hold it).

An Amida-class event should be the only time in the game the screen goes **white**.

### Damage types

Adopt the GOC matrix directly. It's canon, it's numeric, and it's a working rock-paper-scissors:

| Enemy type | Weak to | Note |
|---|---|---|
| Type Blue (thaumaturge) | Cold iron (Ochre), Alderwood iron (Malachite); gagging; forcing relocation | Counter depends on their *Performance* — speech, gesture, symbol, or pure wish |
| Type Magenta (psionic) | TICK rounds (non-lethal, whitelisted) | **Lethal weapons are a liability — your squad can be turned** |
| Type Yellow (polymorph) | Head/heart 99%, fire/cold 95%, cold iron 88%. **Silver only 55%.** | Folklore is mostly wrong |
| Type Gray (reanimated) | Immolation 99.9%. Toxins 3%. | Burn it |
| Type Red (regenerator) | Cold iron poisoning; submersion; incineration *in perpetua* | You don't kill them, you make stopping expensive |
| Type Green (reality bender) | Surprise, speed, one shot. They can't predict the future. | See the four-phase arc |

### The party

**Your psionic is a Type Magenta.**

The GOC has a field manual on killing them. The Foundation stocks electrum-lined helmets and hollow-cavity firearms designed for maximum cerebral damage. Your combat specialist could be carrying TICK rounds *right now*, and TICK rounds have a whitelist function, and someone had to decide who was on it.

Give the psionic a color the party can see. Make it magenta. Let it bloom when they push.

### The slop

If the antagonist is semiotic decay — meaning coming loose from things — then the visual grammar writes itself and it is the **inverse** of everything above:

- Not saturation. Not luminance. **Mush.** Colors that are *nearly* right. Palettes with no contrast. Gradients where there should be edges.
- The **Gat-Hayes Semantic Stabilization Device** (SCP-6254) restores edges. It doesn't add color; it *reasserts boundaries*.
- Slop should be the one thing in the game the Kant Counter reads as **perfectly normal.** It isn't a reality violation. That's what makes it worse.

---

## 7. Design guardrails

**Do:**
- Keep the Foundation grayscale. The restraint is the brand.
- Use luminosity, not hue, for threat.
- Attribute the color-type taxonomy to the **GOC**, in-fiction. A Foundation character who says "Type Green" should be signalling something about themselves.
- Let colors *compose* (Yellow-Silver, Yellow-Red). Canon does.

**Don't:**
- Give the Foundation a house palette. It doesn't have one and shouldn't.
- Use "Type Green/Blue" as Foundation vocabulary. **Common amateur tell.** The Foundation says "reality bender" and "thaumaturge."
- Treat ACS as universal canon. It's contested.
- Cite keycard colors as lore. That's the games, not the wiki.
- Make color mean *danger*. In this setting color means **deviation**, and the two are not the same. A Safe-class object can be gloriously colorful. A Keter can be beige.

---

## 8. References

| Topic | URL |
|---|---|
| **ACS Guide** (Disruption = luminosity; Risk classes) | https://scp-wiki.wikidot.com/anomaly-classification-system-guide |
| ACS Component Bar (the icons and CSS) | https://scp-wiki.wikidot.com/component:anomaly-class-bar |
| **GOC PHYSICS Field Manual 13** (the Type system, Magenta classes, Green phases, resistance tables) — *by DrClef* | https://scp-wiki.wikidot.com/goc-supplemental-humanoid-guide |
| GOC Equipment and Gear (suits, ammunition) | https://scp-wiki.wikidot.com/goc-supplemental-equipment |
| GOC Threat Entity Database | https://scp-wiki.wikidot.com/goc-supplemental-threat-entities |
| GOC Hub | https://scp-wiki.wikidot.com/goc-hub-page |
| Object Classes | https://scp-wiki.wikidot.com/object-classes |
| Security Clearance Levels | https://scp-wiki.wikidot.com/security-clearance-levels |
| SCP-6254 "OBJECT" (Gat-Hayes Semantic Stabilization Device) | https://scp-wiki.wikidot.com/scp-6254 |
| Licensing Guide | https://scp-wiki.wikidot.com/licensing-guide |

**Source-quality note.** §1–§4 are from primary `scp-wiki.wikidot.com` pages. §5 (faction palettes) is largely **convention, not stated canon** — synthesized from the wiki's CSS theme ecosystem and article art. Type Black / White / Beige come only from secondary fan wikis and should be verified before shipping. §6 is invention.

---

## 9. Open questions

1. **Is saturation the Hume meter?** (§6) If yes, the Scranton Reality Anchor visibly drains the world of color when you deploy it — the safety device that makes everything gray. That's a thesis statement, not a mechanic.
2. **What's on the TICK whitelist?** If your combat specialist carries anti-psionic rounds with a friendly-fire whitelist, then somebody filled out that whitelist, and either the psionic is on it or they aren't.
3. **Does the game ever go white?** Reserve Amida. Use it once.
