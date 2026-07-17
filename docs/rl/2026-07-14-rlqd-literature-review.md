# RL/QD Optimization of *Foundation vs. Slop* — Guiding Literature Review

*Created 2026-07-14. This is the shared research spine for the RL + Quality-Diversity optimization
effort. Implementation agents should read this first. Wherever a paper informs code, leave its DOI in a
`//` comment (per CLAUDE.md).*

---

## 1. Purpose & how to use this document

This grounds the game's existing simulation systems in their source literature, selects and justifies
computational **proxies** for experience/playability/fun, surveys the **QD and RL algorithms** the
optimizer uses or should adopt, and maps each body of work to a concrete **gap/phase** in the roadmap
(§9). Citation counts referenced during research are indicative only. ~70 sources; DOIs/arXiv ids inline
in the reference list (§8).

## 2. What already exists (the baseline to improve on)

`src/squad_ai/` implements MAP-Elites (batch emitter), CMA-ME, POET, neuroevolution, an island-model search
(`cargo train`), 6 genome populations decoded onto `assets/config/config.ron`, and a headless deterministic
harness (`src/sim_harness.rs`). Its fitness stack is **witnessed learnable-surprise** `W·S·L`
(`surprise.rs`) + **human-interest** proxies (`interest.rs`: suspense / outcome-surprise / effectance),
gated by a `minimal_criterion`.

**Identified gaps** (the roadmap closes these):
- **G0 / G0b / G0c — ALL FIXED (2026-07-16). The gap is CLOSED.** *Search rollouts were not reproducible* —
  identical evaluations scored ~6% apart, so every search below was optimizing partly-noisy fitness.
  `replay::search_rollouts_are_reproducible_under_load` is now green on **both** held-in seeds: 12 rollouts ×
  7200 ticks × 2 worlds, under CPU load, all bit-identical. **Archives are trustworthy for `train apply`
  again.** Three distinct root causes:
  - **G0** — `laser::fire_laser` drew aim-scatter from a **shared stream in raw ECS query order**, so two
    units firing on one tick could swap cones and send a bolt at a different hostile.
  - **G0b** — `config.ron` held a machine-baked levels elite instead of the authored level, so the archive
    came back empty and `search_parallel` never reached its real assertion.
  - **G0c** — **`GibKey` was derived from the death origin position**, so it could not break the position tie
    it existed to break: two creatures dying on one coordinate minted identical keys,
    `crab::assign_meat_targets`' sort tied, and crabs committed to different meat chunks per run. Crabs die
    on bit-identical coordinates routinely (`clamp_to_patch` pins them to the same float), which is why
    `0xA11CE` (74 kills) diverged and `0x5C09191` (47) did not. Fixed with a monotonic `GibSeq` mixed into
    the key, made deterministic by sorting the `GoreQueue` canonically at its single consumer.
  Ten order-dependence bugs of this class were fixed in total (`fire_laser`, `update_lasers` incl. a
  `LastAttacker` last-writer-wins into instakill targeting, the ORCA tiebreak, `almond_water_effect`,
  `smiley_defense`'s lethal cull pick, `nest_reproduce`'s shared spawn seq, `crab_jump`'s landing bite,
  `light`'s cone compose, the manca swarm hash, and `GibKey`).
  **The durable outcome is the enforcement, not the fixes.** This class rested on ~dozens of query-iterating
  sites each remembering a stable total order, enforced only by comments — and four sites *documented the
  exact trap they fell into*. It is now mechanical: `sort_total!` panics on a tied key naming the site,
  `util::sort_value_canonical` states the interchangeable-ties claim, and `tests/determinism_lint.rs` fails
  the hard gate on any unannotated sort. The lint found G0c in **one second** after a whole session of
  bisecting failed to name it. Full diagnosis: `2026-07-16-search-rollout-nondeterminism.md`.
- **Process footgun — FIXED (2026-07-16):** `apply_archive` re-pinned the replay goldens automatically, at
  odds with TESTING.md (*"a deliberate, human-reviewed act — never auto-approve a diff"*), and `splice_block`
  wrote bare serialized RON, destroying every comment in the slices it rewrote. A bake could silently replace
  the authored level **and** move the ruler that would have caught it. Now: (1) `apply` **aborts** on golden
  drift reporting `old -> new` unless `--repin-goldens` is passed, and the unattended `cargo train all`
  callers never pass it; (2) `repin_replay` rewrites only the `const` declaration — it used to `str::replace`
  the hash across the whole file, rewriting the prose hashes in `replay.rs`'s incident log, i.e. eating its
  own audit trail; (3) `splice_block` now substitutes only the scalars that **changed**, comparing by parsed
  value so `seed: 0x5C09191` vs the serializer's `96506257` reads as equal and the line is never touched —
  every comment, hex literal and column of alignment survives (pinned by a byte-identical no-op round-trip
  over all 8 shipped slices, 1356 leaves). It **refuses** rather than guessing when an elite changes the
  block's *shape* (a dropped `room_types` entry, a vanished `Option`) or moves a field the authored file
  leaves at its serde default: there is no honest edit, because the prose around the old shape describes a
  design the elite no longer has, and a preserved-but-false comment is worse than a deleted one. `--dim
  levels` elites that drop a room type must ship through the runtime overlay (`FVS_LEVELS_ELITE`) instead.
- **G1** proxies are never calibrated to a human;
- **G2** replayability is not an objective;
- **G3** levels are scored statically, not by play;
- **G4** map size/complexity are fixed code constants;
- **G5** the mold ecosystem is essentially cosmetic;
- **G6** no SCP/Backrooms *tone* objective;
- **G7** the playtester is scripted (no learning agent);
- **G8** single scalar objective + hand-picked 2D descriptors.

## 3. Simulation foundations — grounding the systems we optimize

The game's mechanics are textbook emergent-systems models; optimization must respect their dynamics.
- **Flocking / swarm** (crabs): Reynolds boids [S1].
- **Local avoidance** (squad): RVO → ORCA [S2, S3].
- **Flow-field navigation**: Continuum Crowds / eikonal potential fields [S4].
- **Stigmergy / influence-map fields** (scent, threat, rally): ACO pheromone dynamics [S5] and the
  stigmergy concept [S6] — deposit/diffuse/evaporate is the field-update math.
- **WFC dungeon**: WFC-as-constraint-solving [S7] (the algorithm itself is Gumin's repo, non-academic).
- **Mold blooms**: reaction-diffusion / Turing patterns [S8] and Gray-Scott parameterization [S9].
- **Mold veins**: Physarum adaptive transport networks [S10] and their agent-based (chemotaxis-particle)
  approximation [S11] — bridges the stigmergy fields to the vein topology.
- **Emergence caution**: optimizing simulation rules silently changes which emergent (and exploit)
  behaviors are permitted [S12] — a direct argument for the RL exploit-finder (Phase 4).
- *Non-academic but load-bearing* (cite as practitioner refs): utility AI (Dave Mark, *Behavioral
  Mathematics*); Reynolds steering behaviors (GDC 1999); influence maps (*Game AI Pro*).

## 4. Selecting proxies for experience / playability / fun

**Framework.** Experience-Driven PCG [P1/X1] is the master seam: the optimizer maximizes a
*computational model of player experience* + a content-quality term. MDA [X16] supplies the vocabulary
(eight "kinds of fun") reminding us a single fun-scalar is insufficient → multi-objective. Validated
inventories PXI [X2] and GUESS [X3] give orthogonal dimensions and a survey instrument for the
human-audition gate (Phase 5).

**Chosen proxies and their grounding:**
- **Surprise / salience** = Bayesian surprise, KL(posterior‖prior) [X8]. Already the core of
  `surprise.rs`; keep, but compute against a belief model, not only a baseline brain.
- **Interest = learnable novelty**, i.e. reward the *derivative of learning progress*, decaying as
  content becomes predictable [X7] (Schmidhuber). Anti-repetition; grounds the curiosity term.
- **Interest vs. confusion** as a 2-axis appraisal (novel × comprehensible) [X10]; the arousal/Wundt
  lineage and its critique [X9] → keep interest legible, not just complex.
- **Flow / pacing** = challenge↔skill balance over time, GameFlow's eight elements [X4]; the flow-arc
  proxy (rising tension → climax → resolution) needs longer rollouts.
- **Difficulty / fairness** = DDA target band [X5, X6]; measured by a learned playtester's pass-rate and
  by exploitability (Phase 4).
- **Competence / effectance** = SDT mastery signal [X12]; Malone's challenge/curiosity/control [X11].
- **Dread / tension / discomfort** = productive discomfort has an inverted-U — bound it [X15];
  operational fear knobs (expression fidelity, audio desync) [X14]; uncanny valley [X13]. These are the
  SCP/Backrooms tone proxies (G6). *Uncanny* stays partly qualitative → human-audition gate.

## 5. Replayability & generator diversity

Ship a *generator*, not a point. Justification and tooling:
- **Expressive Range Analysis** [P6] — visualize generator output over a metric pair to expose
  variety/bias/coverage-holes; expanded, statistically-principled ERA [P7]; **how to choose the metric
  pair** [P8]. These define the replayability descriptors.
- **QD-for-PCG** [P9] and **constrained MAP-Elites** [P10] — illuminate a diverse archive of *playable*
  levels under hard validity constraints (connectivity/solvability). Ship a sampler over a
  quality-filtered archive region.
- **Controllable generators** [P5] and **latent-space illumination** [P11] — targeted diversity along
  designer axes rather than collapse to one optimum. The "don't overfit one artefact" rationale is
  distributed across [P5, P9, P6–P8].

## 6. Playtest-scored levels & structural search

Move level scoring from static structure to *simulated play* (G3):
- **PCGRL** [P4] — level design as an MDP; its narrow/turtle/wide action formulations are reusable for
  editing WFC tiles. Controllable PCGRL [P5] adds goal-conditioning.
- **SBPCG taxonomy** [P2] and **PCGML survey** [P3] situate representation/evaluation choices.
- **Automatic game design via simulated play** [P12] and continuous automated design [P13] justify an
  RL optimizer standing in for human playtesting.

## 7. QD & RL algorithms — what to keep, what to adopt

**Keep / already in-repo:** MAP-Elites [Q1], novelty search + NSLC [Q2, Q3], CMA-ME [Q7], POET [R13].

**Adopt (SOTA upgrades):**
- **CMA-MAE** [Q8] — fixes CMA-ME's failure modes; default continuous-QD algorithm.
- **MOME** [Q12] + **MOME-PGX** [Q13] — multi-objective QD (Pareto front per cell) → the vehicle for the
  multi-objective proxy set (G8); PGX for data efficiency.
- **CVT-MAP-Elites** [Q9] — scale archives past ~3–4 descriptor dims (needed once descriptors grow).
- **ME-MAP-Elites** [Q10] — bandit-allocated heterogeneous emitters; spend the scarce eval budget well.
- **AURORA** [Q14] / **VQ-Elites** [Q15] — *learned* behavioral descriptors (removes hand-picked 2D
  descriptors, G8); VQ-Elites also gives unsupervised diversity metrics.
- **DSA-ME / DSAGE** [Q16] — deep-surrogate-assisted MAP-Elites; cut the cost of expensive rollouts
  (essential once the search space grows in Phase 3). Foundational surveys: [Q4, Q5, Q6].

**Learned playtester (pure-Rust-first) & intrinsic motivation:**
- **NEAT** [R17] and **Evolution Strategies** [R18] — gradient-free, embarrassingly parallel,
  sparse-reward-tolerant → the pure-Rust neuroevolution substrate (matches the 24-thread box + the
  deterministic-core rollout model).
- **RL playtesting precedents**: Match-3 difficulty [R1], human-like playtesting [R2], "winning is not
  everything" skill×style [R3], synthetic vs human-like testers [R4], predicting difficulty+engagement
  ([R5]: use *best-case* runs, not average).
- **Exploit / coverage finding**: DRL testing [R6], curiosity-for-coverage [R7], 3D-game testing [R8].
  Intrinsic-reward toolbox: ICM [R9], **RND [R10]** (best default novelty for state-coverage in Rust),
  count-based via hashing [R11], **Go-Explore [R12]** (turns our deterministic-core snapshot/reset into
  an exploration superpower for reaching deep exploit states).
- **Open-endedness / world co-evolution**: POET [R13], minimal-criterion coevolution [R14], POET applied
  to games (PINSKY) [R15], **regret-based curricula (ACCEL) [R16]** (keep worlds at the difficulty
  frontier), emergent autocurricula / physics-exploit discovery [R19].

## 8. Reference list (grouped; DOIs inline)

### Sim foundations [S]
- [S1] Reynolds (1987). Flocks, Herds, and Schools. SIGGRAPH. `10.1145/37401.37406`.
- [S2] van den Berg, Lin & Manocha (2008). Reciprocal Velocity Obstacles. ICRA. `10.1109/ROBOT.2008.4543489`.
- [S3] van den Berg et al. (2011). Reciprocal n-Body Collision Avoidance (ORCA). ISRR. `10.1007/978-3-642-19457-3_1`.
- [S4] Treuille, Cooper & Popović (2006). Continuum Crowds. SIGGRAPH/TOG. `10.1145/1141911.1142008`.
- [S5] Dorigo, Maniezzo & Colorni (1996). Ant System (ACO). IEEE SMC-B. `10.1109/3477.484436`.
- [S6] Theraulaz & Bonabeau (1999). A Brief History of Stigmergy. Artificial Life. `10.1162/106454699568700`.
- [S7] Karth & Smith (2017). WaveFunctionCollapse is Constraint Solving in the Wild. FDG. `10.1145/3102071.3110566`.
- [S8] Turing (1952). The Chemical Basis of Morphogenesis. Phil. Trans. R. Soc. B. `10.1098/rstb.1952.0012`.
- [S9] Pearson (1993). Complex Patterns in a Simple System (Gray-Scott). Science. `10.1126/science.261.5118.189`.
- [S10] Tero et al. (2010). Rules for Biologically Inspired Adaptive Network Design (Physarum). Science. `10.1126/science.1177894`.
- [S11] Jones (2010). Pattern Formation in Approximations of Physarum Transport Networks. Artificial Life. `10.1162/artl.2010.16.2.16202`.
- [S12] Lehman, Clune, Misevic et al. (2020). The Surprising Creativity of Digital Evolution. Artificial Life. `10.1162/artl_a_00319` (arXiv:1803.03453).

### Quality-Diversity algorithms [Q]
- [Q1] Mouret & Clune (2015). Illuminating Search Spaces by Mapping Elites (MAP-Elites). arXiv:1504.04909.
- [Q2] Lehman & Stanley (2011). Abandoning Objectives: Evolution Through Novelty Search. Evol. Comput. `10.1162/evco_a_00025`.
- [Q3] Lehman & Stanley (2011). Evolving a Diversity of Virtual Creatures (NSLC). GECCO. `10.1145/2001576.2001606`.
- [Q4] Pugh, Soros & Stanley (2016). Quality Diversity: A New Frontier for EC. Front. Robot. AI. `10.3389/frobt.2016.00040`.
- [Q5] Cully & Demiris (2018). Quality and Diversity Optimization: A Unifying Modular Framework. IEEE TEVC. `10.1109/tevc.2017.2704781`.
- [Q6] Chatzilygeroudis et al. (2021). Quality-Diversity Optimization: A Novel Branch of Stochastic Optimization. Springer. `10.1007/978-3-030-66515-9_4`.
- [Q7] Fontaine, Togelius, Nikolaidis & Hoover (2020). CMA-ME. GECCO. `10.1145/3377930.3390232`.
- [Q8] Fontaine & Nikolaidis (2023). CMA-MAE. GECCO. `10.1145/3583131.3590389`. Journal: `10.1145/3665336` (adds CMA-MAEGA).
- [Q9] Vassiliades, Chatzilygeroudis & Mouret (2017). CVT-MAP-Elites. IEEE TEVC. `10.1109/tevc.2017.2735550`. (Centroids: Mouret 2023, `10.1145/3583133.3590726`.)
- [Q10] Cully (2021). Multi-Emitter MAP-Elites. GECCO. `10.1145/3449639.3459326`.
- [Q11] Choi & Togelius (2021). Self-referential QD through Differential MAP-Elites. GECCO. `10.1145/3449639.3459383`.
- [Q12] Pierrot, Richard, Beguir & Cully (2022). Multi-Objective Quality Diversity (MOME). GECCO. `10.1145/3512290.3528823`.
- [Q13] Janmohamed, Pierrot & Cully (2023). MOME-PGX. GECCO. `10.1145/3583131.3590470` (arXiv:2302.12668).
- [Q14] Grillotti & Cully (2022). Unsupervised Behaviour Discovery with QD (AURORA). IEEE TEVC. `10.1109/tevc.2022.3159855`. (Precursor: Cully 2019, `10.1145/3321707.3321804`.)
- [Q15] Tsakonas & Chatzilygeroudis (2025). Vector Quantized-Elites (VQ-Elites). IEEE TEVC. `10.1109/tevc.2025.3631786` (arXiv:2504.08057).
- [Q16] Zhang, Fontaine, Hoover & Nikolaidis (2022). Deep Surrogate Assisted MAP-Elites (DSA-ME). GECCO. `10.1145/3512290.3528718`. (DSAGE: Bhatt et al. 2022, NeurIPS.)

### PCG / EDPCG / expressive range [P]
- [P1] Yannakakis & Togelius (2011). Experience-Driven PCG. IEEE TAFFC. `10.1109/t-affc.2011.6`. (= [X1])
- [P2] Togelius, Yannakakis, Stanley & Browne (2011). Search-Based PCG: Taxonomy and Survey. IEEE TCIAIG. `10.1109/tciaig.2011.2148116`.
- [P3] Summerville et al. (2018). PCG via Machine Learning (PCGML). IEEE TG. `10.1109/tg.2018.2846639`.
- [P4] Khalifa, Bontrager, Earle & Togelius (2020). PCGRL. AIIDE. `10.1609/aiide.v16i1.7416`.
- [P5] Earle, Edwards, Khalifa, Bontrager & Togelius (2021). Learning Controllable Content Generators. CoG. arXiv:2105.02993.
- [P6] Smith & Whitehead (2010). Analyzing the Expressive Range of a Level Generator. PCGames/FDG. `10.1145/1814256.1814260`.
- [P7] Summerville (2018). Expanding Expressive Range. AIIDE. `10.1609/aiide.v14i1.13012`.
- [P8] Withington & Tokarchuk (2023). The Right Variety: Metric Selection for ERA. FDG. `10.1145/3582437.3582453` (arXiv:2304.02366).
- [P9] Gravina, Khalifa, Liapis, Togelius & Yannakakis (2019). PCG through Quality Diversity. CoG. `10.1109/cig.2019.8848053` (arXiv:1907.04053).
- [P10] Khalifa, Lee, Nealen & Togelius (2018). Talakat: Bullet Hell Generation through Constrained MAP-Elites. GECCO. `10.1145/3205455.3205470`.
- [P11] Fontaine, Liu, Khalifa, Modi, Togelius, Hoover & Nikolaidis (2021). Illuminating Mario Scenes in the Latent Space of a GAN. AAAI. `10.1609/aaai.v35i7.16740`.
- [P12] Togelius & Schmidhuber (2008). An Experiment in Automatic Game Design. CIG. `10.1109/cig.2008.5035629`.
- [P13] Cook (2017). A Vision for Continuous Automated Game Design (ANGELINA). AIIDE. `10.1609/aiide.v13i2.12967`.
- [P14] Liapis, Yannakakis & Togelius (2014). Designer Modeling for Sentient Sketchbook. CIG. `10.1109/cig.2014.6932873`.
- [P15] Togelius, Champandard, Lanzi, Mateas, Paiva, Preuss & Stanley (2013). PCG: Goals, Challenges and Actionable Steps. Dagstuhl Follow-Ups 6. `10.4230/dfu.vol6.12191.61`.
- [P16] Volz et al. (2018). Evolving Mario Levels in the Latent Space of a DCGAN. GECCO. `10.1145/3205455.3205517`. (GAN-PCG substitute for the missing Horn et al.)

### Player experience / fun / flow / tone [X]
- [X1] Yannakakis & Togelius (2011). Experience-Driven PCG. `10.1109/t-affc.2011.6`. (= [P1])
- [X2] Vanden Abeele, Spiel, Nacke, Johnson & Gerling (2019). Player Experience Inventory (PXI). IJHCS. `10.1016/j.ijhcs.2019.102370`.
- [X3] Phan, Keebler & Chaparro (2016). Game User Experience Satisfaction Scale (GUESS). Human Factors. `10.1177/0018720816669646`.
- [X4] Sweetser & Wyeth (2005). GameFlow. ACM CIE. `10.1145/1077246.1077253`.
- [X5] Hunicke (2005). The Case for Dynamic Difficulty Adjustment (Hamlet). ACE. `10.1145/1178477.1178573`.
- [X6] Zohaib (2018). Dynamic Difficulty Adjustment: A Review. Adv. HCI. `10.1155/2018/5681652`.
- [X7] Schmidhuber (2010). Formal Theory of Creativity, Fun, and Intrinsic Motivation. IEEE TAMD. `10.1109/tamd.2010.2056368`.
- [X8] Itti & Baldi (2009). Bayesian Surprise Attracts Human Attention. Vision Research. `10.1016/j.visres.2008.09.007`.
- [X9] Silvia (2005). Emotional Responses to Art: Collation, Arousal, Cognition, Emotion. Rev. Gen. Psych. `10.1037/1089-2680.9.4.342`.
- [X10] Silvia (2010). Confusion and Interest. Emotion. `10.1037/a0017081`.
- [X11] Malone (1981). Toward a Theory of Intrinsically Motivating Instruction. Cognitive Science. `10.1207/s15516709cog0504_2`.
- [X12] Ryan, Rigby & Przybylski (2006). The Motivational Pull of Video Games (SDT). Motivation & Emotion. `10.1007/s11031-006-9051-8`.
- [X13] Mori, MacDorman & Kageki (2012 [1970]). The Uncanny Valley. IEEE RAM. `10.1109/mra.2012.2192811`.
- [X14] Tinwell, Grimshaw & Williams (2010). Uncanny Behaviour in Survival Horror Games. J. Gaming & Virtual Worlds. `10.1386/jgvw.2.1.3_1`.
- [X15] Gowler & Iacovides (2019). "Horror, Guilt and Shame": Uncomfortable Experiences in Digital Games. CHI PLAY. `10.1145/3311350.3347179`.
- [X16] Hunicke, LeBlanc & Zubek (2004). MDA: A Formal Approach to Game Design. AAAI Workshop (no DOI; canonical PDF).

### RL playtesting / intrinsic motivation / open-endedness [R]
- [R1] Shin, Kim, Jin & Kim (2020). Playtesting in Match-3 via RL. IEEE Access. `10.1109/access.2020.2980380`.
- [R2] Gudmundsson et al. (2018). Human-Like Playtesting with Deep Learning. CIG. `10.1109/cig.2018.8490442`.
- [R3] Zhao et al. (2020). Winning Is Not Everything: Intelligent Agents for Game Dev. IEEE TG. `10.1109/tg.2020.2990865` (arXiv:1903.10545).
- [R4] Ariyurek, Betin-Can & Surer (2021). Automated Video Game Testing Using Synthetic and Humanlike Agents. IEEE TG. `10.1109/tg.2019.2947597`. (MCTS companion: `10.1109/cog47356.2020.9231670`.)
- [R5] Roohi et al. (2021). Predicting Game Difficulty and Engagement Using AI Players. Proc. ACM HCI (CHI PLAY). `10.1145/3474658` (arXiv:2107.12061).
- [R6] Bergdahl, Gordillo, Tollmar & Gisslén (2020). Augmenting Automated Game Testing with DRL. CoG. `10.1109/cog47356.2020.9231552`.
- [R7] Gordillo, Bergdahl, Tollmar & Gisslén (2021). Improving Playtesting Coverage via Curiosity-Driven RL. CoG. arXiv:2103.13798.
- [R8] Ferdous, Kifetew, Prandi & Susi (2022). Agent-Based Testing of 3D Games using RL. A-TEST. `10.1145/3551349.3560507`. (Production notes: Gillberg et al. 2023, arXiv:2307.11105.)
- [R9] Pathak, Agrawal, Efros & Darrell (2017). Curiosity-Driven Exploration by Self-Supervised Prediction (ICM). ICML. `10.1109/cvprw.2017.70` (arXiv:1705.05363).
- [R10] Burda, Edwards, Storkey & Klimov (2018). Exploration by Random Network Distillation (RND). ICLR. arXiv:1810.12894.
- [R11] Tang et al. (2016). #Exploration: Count-Based Exploration for Deep RL. NeurIPS. arXiv:1611.04717.
- [R12] Ecoffet, Huizinga, Lehman, Stanley & Clune (2019/2021). Go-Explore. Nature. arXiv:1901.10995.
- [R13] Wang, Lehman, Clune & Stanley (2019). Paired Open-Ended Trailblazer (POET). GECCO. `10.1145/3321707.3321799`.
- [R14] Brant & Stanley (2017). Minimal Criterion Coevolution. GECCO. `10.1145/3071178.3071186`.
- [R15] Dharna, Togelius & Soros (2020). Co-Generation of Game Levels and Game-Playing Agents (PINSKY). AIIDE. `10.1609/aiide.v16i1.7431`.
- [R16] Parker-Holder et al. (2022). Evolving Curricula with Regret-Based Environment Design (ACCEL). ICML. arXiv:2203.01302.
- [R17] Stanley & Miikkulainen (2002). Evolving Neural Networks through Augmenting Topologies (NEAT). Evol. Comput. `10.1162/106365602320169811`.
- [R18] Salimans, Ho, Chen, Sidor & Sutskever (2017). Evolution Strategies as a Scalable Alternative to RL. arXiv:1703.03864.
- [R19] Baker et al. (2019). Emergent Tool Use from Multi-Agent Autocurricula. ICLR. arXiv:1909.07528.

*Practitioner (non-academic) refs to cite where relevant:* Dave Mark, *Behavioral Mathematics for Game
AI* (utility AI); Gumin, WaveFunctionCollapse (GitHub); Reynolds, "Steering Behaviors for Autonomous
Characters" (GDC 1999); *Game AI Pro* (influence maps).

## 9. How this maps to the roadmap

The full extension roadmap lives in the approved plan; this section pins each phase to its literature.

- **Phase 1 — Fun proxies (G6, G8, partial G1).** `src/squad_ai/experience.rs`: dread [X15, X8],
  loneliness/liminality (tied to `dungeon.liminality`), pacing/flow-arc over longer rollouts [X4, X7],
  fairness placeholder. Scalar → objective vector; target **MOME** [Q12, Q13]; keep `minimal_criterion`.
- **Phase 2 — Replayability + ship-a-generator (G2).** Expressive-range descriptors [P6–P8]; inter-seed
  variance objective [P5, P9]; ship a sampler over a quality-filtered archive region (no fallback path).
- **Phase 3 — Full structural search (G3, G4, G5).** Playtest-scored levels [P4, P10]; promote map
  size/`room_types`/`liminality`/`notch` into `level_genome` (re-bake determinism goldens); add CPU-side
  `FixedUpdate` ecosystem couplings (mirroring `mycelia/grazing.rs`). Adopt **CMA-MAE** [Q8],
  **CVT-MAP-Elites** [Q9], **DSA-ME** surrogate [Q16].
- **Phase 4 — RL playtesting agent, pure-Rust neuroevolution (G7, fills fairness).** Learned
  `NeuralPolicy` via **NEAT/ES** [R17, R18]; competent difficulty gauge [R1, R2, R5] + curious/adversarial
  exploit finder [R6, R7, R10–R12]. Exploitability → fairness penalty into Phase-1's vector.
- **Phase 5 — Hybrid audition gate + weight fitting (closes G1).** `devshot` keyframes → `train audition`
  → re-fit objective weights (the EDPCG loop [X1]); gate `train apply`. Rating rubric from PXI/GUESS
  [X2, X3].

**Determinism & one-path (CLAUDE.md/TESTING.md):** new scoring that reads pinned state → `FixedUpdate`,
side-buffers must not change `snapshot_hash`; exact-hash only physics-off `deterministic_core`,
physics-on → liveness; no `unwrap`/panic; seed everything; one App/process, hold `serial_guard()`;
re-bake goldens only in Phase 3; the archive-sampler is the sole config source (no default-elite fallback).

## 10. Papers not yet in the `home-still` corpus

These could not be retrieved during the review; the text cites the noted substitute. Run
`paper_search → paper_download → scribe_convert → distill_index` to ingest any you want indexed.

1. Horn et al. (2014). Comparative Evaluation of Mario Level Generators. FDG. → substitute [P16].
2. Liapis, Yannakakis & Togelius (2013). Sentient Sketchbook (original). → used [P14].
3. Csikszentmihalyi (1990). *Flow* (book; needs ISBN). → covered via [X4, X9].
4. Berlyne (1960/1971) primaries + the Wundt curve. → bridged by [X9].
5. Yannakakis, Spronck, Loiacono & André (2013). Player Modeling. Dagstuhl. → PEM via [X1, X2, X3].
6. Computational suspense / dramatic-arc models. → partial via [X15, X8].
7. Liminality / environmental-horror academic work. → genuine gap; nearest [X13, X14, X15].
8. Grassé (1959). Original stigmergy (French, no DOI). → cite via [S6].
9. Bellemare et al. (2016). Unifying Count-Based Exploration. arXiv:1606.01868. → used [R11].
10. Wang et al. (2020). Enhanced POET. arXiv:2003.08536. → used [R13].
11. Team et al. (2021, DeepMind). Open-Ended Learning (XLand). arXiv:2107.12808. → adjacent Hughes et al. 2024 (arXiv:2406.04268).
12. Empowerment as intrinsic motivation (Klyubin/Polani; Salge). → adjacent Oudeyer 2007 (`10.3389/neuro.12.006.2007`).
13. Wuji (Zheng et al., 2019, ASE) — multi-objective RL+EA game-bug finding.
