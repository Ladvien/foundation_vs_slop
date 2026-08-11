//! **A mid-level constraint API over Boolean variables, and the grid rules written against it.**
//!
//! This is Cooper's Sturgeon architecture (`10.1609/aiide.v18i1.21944`), whose central claim is that
//! level generation needs *"only a few functions"* between the generator and whatever decides the
//! Booleans — which is why Sturgeon can run the same rules against SAT, Answer Set Programming and
//! SMT without the rules knowing. [`Problem`] is that seam here.
//!
//! # Why this exists at all
//!
//! `docs/research/2026-08-10-expressive-range.md` measured the composition grammar over the shipped
//! Site kit: **128 solves, zero enclosed regions.** Two explanations were then falsified rather than
//! argued. The metric can see a room (a hand-laid one scores enclosure 1.0) and the room is *legal*
//! under the learned support, so it is not the vocabulary; and sweeping `Empty`'s weight to 0.0 —
//! removing it from the output entirely — still leaves median enclosure at 0.000, so it is not the
//! sampling distribution either.
//!
//! What is left is the solver. WFC is a greedy, non-backtracking approximation of constraint solving
//! (Karth & Smith 2017), and a closed boundary is a property *over distance* that local pairwise
//! support cannot express — the term Sandhu, Chen & McCoy name (`10.1145/3337722.3337752`) when they
//! *"extend the local constraint reasoning by incorporating constraints that can work over any
//! distance"*. No reweighting of local choices produces one. That is the measurement, and this module
//! is the response to it.
//!
//! # The layers
//!
//! **L1** is everything above the `── L2` banner: [`Var`], [`Lit`], [`Problem`], [`Solution`]. It
//! knows nothing about grids. **L2** is below it: [`tile_rules`], [`domain_rules`], [`pattern_rules`]
//! — the rules a tiled region needs, written only in terms of L1.
//!
//! # Hard and soft
//!
//! Every constraint takes `weight: Option<u32>`. `None` is hard — the solver must satisfy it or
//! report that it cannot. `Some(w)` is soft: the constraint is guarded by a fresh indicator variable,
//! and [`Solution::unmet`] adds up `w` over every guard the solver left false.
//!
//! That is what turns a flat refusal into *"everything except the north corridor"*. It is also the
//! only mechanism by which a seed may change the output: the solver is a function of the problem, so
//! **variety comes from varying the problem** — seeded soft weights drawn from [`crate::rng`] — and
//! never from solver randomness, which would not survive a solver upgrade.
//!
//! # No panics, and no `unwrap`
//!
//! Every entry point takes indices that a caller could get wrong. Rather than indexing and panicking,
//! each returns a named `Result` or is documented as a no-op on a malformed shape. An impossible
//! constraint (`lo > hi`) is not a panic either: it emits the falsifying clause, so a *hard* one
//! makes the problem honestly unsatisfiable and a *soft* one honestly costs its weight.

/// A Boolean variable, identified by its index into a [`Problem`].
///
/// Opaque on purpose: a `Var` is only meaningful against the `Problem` that issued it, and an integer
/// that can be built from nowhere invites exactly the off-by-one this type exists to prevent.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Var(u32);

impl Var {
    /// This variable's index, for a back-end that needs to size an array by [`Problem::vars`].
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A variable together with a sign — the unit a clause is built from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Lit {
    var: Var,
    positive: bool,
}

impl Lit {
    /// The literal that is true when `v` is true.
    #[inline]
    pub fn pos(v: Var) -> Self {
        Lit { var: v, positive: true }
    }

    /// The literal that is true when `v` is false.
    #[inline]
    pub fn neg(v: Var) -> Self {
        Lit { var: v, positive: false }
    }

    /// The variable this literal is about.
    #[inline]
    pub fn var(self) -> Var {
        self.var
    }

    /// Whether this literal is the variable itself rather than its negation.
    #[inline]
    pub fn is_positive(self) -> bool {
        self.positive
    }

    /// Is this literal true under an assignment indexed by [`Var::index`]?
    ///
    /// `false` when the assignment is too short to say — a malformed assignment reads as
    /// unsatisfying rather than panicking.
    #[inline]
    pub fn holds(self, values: &[bool]) -> bool {
        match values.get(self.var.index()) {
            Some(&v) => v == self.positive,
            None => false,
        }
    }
}

impl std::ops::Not for Lit {
    type Output = Lit;
    #[inline]
    fn not(self) -> Lit {
        Lit { var: self.var, positive: !self.positive }
    }
}

/// One soft constraint: satisfied exactly when `guard` is true, and worth `weight` if it is not.
#[derive(Clone, Copy, Debug)]
struct Soft {
    guard: Var,
    weight: u32,
}

/// A set of Boolean variables, clauses over them, and soft constraints with weights.
///
/// Clauses are stored flat — `lits[starts[i]..starts[i + 1]]` is clause `i` — because a back-end
/// wants a contiguous CNF and a `Vec<Vec<Lit>>` at this scale is a hundred thousand allocations for
/// no gain.
#[derive(Debug)]
pub struct Problem {
    n_vars: u32,
    lits: Vec<Lit>,
    /// One more entry than there are clauses; `starts[0]` is 0 and the last entry is `lits.len()`.
    starts: Vec<u32>,
    soft: Vec<Soft>,
}

impl Default for Problem {
    fn default() -> Self {
        Self::new()
    }
}

impl Problem {
    /// An empty problem.
    ///
    /// Variable 0 is allocated immediately and asserted true, so [`Problem::always_true`] and
    /// [`Problem::always_false`] are ordinary literals rather than a second kind of thing every
    /// encoding below would have to special-case.
    pub fn new() -> Self {
        let mut p = Problem {
            n_vars: 0,
            lits: Vec::new(),
            starts: vec![0],
            soft: Vec::new(),
        };
        let t = p.var();
        // Pushed raw: `clause` recognises this very literal as already-true and would drop the unit
        // that makes it so.
        p.lits.push(Lit::pos(t));
        p.starts.push(p.lits.len() as u32);
        p
    }

    /// A fresh variable, unconstrained.
    pub fn var(&mut self) -> Var {
        let v = Var(self.n_vars);
        self.n_vars += 1;
        v
    }

    /// How many variables exist, including the auxiliaries the encodings allocate.
    #[inline]
    pub fn vars(&self) -> usize {
        self.n_vars as usize
    }

    /// How many clauses exist.
    #[inline]
    pub fn clauses(&self) -> usize {
        self.starts.len() - 1
    }

    /// Clause `i`, or `None` past the end.
    #[inline]
    pub fn clause(&self, i: usize) -> Option<&[Lit]> {
        let (a, b) = (*self.starts.get(i)?, *self.starts.get(i + 1)?);
        self.lits.get(a as usize..b as usize)
    }

    /// Every clause, in the order it was added.
    pub fn iter_clauses(&self) -> impl Iterator<Item = &[Lit]> + '_ {
        (0..self.clauses()).filter_map(|i| self.clause(i))
    }

    /// The literal that is true in every model.
    #[inline]
    pub fn always_true(&self) -> Lit {
        Lit::pos(Var(0))
    }

    /// The literal that is false in every model.
    #[inline]
    pub fn always_false(&self) -> Lit {
        Lit::neg(Var(0))
    }

    /// Add a clause, simplified.
    ///
    /// Three reductions, all of which are sound because variable 0 is unit-asserted true in
    /// [`Problem::new`]: a clause containing [`Problem::always_true`] or both polarities of one
    /// variable is a tautology and is dropped entirely; [`Problem::always_false`] and duplicate
    /// literals are dropped from the clause. **An empty result is kept** — that is the falsifying
    /// clause, and dropping it would silently turn an impossible constraint into no constraint.
    ///
    /// This is not an optimisation pass so much as what keeps the encodings below readable: the
    /// cardinality recurrence names `s[i][0]`, which *is* [`Problem::always_true`], and writing the
    /// base cases out by hand instead would be four more branches in the one place this module is
    /// most likely to be subtly wrong.
    pub fn add_clause(&mut self, lits: &[Lit]) {
        let t = self.always_true();
        let f = self.always_false();
        let mut out: Vec<Lit> = Vec::with_capacity(lits.len());
        for &l in lits {
            if l == t || out.contains(&!l) {
                return; // tautology
            }
            if l == f || out.contains(&l) {
                continue;
            }
            out.push(l);
        }
        self.lits.extend_from_slice(&out);
        self.starts.push(self.lits.len() as u32);
    }

    /// A literal that is true exactly when every literal in `lits` is true.
    ///
    /// No literals is [`Problem::always_true`] (the empty conjunction), and one literal is itself —
    /// neither allocates. Otherwise a fresh variable `y` is tied to the conjunction in both
    /// directions, `y ↔ ⋀lits`, so `y` may be used with either sign.
    pub fn conj(&mut self, lits: &[Lit]) -> Lit {
        match lits {
            [] => return self.always_true(),
            [only] => return *only,
            _ => {}
        }
        let y = Lit::pos(self.var());
        // y → lᵢ
        for &l in lits {
            self.add_clause(&[!y, l]);
        }
        // ⋀lᵢ → y
        let mut back: Vec<Lit> = lits.iter().map(|&l| !l).collect();
        back.push(y);
        self.add_clause(&back);
        y
    }

    /// Register a soft constraint and return the literal to guard its assertions with.
    ///
    /// `None` is hard, and guards with nothing: [`Problem::always_false`] disappears from any clause
    /// it is added to, so the caller writes one code path for both cases.
    fn guard(&mut self, weight: Option<u32>) -> Lit {
        match weight {
            None => self.always_false(),
            Some(w) => {
                let g = self.var();
                self.soft.push(Soft { guard: g, weight: w });
                Lit::neg(g)
            }
        }
    }

    /// **`lo ≤ Σ vs ≤ hi`.**
    ///
    /// A sequential unary counter in the family Sinz introduced (*Towards an Optimal CNF Encoding of
    /// Boolean Cardinality Constraints*, CP 2005, `10.1007/11564751_73`), carrying the *biconditional*
    /// `s[i][j] ↔ "at least j of vs[..i] are true"` rather than Sinz's one-directional form. The
    /// stronger semantics is what lets one structure serve both bounds — assert `s[n][lo]` for the
    /// floor and `¬s[n][hi+1]` for the ceiling — instead of two encodings that could disagree.
    ///
    /// Counting stops at `hi + 1`, so an exactly-one constraint over 14 prototypes costs two counter
    /// levels rather than fourteen. The counter's own defining clauses are **always hard**, even for a
    /// soft constraint: they define what the count *is*, and a solver allowed to relax them would buy
    /// the constraint by lying about the arithmetic. Only the two assertions are guarded.
    ///
    /// A vacuous request (`lo == 0` and `hi ≥ vs.len()`) emits nothing. An impossible one (`lo > hi`,
    /// or `lo` past what `vs` could ever reach) emits the falsifying clause.
    pub fn count(&mut self, vs: &[Lit], lo: u32, hi: u32, weight: Option<u32>) {
        let n = vs.len() as u32;
        if lo > hi || lo > n {
            let g = self.guard(weight);
            self.add_clause(&[g]); // hard: the empty clause. soft: forces the guard false.
            return;
        }
        if lo == 0 && hi >= n {
            return; // nothing to say
        }

        // Levels the assertions need: `lo` for the floor, `hi + 1` for the ceiling.
        let ceiling = if hi < n { hi + 1 } else { 0 };
        let k = lo.max(ceiling);
        let f = self.always_false();
        let s: Vec<Lit> = (0..n * k).map(|_| Lit::pos(self.var())).collect();
        // `s[i][j]` for i in 1..=n, j in 1..=k, with the two base cases the recurrence needs:
        // nothing has at least one of anything, and everything has at least zero.
        let at = |i: u32, j: u32| -> Lit {
            if j == 0 {
                Lit::pos(Var(0)) // always true
            } else if i == 0 {
                f
            } else {
                s[((i - 1) * k + (j - 1)) as usize]
            }
        };

        for i in 1..=n {
            let Some(&x) = vs.get((i - 1) as usize) else {
                continue; // unreachable: i ranges over vs
            };
            for j in 1..=k {
                let sij = at(i, j);
                let prev_j = at(i - 1, j);
                let prev_below = at(i - 1, j - 1);
                // s[i][j] ← s[i-1][j] ∨ (x ∧ s[i-1][j-1])
                self.add_clause(&[!prev_j, sij]);
                self.add_clause(&[!x, !prev_below, sij]);
                // s[i][j] → s[i-1][j] ∨ (x ∧ s[i-1][j-1])
                self.add_clause(&[!sij, prev_j, x]);
                self.add_clause(&[!sij, prev_j, prev_below]);
            }
        }

        let g = self.guard(weight);
        if lo > 0 {
            self.add_clause(&[g, at(n, lo)]);
        }
        if ceiling > 0 {
            self.add_clause(&[g, !at(n, ceiling)]);
        }
    }

    /// **`l → ⋁ ms`.**
    ///
    /// The workhorse: every adjacency rule in [`pattern_rules`] is one of these, and so is the
    /// justification step that makes reachability founded. An empty `ms` says `¬l`, which is the
    /// correct reading — *"this must be followed by one of nothing"*.
    pub fn implies_any(&mut self, l: Lit, ms: &[Lit], weight: Option<u32>) {
        let g = self.guard(weight);
        let mut clause = Vec::with_capacity(ms.len() + 2);
        clause.push(g);
        clause.push(!l);
        clause.extend_from_slice(ms);
        self.add_clause(&clause);
    }

    /// Does `values` satisfy every hard clause? `Err` names the first clause that it does not.
    ///
    /// **This is the referee, and it is deliberately independent of any solver.** A back-end that
    /// returns a wrong model and an encoding that is wrong in the same direction would agree with
    /// each other; this re-reads the clauses that were actually built. The tests below use it to
    /// check the encodings by exhaustive enumeration, with no solver in the picture at all.
    pub fn check(&self, values: &[bool]) -> Result<(), String> {
        if values.len() < self.vars() {
            return Err(format!(
                "constraints: an assignment of {} values cannot answer for {} variables",
                values.len(),
                self.vars()
            ));
        }
        for (i, clause) in self.iter_clauses().enumerate() {
            if !clause.iter().any(|l| l.holds(values)) {
                return Err(format!("constraints: clause {i} of {} is unsatisfied", self.clauses()));
            }
        }
        Ok(())
    }

    /// The total weight of the soft constraints `values` leaves unmet.
    ///
    /// Weights are `u32` and accumulate into a `u64`, so a problem would need four billion maximal
    /// soft constraints to overflow — but the sum is still saturating rather than wrapping, because a
    /// silently-wrapped objective is a generator that prefers the worst answer.
    pub fn unmet(&self, values: &[bool]) -> u64 {
        self.soft
            .iter()
            .filter(|s| !Lit::pos(s.guard).holds(values))
            .fold(0u64, |acc, s| acc.saturating_add(s.weight as u64))
    }

    /// How many soft constraints have been registered.
    #[inline]
    pub fn soft_count(&self) -> usize {
        self.soft.len()
    }

    /// **Solve.** `conflicts` bounds the search; 0 is unbounded.
    ///
    /// The back-end is [`deterministic_solver`], and this is the facade over it — the same shape
    /// `Stig` and `LightField` use, so swapping the solver is one function rather than every call
    /// site. Nothing above this line knows a solver exists, and the back-end knows nothing about
    /// grids: it is handed clause literals as plain integers.
    ///
    /// # There is no seed, and that is the design
    ///
    /// The plan's sketch wrote `solve(&mut self, seed: u64)`. A seed here would have nothing to do:
    /// the solver is a deterministic function of the clauses, and the whole point of §6's rule is
    /// that **variety comes from varying the problem, never from the solver**. The seed belongs
    /// where the problem is built — drawing soft weights from [`crate::rng`] — and a parameter that
    /// is accepted and ignored is worse than one that is absent, because it reads as a promise.
    ///
    /// # Every model is re-checked before it is returned
    ///
    /// Through [`Solution::from_assignment`], against the clauses this module built rather than
    /// against the solver's own account of them. A back-end returning a wrong model is the failure
    /// this cannot afford to absorb, and the check is linear in the clause count.
    ///
    /// # Soft constraints are refused rather than approximated
    ///
    /// [`Solution::unmet`] is a *minimisation* objective, and a plain satisfiability solver does not
    /// minimise — reaching an optimum takes a search over relaxations that is not built yet. Solving
    /// a problem that carries soft constraints therefore fails by name. It would be easy to return
    /// the first model that satisfies the hard clauses and report whatever it happened to leave
    /// unmet; that answer would be *valid* and not *optimal*, which is exactly the silent degradation
    /// one path per feature exists to forbid — a wall that could have been left standing, quietly not.
    pub fn solve(&self, conflicts: u64) -> Result<Solution, String> {
        if !self.soft.is_empty() {
            return Err(format!(
                "constraints: this problem carries {} soft constraint(s), and solving one needs an \
                 optimisation loop over relaxations that is not built yet. Returning a model that \
                 merely satisfies the hard clauses would report a cost nobody minimised.",
                self.soft.len()
            ));
        }
        let mut solver = deterministic_solver::Solver::new(self.vars())?;
        for clause in self.iter_clauses() {
            let lits: Vec<deterministic_solver::Literal> = clause.iter().map(to_dimacs).collect();
            solver.add_clause(&lits)?;
        }
        match solver.solve(&[], deterministic_solver::Budget::conflicts(conflicts))? {
            deterministic_solver::Answer::Satisfied(m) => {
                Solution::from_assignment(self, m.values().to_vec())
            }
            deterministic_solver::Answer::Unsatisfiable { .. } => Err(
                "constraints: no arrangement satisfies these rules. The grammar cannot tile this \
                 region under what has been asked of it — free some pinned cells, or extend the \
                 example so there are more ways to join things up."
                    .to_owned(),
            ),
            deterministic_solver::Answer::Exhausted => Err(format!(
                "constraints: gave up after {conflicts} conflicts without settling the question. \
                 That is not a statement that no arrangement exists — raise the budget to find out."
            )),
        }
    }
}

/// One [`Lit`] as the DIMACS integer the back-end speaks: variable 0 becomes 1, and the sign carries
/// the polarity.
fn to_dimacs(l: &Lit) -> deterministic_solver::Literal {
    let v = l.var().index() as deterministic_solver::Literal + 1;
    if l.is_positive() { v } else { -v }
}

/// What a solve produced: a value per variable, and what it could not satisfy.
#[derive(Clone, Debug)]
pub struct Solution {
    values: Vec<bool>,
    unmet: u64,
}

impl Solution {
    /// Build a solution from a full assignment, scoring it against the problem it answers.
    ///
    /// `Err` when the assignment does not satisfy every hard clause — so a back-end cannot hand back
    /// a model this module has not itself re-checked.
    pub fn from_assignment(problem: &Problem, values: Vec<bool>) -> Result<Self, String> {
        problem.check(&values)?;
        let unmet = problem.unmet(&values);
        Ok(Solution { values, unmet })
    }

    /// The value of `v`. A variable from another problem reads as `false` rather than panicking.
    #[inline]
    pub fn get(&self, v: Var) -> bool {
        self.values.get(v.index()).copied().unwrap_or(false)
    }

    /// The total weight of the soft constraints this solution leaves unmet. Zero is a perfect answer.
    #[inline]
    pub fn unmet(&self) -> u64 {
        self.unmet
    }

    /// The raw assignment, indexed by [`Var::index`].
    #[inline]
    pub fn values(&self) -> &[bool] {
        &self.values
    }
}

// ── L2 — rules over a tiled region ───────────────────────────────────────────────────────────────
//
// Sturgeon's four rule families, of which three are here. `tile_rules` and `domain_rules` are the
// tile family (what may go where); `pattern_rules` is the pattern family (what may go beside what),
// and it is exactly the relation `grammar::from_compositions` already learns. Distribution and
// reachability follow.

/// **Exactly one prototype per cell**, the choice variables everything else is written over.
///
/// Returns `place[cell][proto]`, so `place[c][p]` is true when cell `c` holds prototype `p`. This is
/// implicit in `wfc::collapse_grid` — a domain is a `u32` mask and collapsing sets one bit — and has
/// to be said out loud once the choice is a Boolean rather than a bitmask.
pub fn tile_rules(p: &mut Problem, cells: usize, protos: usize) -> Vec<Vec<Var>> {
    let mut place: Vec<Vec<Var>> = Vec::with_capacity(cells);
    for _ in 0..cells {
        let row: Vec<Var> = (0..protos).map(|_| p.var()).collect();
        let lits: Vec<Lit> = row.iter().map(|&v| Lit::pos(v)).collect();
        p.count(&lits, 1, 1, None);
        place.push(row);
    }
    place
}

/// **Narrow cells to a starting domain** — the unary constraint `wfc::collapse_grid` takes as
/// `initial`, and the mechanism `grammar::solve` uses to pin an owned placement.
///
/// `initial[c]` is a bitmask over prototypes; a clear bit forbids that prototype in that cell. A cell
/// whose mask is empty is left to [`tile_rules`]' exactly-one to refuse, which is the honest failure:
/// the region cannot be tiled, and the refusal names it rather than a domain function inventing a
/// substitute.
///
/// Bits at or above `protos` are ignored rather than treated as prototypes that do not exist.
pub fn domain_rules(p: &mut Problem, place: &[Vec<Var>], initial: &[u32]) {
    for (c, row) in place.iter().enumerate() {
        let Some(&mask) = initial.get(c) else {
            continue; // a short `initial` constrains only the cells it covers
        };
        for (proto, &v) in row.iter().enumerate() {
            if proto < 32 && mask & (1u32 << proto) == 0 {
                p.add_clause(&[Lit::neg(v)]);
            }
        }
    }
}

/// **What may sit beside what**, over a row-major `w × h` grid.
///
/// `support[dir][a]` is the bitmask of prototypes that may sit on `a`'s `dir` side, in `wfc`'s
/// N/E/S/W order — the same table `grammar::from_compositions` learns and `wfc::propagate` narrows
/// with. Each rule is *"if this cell is `a`, its `dir` neighbour is one of these"*, which is
/// [`Problem::implies_any`] once per cell, direction and prototype.
///
/// **This is arc consistency stated as constraints rather than enforced as a fixpoint**, and that is
/// the whole difference. `propagate` runs the same relation to a fixed point after every greedy
/// choice and can only ever narrow; here the relation is a clause the solver may backtrack over. The
/// rules are identical, so a faithful port must produce the same *kind* of output — that is the
/// checkpoint this file has to pass before any global constraint is worth writing.
///
/// Off-grid neighbours are skipped, exactly as `propagate` skips them.
pub fn pattern_rules(
    p: &mut Problem,
    place: &[Vec<Var>],
    support: &[Vec<u32>; 4],
    w: usize,
    h: usize,
) {
    // N, E, S, W as (dx, dz), matching `wfc::propagate`'s stencil: north is one row back.
    const STEPS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
    for z in 0..h {
        for x in 0..w {
            let c = z * w + x;
            let Some(row) = place.get(c) else {
                continue;
            };
            for (dir, (dx, dz)) in STEPS.iter().enumerate() {
                let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                if nx < 0 || nz < 0 || nx as usize >= w || nz as usize >= h {
                    continue;
                }
                let Some(neighbour) = place.get(nz as usize * w + nx as usize) else {
                    continue;
                };
                for (a, &va) in row.iter().enumerate() {
                    let Some(mask) = support.get(dir).and_then(|t| t.get(a)).copied() else {
                        continue;
                    };
                    let allowed: Vec<Lit> = neighbour
                        .iter()
                        .enumerate()
                        .filter(|(b, _)| *b < 32 && mask & (1u32 << *b) != 0)
                        .map(|(_, &vb)| Lit::pos(vb))
                        .collect();
                    p.implies_any(Lit::pos(va), &allowed, None);
                }
            }
        }
    }
}

/// **A tiled region encoded as Booleans, and the way back to a grid of prototype indices.**
///
/// The way back matters as much as the way out. `wfc::collapse_grid` answers with a `Vec<usize>`, and
/// `grammar::Solved::grid`, `range::measure` and the editor's stamp write-back all read that shape.
/// Keeping the translation here — rather than letting each caller pick the true bit out of
/// [`GridProblem::place`] — is what makes swapping the solver a swap rather than a rewrite.
#[derive(Debug)]
pub struct GridProblem {
    /// The clauses. Public because adding a rule to an encoded region is the whole point: enclosure
    /// and distribution are further calls against this, made after [`GridProblem::encode`] returns.
    pub problem: Problem,
    /// `place[cell][proto]`, with cells row-major (`z * width + x`) exactly as `wfc` indexes them.
    pub place: Vec<Vec<Var>>,
    pub width: usize,
    pub height: usize,
    protos: usize,
}

impl GridProblem {
    /// Encode a region: exactly one prototype per cell, the starting domains, and the adjacency rules.
    ///
    /// **This is the faithful port and nothing more.** It is the same three questions
    /// `wfc::collapse_grid` answers — what may go in a cell, what the caller has already pinned, and
    /// what may sit beside what — carrying no constraint that WFC could not express. Whether that
    /// reproduces today's behaviour through new machinery is the checkpoint the plan puts before any
    /// global constraint is worth writing (`docs/2026-08-10-constraint-solver-plan.md` §3), and it is
    /// a checkpoint precisely because a wrong encoding here would look like a solver that cannot do
    /// enclosure rather than like a bug.
    ///
    /// `initial[c]` is cell `c`'s starting domain as a bitmask, exactly as `collapse_grid` takes it —
    /// all bits set is fully permissive.
    pub fn encode(
        support: &[Vec<u32>; 4],
        initial: &[u32],
        width: usize,
        height: usize,
        protos: usize,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err(format!("constraints: a {width} x {height} region has no cells to fill"));
        }
        if protos == 0 {
            return Err("constraints: a grammar with no prototypes cannot tile anything".to_owned());
        }
        // The domain masks are `u32`, the same ceiling `grammar::MAX_PROTOTYPES` states and for the
        // same reason. A wider alphabet is a real change to make, not one to make by accident here.
        if protos > 32 {
            return Err(format!(
                "constraints: {protos} prototypes is more than a 32-bit domain mask can carry"
            ));
        }
        let cells = width * height;
        if initial.len() != cells {
            return Err(format!(
                "constraints: {} starting domains for a {width} x {height} region of {cells} cells",
                initial.len()
            ));
        }
        for (dir, table) in support.iter().enumerate() {
            if table.len() != protos {
                return Err(format!(
                    "constraints: the support table for direction {dir} covers {} prototypes, not {protos}",
                    table.len()
                ));
            }
        }

        let mut problem = Problem::new();
        let place = tile_rules(&mut problem, cells, protos);
        domain_rules(&mut problem, &place, initial);
        pattern_rules(&mut problem, &place, support, width, height);
        Ok(GridProblem { problem, place, width, height, protos })
    }

    /// Read a solution back as one prototype index per cell, row-major.
    ///
    /// `Err` when a cell does not hold exactly one prototype. That cannot happen for a solution to
    /// the problem [`GridProblem::encode`] built — [`tile_rules`] forbids it — so this is checking a
    /// back-end's answer rather than guarding a possibility. It is checked anyway: a solver that
    /// returned a malformed model would otherwise surface three layers away as a grid with a wrong
    /// cell in it, and reading `.position(..).unwrap_or(0)` would quietly fill that cell with
    /// prototype 0, which is `Empty` — a hole, indistinguishable from a solver that chose one.
    pub fn read(&self, solution: &Solution) -> Result<Vec<usize>, String> {
        let mut grid = Vec::with_capacity(self.place.len());
        for (c, row) in self.place.iter().enumerate() {
            let mut held: Option<usize> = None;
            for (proto, &v) in row.iter().enumerate() {
                if solution.get(v) {
                    if held.is_some() {
                        return Err(format!(
                            "constraints: the solution puts more than one prototype in cell {c}"
                        ));
                    }
                    held = Some(proto);
                }
            }
            match held {
                Some(p) => grid.push(p),
                None => {
                    return Err(format!("constraints: the solution puts no prototype in cell {c}"))
                }
            }
        }
        Ok(grid)
    }

    /// How many prototypes the alphabet holds.
    #[inline]
    pub fn protos(&self) -> usize {
        self.protos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many models one test may enumerate before it is considered to have run away.
    const MODEL_CAP: usize = 100_000;

    /// Unit propagation to a fixed point. `false` on conflict — which includes the empty clause, so
    /// a falsified problem is detected here rather than by an absence of models.
    fn propagate(p: &Problem, val: &mut [Option<bool>]) -> bool {
        loop {
            let mut changed = false;
            for clause in p.iter_clauses() {
                let mut free: Option<Lit> = None;
                let mut n_free = 0usize;
                let mut satisfied = false;
                for &l in clause {
                    match val.get(l.var().index()).copied().flatten() {
                        Some(b) if b == l.is_positive() => {
                            satisfied = true;
                            break;
                        }
                        Some(_) => {}
                        None => {
                            free = Some(l);
                            n_free += 1;
                        }
                    }
                }
                if satisfied {
                    continue;
                }
                match (n_free, free) {
                    (0, _) => return false,
                    (1, Some(l)) => {
                        val[l.var().index()] = Some(l.is_positive());
                        changed = true;
                    }
                    _ => {}
                }
            }
            if !changed {
                return true;
            }
        }
    }

    /// Every assignment over `p`'s variables that satisfies every hard clause.
    ///
    /// A depth-first enumeration with unit propagation, and **that is not a solver standing in for
    /// the one this module still needs** — it is exhaustive rather than heuristic, and it decides
    /// nothing (no weights, no seed, no choice of model). Propagation is only what makes the search
    /// tractable: the auxiliaries [`Problem::count`] and [`Problem::conj`] allocate are *defined* by
    /// biconditionals, so they follow from the inputs and never need branching. Without it a 2 x 2
    /// grid of three prototypes would be 2^25 assignments instead of a few thousand.
    ///
    /// The tests below check the encodings against this rather than against any back-end, so an
    /// encoding and a solver that are wrong in the same direction cannot agree with each other.
    fn models(p: &Problem) -> Vec<Vec<bool>> {
        fn search(p: &Problem, val: &mut Vec<Option<bool>>, out: &mut Vec<Vec<bool>>) {
            if !propagate(p, val) {
                return;
            }
            match val.iter().position(|v| v.is_none()) {
                None => {
                    let full: Vec<bool> = val.iter().map(|v| v.unwrap_or(false)).collect();
                    assert!(p.check(&full).is_ok(), "propagation completed an unsatisfying assignment");
                    out.push(full);
                    assert!(out.len() <= MODEL_CAP, "more than {MODEL_CAP} models — the fixture is too big");
                }
                Some(next) => {
                    for guess in [true, false] {
                        let mut branch = val.clone();
                        branch[next] = Some(guess);
                        search(p, &mut branch, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        search(p, &mut vec![None; p.vars()], &mut out);
        out
    }

    /// How many of `vs` are true under `values`.
    fn popcount(vs: &[Lit], values: &[bool]) -> u32 {
        vs.iter().filter(|l| l.holds(values)).count() as u32
    }

    #[test]
    fn variable_zero_is_true_in_every_model() {
        let p = Problem::new();
        let ms = models(&p);
        assert!(!ms.is_empty(), "an empty problem has models");
        for m in &ms {
            assert!(p.always_true().holds(m), "always_true must hold");
            assert!(!p.always_false().holds(m), "always_false must not hold");
        }
    }

    #[test]
    fn conj_is_the_conjunction_in_both_directions() {
        let mut p = Problem::new();
        let a = Lit::pos(p.var());
        let b = Lit::pos(p.var());
        let c = Lit::pos(p.var());
        let y = p.conj(&[a, b, c]);
        let ms = models(&p);
        assert_eq!(ms.len(), 8, "three free inputs, and y is determined by them");
        for m in &ms {
            let all = a.holds(m) && b.holds(m) && c.holds(m);
            assert_eq!(y.holds(m), all, "y must be exactly a ∧ b ∧ c");
        }
    }

    #[test]
    fn conj_of_nothing_and_of_one_allocate_nothing() {
        let mut p = Problem::new();
        let before = p.vars();
        assert_eq!(p.conj(&[]), p.always_true(), "the empty conjunction is true");
        let a = Lit::pos(p.var());
        assert_eq!(p.conj(&[a]), a, "a one-literal conjunction is that literal");
        assert_eq!(p.vars(), before + 1, "only `a` itself was allocated");
    }

    /// The cardinality encoding, checked against enumeration at every bound a 4-literal constraint
    /// can carry — including the vacuous and impossible ones.
    #[test]
    fn count_admits_exactly_the_assignments_in_range() {
        const N: u32 = 4;
        for lo in 0..=N + 1 {
            for hi in 0..=N + 1 {
                let mut p = Problem::new();
                let vs: Vec<Lit> = (0..N).map(|_| Lit::pos(p.var())).collect();
                p.count(&vs, lo, hi, None);
                let ms = models(&p);
                let want_any = lo <= hi && lo <= N;
                assert_eq!(ms.is_empty(), !want_any, "lo={lo} hi={hi}: satisfiability");
                for m in &ms {
                    let k = popcount(&vs, m);
                    assert!(k >= lo && k <= hi, "lo={lo} hi={hi}: admitted a count of {k}");
                }
                // Every in-range assignment must be reachable, not merely every admitted one legal —
                // an encoding that forbids too much passes the check above and fails here.
                let reached: std::collections::BTreeSet<u32> =
                    ms.iter().map(|m| popcount(&vs, m)).collect();
                for k in 0..=N {
                    if k >= lo && k <= hi {
                        assert!(reached.contains(&k), "lo={lo} hi={hi}: a count of {k} is unreachable");
                    }
                }
            }
        }
    }

    #[test]
    fn count_of_exactly_one_admits_each_singleton_and_nothing_else() {
        let mut p = Problem::new();
        let vs: Vec<Lit> = (0..5).map(|_| Lit::pos(p.var())).collect();
        p.count(&vs, 1, 1, None);
        let ms = models(&p);
        let reached: std::collections::BTreeSet<usize> = ms
            .iter()
            .filter_map(|m| vs.iter().position(|l| l.holds(m)))
            .collect();
        assert_eq!(reached.len(), 5, "each of the five must be reachable alone");
        for m in &ms {
            assert_eq!(popcount(&vs, m), 1, "exactly one must mean exactly one");
        }
    }

    #[test]
    fn count_over_negative_literals_counts_them_as_written() {
        // The API takes literals, not variables — a rule that says "at most one of these is *absent*"
        // must encode as readily as its positive twin.
        let mut p = Problem::new();
        let a = Lit::pos(p.var());
        let b = Lit::pos(p.var());
        let c = Lit::pos(p.var());
        p.count(&[!a, !b, !c], 2, 2, None);
        for m in &models(&p) {
            assert_eq!(popcount(&[!a, !b, !c], m), 2, "exactly two must be false");
        }
    }

    #[test]
    fn an_impossible_hard_count_makes_the_problem_unsatisfiable() {
        let mut p = Problem::new();
        let vs: Vec<Lit> = (0..3).map(|_| Lit::pos(p.var())).collect();
        p.count(&vs, 3, 1, None); // lo > hi
        assert!(models(&p).is_empty(), "an impossible hard constraint admits nothing");

        let mut q = Problem::new();
        let ws: Vec<Lit> = (0..2).map(|_| Lit::pos(q.var())).collect();
        q.count(&ws, 3, 3, None); // lo past what two literals can reach
        assert!(models(&q).is_empty(), "asking for more than exists admits nothing");
    }

    #[test]
    fn a_vacuous_count_says_nothing() {
        let mut p = Problem::new();
        let before = (p.vars(), p.clauses());
        let vs: Vec<Lit> = (0..4).map(|_| Lit::pos(p.var())).collect();
        p.count(&vs, 0, 9, None);
        assert_eq!(p.clauses(), before.1, "a vacuous bound must add no clause");
        assert_eq!(p.vars(), before.0 + 4, "and allocate no counter");
    }

    #[test]
    fn implies_any_is_the_clause_it_claims() {
        let mut p = Problem::new();
        let l = Lit::pos(p.var());
        let m1 = Lit::pos(p.var());
        let m2 = Lit::pos(p.var());
        p.implies_any(l, &[m1, m2], None);
        for m in &models(&p) {
            if l.holds(m) {
                assert!(m1.holds(m) || m2.holds(m), "l must imply one of the ms");
            }
        }
        // And it forbids nothing else: l false leaves both ms free, so all four such rows survive.
        let free = models(&p).iter().filter(|m| !l.holds(m)).count();
        assert_eq!(free, 4, "a false antecedent constrains nothing");
    }

    #[test]
    fn implies_any_of_nothing_denies_its_antecedent() {
        let mut p = Problem::new();
        let l = Lit::pos(p.var());
        p.implies_any(l, &[], None);
        for m in &models(&p) {
            assert!(!l.holds(m), "with nothing to imply, l cannot hold");
        }
    }

    // ── soft constraints ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_soft_constraint_can_be_bought_and_costs_its_weight() {
        let mut p = Problem::new();
        let a = Lit::pos(p.var());
        let b = Lit::pos(p.var());
        // Hard: not both. Soft: both. The soft one is unsatisfiable alongside the hard one, so every
        // model must pay for it — and models still exist, which is the point of it being soft.
        p.count(&[a, b], 0, 1, None);
        p.count(&[a, b], 2, 2, Some(7));
        let ms = models(&p);
        assert!(!ms.is_empty(), "a soft conflict must not make the problem unsatisfiable");
        for m in &ms {
            assert!(!(a.holds(m) && b.holds(m)), "the hard bound still holds");
            assert_eq!(p.unmet(m), 7, "the unmeetable soft constraint costs its weight");
        }
    }

    #[test]
    fn a_satisfiable_soft_constraint_can_cost_nothing() {
        let mut p = Problem::new();
        let a = Lit::pos(p.var());
        let b = Lit::pos(p.var());
        p.count(&[a, b], 2, 2, Some(5));
        let ms = models(&p);
        assert!(
            ms.iter().any(|m| p.unmet(m) == 0 && a.holds(m) && b.holds(m)),
            "a soft constraint that can be met must have a zero-cost model that really meets it"
        );
        // Only one direction is a property of the ENCODING. Nothing forces a guard true — that is the
        // optimiser's job — so an enumeration contains models that pay for a constraint they happen to
        // satisfy anyway. What must never happen is the reverse: costing nothing while unmet.
        for m in &ms {
            if p.unmet(m) == 0 {
                assert!(a.holds(m) && b.holds(m), "a zero cost must mean the constraint really held");
            }
        }
    }

    #[test]
    fn soft_weights_add_up_across_constraints() {
        let mut p = Problem::new();
        let a = Lit::pos(p.var());
        p.add_clause(&[!a]); // hard: a is false
        p.implies_any(p.always_true(), &[a], Some(3));
        p.implies_any(p.always_true(), &[a], Some(4));
        let ms = models(&p);
        assert!(!ms.is_empty());
        for m in &ms {
            assert_eq!(p.unmet(m), 7, "two unmeetable soft constraints cost 3 + 4");
        }
    }

    #[test]
    fn an_impossible_soft_count_costs_its_weight_rather_than_refusing() {
        let mut p = Problem::new();
        let vs: Vec<Lit> = (0..2).map(|_| Lit::pos(p.var())).collect();
        p.count(&vs, 2, 1, Some(11)); // lo > hi, but soft
        let ms = models(&p);
        assert!(!ms.is_empty(), "an impossible SOFT constraint must not refuse");
        for m in &ms {
            assert_eq!(p.unmet(m), 11, "it costs its weight in every model");
        }
    }

    #[test]
    fn the_counters_own_clauses_are_not_relaxable() {
        // The guard must reach the assertion and nothing else. If the recurrence were guarded too, a
        // solver could buy the constraint by lying about the count — and `unmet` would then say a
        // problem was met that was not.
        let mut p = Problem::new();
        let vs: Vec<Lit> = (0..3).map(|_| Lit::pos(p.var())).collect();
        p.count(&vs, 3, 3, Some(2));
        let ms = models(&p);
        assert!(ms.iter().any(|m| popcount(&vs, m) < 3), "the fixture must reach unmet states");
        for m in &ms {
            if p.unmet(m) == 0 {
                assert_eq!(popcount(&vs, m), 3, "a zero cost must mean the count really reached 3");
            }
        }
    }

    // ── clause hygiene ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_tautological_clause_is_dropped_and_a_false_literal_removed() {
        let mut p = Problem::new();
        let a = Lit::pos(p.var());
        let before = p.clauses();
        p.add_clause(&[a, !a]);
        p.add_clause(&[a, p.always_true()]);
        assert_eq!(p.clauses(), before, "tautologies add nothing");
        p.add_clause(&[a, p.always_false(), a]);
        assert_eq!(p.clauses(), before + 1, "and the real clause is kept");
        assert_eq!(p.clause(before), Some(&[a][..]), "with the dead literals removed");
    }

    #[test]
    fn the_empty_clause_is_kept() {
        let mut p = Problem::new();
        p.add_clause(&[p.always_false()]);
        assert!(models(&p).is_empty(), "the falsifying clause must survive simplification");
    }

    #[test]
    fn check_refuses_an_assignment_that_is_too_short() {
        let mut p = Problem::new();
        let _ = p.var();
        assert!(p.check(&[true]).is_err(), "a short assignment cannot answer for every variable");
    }

    // ── L2 ───────────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn tile_rules_put_exactly_one_prototype_in_every_cell() {
        let mut p = Problem::new();
        let place = tile_rules(&mut p, 2, 3);
        assert_eq!(place.len(), 2);
        for m in &models(&p) {
            for row in &place {
                let k = row.iter().filter(|&&v| m[v.index()]).count();
                assert_eq!(k, 1, "each cell holds exactly one prototype");
            }
        }
        assert_eq!(models(&p).len(), 9, "3 prototypes over 2 cells is 9 arrangements");
    }

    #[test]
    fn domain_rules_forbid_the_prototypes_a_mask_clears() {
        let mut p = Problem::new();
        let place = tile_rules(&mut p, 2, 3);
        // Cell 0 pinned to prototype 2; cell 1 may be 0 or 1.
        domain_rules(&mut p, &place, &[1 << 2, 0b011]);
        let ms = models(&p);
        assert_eq!(ms.len(), 2, "one pinned cell times two free choices");
        for m in &ms {
            assert!(m[place[0][2].index()], "cell 0 must be prototype 2");
            assert!(!m[place[1][2].index()], "cell 1 must not be prototype 2");
        }
    }

    #[test]
    fn an_empty_domain_refuses_rather_than_substituting() {
        let mut p = Problem::new();
        let place = tile_rules(&mut p, 1, 3);
        domain_rules(&mut p, &place, &[0]);
        assert!(models(&p).is_empty(), "a cell with nothing legal must make the problem refuse");
    }

    /// The alphabet `wfc`'s own tests use for a two-prototype checkerboard: each may only sit beside
    /// the other, in every direction.
    fn checkerboard_support() -> [Vec<u32>; 4] {
        let alternate = vec![0b10u32, 0b01];
        [alternate.clone(), alternate.clone(), alternate.clone(), alternate]
    }

    #[test]
    fn pattern_rules_admit_exactly_the_two_checkerboards() {
        let mut p = Problem::new();
        let (w, h) = (2, 2);
        let place = tile_rules(&mut p, w * h, 2);
        pattern_rules(&mut p, &place, &checkerboard_support(), w, h);
        let ms = models(&p);
        assert_eq!(ms.len(), 2, "a 2x2 checkerboard has exactly two colourings");
        for m in &ms {
            for z in 0..h {
                for x in 0..w {
                    let here = if m[place[z * w + x][0].index()] { 0 } else { 1 };
                    let want = (x + z) % 2;
                    let first = if m[place[0][0].index()] { 0 } else { 1 };
                    assert_eq!(here, (want + first) % 2, "cell ({x},{z}) breaks the alternation");
                }
            }
        }
    }

    #[test]
    fn pattern_rules_agree_with_wfcs_own_propagation_on_what_is_legal() {
        // The rules here and `wfc::propagate`'s narrowing read the same `support` table, so every
        // arrangement one calls legal the other must too. **The four directions carry different
        // tables on purpose**: a rule set that transposed a direction — the easiest mistake to make
        // in `pattern_rules`' stencil, and the one that costs a mirrored kit with nothing going red —
        // agrees with a symmetric fixture and disagrees with this one.
        let (w, h) = (2usize, 2usize);
        let protos = 3usize;
        let support: [Vec<u32>; 4] = [
            vec![0b001, 0b010, 0b100], // N: only its own kind may sit above
            vec![0b011, 0b110, 0b101], // E
            vec![0b111, 0b111, 0b111], // S: anything may sit below
            vec![0b101, 0b011, 0b110], // W
        ];
        let mut p = Problem::new();
        let place = tile_rules(&mut p, w * h, protos);
        pattern_rules(&mut p, &place, &support, w, h);

        // Read each model back as a grid of prototype indices.
        let mut from_sat: Vec<Vec<usize>> = models(&p)
            .iter()
            .filter_map(|m| {
                (0..w * h)
                    .map(|c| place[c].iter().position(|&v| m[v.index()]))
                    .collect::<Option<Vec<usize>>>()
            })
            .collect();
        from_sat.sort();

        // The same question answered by hand: enumerate every grid and keep the ones whose every
        // orthogonal pair is permitted, exactly as `propagate` requires.
        const STEPS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        let mut by_hand: Vec<Vec<usize>> = Vec::new();
        for bits in 0..protos.pow((w * h) as u32) {
            let grid: Vec<usize> = (0..w * h).map(|c| bits / protos.pow(c as u32) % protos).collect();
            let ok = (0..h).all(|z| {
                (0..w).all(|x| {
                    STEPS.iter().enumerate().all(|(dir, (dx, dz))| {
                        let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                        if nx < 0 || nz < 0 || nx as usize >= w || nz as usize >= h {
                            return true;
                        }
                        let a = grid[z * w + x];
                        let b = grid[nz as usize * w + nx as usize];
                        support[dir][a] & (1 << b) != 0
                    })
                })
            });
            if ok {
                by_hand.push(grid);
            }
        }
        by_hand.sort();

        assert_eq!(from_sat, by_hand, "the clauses and the adjacency table must agree exactly");
        assert!(!by_hand.is_empty(), "the fixture must not be vacuous");
    }

    // ── the grid seam ────────────────────────────────────────────────────────────────────────────

    #[test]
    fn encode_then_read_returns_the_grid_wfc_would_have_returned() {
        let (w, h) = (2usize, 2usize);
        let full = 0b11u32;
        let gp = GridProblem::encode(&checkerboard_support(), &[full; 4], w, h, 2)
            .expect("a permissive 2x2 must encode");
        assert_eq!(gp.protos(), 2);
        let mut grids: Vec<Vec<usize>> = models(&gp.problem)
            .iter()
            .map(|m| {
                let s = Solution::from_assignment(&gp.problem, m.clone()).expect("a model is a solution");
                gp.read(&s).expect("every solution reads back as a grid")
            })
            .collect();
        grids.sort();
        // The two checkerboards, in the row-major order `range::measure` and `Solved::grid` expect.
        assert_eq!(grids, vec![vec![0, 1, 1, 0], vec![1, 0, 0, 1]]);
    }

    #[test]
    fn encode_honours_a_pinned_cell_the_way_grammar_solve_does() {
        // `grammar::solve` pins an owned placement by handing `collapse_grid` a one-bit domain. The
        // same mask must mean the same thing here, or the editor's pinned cells would move.
        let (w, h) = (2usize, 2usize);
        let mut initial = [0b11u32; 4];
        initial[0] = 1 << 1; // pin cell 0 to prototype 1
        let gp = GridProblem::encode(&checkerboard_support(), &initial, w, h, 2)
            .expect("a pinned 2x2 must encode");
        let grids: Vec<Vec<usize>> = models(&gp.problem)
            .iter()
            .map(|m| {
                let s = Solution::from_assignment(&gp.problem, m.clone()).expect("a model is a solution");
                gp.read(&s).expect("reads back")
            })
            .collect();
        assert_eq!(grids, vec![vec![1, 0, 0, 1]], "pinning must leave exactly the one checkerboard");
    }

    #[test]
    fn encode_names_every_malformed_region_rather_than_panicking() {
        let s = checkerboard_support();
        let cases: [(usize, usize, usize, &[u32], &str); 5] = [
            (0, 2, 2, &[0b11; 4], "no cells"),
            (2, 2, 0, &[0b11; 4], "no prototypes"),
            (2, 2, 33, &[0b11; 4], "too many prototypes"),
            (2, 2, 2, &[0b11; 3], "too few starting domains"),
            (2, 2, 3, &[0b11; 4], "a support table of the wrong width"),
        ];
        for (w, h, protos, initial, why) in cases {
            assert!(
                GridProblem::encode(&s, initial, w, h, protos).is_err(),
                "{why} must be refused by name"
            );
        }
    }

    // ── the back-end ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn solve_returns_one_of_the_arrangements_the_enumerator_finds() {
        // The solver and the reference enumerator answer the same question two entirely different
        // ways. Agreement is the check; the solver picking a particular one of the two checkerboards
        // is not something this asserts, because which model a solver returns is its own business.
        let gp = GridProblem::encode(&checkerboard_support(), &[0b11; 4], 2, 2, 2).expect("encodes");
        let solution = gp.problem.solve(0).expect("a satisfiable problem must solve");
        let grid = gp.read(&solution).expect("and read back");
        let expected: Vec<Vec<usize>> = models(&gp.problem)
            .iter()
            .filter_map(|m| {
                let s = Solution::from_assignment(&gp.problem, m.clone()).ok()?;
                gp.read(&s).ok()
            })
            .collect();
        assert!(expected.contains(&grid), "the solver returned {grid:?}, not among {expected:?}");
    }

    #[test]
    fn solve_refuses_an_unsatisfiable_problem_by_name() {
        let gp = GridProblem::encode(&checkerboard_support(), &[0; 4], 2, 2, 2).expect("encodes");
        let err = gp.problem.solve(0).expect_err("an empty domain admits nothing");
        assert!(err.contains("no arrangement"), "the refusal must say what happened: {err}");
    }

    #[test]
    fn solve_refuses_a_soft_problem_rather_than_reporting_a_cost_nobody_minimised() {
        let mut p = Problem::new();
        let a = Lit::pos(p.var());
        p.implies_any(p.always_true(), &[a], Some(3));
        let err = p.solve(0).expect_err("a soft problem has no optimiser yet");
        assert!(err.contains("soft constraint"), "the refusal must name the reason: {err}");
    }

    #[test]
    fn solve_is_deterministic() {
        // The property the whole back-end was chosen for, asserted where this project consumes it.
        let support: [Vec<u32>; 4] = [
            vec![0b011, 0b110, 0b101],
            vec![0b111, 0b011, 0b110],
            vec![0b110, 0b101, 0b011],
            vec![0b101, 0b111, 0b110],
        ];
        let gp = GridProblem::encode(&support, &[0b111; 36], 6, 6, 3).expect("encodes");
        let first = gp.read(&gp.problem.solve(0).expect("solves")).expect("reads");
        for round in 1..6 {
            let again = gp.read(&gp.problem.solve(0).expect("solves")).expect("reads");
            assert_eq!(again, first, "round {round} produced a different arrangement");
        }
    }

    #[test]
    fn a_solved_grid_is_legal_under_the_support_table_it_was_built_from() {
        // The faithful-port claim in miniature: what the solver returns must satisfy the same
        // adjacency relation `wfc::propagate` enforces, checked directly against the table rather
        // than through the clauses that were derived from it.
        let support: [Vec<u32>; 4] = [
            vec![0b011, 0b110, 0b101],
            vec![0b111, 0b011, 0b110],
            vec![0b110, 0b101, 0b011],
            vec![0b101, 0b111, 0b110],
        ];
        let (w, h) = (6usize, 6usize);
        let gp = GridProblem::encode(&support, &[0b111; 36], w, h, 3).expect("encodes");
        let grid = gp.read(&gp.problem.solve(0).expect("solves")).expect("reads");
        const STEPS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        for z in 0..h {
            for x in 0..w {
                for (dir, (dx, dz)) in STEPS.iter().enumerate() {
                    let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                    if nx < 0 || nz < 0 || nx as usize >= w || nz as usize >= h {
                        continue;
                    }
                    let a = grid[z * w + x];
                    let b = grid[nz as usize * w + nx as usize];
                    assert!(
                        support[dir][a] & (1 << b) != 0,
                        "({x},{z}) = {a} may not have {b} on side {dir}"
                    );
                }
            }
        }
    }

    #[test]
    fn read_refuses_a_model_that_leaves_a_cell_unfilled() {
        // A back-end that answered with a short or wrong assignment must be caught here rather than
        // three layers away, where an unfilled cell would be indistinguishable from a chosen `Empty`.
        let gp = GridProblem::encode(&checkerboard_support(), &[0b11; 4], 2, 2, 2).expect("encodes");
        let elsewhere = Problem::new();
        let alien = Solution::from_assignment(&elsewhere, vec![true]).expect("a trivial solution");
        let err = gp.read(&alien).expect_err("a foreign solution cannot fill this grid");
        assert!(err.contains("no prototype in cell"), "the refusal must name what is wrong: {err}");
    }
}
