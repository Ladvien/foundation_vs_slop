# Dev Journal: 2026-07-21 — VALKYRIE Mixamo Rifle Retarget

> The squad-unit figurine (`assets/characters/valkyrie.glb`) got a professional rifle
> animation set. Work happened in the sibling asset repo `SCP_Characters/` (branch
> `worktree-valkyrie-aim-ik`, PR #13); this entry records the non-obvious parts. The
> `.glb` in `assets/characters/` was regenerated at the end.

**Walkthrough:** None (direct implementation, user-directed).
**Purpose:** Replace VALKYRIE's hand-tuned rifle stance (held-at-chest, wrong barrel
azimuth) with retargeted Mixamo mocap — which uncovered and fixed a latent retarget bug.

## Log

### 17:43 — The world-space retarget bug (the one that ate the day)

**What:** Uploaded VALKYRIE's T-pose to Mixamo, auto-rigged, downloaded a 15-clip
"Shooter Pack" (aim/fire/walk/run/strafe/jump/... "FBX Without Skin", 30 fps). The repo
already had a mocap retarget pipeline (`scp_characters/mocap/` + `retarget.py`,
`MixamoSource`/`MIXAMO_PROFILE`), so this looked like pure wiring. It was not.

**Bug/Challenge:**
- **Symptom:** the retargeted "rifle aiming idle" baked to a hunched, laterally-contorted
  mess — torso tilted ~37° forward with a sideways bend. Not a rifle aim at all.
- **Hypothesis 1 (wrong):** "Mixamo's clip is just an aggressive crouched pose." Ruled out
  by importing the source `mixamorig` skeleton and measuring it directly — the SOURCE torso
  tilt was a clean **15°** (a proper shouldered aim). So the bake was corrupting a good clip.
- **Investigation:** read `retarget._bake_onto`. It builds a per-bone conjugation
  `conj = source_rest⁻¹ · target_rest` from **armature-space** rest matrices
  (`bone.matrix_local`), but applies it to **parent-relative** pose rotations
  (`matrix_basis`). That's a space mismatch. Tried a parent-relative conjugation live —
  made it *worse* (45°). Then tried **world-space orientation matching** (copy each bone's
  world rotation-delta-from-rest, convert to a parent-relative basis analytically,
  parents-before-children) → torso tilt **19°**, matching the source. Clean aim.
- **Root cause:** the armature-space conjugation is only correct for bones whose parent is
  axis-aligned with the armature; for CHAINED bones (spine → clavicle → arm → fingers) the
  error compounds into contortion.
- **Why it survived so long:** (1) the only end-to-end mocap test (`TestMocapRun`, CMU run)
  routes through `arm_mocap_run`, which `overlay_hold_on_action`s a static hold onto the
  arms — **throwing away the buggy arm bake** — and CMU's spine barely moves; (2) the
  **Mixamo bake was never tested end-to-end** (only catalog/fetch). My raw-arm path used
  spine+arms+fingers directly, so the bug surfaced instantly.
- **Solution:** rewrote `_bake_onto` to world-space delta matching (commit `5181d38`).
  Verified: Mixamo aim → clean shouldered stance; `TestMocapRun` still 4/4 (pelvis tilt
  7°<20°, feet on ground) — no CMU regression.

**Lesson:** when a "faithful copy" produces garbage, **ground-truth the source** before
touching the transform (import it, measure it). And a bug can hide indefinitely behind a
test that discards the very output it would expose — the CMU test *looked* like it covered
retargeting but overlaid the arms it retargeted.

### 17:43 — Raw arms, foot-grounding, and body-agnostic orchestration

**What:** built the reusable clip library (`combat_animation.build_mocap_shooter_library`,
commits `808132c`/`d0ed2dd`). Key decisions:
- **RAW arms, not overlaid.** The CMU run overlays a hand-tuned hold (CMU had no rifle).
  The Mixamo clips ARE rifle clips — the arms already hold the weapon in the pro stance,
  which is the whole point — so use the retargeted arms directly. Overlaying would have
  re-imposed the broken hand-tuned hold.
- **Foot-grounding.** `vertical_bob` only dropped the pelvis ~2 cm on a crouched aim, so the
  feet floated ~14 cm (the bake copies leg *rotations* but nothing *places* the feet). Added
  `retarget.ground_feet_on_action`: per-frame vertical root shift so the lowest foot tail =
  ground. A global root shift (no per-joint IK — AMASS warns IK retargeting corrupts mocap),
  and following the lowest foot reproduces the natural bob for free. Jumps skip it
  (`root_motion` keeps their arc).
- **Body-agnostic.** The builder takes `(rig, rifle_obj, source)` — nothing VALKYRIE-
  specific — so any game_engine character reuses it. Replace-semantics so Mixamo clips
  override same-named procedural ones cleanly (a fresh build's name collision → Blender
  `.001` suffix was the tell in `TestShooterClips`).

**Lesson:** the multi-angle + bold-random-per-mesh-color inspection habit earned its keep
again — the contortion was obvious from the front but ambiguous from a single ¾ view, and
bold colors separated the limbs. Single-angle "looks good" is not verification.

### 17:43 — Barrel into her hand + dynamic grip tests

**What:** the rifle is rigidly bone-parented to `hand_r`, so the barrel-in-hand direction
is **identical on all 15 clips** — a single tweak, not per-animation (user assumed the
latter). `orient_matrix` left the barrel ~17° muzzle-up on the aim hand.

**Bug/Challenge:** the obvious knob (`barrel_forward_dir` passed to `orient_matrix`) is a
trap — it's *projected perpendicular to the grip axis*, so `(0,-1,-0.15)` gave +17° *up*,
not down. The clean, predictable knob is a small direct rotation of the rifle relative to
the hand AFTER parenting (rifle local −Y = barrel, X = pitch). `RIFLE_GRIP_PITCH_DEG = 12°`
levels the aim to ~+5° and, being on the rigid parent, applies uniformly (commit `0ddbe33`).

**Lesson:** pinned it with `TestRifleGrip` — **dynamic** (per-frame) contracts, as the user
asked: both hands stay ≤12 cm from the rifle, grip span 20–36 cm, elbows in a rifle-grip
ROM, aim barrel stays −4..+13° (catches the tweak regressing to +17°), and barrel-in-hand
identical on every clip. 14/14 green (TestRifleGrip + TestShooterClips + TestMocapRun).
Measuring geometry across frames catches what a single-pose assert can't.

**Game-side follow-up:** the new clip set reordered the glTF animation indices (`walk` 2→7,
`run` 8→13). `src/squad.rs` loads by index — switch to `Gltf::named_animations` (robust) or
update the indices per the table in the `valkyrie_bevy_integration.md` clip list.
