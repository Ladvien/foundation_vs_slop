# The mount rules, audited — 2026-08-10

Asked, after a labelling pass moved four site pieces from `OnFloor` to `Overlay(on: Floor)`:

> *"Doesn't almost everything overlay floor? What other logic fallacies are in these mounting rules?"*

Yes — and the question found a real one. Five findings, worst first. The stool regression that
prompted this is **finding 3**, and it is a symptom rather than the subject.

---

## 1. `Overlay` does not mean what its name says, and the misreading has already shipped

`Mount::Overlay { on }` means **a decal**: something flat, lying on a plane, that *claims no volume
and never participates in the overlap rule*. Its own doc says so, and `same_layer` enforces it by
omission — `Overlay` appears in no arm, so it falls to `_ => false` and contests nothing, ever.

The name says something else. **"Overlay, on: Floor" reads as "sits on top of the floor"**, which is
true of almost every piece in the kit. That is not a hypothetical misreading: the labelling pass moved

| piece | from | to |
|---|---|---|
| `site/wall` | `OnFloor` | `Overlay(on: Floor)` |
| `site/wall_corner` | `OnFloor` | `Overlay(on: Floor)` |
| `site/mess_table` | `OnFloor` | `Overlay(on: Floor)` |
| `site/coffee_machine` | `OnFloor` | `Overlay(on: Floor)` |

Every one of those is a volumetric object standing on the deck. As `Overlay` they have **left the
overlap rule entirely**: two walls may now occupy one cell, a table may interpenetrate a wall, and
nothing refuses either. That is precisely the accident `blocking`'s doc names — *"a mesh hidden inside
another is the kind of authoring accident that is only found by counting draw calls"* — arrived at
through the schema instead of through carelessness.

**Recommendation: rename the variant to `Decal { on: … }`.** The doc comment already reaches for the
word "decal" to explain it. A name that needs its doc comment to avoid being read backwards will be
read backwards again — by the next labelling pass, by the next author, by the next VLM. The rename is
mechanical (`Mount::Overlay` has one construction site per kit file and one arm each in `datum` and
`same_layer`), and it makes the wrong labelling unwritable rather than merely wrong.

## 2. The family conflates *where a piece sits* with *what it contests*

> **CLOSED 2026-08-10.** `Mount`'s own doc now carries the contest column for all seven variants,
> including that a heterogeneous pair contests nothing (`same_layer`'s `_ => false`). The table is
> restated there so choosing a mount does not require reading `stack.rs`.

Three variants put a piece at floor level and mean three different things about collision:

| variant | height | contests |
|---|---|---|
| `OnFloor` | the deck | every other floor-standing piece |
| `Tiled` | the deck | only other tiles — which is what lets floor go under a dressed room |
| `Overlay(Floor)` | the deck | **nothing** |

The variant name states the first column. The second column — the one that decides whether an edit is
refused — is invisible at the call site and lives in `same_layer`'s match arms. An author choosing a
mount is choosing collision semantics while reading a positional word.

**Recommendation: say the layer in the type, or document the contest column beside every variant.**
The cheap version is a doc table on `Mount` itself listing what each variant contests; the honest
version is that `same_layer`'s table *is* the schema and should be reachable from the enum.

## 3. A seat can be pulled up to itself — `src/site/layout.rs:1010`

The rule that produced five failures:

```rust
for q in layout.props.iter().filter(|q| kit.is_surface(q.piece)) { … }
```

It looks for the nearest **surface** within reach and never excludes the seat doing the looking. The
labelling gave `site/stool` `offers.surfaces: ["support"]`, so every stool became a surface, and each
one is its own nearest surface at distance **0.00 m** — hence the message *"is 90° off the Stool it is
pulled up to 0.00 m away"*, which reads as nonsense because it is.

**This is a bug at any distance and independent of the stool.** A bench that legitimately offers a
surface will hit it. So will a counter with a stool built into it.

`"support"` is **not** a bad token: other site pieces both offer it and rest on it, and
`Library::resolve` validates against the kit vocabulary, so the write would have been refused
otherwise. Whether a stool *should* offer a support surface is a design call. Whether a seat can be
its own surface is not.

**Recommendation: exclude self, then decide the data separately.** One guard, and the five failures
go with it.

## 4. The overlap rule is 2-D, and the layer partition hides the rest

> **CLOSED 2026-08-10.** `blocking`'s doc now states the bounded claim — *within a layer, in plan* —
> with both worked cases and a note that the information to close it already exists.

`blocking` compares **plan rectangles** — `plans_overlap` is a separating-axis test over two oriented
boxes in X/Z. Height never enters. That is fine within a layer, because a layer is a horizontal
stratum; it is not fine *across* layers, and `same_layer` returns `false` for every heterogeneous
pair. So:

- a 2.2 m floor-standing cabinet and a sconce at 1.8 m on the wall behind it never contest, and
  interpenetrate silently;
- `InOpening` contests only other `InOpening`, so a door and the wall it is set into cannot conflict —
  correct — but neither can a door and a crate standing in the doorway.

This is a **known limitation stated nowhere**, not a fallacy in the logic. The information to close it
exists: `Mount::OnWall` and `OverlayHost::Wall` both carry a height, and `Extent::height` is measured
for every piece.

**Recommendation: name it in `blocking`'s doc as a bounded claim** — "contests within a layer, in
plan" — so the next person to find a floating interpenetration knows it was scoped rather than missed.

## 5. `same_layer`'s `_ => false` makes "contests nothing" the default for new variants

```rust
match (a.mount.as_ref(), b.mount.as_ref()) {
    (None | Some(OnFloor), None | Some(OnFloor)) => true,
    …
    _ => false,
}
```

A variant added tomorrow contests **nothing, including another of itself**, until someone remembers to
add an arm. It fails open, silently, in the direction of "the edit is allowed". `Overlay` relies on
exactly this behaviour, which is why the hole is invisible: the one variant that *should* fall through
does, so the fall-through looks intentional for every variant.

**Recommendation: a ratchet test — every `Mount` variant must contest itself unless it is named in an
explicit exemption list.** `Overlay` (or `Decal`) goes in the list with its reason. Anything added
later fails the test until its author decides, which is the same shape as the determinism lint and the
dependency ratchets already in this workspace.

---

## What this says about the stool

The five failures are finding 3, and finding 3 is a one-line guard. The labelling that exposed it is
mostly good work — but finding 1 says four of its mount changes are wrong in a way that removes real
protection, and those should be reverted to `OnFloor` whatever else happens.

Order that follows from the above:

1. **Revert the four `Overlay(on: Floor)` mounts** to `OnFloor` — they are decal claims about solid objects.
2. **Guard the seat rule** against pairing with itself.
3. **Rename `Overlay` → `Decal`**, so the mistake is not re-authorable.
4. **Add the `same_layer` ratchet**, so the next variant cannot fail open.
5. Decide, separately and unhurriedly, whether a stool offers `support`.


---

## Postscript, 2026-08-10 — the `front: Some(South)` loose end, and a correction

`site/floor` and `site/wall` carried `front: Some(South)` from the labelling pass. Both are now
`None`, which is the honest claim: the schema records *"`None` means the mesh is symmetric and has no
front, which is a different claim from `Some(Face::South)`"*, and `site/wall_header` — the same family
of piece — already said `None`.

**The handoff's reason for leaving it was wrong, and the correction matters more than the change.**
It read *"meaningless … since no rule reads it"*. A rule does read it: `composition.rs`'s descriptor
**fingerprint** encodes `align.front`, so editing the field re-fingerprints every composition using
that descriptor and would flip them to STALE.

Measured before touching it, and the blast radius is nil: `assets/emerge/site/compositions.ron`
records **no** `of_fingerprint` at all, no site map records one, and the root
`assets/emerge/compositions.ron` shares no `site/` descriptors. The full suite is green either way and
the expressive-range census is byte-identical across the change.

Anything else editing an `align` field should re-run that check rather than inherit this answer.
