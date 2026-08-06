# Anim Bench — Ranked Feature Priorities

**Date:** 2026-08-06
**Scope:** `crates/emerge-mapper/src/anim_tab.rs`, the read-only animation bench
**Question:** what should we add, in order of helpfulness, to make our devs produce great animation content faster?

---

## 0. Honesty about the evidence

There is no literature on "animation verification benches" specifically. What exists is three separate bodies of work that bear on the problem, and I searched all three:

| Source | What I searched | What it gave |
|---|---|---|
| **home-still corpus** (semantic search over the local library) | animation authoring workflow, foot skating / phase / stride, blend trees & state machines, asset pipeline validation, contact detection, phase manifolds, creativity-support tooling, hot reload | *Game Engine Architecture* (Gregory), *Game AI Pro 2* ch. 12, Kovar's Motion Graphs, Holden's motion manifolds, WalkTheDog / MotionPyramid phase manifolds, GANimator, Skeleton-Aware Networks, Ciccone's motion-cycle authoring, PCG error-checking, alert-fatigue research |
| **Web / industry practice** | GDC animation tooling, Unreal Animation Insights & Rewind Debugger, sync groups & markers, automated foot-sync-marker generation, asset validators, motion matching adoption | Concrete engine-level precedents for almost everything below |
| **Your own doc** | — | The failure model, the architecture constraints, the two stated limitations |

**The ranking below is my synthesis, not a measured result.** No study compares these features head-to-head. Where I rank something high it is because (a) it attacks the failure you named as the reason the tab exists, (b) it has a working precedent in a shipping engine, or (c) both. Where I'm less sure, I say so.

---

## 1. The framing that drives the ranking

Your doc states the thesis cleanly: *"A manifest agreeing with itself proves nothing."* The bench exists because `rigs.ron` and the GLB can drift apart, and only the GLB is ground truth.

But look at the current loop from the animator's side:

```
re-export GLB
  → remember the bench exists
  → open editor, press 3
  → find the rig, read four findings
  → open rigs.ron in an editor
  → hand-transcribe three floats
  → re-open the bench to confirm
```

Every step after "re-export GLB" is overhead that exists *only because the numbers are declared rather than derived.* The bench is a very good detector for a problem that mostly shouldn't be detectable.

Two industry sources say the same thing from different angles. *Game Engine Architecture* frames the goal of data-driven animation tooling as short **round-trip iteration time** — the gap between making a change and seeing its effect — and notes that editors integrated with the asset database (UnrealEd is the canonical example) are what makes rapid iteration achievable. And in GDC's Animation Bootcamp roundtable, Simon Unger (ex-EA) puts it bluntly: *"Longer iteration times equals less animation quality."* Not "less quantity" — less **quality**, because animators stop iterating.

So the ranking is: **first shorten the loop, then sharpen the diagnostic, then broaden the coverage.**

---

## 2. Tier 1 — highest leverage

### 1. Write measured values back into `rigs.ron`, with a provenance stamp

**What:** an explicit "adopt measured values" action per slot / per rig. It writes `duration`, `phase_offset`, `cycle_distance`, plus a small provenance record — GLB content hash, clip count, tool version, timestamp.

**Why it's #1:** it deletes the hand-transcription step, which is the single largest chunk of the loop above. Your own doc already nominates it and notes it reuses the Tiles tab's persist pattern. The literature agrees but adds an important refinement.

**The refinement — this is the part worth getting right.** Do *not* simply auto-derive and drop the declared value. That would destroy the cross-check the tab exists to provide. Instead, change what the check *means*:

- **Today:** "does the human-typed number match the asset?" — a value comparison
- **Better:** "is the recorded measurement stale relative to the asset?" — a provenance comparison

With a content hash stored next to the numbers, the bench can say the thing that is actually true and actually actionable: *"this GLB changed since these numbers were measured."* That is a strictly stronger statement than "1.417s declared vs 1.402s measured," because it catches the case where a re-export changes something the four checks don't look at, and it never produces a false alarm from float drift.

Keep declared-as-override for the cases where intent beats measurement — `docs/artist_guide.md` admits its back/strafe numbers are rough, and there will be slots where the artist deliberately wants a number the asset doesn't support. Show both: `1.388 declared (override) · 1.373 measured`.

**Precedent:** Unreal's **Animation Modifiers** are exactly this pattern — scripted passes that analyse a sequence and write derived data (curves, sync markers) back onto the asset, re-runnable on re-import. *Game Engine Architecture*'s treatment of the asset conditioning pipeline makes the same structural point: derived data belongs to the build, and interdependencies should tell you what to rebuild when a source asset changes.

**Cost:** moderate. Needs a `rigs.ron` writer that preserves comments and human notes (this matters — the Valkyrie's LEFTWARD note is load-bearing documentation), plus undo integration, which the tab currently and correctly doesn't have.

**What it does not solve:** nothing about whether the animation *looks* right.

---

### 2. Watch the GLB directory; re-measure on change

**What:** a file watcher on the asset paths for loaded rigs. On change, invalidate the per-selection cache and re-run the four checks. Surface a persistent badge — "3 rigs stale" — that survives tab switches.

**Why #2:** this is the cheapest possible reduction in round-trip time, and it converts the bench from *a tool you must remember to point at a rig* into *a thing that tells you.* Combined with #1, the whole loop becomes: re-export → glance at the bench → press adopt.

The lazy-load design you have (read on first entry, not startup) is right and should stay; watching is additive and only applies to already-loaded manifests.

**Cost:** low. `notify` crate, a debounce, and cache invalidation you already have the shape of.

**Caveat:** re-measurement runs FK over every keyframe. Debounce hard, and do it off the UI thread — a re-export that touches 16 GLBs shouldn't stall the editor.

---

### 3. Phase-aligned diagnostic plots

**What:** for the selected rig, a 2D plot with **phase (0..1) on the x-axis** and, per gait slot drawn at its declared `phase_offset`:

- foot-chain **height** vs phase (stance is the flat part)
- foot-chain **speed in the ground plane** vs phase (during stance this should be ≈ the negative of the character's forward speed; deviation *is* foot skate, made visible)
- root displacement vs phase (should be flat — check 3, but as a curve rather than a boolean)
- a top-down trace of net displacement per cycle (check 4, but showing *direction*, not just magnitude)

**Why #3:** your four checks answer *whether* the numbers agree. They cannot answer *where* they disagree, and for the failure mode your architecture actually produces, that gap matters a lot.

Your doc makes the key observation: because there are no transitions and the blender only moves weights along one shared phase, a wrong duration *"doesn't glitch, it skates."* Skating is a continuous, low-amplitude error distributed across the cycle. A scalar verdict ("off by 0.08s") tells you it exists. A foot-speed-vs-phase plot shows you it's concentrated in the second half of stance, which tells you whether the fix is the duration, the phase offset, or the source clip.

This also directly attacks the weakest of the four checks — *"a clip with no planted-foot stance says so as a note rather than guessing."* A foot-height curve turns that from an unresolvable note into something a human reads in half a second.

**And it resolves the Valkyrie case.** The note *"carries the body LEFTWARD — the asset names this clip strafe_r"* is doing a job that a top-down displacement arrow would do automatically, for every clip, without anyone having to write it down.

**Cost:** low-to-moderate, and notably **cheaper than a 3D preview** while being a better diagnostic for your specific failure modes. You already run FK over every keyframe — the curves are a by-product of work you're doing.

**Precedent:** Unreal's Animation Insights is a timeline of exactly this kind of per-frame curve data (notifies, curves, pose, blend weights), and the animator-facing framing there is identical: visualise per-frame data to find problem spots. The FootSyncMarkerGenerator plugin generates distance and velocity curves explicitly "for debugging/visualisation" alongside its markers.

---

## 3. Tier 2 — high value, moderate cost

### 4. Robust contact/anchor detection — and fail loudly when it can't run

**Problem:** you flagged it yourself. The FK checks anchor on nodes literally named `Root` and `foot_l`. A rig without those names *silently* gets no root-motion and no cycle-distance check. Silence looks like a pass. This is the same failure class as the empty rig list you already refuse to tolerate ("an empty list that looks like *this project has no rigs*").

**Fix, in three parts:**

1. **Make it loud.** If an anchor isn't found, emit a `Bad` or at minimum a distinct finding — *"no node named `foot_l`; cycle-distance and root-motion checks skipped"* — with the fix stated, per your own house rule about warnings.
2. **Make it configurable.** Per-rig `root_node` / `contact_chains` in `rigs.ron`, defaulting to the current names. Costs almost nothing and covers every non-conforming rig you'll ever add.
3. **Make it robust.** Replace "planted-foot stance" heuristics with velocity-threshold contact labelling: a joint is in contact on frame *t* when its FK velocity magnitude falls below ε. This is the standard formulation — GANimator uses precisely this (`L^tj = 1[‖FK_S([R,O])^tj‖₂ < ε]`) to label foot contacts across arbitrary creature skeletons, and Skeleton-Aware Networks make the structural argument for why end-effectors are the right anchor: skeletons that differ in joint count still share their end-effector set, and zero-velocity frames in the source should stay zero-velocity in the target — which is exactly the foot-sliding condition.

For discovering *which* joints to treat as contacts without names, the practical trick is: leaf joints of the kinematic tree whose FK velocity spends a meaningful fraction of the cycle near zero. That generalises to the crab.

**Cost:** low for parts 1–2, moderate for part 3.

---

### 5. Kill `FIGURINE_SCALE = 1.13`

**What:** per-rig scale, read from the manifest or measured from the mesh bounds, not a hardcoded copy of the squad's humanoid figurine scale.

**Why:** your doc's own assessment is correct — *"right for the humanoids, questionable for a crab"* — and correctly notes it doesn't bite today only because the non-humanoid rigs carry no gaits. That's a latent bug with a trigger condition: the first person who adds a gait to a non-humanoid rig gets a confidently wrong cycle-distance measurement, and the 20% tolerance is loose enough to swallow a wrong answer rather than reject it.

This is cheap, and "wrong and confident" is the worst output a verification instrument can produce.

**Cost:** trivial.

---

### 6. Project-wide check run, with CI parity

**What:** a "check all rigs" mode that runs the four checks across all 16 rigs and shows a summary — counts by severity, list of offending rig/slot pairs, jump-to-detail. Same code path as `rigs_match_assets.rs`, same verdicts, same wording.

**Why:** two reasons.

The **workflow** reason: today's per-selection, cached-on-select design means the only way to know the project is healthy is to click through 16 rigs. That's an audit nobody performs. NVIDIA's Asset Validator documentation frames the correct posture — run the validator frequently as you assemble and modify, treat it as a triage tool, and fix structural blockers first; maintaining quality is far cheaper than repairing a deeply broken hierarchy later.

The **debuggability** reason: when CI goes red, the developer's first move should be one keystroke that reproduces it locally with identical output. Sharing the code path is what guarantees that. This is the strongest argument for building it — it makes CI failures cheap instead of mysterious.

**Cost:** low, because the check logic already exists and already runs headless.

---

### 7. Show glTF clip names, and diff against last-known-good

**Two small things bundled:**

- **Clip names.** glTF animations carry an optional `name`. Rendering `3 - clip 11 (strafe_r)` costs nothing and makes the Valkyrie's backwards-naming note self-evidencing rather than folklore. It also makes check 1 ("the clip exists") far more useful — *"slot 4 names clip 14; the asset has 12 (`walk_f`, `walk_b`, …)"* tells you what to pick.
- **Diff against the stored fingerprint** (from #1). *"Clip list changed: `strafe_l` added at index 6, all indices after 5 shifted."* That is the actual root cause of the failure you built the tab to catch, stated causally. Right now the bench reports the symptom (index out of range) and leaves the diagnosis to the human.

**Cost:** low. Depends on #1's provenance record for the diff half.

---

## 4. Tier 3 — worth doing, but later or more speculatively

### 8. Staged preview — in this order: top-down trace, then 3D viewport

The Tiles tab already stages a mesh, so the renderer exists. But be deliberate about *why* you'd add it, because "no playback" is currently a defensible design choice, not an omission.

- **The high-value 80%** is a **top-down displacement trace** with a ground grid at the declared `cycle_distance` — direction, distance, and drift, visible instantly. This is a 2D drawing and belongs with #3.
- **A 3D viewport** adds real value in one specific mode that nothing else covers: **drive all resident clips simultaneously from a single scrubbable phase slider at their declared `phase_offset`s, with blend weights exposed.** That is a faithful simulation of what your runtime does — no transitions, everything resident, weights moving along one shared phase — and it's the only way to see whether the *set* reads correctly rather than whether each clip is individually valid. A conventional "play clip N" preview would be much less useful for your architecture.
- Unreal's Rewind Debugger is the industrial version of this idea: record a segment, scrub it, and inspect pose blends and per-node influence frame by frame; recorded gameplay is specifically valued because it preserves incorrect behaviour for collaborative debugging.

**Cost:** high for the viewport. Rank it here, not in Tier 1, because the plots in #3 catch your named failure modes more precisely and much more cheaply.

---

### 9. Derive `phase_offset` rather than measuring it by hand

Your `phase_offset` is a hand-authored scalar that says "where in the shared cycle does this clip's contact fall." The industry's name for this is **sync markers**, and the industry's answer is to generate them.

- Unreal's sync groups sync playback by *relative position between markers* rather than by scaled clip length, which handles the cases a single scalar handles badly: run and walk with different step counts, different stride lengths, and non-looping starts/stops.
- Generating them is a solved-enough problem. Open plugins detect foot contact via pelvis-crossing, velocity-curve, and trajectory-curvature-saliency detectors, then combine them by weighted voting and write sync markers automatically.
- Once you have contact detection from #4, deriving `phase_offset` is cross-correlation of contact events against a reference slot. That's a small amount of code on top of work you'd already have done.

**The research ceiling, for completeness and not for now:** learned phase manifolds (Starke et al.'s DeepPhase; WalkTheDog, Li et al. 2024, `10.1145/3641519.3657508`) extract a continuous per-frame phase variable in an unsupervised way, align clips of differing frequency, and — notably — align across *morphologies*, learning a shared manifold for a human and a dog without supervision. That is the principled version of the thing your doc dismisses as impossible today ("a scuttle and a chomp share no cycle, so there is nothing to phase-lock"). It's genuinely a research-grade dependency and I would not build it. Worth knowing the ceiling exists.

---

### 10. Tolerance policy: per-slot, explained, and tracked

The 20% cycle-distance tolerance is deliberately loose because the artist guide's numbers are rough — a good call, honestly reasoned. But loose global tolerances have a known decay path: they absorb real errors, and a check that never fires stops being read.

The alert-fatigue literature is instructive here even though it comes from clinical decision support. Retiring irrelevant alerts measurably reduced override rates (93% → 86%), and the factor most associated with inappropriate overrides was the volume of low-informativeness alerts arriving alongside. The mechanism transfers: a channel's credibility is a shared resource, and uninformative findings spend it.

Concretely: make tolerance per-slot with a documented default, show the tolerance inline alongside the pair you already print (`measures 1.373 m/cycle vs 1.388 declared, ±20%`), and — once #1 exists — tighten the default, because measured-and-adopted values should agree to float precision. The loose tolerance is compensating for hand-entry error that write-back eliminates.

---

## 5. What I would not build

| Thing | Why not |
|---|---|
| **Motion matching / learned motion matching** | Wrong shape for you. A production locomotion database is typically 200–600 clips; motion matching's known cost is that memory scales linearly with database size, and its whole premise is replacing explicit state machines with per-frame search. You have a handful of slots per rig, no transitions by design, and 16 rigs. It would solve a problem you don't have and add one you don't want. |
| **Full in-editor animation authoring** | Blender/Maya do this, and the research on simplified cycle authoring (Ciccone et al.'s performance-driven MoCurves) is delivered as a *Maya plugin* — i.e. even the people trying to simplify cycle authoring build into the DCC, not into the game editor. The bench's comparative advantage is being the verifier. |
| **RL/QD wiring** | Already correctly excluded by your documented exception — the animation layer is cosmetic and invisible to `snapshot_hash` by construction. |
| **Free-form editing of `rigs.ron` in the bench** | Even after #1 lands, keep writes to one explicit "adopt measured" action. The tab's read-only discipline is why it's trustworthy; a general editor makes it another surface that can be wrong. |

---

## 6. Suggested sequencing

| Phase | Items | Rationale |
|---|---|---|
| **1 — collapse the loop** | #2 (watcher), #5 (per-rig scale), #7a (clip names) | All cheap, all independent, all ship in days. #5 and #7a are pure bug/clarity fixes. |
| **2 — kill the transcription step** | #1 (write-back + provenance), #7b (fingerprint diff), plus undo integration | The big one. Do it after phase 1 so the watcher is already telling you when to press adopt. |
| **3 — sharpen the diagnostic** | #3 (phase plots), #4 (contact detection), #8a (top-down trace) | #4 unblocks the plots for non-humanoid rigs; do them together. |
| **4 — broaden coverage** | #6 (project-wide + CI parity), #10 (tolerance policy) | Tolerance tightening only makes sense once #1 removes hand-entry error. |
| **Later / optional** | #8b (3D phase-scrub viewport), #9 (derived phase offsets) | Both are real wins; neither is on the critical path. |

---

## 7. The one-line version

**The bench is an excellent detector for a problem that mostly shouldn't exist.** Make the numbers derived and provenance-stamped rather than declared and hand-transcribed (#1), tell the developer the moment an asset changes underneath them (#2), and when something does disagree, show them *where* in the cycle rather than *by how much* (#3). Everything else is refinement.

---

## Sources

**Local corpus (home-still):**

- Gregory, J. *Game Engine Architecture*, 3rd ed. (2018) — §12.10 animation state/blend-tree specification and the rapid-iteration goal; §15.4 round-trip iteration time and integrated asset tools; §7.2 asset conditioning pipeline, resource dependencies and build rules.
- *Game AI Pro 2*, ch. 12, "Separation of Concerns Architecture for AI and Animation" — animgraph complexity explosion; animation events as temporal annotations (foot contact periods); code/data dependency and its effect on iteration speed.
- Kovar, Gleicher & Pighin, "Motion Graphs" (SIGGRAPH 2002, `10.1145/566570.566605`) — clip annotation with constraint information ("left heel planted on these frames").
- Li, Starke, Ye & Sorkine-Hornung, "WalkTheDog: Cross-Morphology Motion Alignment via Phase Manifolds" (SIGGRAPH 2024, `10.1145/3641519.3657508`) — vector-quantised periodic autoencoder; phase linearity and amplitude constancy; unsupervised cross-morphology alignment; comparison to DeepPhase.
- "GANimator: Neural Motion Synthesis from a Single Sequence" — FK-velocity-threshold foot contact labelling, generalised across creature skeletons.
- Aberman et al., "Skeleton-Aware Networks for Deep Motion Retargeting" — end-effector loss and normalised velocity; zero-foot-velocity preservation as the anti-sliding condition.
- Ciccone et al. (ETH CGL, 2017) — motion cycle authoring; the cost of general-purpose DCC packages for what is only a few frames of repeated animation; implemented as a Maya plugin.
- Holden, Saito & Komura, "A Deep Learning Framework for Character Motion Synthesis and Editing" (`10.1145/2897824.2925975`) — manual preprocessing (segmentation, alignment, labelling) as the dominant cost in data-driven motion pipelines.
- Deolikar & Lupiani, *Procedural Content Generation for Games* (Apress, 2025) — error checking of generated content; constructive vs. post-generation vs. generate-and-test.
- Ancker et al., "Effects of workload, work complexity, and repeated alerts on alert fatigue" (`10.1186/s12911-017-0430-8`) — override rates after retiring uninformative alerts; low-informativeness alert volume as the driver of inappropriate overrides.

**Industry practice (web):**

- Unreal Engine docs — Animation Insights, Rewind Debugger, Pose Watching; Animation Sync Groups and Sync Markers.
- Unreal Animation Modifiers — automated foot sync markers and foot position curves (A Clockwork Berry; `gportelli/FootSyncMarkers`); `HaJH/FootSyncMarkerGenerator` (pelvis-crossing, velocity-curve, saliency, composite detectors).
- Game Developer, "Game Animation Bootcamp: An expert roundtable Q&A" — iteration time vs. animation quality.
- NVIDIA Asset Validator documentation — validator-as-triage-tool posture.
- Büttner & Clavet, "Motion Matching: The Road to Next Gen Animation" (GDC 2015); Holden et al., "Learned Motion Matching" (`10.1145/3386569.3392440`) — database scale and linear memory cost.