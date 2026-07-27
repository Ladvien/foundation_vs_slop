# Handoff — overnight session, 2026-07-26/27

**Delete this once absorbed.** Session state, not documentation. The durable records are `BACKLOG.md`,
`TESTING.md`, and the commit messages — every decision below is written up at the code it affects.

---

## 1. Read this first: one thing I got wrong

**FVS-N-8 is NOT closed, and I said it was.** Mid-session I found the root cause (`autogib::seed_from`
hashed an `AssetId` — an arena slot assigned by async load order — so the fracture sliced along
different planes every run), fixed it, watched a clean 48-minute replay and a green un-ignored
reproducer, and reported it fixed.

Both measurements were true. **Both were taken under a lighter load than the one that still breaks it.**
That is TESTING.md invariant 13 applied to my own claim, and I should have caught it.

Reproduce it in about two minutes:

```
cargo test >/dev/null                                        # saturate the box
cargo test --features test-harness --test session -- --test-threads=1
```

`gib_spawn_positions_stay_identical_under_load` fails roughly once per run in that shape. It does
**not** fail running `session` alone and idle (20/20, four consecutive runs), nor running only that
test after the gate. It needs the other 19 tests' load on top of a still-settling box.

The bake *does* settle — the panic is the gib-split assertion, not the `step_until_autogib_ready`
precondition — so the seed fix stands and is worth keeping (`tests/autogib_determinism.rs` proves the
bake reproducible: 0 of 23 fragments differing, where it was 23 of 23). Something **downstream** still
varies. Next two places to look, in `BACKLOG.md` N-8:

1. `GibSeq`'s cumulative counter, if two runs process different numbers of gore events before the kill.
2. Whether `AutogibCache` insertion order reaches anything else.

---

## 2. What landed

Branch `feat/m1-loop-closure`, **37 commits**, tree clean, **683 pass / 0 fail** on the hard gate.

| Push | State |
|---|---|
| P1 M0 session | complete |
| P2 M1 containment | complete — `ExtractContained`, all four player verbs, RL/QD wired |
| P3 roster | **all unblocked items done** (C-3, C-4, C-5, D-1) |
| P4 research | **complete** (E-1…E-4, L-2) |
| P5 Site | G-1, G-2, G-4, G-5, D-4, F-1 flags, P-1, P-2 |
| P10 knowledge | O-1 done; O-2 wired **inert** |

The loop now runs end to end: capture by three distinct verbs → extract → return to Site-67 → see the
specimen in a cell → research it → unlock a capability → it persists across a restart.

---

## 3. Decisions I made on your behalf

You said to use my judgement. These are the ones worth overruling if you disagree — each is a one-line
change and each is argued at the code:

* **The O5 budget floor is the price of one capture device.** The floor's job is not generosity; it is
  that the loop stays *attemptable*. A Director who cannot afford to contain anything is in a state the
  game has no way out of.
* **There is no "relieved of command" outcome.** A review that could end a campaign is a second lose
  condition competing with the squad wipe, and a worse one — it fires from accumulated mediocrity
  rather than from anything you can watch happen.
* **Save/load refuses a version mismatch rather than migrating.** A version field with per-version
  paths is the multi-path shape this codebase rejects. Right for a game under construction; revisit
  when the format stabilises.
* **Loading is a replacement, not a merge.** Merging would silently double a campaign each load.
* **O-2's FEAR coupling ships at gain zero** — bit-exact inert, so the goldens do not move. Turning it
  on is a deliberate act and will need a re-pin.

---

## 4. Open, and what I did not touch

* **FVS-N-8** — see §1. The only known-red thing.
* **FVS-N-11** — a `session` flake I filed earlier is now *characterised*: it is the same load-shape
  issue as N-8's residual. Likely one item, not two.
* **FVS-C-1 (SCP-610)** — still blocked on a `.glb` that does not exist. Only a Blender generator does.
  Your call whether to run it.
* **FVS-H-1 retrain** — I deliberately did **not** start it. It is hours of machine time and I had
  already cost you load once tonight (see §5). Every baked archive is stale for two independent reasons
  now, so it is a prerequisite for further QD work.
* **F-3 (Thaumiel curriculum)**, **L-3 (Site/tech-tree HUD)**, **O-3/O-4/O-5**, **Push 6/7** — not
  started. F-1 is deliberately partial: the flags exist, the prerequisite *graph* does not, because
  four unlocks with no dependencies is a list rather than a curriculum.
* **Nothing pushed.** All local.

---

## 5. Two mistakes worth knowing about

**I left 12 busy-loop processes running on the box.** I spawned them to test the session suite under
load — the right instinct — but cleaned up with `kill $(jobs -p)`, which does not capture backgrounded
subshells in a non-interactive shell. They ran ~4 minutes at 99% each and almost certainly caused a
54-minute replay job to be killed. If you see stray `zsh` processes at 99%, that is the shape to look
for.

**I broke the build "fixing" a warning** with a blind pattern replace that also hit a test genuinely
using the binding it renamed. One warning became four compile errors. Read the site, then edit it.

---

## 6. Where I would start

1. **N-8's residual** (§1). It is reproducible in two minutes, and it is the only red thing.
2. **Play it.** The whole loop is walkable now and only you can tell me whether it is any good —
   particularly the ASYNC aperture, whose shader I wrote blind and whose uniform defaults are guesses.
   It only renders once you are in `AppState::Site`, i.e. after one expedition and a debrief.
3. **H-1's retrain**, whenever you can spare the box for a few hours.
