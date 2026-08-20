# Version control for labelled data

*A plan, 2026-08-19. Asked for at the keyboard: "I would really like to have a version control library
for labelled data."*

## What is actually broken

Four facts, read out of the code rather than remembered:

1. **A `Descriptor` has no provenance field of any kind.** `emerge_core::descriptor::Descriptor`
   carries `kind`, `effects`, `look`, `offers`, `mount`, `align.front`, `note` and `placement` — and
   nothing that says where any of them came from.
2. **`labels::apply_fields` throws the provenance away.** It receives a `labels::Entry`, which holds
   `provenance { model, date, attempts }` and a `confidence`, copies the *values* into the
   descriptor, and drops the rest. One keypress after the pane has shown you *"PROPOSED by qwen3-vl
   (2026-08-19, confidence High)"*, that sentence is unrecoverable.
3. **The only surviving record lives in the build directory.** `labels::CACHE_PATH` is
   `target/vlm_suggestions.ron`, and `target` is line 4 of `.gitignore`. Every record of what any
   model has ever said about this kit is deleted by `cargo clean`.
4. **The library is one file.** `assets/emerge/furniture/library.ron` is **5,781 lines for 88
   pieces**, rewritten wholesale by `write_library` on every commit. Git is versioning it, so the
   history exists — but a 700-mesh labelling run would land as one diff nobody can read, and two
   people labelling different pieces collide in it.

So the ask is not really "add version control". Git is already the store. Two specific things stop it
working: **the provenance is destroyed at the moment of acceptance**, and **the grain is wrong**.

## What the literature says

**Provenance has a standard shape, and it is small.** PROV-DM (Belhajjame et al., W3C 2013) models
provenance as *entity*, *activity* and *agent* — "information about entities, activities, and people
involved in producing a piece of data or thing, which can be used to form assessments about its
quality, reliability or trustworthiness". Our entity is a descriptor's label set, our activity is an
import, a labelling run or a hand edit, and our agent is the importer, a named model, or the author.
Three fields, not a schema.

**Recording *how* a value arrived is the point, because acceptance is not review.** Goddard, Roudsari
& Wyatt 2011 (`10.1136/amiajnl-2011-000089`, ~980 citations) review automation bias across fields:
the measured tendency of an operator to over-accept an automated recommendation, producing errors of
commission that would not occur without the aid. Their named mitigators map onto this editor almost
line for line — *emphasising user accountability*, *attaching updated confidence levels to the
output*, and *providing information rather than a recommendation*. The practical consequence for us:
"the model proposed this and I pressed `U`" and "I typed this myself" must not be the same record,
because after the fact they are indistinguishable in the file and very distinguishable in how much
they should be trusted.

**A versioning scheme has to say what an iteration is.** Bayram & Ahmed 2024 (`10.1145/3708497`, in
the local corpus) on robustness in MLOps: *"the main benefits of ML versioning are: reproducibility
and traceability… a robust versioning mechanism must ensure that the different ML artifacts… have
consistent version numbers for a specific iteration. Moreover, the versioning mechanism should
clearly define what is considered an iteration."* Ours is a **labelling run**: one `Shift+L`, one
model, N pieces.

**Documentation of a dataset is a product, not a byproduct.** Pushkarna, Zaldivar & Kjartansson 2022
(`10.1145/3531146.3533231`) argue dataset documentation must be treated as user-centric in its own
right, covering "upstream sources, data collection and annotation methods". The kit *is* a dataset —
labelled 3-D assets — and this is the argument for showing provenance in the pane rather than only
storing it.

## The plan

Four stages. Each ships on its own and each is useful without the next.

### Stage 1 — the descriptor records how each label arrived

Add one field to `Descriptor`:

```ron
labels: Some((
    by: ( kind: model, effects: model, look: human, mount: import, note: model ),
    model: Some("qwen3-vl"),
    at: "2026-08-19",
    confidence: Some(High),
)),
```

`by` is per-axis and compact — one line — because the mixed case is the normal case: you accept the
model's `kind` and rewrite its `note`. A single blob would call the whole thing "human" the moment
you touched one word, which is precisely the distinction automation bias says is worth keeping.

- `labels::apply_fields` stamps it; it already holds every value it needs.
- A hand edit through the pane stamps `human` for the axis it touched.
- The importer stamps `import` for what it measured.
- Schema version bump, and `None` reads as "no record" rather than as "human".

**What it buys:** "which pieces has a model labelled and I have never checked?" becomes a query, and
the pane can say so quietly under the id. It also makes the *next* stages worth having.

### Stage 2 — one file per piece

`assets/emerge/<kit>/library/<id>.ron` instead of one `library.ron`.

- `emerge_core::policy::layered_library` reads the directory; `write_library` writes the pieces it
  changed instead of rewriting 5,781 lines.
- `git log --follow assets/emerge/furniture/library/drawer_b.ron` is the history of that piece.
- `git blame` answers "when did this become a container".
- Two authors labelling different pieces stop conflicting.
- A 700-piece run is 700 small diffs, which is reviewable in a way one diff is not.

**This is the "version control" in the ask**, and it is mostly a loader/writer change: git supplies
the mechanism, the file layout is what makes it usable.

### Stage 3 — a labelling run is one commit

After a batch, offer to commit it with a generated message: the model, the count, the ids, and the
vocabulary version it was judged against. This is the *iteration* the MLOps literature says the
scheme must define — and it is what makes `git revert` a meaningful verb here: undoing "the labels
qwen3-vl proposed on 2026-08-19" is one operation rather than a hunt.

The editor already owns a commit door for staged edits; this reuses it rather than adding a second.

### Stage 4 — history in the pane, and revert one field

`Cmd+E` established the shape: a panel that is not on screen until asked for. The same shape over a
piece's own history, read from git, with a verb to put one axis back to what it was. Optional, and
only worth building once 1–3 are in — at which point it is a reader, not a store.

## What I am deliberately not proposing

**An append-only label journal with the library derived from it.** Event sourcing would give
per-field revert and in-app history without touching git — and it makes the library a derived
artifact, so the file an author reads is no longer the file that decides anything. That is two
representations of one fact unless the whole editor is rewritten around the journal, which collides
with this repo's one-path rule for a benefit git already provides. Revisit only if Stage 4 proves
reading git too slow or too awkward.

**A content-addressed store like the derived-asset cache.** `assets/derived.json` and
`fvs_derived_cache` exist because meshes are *build output* — a recipe plus a hash, restorable by
re-running Blender. A labelled descriptor is the opposite: it is authored judgement that nothing can
regenerate. It belongs in git, in full, forever.

## Verification

- **Stage 1**: a headless test that applies a suggestion and asserts the descriptor comes back with
  `by.kind == model` and the model's name; and that a hand edit to one axis leaves the others'
  provenance alone. Plus a round-trip through `layered_library`, the way the authored-lattice test
  works.
- **Stage 2**: `the_editor_boots_on_the_shipped_kit` already opens the real kit — it covers the
  loader change for free. Add one that writes a piece and asserts only that piece's file changed.
- **Stage 3**: drive the batch in the harness and assert the generated message names the model and
  the count. Do not shell out to git in a test; assert the message.
- **Stage 4**: nothing new — it reads what Stages 1–3 write.

## The order I would take it

Stage 1 first, alone. It is small, it stops the daily loss of information, and every later stage is
worth more once the data is there. Stage 2 next, because it is what "version control" actually
means here and it is mechanical. Stage 3 when a labelling run happens often enough to be annoying.
Stage 4 probably never, or much later.
