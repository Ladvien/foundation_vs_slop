# Which low-level solver backs `Problem::solve`

**Written 2026-08-10** for `docs/2026-08-10-constraint-solver-plan.md` §5.1. Read that plan first — this document only answers its first open question.

**Recommendation: `batsat 0.6.0`, reached through a back-end-only sibling crate rather than linked into `emerge-core` directly.** The argument is in §5. The strongest case against it is in §6.

**The corpus cannot help here and this document does not pretend otherwise.** `docs/research/2026-08-10-pcg-solver-corpus.md` §7 searched for it directly and found nothing: no pure-Rust solver literature, no solver-determinism literature, no cross-platform reproducibility work. Its one Rust-and-solvers document is a verification thesis about an unoptimised research SMT solver whose formalisation *"does not specify data structures or even a deterministic process for execution."* So everything below is measurement and source-reading, and every claim says which.

---

## 0. How this was checked

Three methods, and each claim below says which one it rests on.

1. **crates.io API** for versions, release dates, licences and yank status; **`gh api`** for repository push dates and open issues. Checked 2026-08-10.
2. **Reading the shipped source.** Every candidate's `.crate` tarball was downloaded and unpacked, and the *published* source read — not the repository's `main`, which is not what Cargo would compile. This is how the two disqualifying findings in §2 were made; neither is documented anywhere a reader would find it.
3. **A measurement.** A throwaway Cargo project outside this repo encodes one fixed CNF shaped like the plan's L2/L3 and hands the identical clause list to each solver, fingerprinting the returned model with FNV-1a. The instance is satisfiable with astronomically many models on purpose — so **which** model comes back is a sharp fingerprint of internal search order, and a hash that moves between processes is exactly the failure we are testing for.

The probe encodes: exactly-one prototype per cell (one-hot, pairwise at-most-one), a fixed pseudo-random ~55%-dense support relation across horizontal and vertical seams, a Sinz (2005) sequential-counter cardinality bound, and a founded-reachability rank encoding with `outside[c]` justified by a strictly-lower-ranked open neighbour. Grid size, prototype count and the cardinality bound are environment variables, so the same binary covers the plan's target shape and larger stress sizes.

It lived in the session scratchpad and is **gone** — it was a measuring instrument, not an artefact. The description above is enough to rebuild it, and §2.5 records what it measured.

---

## 1. The candidates

Versions, dates and licences are from the crates.io API; "repo last commit" is `gh api repos/<r>/commits`; "transitive deps" is `cargo tree --edges normal` in the probe workspace, counting unique crates excluding the candidate itself.

| Crate | Version | Published | Licence | Repo last commit | Transitive deps | Downloads |
|---|---|---|---|---|---|---|
| `varisat` | 0.2.2 | 2020-09-09 | MIT OR Apache-2.0 | 2022-11-02 | **32** | 7,426,135 |
| `splr` | 0.17.2 | 2024-02-04 | MPL-2.0 | 2026-03-14 | 1 (`bitflags`) | 49,065 |
| `batsat` | 0.6.0 | 2025-01-20 | MIT | **2026-05-05** | **1** (`bit-vec 0.5.1`) | 87,477 |
| `cadical` | 0.1.16 | 2025-04-24 | MIT | 2025-05-08 | 0 Rust, 83 `.cpp` | 40,158 |
| `minisat` | 0.4.4 | 2019-11-17 | MIT | 2022-01-26 | `itertools` + `bindgen` | 26,339 |
| `rustsat` | 0.7.5 | 2026-01-30 | MIT | **2026-08-10** | many; **ships no solver** | 90,437 |
| `clingo` | 0.8.0 | 2023-10-20 | MIT | 2024-01-08 | `clingo-sys 0.7.2` (33 MB C++) | 76,067 |

### Capability matrix

"Native cardinality" means the solver has an `atMostK` constraint type that is not compiled to clauses. **No candidate has one**, which is unsurprising: Cooper hit the same wall and said so — *"since PySat only supported hard native atMostK constraints, all such soft constraints must be encoded"* (corpus §1.1 footnote †). Encoding `count` is therefore what Sturgeon does too, not a compromise forced by this choice.

| | Incremental | Assumptions | Unsat core | Native cardinality | Deterministic budget |
|---|---|---|---|---|---|
| `varisat` | yes | `assume(&[Lit])` | `failed_core()` | no | **none at all** |
| `batsat` | yes (IPASIR) | `solve_limited(assumps)` | `unsat_core()` | no | **conflict + propagation** |
| `cadical` | yes | `solve_with(assumps)` | `failed(lit)` | no | `set_limit("conflicts"/…)` |
| `minisat` | yes | yes | yes | no | conflict budget |
| `splr` | feature-gated | feature-gated | — | no | wall clock only |

Two entries in the last column decide more than the rest of the table put together, and §2.2 and §2.3 are why.

### Per-crate notes

**`varisat 0.2.2`** — CDCL, ~7,000 lines, MIT OR Apache-2.0 with both licence files shipped. `Solver` implements `ExtendFormula`, so `new_var()` / `add_clause(&[Lit])` / `solve() -> Result<bool, SolverError>` / `assume()` / `failed_core()` / `model()`. It is the most-downloaded Rust SAT crate by a factor of eighty, and it is **effectively unmaintained**: last release 2020-09-09, last commit 2022-11-02, 25 open issues of which every one is a feature request or a docs task — none is a soundness or determinism report. `rand` appears in the source only inside `#[cfg(test)] mod tests` and is a dev-dependency in the manifest, so no RNG ships.

**`batsat 0.6.0`** — a fork of `ratsat`, itself a Rust reimplementation of MiniSat. MIT, actively maintained (last commit 2026-05-05). Its README advertises exactly the two things a MaxSAT loop over soft constraints needs: *"easy access to unsat-cores (as subset of assumptions)"* and *"ipasir interface for incremental solving"*. Total dependency footprint is **one crate**, `bit-vec 0.5.1`. Caveat: the published tarball contains `Cargo.toml`, `README.md` and `src` and **no LICENSE file** — the licence is declared as `license = "MIT"` in the manifest and stated in the README, and GitHub's detector reports `NOASSERTION`. That is a hygiene gap to note, not a licensing obstacle.

**`cadical 0.1.16`** — Rust bindings around a vendored **CaDiCaL 1.9.5** (83 `.cpp` files), built by `cc` at C++17, `-O3`, `NDEBUG`. Zero Rust dependencies, which flatters the tree and hides that it is a C++ toolchain requirement. Genuinely the strongest solver in the list on hard instances.

**`minisat 0.4.4`** — **does not build here.** Its `build.rs` runs `bindgen 0.42.3` (a 2018-era release), which needs `libclang`; the probe build failed with `Unable to find libclang: … set the LIBCLANG_PATH environment variable`. That is a hard cross-platform build burden for a crate whose last release was 2019-11-17 and last commit 2022-01-26. It bundles both MiniSat and glucose-syrup. Not a candidate.

**`splr 0.17.2`** — pure Rust, MPL-2.0, based on Glucose 4.1, one dependency (`bitflags`). Disqualified in §2.2, and separately hobbled: **assumptions live behind the non-default `support_user_assumption` feature and incremental solving behind the non-default `incremental_solver` feature**, so the two capabilities the soft-constraint loop needs are both opt-in. Repository shows a `Version 0.18.0` commit dated 2026-03-14 that has not been published to crates.io.

**`rustsat 0.7.5`** — worth naming because it looks like the answer and is not. It is an *encoding library plus solver interfaces*: `src/encodings/{am1, card, pb, totdb, nodedb}` is the best collection of cardinality and pseudo-Boolean encodings in the Rust ecosystem, actively developed (last commit the morning this was written). But `src/solvers.rs` documents that *"Solvers are available through separate crates"* — every one of them (`rustsat-cadical`, `rustsat-kissat`, `rustsat-minisat`, `rustsat-glucose`) wraps C or C++. Taking `rustsat` for its encodings alone still drags in `anyhow`, `nom`, `itertools`, `thiserror` and `tempfile`; the last of those touches the filesystem, which is not something a leaf crate in this workspace should acquire silently. **Its encodings are worth reading while implementing `count`** — that is the sibling document `-constraint-encodings.md`'s business — but it is not the back-end.

---

## 2. Determinism — the deciding criterion

The plan's §6 states the constraint: *"The solver is deterministic given identical input, so variety must come from varying the problem per seed."* The corpus independently reaches the same conclusion from the ASP side — Nelson & Smith's approximate-optimisation trick is *"stopping the solver once it gets close enough or runs for enough time"*, and the corpus doc's §3.2 flags it: *"wall-clock cannot appear anywhere that has to hash-match. Budget in conflicts or decisions."*

So the question is not "is it deterministic in practice" but "is there any input to the search other than the formula". Four things can be: an RNG, a hash with a per-process seed, threads, and a clock.

### 2.1 `varisat` — deterministic, with one hazard that turns out to be safe

No RNG ships (§1). No threads, no `Instant`, no `SystemTime` anywhere in the library. Restarts use a Luby sequence, which is a pure function of the restart index.

The hazard is real and worth recording because it is the exact shape `tests/determinism_lint.rs` exists to catch. `src/variables.rs` keeps three free-lists and allocates from them by **iteration order**:

```rust
pub fn next_unmapped_solver(&self) -> Var {
    self.solver_freelist
        .iter()
        .next()
        .cloned()
        .unwrap_or_else(|| Var::from_index(self.solver_watermark()))
}
```

`iter().next()` on a hash set is a last-writer-wins pick with no total order — and this is not a dormant path: `solver_freelist` is populated by `remove_solver_var`, which `unit_simplify.rs:195` calls during **ordinary internal simplification**, not in response to any user action. Every variable fixed by a unit clause and removed feeds this list, and the next variable mapping draws from it.

It is nonetheless safe, for one reason: line 3 of the file is

```rust
use rustc_hash::FxHashSet as HashSet;
```

`rustc-hash 1.1.0` defines its multiplier as a compile-time constant (`const K: usize = 0x517cc1b727220a95` on 64-bit, `0x9e3779b9` on 32-bit) with no `RandomState` and no OS entropy anywhere in the crate. Iteration order is therefore a deterministic function of the inserted keys and the growth history, identical across processes and across every 64-bit target. **Had that one import read `std::collections::HashSet`, varisat would be disqualified** — and nothing in its documentation tells you which it is.

The genuine defect is the other column of the table: **varisat has no resource limit of any kind.** No timeout, no conflict budget, no propagation budget — `config.rs` and `schedule.rs` contain none, and issue #111 ("Solving timeout") has been open since 2019-08-21. A pathological instance runs until the process is killed, with no reproducible way to give up. For a solver sitting behind an editor's interactive path, that is a real operational problem, and the only fix available is a wall clock in the caller, which reintroduces exactly what §2.2 disqualifies.

### 2.2 `splr` — disqualified, from the search loop

`src/solver/search.rs:266-272`, in the main search loop, at every stage boundary:

```rust
if state.stm.stage_ended(num_learnt) {
    if let Some(p) = state.elapsed() {
        if 1.0 <= p {
            return Err(SolverError::TimeOut);
        }
    } else {
        return Err(SolverError::UndescribedError);
    }
```

and `state.rs:375-379`:

```rust
fn elapsed(&self) -> Option<f64> {
    Some(
        self.start.elapsed().as_secs_f64()
            / Duration::from_secs(self.config.c_timeout as u64).as_secs_f64(),
    )
}
```

`self.start` is an `Instant::now()` taken at construction. **Whether the solver returns an answer or an error is a function of wall-clock time**, so the same input on a slower machine, or on the same machine under load, can produce a different outcome. It cannot be turned off: `c_timeout` defaults to 5000.0 seconds, and setting it to 0 makes the divisor zero, the ratio infinite, and the very first stage boundary return `TimeOut`.

That is a second execution path selected by the clock, which is what "one path per feature, no fallbacks" forbids, and it is unreachable from the API — no configuration removes it. `splr` also depends on the `instant` crate precisely to keep this working under `wasm32`.

**Honesty about the measurement:** the probe never triggered this. At 135 ms against a 5000-second budget it was never going to. The disqualification is from reading the search loop, not from observing a divergence, and the probe's stable `splr` hashes in §2.5 should not be read as a clean bill of health.

### 2.3 `batsat` — deterministic, and the only one with the right kind of budget

MiniSat's randomisation is present and is **off by default and deterministic when on**. `core.rs:2483-2495`:

```
random_var_freq: 0.0,   // the RNG is never consulted for variable choice
rnd_pol:        false,
rnd_init_act:   false,
random_seed:    91648253.0,
```

and `drand` is MiniSat's float LCG over that seed field — a pure function, threaded through `&mut self.opts.random_seed`, with no entropy source. Grepping the whole library for `Instant`, `SystemTime`, `std::time`, `thread` and `rayon` returns **nothing**. There is no `std::collections::HashMap` or `HashSet` in the library at all.

The budget is the part that matters. `core.rs:980-983`:

```rust
fn within_budget(&self) -> bool {
    (self.v.conflict_budget < 0 || self.v.conflicts < self.v.conflict_budget as u64)
        && (self.v.propagation_budget < 0
            || self.v.propagations < self.v.propagation_budget as u64)
}
```

Both default to `-1`, so `solve_limited(&[])` is an unlimited solve. When set, they count **conflicts and propagations** — deterministic counters, not elapsed time. That is precisely the shape the corpus doc §3.2 says an anytime budget must have, and it is the mechanism that lets `Problem::solve` fail loudly and *reproducibly* on a pathological instance instead of either hanging forever (varisat) or answering differently depending on the machine (splr).

Caveat recorded rather than hidden: `batsat` contains panics — `expect("NaN activity")`, `expect("heap is empty")`, `expect("Watcher not found")`, `panic!("conflicts must have a least one literal")`. They are internal-invariant assertions, not input validation, so they fire on solver bugs rather than on malformed encodings. The project's no-panic rule cannot be enforced inside a dependency; what it can do is keep the encoding layer — the part under our control — total.

### 2.4 `cadical` — deterministic in the library, with an unmeasured cross-platform risk

The library search consults no clock. Grepping every `time ()` call site in CaDiCaL's sources leaves only `parse.cpp`, `profile.cpp`, `report.cpp`, `solver.cpp` and `walk.cpp`, and in every case the value goes into a statistic or a log string; the one place wall time gates a decision is `signal.cpp`'s `SIGALRM` handler, which belongs to the standalone binary's `--time` limit and is not reachable through the C API. No `pthread_create`, no `std::thread`, no OpenMP.

The RNG is deterministic in library use. `random.hpp` declares a constructor documented as *"Without argument use a machine, process and time dependent seed"* and `random.cpp:202` implements it by hashing the machine identifier, network addresses, clock cycles, the process id and the time — but that constructor is used **only in `mobical.cpp`**, the model-based fuzzer, which `build.rs` explicitly excludes from the build. The default-enabled random walk (`OPTION(walk, 1, …)`) constructs `random (internal->opts.seed)` with `seed` defaulting to 0 and perturbs it by `internal->stats.walk.count`, a deterministic counter.

So CaDiCaL is deterministic **for a fixed binary**. The risk is what happens when the binary is not fixed, and it is a risk I did not measure.

CaDiCaL steers restarts with `double` EMAs — `restart.cpp:72-74` compares `averages.current.glue.fast` against `margin * averages.current.glue.slow`. EMA updates have the shape `value += alpha * (next - value)`, which is a textbook candidate for fused-multiply-add contraction. `build.rs` sets `-O3` and passes no `-ffp-contract` flag, so the host compiler's default applies, and that default differs between GCC and Clang and between targets. A single-ulp difference in one EMA flips one restart decision, which changes the search path, which returns a different — still correct — satisfying assignment. The project's replay invariant cannot absorb that.

**I did not observe this.** I have one platform (aarch64 macOS) and one compiler. The mechanism is real and the flags are as described; whether it bites across this project's actual hosts is unverified, and verifying it would mean building the same instance on `big` (x86_64 Linux) and a Pi (aarch64 Linux) and comparing hashes. Rust has no equivalent exposure: it guarantees IEEE-754 semantics and never contracts `a*b + c` into an FMA without an explicit `mul_add` call, and neither `varisat` nor `batsat` calls `mul_add` anywhere.

### 2.5 What the probe measured

Identical CNF, fresh process per row, model fingerprinted with FNV-1a over the `place` and `outside` variables in encoding order.

**The plan's actual target shape — 12×12 grid, 36 prototypes — is 12,064 variables and 114,811 clauses:**

| Solver | Model hash | Time |
|---|---|---|
| `varisat` | `421a6a2e4123ca9f` | 13.0 ms |
| `batsat` | `47119f8bb0b478c5` | 9.5 ms |
| `cadical` | `ea5452d16b953bb1` | 18.0 ms |
| `splr` | `fd9739b7397773a7` | 135.5 ms |

Every solver returns a *different* model from every other, which is expected and harmless — what matters is that each returns **the same** model every time. Across **8 fresh processes on an idle box** and **6 fresh processes with the machine saturated by 12 spinners**, at 12×12×8, every hash was byte-identical for all four solvers. Running the probe only on an idle box would have proved nothing, which is why the loaded run is there.

Scaling, three runs each, hashes stable at every size:

| Instance | Vars / clauses | `batsat` | `varisat` | `cadical` | `splr` |
|---|---|---|---|---|---|
| 12×12, 8 protos | 8,032 / 20,731 | 1.4–1.6 ms | 2.3–3.1 ms | 2.4–3.4 ms | 17.9–21.1 ms |
| 12×12, **36 protos** | 12,064 / 114,811 | 9.5 ms | 13.0 ms | 18.0 ms | 135.5 ms |
| 24×24, 16 protos | 106,192 / 285,139 | 32–39 ms | 37–46 ms | 49–53 ms | 1,131–1,191 ms |
| 32×32, 24 protos | 238,096 / 765,707 | 83–86 ms | 105–113 ms | 135–140 ms | 1,570–1,801 ms |

**Two readings, and the second is the one that decides.**

First: CaDiCaL is the *slowest* of the three serious candidates here, and that is not a claim it is a weaker solver. These instances are easy and massively under-constrained — they are satisfied without real search, so CaDiCaL's inprocessing pays setup cost it never recovers. On a hard instance the ranking would invert. The corpus supports treating this regime as the expected one: Cooper's 12×12-equivalent sizes sat inside an *"about 10s or less by most solvers"* band **in Python**, and Karth & Smith measured **zero conflicts** solving WFC's own problem in clingo across three scenarios of up to 698,544 variables.

Second, and this is the point: **at the plan's real size every candidate is fast enough by two to three orders of magnitude.** Performance does not decide this. Determinism and dependency weight do.

The honest limit on all of it: this is one platform, aarch64 macOS 26.4.1, one Rust toolchain, one C++ compiler. Determinism *across* platforms is unmeasured for every candidate. What §2.4 argues is that a pure-Rust solver has no mechanism by which it could differ, and a C++ one does.

---

## 3. clingo / ASP

The plan's §L3 offers one reason to pay for clingo, and it is the whole case: *"ASP's minimal-model semantics gives this free; that is why Cooper used clingo."*

**That reason does not survive the corpus.** `docs/research/2026-08-10-pcg-solver-corpus.md` §1.3 read Sturgeon and found Cooper's reachability encoding admits unfounded cycles **on every back-end, clingo included** — the paper draws them, in gold, in Figure 1(d), and describes them in prose: *"in addition to the path from start to goal, the solver can include additional closed cycles off the main path in the solution."* The reason is structural, and the corpus doc states it precisely: foundedness comes from **head polarity, not solver family**. Cooper's support rule has a negative head, which makes it an integrity constraint, and integrity constraints found nothing on any back-end. A solver-agnostic mid-level API is *designed* to be unable to express the recursive rule with a positive head that ASP semantics would actually reward.

So the rank encoding the plan calls *"the single most likely place to get it subtly wrong"* is **mandatory regardless of solver**. Choosing clingo does not remove it. The one thing that would is abandoning Sturgeon's solver-agnostic API entirely and writing Nelson & Smith's `linked/2` as a genuine ASP program — a different architecture from the one the plan's §1 is built on, and a decision well above this document's pay grade. If that is ever on the table it should be its own plan, not a back-end swap.

With the foundedness advantage gone, what remains is cost, and it is all on one side.

- **`clingo-rs 0.8.0` wraps clingo 5.6.2 and is stale.** Last commit to the default branch 2024-01-08; two of its four open issues are dependency bumps. It has been effectively idle for two and a half years.
- **The default build links against a system clingo.** Its README: *"Per default the crate uses the clingo library via dynamic linking. It is assumed that a clingo dynamic library is installed on the system. While compile time the environment variable `CLINGO_LIBRARY_PATH` must be set."* For a project whose reproducibility argument rests on pinning what gets built, a dynamically-linked system library that no manifest names is the wrong shape of dependency. It also means every machine — `mac_air`, `bmb`, `big`, and any Pi that runs the headless search — needs a matching clingo installed out-of-band before `cargo build` works.
- **The static-linking alternative needs cmake and a C++14 compiler.** `clingo-sys 0.7.2` vendors 33 MB of clingo source plus a `bugfix.patch` and builds it with cmake ≥ 3.1 (3.18 recommended). That is a heavier build dependency than anything currently in this workspace.
- **The corpus already recorded this exact friction from the field.** Xu & Morris (`10.5121_ijaia.2023.14302`): *"answer set solvers such as clingo need to go through complicated installation steps on any local machine in which it is used to function properly. Therefore, it becomes difficult for other implementations to work for the wider public."* Their workaround was to host clingo on a server.
- **The ASP frontend was the slowest thing Cooper measured.** *"clingo-fe and z3 were much slower than the other solvers […] thus we excluded them from further evaluations."* Only `clingo-be`, which bypasses grounding by driving the backend API directly, was competitive. `clingo-rs` does expose that API — `Backend::{rule, weight_rule, minimize, assume, external, heuristic}` — so the fast path exists, but taking it means writing against the backend rather than against ASP source, which discards most of what makes ASP pleasant to write.
- **`ALLOWED_DEPS` would have to admit a C++ build system.** `crates/emerge-core/tests/engine_free.rs` currently permits six names. Adding `clingo` adds `clingo-sys`, cmake, and a vendored C++ tree to a crate whose entire stated purpose is that *"three consumers share this crate — a game, an offline search that fans out across worker subprocesses, and a standalone editor — and none of them should have to agree on a renderer."* The headless search fanning out across worker subprocesses on a Pi is precisely the consumer that a system-library dependency hurts most.
- Minor, but in a repo with a no-panic rule it is worth naming: `clingo-rs`'s `Backend` implements `Drop` with `panic!("Call to clingo_backend_end() failed")`. A panic in a destructor is not something a caller can handle.

**Verdict: reject.** Not because ASP is the wrong model — the corpus is clear that Nelson & Smith's recursive formulation is the encoding the plan's L3 actually wants — but because the specific advantage the plan invoked to justify the cost is not real under Sturgeon's API, and everything else about the dependency runs the wrong way.

---

## 4. Where it lives

**The tree moved while this was being written, and it moved in a way that makes this question much smaller.** As of this writing `crates/emerge-core/src/constraints.rs` exists — 1,186 lines carrying L1 (`Var`, `Lit`, `Problem`, `Solution`) and L2 (`tile_rules`, `domain_rules`, `pattern_rules`), plus a reference enumerator that checks the encodings by exhaustive search — and **`crates/emerge-core/Cargo.toml` is untouched**, still exactly the six allowed dependencies.

That is the right outcome and it should be preserved: building a CNF is plain data and arithmetic, which is what this crate is for. The encoding layer costs the ratchet nothing and belongs where it is.

So the only thing that needs a home is the **back-end** — the function that turns a clause list into an assignment. And `Problem` already exposes the seam for one: `vars()`, `clauses()`, `clause(i)` and `iter_clauses()` are public accessors, so a solver can consume a `Problem` without `Problem` knowing which solver it is.

Read `crates/emerge-core/tests/engine_free.rs` for what the ratchet defends. It is not a general dependency-minimisation rule; the test's own doc comment says the purpose is that the crate stays consumable *"by the game, by the offline search, and by a standalone editor without any of them agreeing on a renderer"*, and `FORBIDDEN_DEP_MARKERS` is `bevy, avian, wgpu, winit`. A pure-Rust SAT solver with no I/O, no threads and no engine does not violate that purpose.

But the test also sets the standard for what a widening argument has to look like, in the `det_rng` note:

> It is on this list because **the dependency surface did not actually grow**: `det_rng`'s own manifest declares `rand` and `rand_chacha` and nothing else, both of which are already here. […] That is the argument this list is supposed to cost. A dependency that pulled in anything not already on this line would need a different one.

`batsat` needs that different argument, because it genuinely grows the surface — by two crates, `batsat` and `bit-vec`. That is the honest framing, and it is worth setting against the alternative: **`varisat` would grow it by thirty-two**, including `syn` at two major versions, `serde_derive`, `proc-macro2`, `quote`, `regex` and `aho-corasick`. Dropping a proc-macro toolchain and a regex engine into the workspace's most carefully-bounded leaf crate is not a small ask, and it is the clearest single reason to prefer `batsat` over the more-downloaded option.

Two placements are available, and with `constraints.rs` already dependency-free they differ by exactly one crate.

**A. Add `batsat` to `emerge-core`'s `ALLOWED_DEPS` and implement `Problem::solve` in `constraints.rs`.** One line of ratchet edit, two crates of new surface (`batsat`, `bit-vec`), and the back-end sits next to `wfc.rs` and `placement/solvers/` where its siblings already live — the crate's own description already claims *"a constraint IR with three solver backends"*. This is the plan's §2 as written, and it is the smallest possible diff.

**B. A back-end-only sibling crate, with `emerge-core` depending on it.** Not L1 and not L2 — those stay exactly where they now are. The new crate is only the thing `Problem::solve` delegates to: a function from a variable count and a clause list to an assignment or a named refusal, with a deterministic conflict budget. It would know nothing about grids, prototypes, `Problem`, or `emerge-core`; its entire dependency list is `batsat`.

This is what the root `CLAUDE.md` asks for — *"When creating new features, attempt to use Bevy's plugin pattern as much as possible. Create separate workspace crates"* — and the kernel/facade split it describes lands cleanly:

- **The back-end crate** takes clauses and returns bits. It is the reusable kernel in the exact sense the root `CLAUDE.md` means: *"A crate is for the reusable kernel, not the game content around it."* It needs no RNG either, since the plan's per-seed variety comes from **seeded soft weights** computed by the caller and passed in as `weight: Option<u32>`.
- **`emerge-core::constraints`** keeps `Problem`, `Solution` and the grid rules, and `Problem::solve` becomes the facade — the same shape as `Stig` and `LightField` wrapping their crates while every call site stays put.
- **L3/L4** (`Wishes`, `solve_constrained(map, grammar, faces, …)`) name `Map`, `Grammar` and `Interface` and stay in `emerge-core` regardless.

The cost is the full new-crate checklist from the root `CLAUDE.md`, all of it enforced by `scripts/mirror_crates.sh`: `README.md` opening with the "Vibe Coded" warning then the mirror notice then an Examples section, 1–3 runnable examples (terminal-output ones, since a SAT solver has nothing to show a GPU), `CLAUDE.md`, `Cargo.toml` with `publish = false`, `LICENSE-MIT` + `LICENSE-APACHE`, a `tests/leaf.rs` ratchet naming `batsat`, an entry in `CRATES` in `scripts/mirror_crates.sh`, and `gh repo create` under an idiomatic name. Engine-free rather than Bevy-facing, so the name should be plain, in the convention `det_rng` and `map_elites` set — something like `cnf_solve` or `det_sat`.

**B still costs a one-line `ALLOWED_DEPS` edit**, because `emerge-core` has to name the new crate. What it buys is that the widening is to a workspace sibling carrying *its own* `tests/leaf.rs`, which is precisely the arrangement the ratchet's doc comment blesses: *"It also carries its own `tests/leaf.rs`, so the boundary is policed on that side rather than taken on trust from here."* It also means the one dependency that could ever go stale or need swapping is named in one file, behind a function signature that mentions no solver.

**I recommend B, weakly, and would not argue with A.** The deciding consideration is that the back-end is the only part of this design likely to be *replaced* — §6 is an argument about exactly that — and isolating a replaceable component behind a crate boundary is cheaper to do before it is written than after. But the gap between the options is now one crate and one ratchet line rather than a re-layering, because the dependency-free encoding layer already landed in the right place. If the extra crate's checklist is not worth it to whoever implements this, **A gives up little**, and the two-crate widening argument in this section is the one to write into `engine_free.rs`.

---

## 5. Recommendation

**`batsat 0.6.0`, behind a back-end-only sibling crate that `emerge-core::constraints::Problem::solve` delegates to.**

The argument, in the order the evidence supports it:

1. **It is deterministic, and the determinism is auditable rather than promised.** No RNG consulted at default settings, and the RNG that exists is a seeded LCG over a constant field with no entropy source. No `Instant`, no `SystemTime`, no threads, no `std` hash map in the entire library. That is four sentences of grep output, not a vendor claim — and it held across fourteen fresh processes, idle and under full CPU saturation, at four problem sizes.

2. **It is the only candidate whose "give up" mechanism is deterministic.** `varisat` cannot give up at all and has had an open timeout issue since 2019. `splr` gives up on a wall clock that cannot be disabled. `batsat` gives up on a conflict or propagation count — a reproducible bound, which is exactly what the corpus doc independently concluded an anytime budget must be. That converts the plan's requirement that *"a solver failure is a named refusal, not a degraded result"* into something a test can actually pin, because the same instance exhausts the same budget every time on every machine.

3. **It costs one transitive dependency.** `bit-vec`. Against `varisat`'s thirty-two, in the workspace's most tightly-bounded leaf, this is the difference between a widening that can be argued in a paragraph and one that cannot.

4. **It has the two capabilities the soft-constraint story needs, and advertises them.** `unmet()` is a MaxSAT objective, and none of these solvers is a MaxSAT solver — the objective has to be built as a relaxation-variable loop over incremental SAT calls under assumptions, the RC2/OLL shape. `batsat`'s README lists *"easy access to unsat-cores (as subset of assumptions)"* and an IPASIR incremental interface as explicit goals, and `unsat_core()` / `solve_limited(assumps)` are on the public trait.

5. **Performance is not a consideration and should not be allowed to become one.** 9.5 ms at the plan's actual target shape, fastest of the four measured, and the margin to anything that matters is three orders of magnitude. It happens to win; it would be the right choice even if it lost.

6. **It is maintained.** Last commit 2026-05-05, against `varisat`'s 2022 and `minisat-rs`'s 2022.

**Adopt it behind the plan's own §3 checkpoint**, which `constraints.rs` has already built the encoding half of. Wire the back-end to the tile and pattern rules that exist, reproduce today's wall-confetti behaviour through the new machinery, and confirm the model hash is stable across processes *before* any global constraint exists. If the encoding is wrong, that is where it shows up cheaply — and `constraints.rs`'s reference enumerator gives a second, solver-independent opinion on the same question, which is worth more than either alone.

**Two things to do on the way in**, both small and both closing gaps this document found:

- Pin the determinism claim with a test that solves a fixed instance and asserts an exact model hash, in the deterministic-core layer where the exact-hash oracle is allowed. That is the only thing that would catch a future `batsat` release changing its default heuristics — a version bump moving the model is not a bug in `batsat`, but it *is* a goldens-moving event here, and it should fail loudly rather than surface as a level that quietly changed shape.
- Verify the cross-platform half on `big` (x86_64 Linux) before shipping, since §2.5 only measured aarch64 macOS. The pure-Rust argument in §2.4 says there is no mechanism for divergence; that is reasoning, and one run on a second architecture would make it a measurement.

---

## 6. The strongest argument against

**`batsat` is a small crate with 32 GitHub stars, and this project would be betting a load-bearing invariant on one maintainer.** `varisat` has 7.4 million downloads to `batsat`'s 87 thousand; CaDiCaL is the product of two decades of competition-driven engineering by Armin Biere and is the solver a specialist would reach for without hesitating. Choosing the fork-of-a-reimplementation-of-MiniSat over both of them is choosing the least-proven option in the list, and the honest reason is dependency hygiene and a budget API — neither of which is about solving satisfiability well.

The specific risk is not that `batsat` is wrong; MiniSat's algorithm is thirty years old and thoroughly understood, and an incorrect model would be caught immediately by validating the assignment against the encoding (which the enclosure work should do anyway, since `range::measure`'s flood fill is right there as an independent oracle). The risk is that a hard instance — Cooper's `repair` shape, the soft-constraint-over-an-existing-artefact case that the corpus doc §1.4 warns is *"structurally the repair case, not the generate case"* at 10.5 minutes median and 24.8 minutes maximum — lands on a solver with none of CaDiCaL's inprocessing, and the answer is "it does not finish". At that point the measured 9.5 ms is worthless, because it was measured on the easy regime, and the fix is a solver swap under a shipped feature.

There is a real counter to that counter, and it is why the recommendation stands: **the L1 API is the seam.** Sturgeon's whole architectural claim is that the mid-level API is solver-agnostic by construction, and this document's §2.5 already exercised four different solvers behind one identical clause list. If `batsat` runs out of road, replacing it is one file in one crate, and the checkpoint in the plan's §3 plus a model-hash test would make the swap's effect visible rather than mysterious. Choosing the lighter dependency first and keeping the seam honest is cheaper than choosing the heavy one first and discovering the seam was never tested.

The weaker counter-argument, worth stating so it is not mistaken for a strong one: `cadical` looks appealing because its Rust dependency count is zero. That number is an artefact of `cargo tree` not counting a `cc` build dependency or 83 files of vendored C++, and §2.4's FMA-contraction exposure is a cross-platform reproducibility risk that this project's core invariant is unusually sensitive to and that I could not rule out.

---

## 7. Rejected, with reasons

| Option | Why not |
|---|---|
| **`clingo` / ASP** | The foundedness advantage the plan invoked does not exist under Sturgeon's API (corpus §1.3) — the rank encoding is mandatory either way. What remains is a stale binding (clingo 5.6.2, idle since 2024-01), a default build that dynamically links a system library no manifest pins, a static build needing cmake and 33 MB of vendored C++, and a documented field complaint about install burden. Cooper measured the ASP frontend as among the *slowest* back-ends. |
| **`varisat`** | Deterministic — verified, and it survives only because one import reads `FxHashSet` rather than `std`'s. But 32 transitive crates including two `syn` majors and `regex`, no maintenance since 2022, and **no resource limit of any kind**, so a pathological instance cannot be abandoned reproducibly. |
| **`splr`** | Disqualified. `search.rs:266-272` returns `SolverError::TimeOut` based on `Instant::now()`, and the timeout cannot be disabled — setting it to zero makes the first stage boundary time out immediately. Assumptions and incremental solving are both behind non-default features. Also 10–14× slower than the alternatives at every size measured. |
| **`cadical`** | The best solver here and deterministic for a fixed binary, but its double-valued restart EMAs are built with the host compiler's default FP contraction, so identical source on different platforms may not be the identical program. Unmeasured, but the project's invariant is exactly what such a divergence breaks. Also the slowest of the three serious candidates on this easy regime. |
| **`minisat`** | Does not build: `bindgen 0.42.3` needs `libclang` and failed here with no `LIBCLANG_PATH`. Last release 2019, last commit 2022. |
| **`rustsat`** | Ships no solver — every back-end is a separate C/C++ crate. Its `encodings/` module is genuinely the best cardinality/PB collection in Rust and should be **read** while implementing `count`, but taking the dependency adds `anyhow`, `nom`, `itertools`, `thiserror` and `tempfile` to a leaf crate. |
| **`varisat-utils`** | Has `add_exactly_one`, `add_at_most_one`, `exactly_k` and `make_sorting_network` — tempting for `count`. Version 0.2.0, published 2020, tied to `varisat`'s types. Not worth taking `varisat` for. |
| **`screwsat`** | Calls `Instant::now()` in `solve` for a timeout. Last release 2021. Not evaluated further. |

---

## 8. What I could not verify

Stated plainly, so nothing here is mistaken for measurement.

- **Cross-platform determinism, for every candidate.** Everything in §2.5 is aarch64 macOS 26.4.1, one toolchain, one C++ compiler. The §2.4 argument that pure Rust has no divergence mechanism and C++ does is reasoning from the language specifications and from `build.rs`, not an observation. One run of the same probe on `big` and on a Pi would settle it.
- **Whether CaDiCaL actually diverges under different FP contraction.** The EMA code and the absent `-ffp-contract` flag are as described; that a contraction difference propagates to a different model is a mechanism I did not demonstrate.
- **`splr`'s timeout firing.** Never triggered at 135 ms against a 5000-second budget. The disqualification is from the search loop's source.
- **Behaviour on hard instances.** Every measurement here is on satisfiable, under-constrained instances solved with little search — which the corpus says is the expected regime, but which is precisely *not* the regime where solver quality separates. Cooper's `repair` case is the shape that would test it and it was not reproduced.
- **`batsat` under incremental use with assumptions.** The API is present and the README claims it; the probe solved one-shot. The MaxSAT loop that `unmet()` requires is the thing that will exercise it, and it should be exercised at the §3 checkpoint rather than assumed.
- **Sturgeon's exact MaxSAT machinery.** Cooper names PySAT's `kmtotalizer` and `RC2` in the corpus doc's Table 1 transcription; I did not read the paper myself for this document and defer to `-pcg-solver-corpus.md` §1.1, which quotes it verbatim.
