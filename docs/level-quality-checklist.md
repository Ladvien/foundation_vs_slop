# The Level-Quality Checklist — every knob, and how to steer it

This is the **grader** the offline level search uses to decide whether an evolved level is "good." It is the one place your taste enters the system: the search can only ever find levels the checklist *rewards*, so if you want different levels, you change the checklist, not the search.

Everything here lives in **one function**: `score()` in `src/squad_ai/level_quality.rs`. It is a short, weighted list of rules. You do not need to touch anything else to retune it.

> **Why a checklist at all?** The search generates tens of thousands of candidate levels. Something has to grade them automatically, fast, without a human watching. That grader is a proxy — it measures things we *can* measure about a level's shape (is it connected? is it balanced?), which is not the same as "is it fun." Treat the checklist as *your instructions to the intern*, and expect to revise it once you see what it produces.

---

## How a grade is computed (two stages)

**Stage 1 — the gate (pass/fail).** Before a level gets any score, it must clear three hard rules (`passes_criterion`). Fail any one and the level is thrown out entirely (no score, not kept). This is the "don't even consider garbage" filter.

**Stage 2 — the weighted score (0.0 to 1.0).** A level that passes the gate is graded on seven rules. Each rule produces a number from 0 (bad) to 1 (perfect). Each rule has a **weight** — how much it matters — and the weights add up to 1.0. The final grade is the weighted average. A grade of **1.0 means every rule was perfectly satisfied.**

Two rules use a shape called a **band**, and one uses **reward-toward** — both explained at the bottom. For now: a *band* means "I want this value between X and Y"; *reward-toward* means "more is better, up to a target."

---

## Stage 1: The gate (hard pass/fail)

If a level breaks any of these, it is rejected outright.

| Gate rule | Plain meaning | Why it's a hard rule |
|---|---|---|
| **Fully connected** | Every walkable floor tile can be reached on foot from the player's start. | A level with a sealed-off pocket of rooms is broken, not just low-quality. Non-negotiable. |
| **At least 2 rooms** | The generator produced two or more rooms. | A one-room "level" isn't a level. |
| **Not solid, not empty** | Between 5% and 95% of the map is floor. | Rejects a near-solid block of rock or a giant empty void — degenerate, not designable. |

*In the code:* `passes_criterion()`. To loosen or tighten a gate, edit the numbers there (e.g. require `room_count >= 4` for busier levels).

---

## Stage 2: The seven grading knobs

Each row is one rule. **Weight** is how much it counts toward the final grade. **Target** is the range or value the rule rewards. The **"turn it toward…"** column is your taste lever — what to change to steer the levels the search finds.

| # | Knob | What it measures (plainly) | Weight | Target | Turn it toward… |
|---|---|---|---|---|---|
| 1 | **Connectivity** | How much of the floor is reachable from start. | **0.20** | 1.0 (all of it) | This is also the gate. Raising its weight makes barely-connected levels score worse; there's rarely a reason to change it. |
| 2 | **Room richness** | How many rooms the level has. | **0.15** | between **6 and 18** rooms | Want *maze-ier*, more-rooms levels? Raise the band (e.g. `10, 30`). Want a few big spaces? Lower it (e.g. `3, 8`). |
| 3 | **Size hierarchy** | How *varied* the room sizes are (a tiny bathroom next to a sprawling hall scores high; all-identical rooms score 0). | **0.15** | variation between **0.3 and 1.2** | Want a strong "grand hall vs. closet" contrast? Raise the upper number. Want uniform rooms? Lower it toward 0. |
| 4 | **Furniture balance** | Average furniture pieces per room. | **0.15** | between **1.5 and 5** per room | Want busier, more-furnished rooms? Raise the band (e.g. `4, 8`). Want bare, empty rooms? Lower it (e.g. `0, 2`). |
| 5 | **Openness balance** | What fraction of the map is floor (open space vs. walls/void). | **0.15** | between **5% and 50%** floor | This band is deliberately *wide and low* because the game is meant to be sparse Backrooms. Want denser, more-built-out levels? Raise it (e.g. `0.3, 0.7`). |
| 6 | **Mushroom amount** | What fraction of the floor is infested with mould. | **0.10** | between **5% and 35%** of floor | Want levels drowning in mould? Raise the upper number (e.g. `0.2, 0.6`). Want a clean level with just a touch? Lower it. |
| 7 | **Mushroom placement** | Whether the mould sits mostly in *rooms* rather than corridors. | **0.10** | **60%+** of mould in rooms | This encodes the shipped design rule "mould is a room thing." Raise the target toward 1.0 to punish corridor mould harder; lower it if you *want* infested hallways. |

*In the code:* the `terms` list inside `score()`. Each line is `(weight, rule)`. To change a **weight**, edit the first number. To change a **target**, edit the numbers inside `band(...)` or `reward_toward(...)`. **Keep the weights adding up to 1.0** so a perfect level still scores exactly 1.0.

---

## The two rule shapes

You'll see two little functions used above. They're simple:

- **`band(value, low, high)`** — the "I want it in a range" shape. Returns **1.0** when `value` is anywhere between `low` and `high`. Outside the range it fades to 0 gradually (it reaches 0 once you're half the band's width past an edge), so a near-miss still scores partial credit rather than falling off a cliff. Example: `band(room_count, 6, 18)` is happy with 6–18 rooms, mildly unhappy at 4 or 22, fully unhappy at 0.
- **`reward_toward(value, target)`** — the "more is better, up to a point" shape. Returns `value / target`, capped at 1.0. So it rewards climbing toward the target and then stops caring once you reach it. Example: `reward_toward(mushroom_room_fraction, 0.6)` gives full marks once 60% of mould is in rooms.

Both live at the top of `level_quality.rs`. You rarely need to change *these*; you change the numbers you pass *into* them (the targets in the table above).

---

## A related knob: how the menu is organized (not grading)

Separate from the grade, the search sorts its results into a grid — the "menu" you pick from — along two axes: **furniture clutter** (across) × **mushroom infestation** (down). This is `descriptor_axes()`. It does **not** affect a level's grade; it only decides *which slot* a level lands in, so the final menu spans "lots of furniture / no mould" through "sparse / heavily infested." If you'd rather organize the menu by, say, room-count vs. openness, that's the function to edit. (Grade = how good; axes = how it's filed.)

---

## How to actually change the checklist

1. Open `src/squad_ai/level_quality.rs` and edit the numbers in `score()` (weights/targets) or `passes_criterion()` (gates), guided by the tables above.
2. Re-run the search to regenerate the menu:
   ```
   cargo run --release --features test-harness --bin train -- levels \
     --generations 800 --batch 48 --seeds 0x5C09191,0xA11CE,0xBEEF
   ```
   It overwrites `assets/config/elites_levels.ron` with the new menu.
3. Pick a level and play it (see the "how to run an elite" notes / the project README).

**Worked examples:**

- *"I want tight, maze-like levels."* → Knob 2: raise the room-richness band to `10, 30`. Optionally Knob 5: keep openness low. Rerun.
- *"I want every level heavily overgrown with mould."* → Knob 6: raise the amount band to `0.25, 0.6`. Optionally Knob 7: it'll follow. Rerun.
- *"Furniture everywhere, barely any mould."* → Knob 4: raise to `4, 8`; Knob 6: lower to `0.0, 0.1`. Rerun.
- *"Only keep levels with at least 4 rooms."* → gate: change `room_count >= 2` to `>= 4` in `passes_criterion()`. Rerun.

---

## The one limit to keep in mind

This checklist grades a level **sitting still** — its shape, balance, and contents. It never plays the level, so it cannot judge pacing, difficulty, or fun. It is a fast, honest proxy for "structurally sound and balanced," and a strong idea-generator — but the final "is this actually good to play?" call is always yours, which is exactly why the search hands you a menu to taste-test rather than shipping a level on its own.
