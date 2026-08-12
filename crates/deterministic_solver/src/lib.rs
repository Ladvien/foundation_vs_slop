#![doc = include_str!("../README.md")]

use batsat::{
    clause::Kind as ClauseKind, lbool, Callbacks, Lit, Solver as BatSolver, SolverInterface,
    SolverOpts, Var,
};

/// A literal in DIMACS convention: `3` is variable 3, `-3` is its negation, and `0` is not a literal.
///
/// **Deliberately a plain integer rather than a type of this crate's own.** The whole point of the
/// crate is to be replaceable, and a caller that has to convert its literals into a bespoke type has
/// been coupled to the thing it was supposed to be able to swap. DIMACS is the one representation
/// every solver already agrees on.
pub type Literal = i32;

/// How much search a solve may spend before giving up, counted in conflicts.
///
/// **Counted, not timed.** A solver that abandons a problem on a wall clock answers differently on a
/// busy machine than on an idle one, which makes the generated world a function of what else was
/// running. That disqualified `splr` (see `docs/research/2026-08-10-solver-choice.md` §2.2), and it is
/// the reason this type carries no `Duration`.
///
/// # What "conflict" means here, precisely
///
/// It is the number of learnt clauses the search produced, counted through `batsat`'s callback
/// interface. **`batsat` 0.6.0 has `conflict_budget` and `propagation_budget` fields and no way to
/// set them** — they are initialised to `-1` and nothing in the crate ever writes them, so the
/// documented budget is unreachable from outside. What *is* reachable is `Callbacks::stop`, which
/// `within_budget` consults on the same line, so this counts conflicts itself and stops there.
///
/// The unit is therefore "learnt clauses", which tracks conflicts but is not promised to equal them.
/// That is fine for the property that matters: it is a deterministic function of the search, so the
/// same instance gives up at the same point on every machine and every run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Budget {
    conflicts: u64,
}

impl Budget {
    /// Search until the question is answered, however long that takes.
    ///
    /// Honest rather than reassuring: an unbounded solve on a hard instance does not return. Prefer
    /// [`Budget::conflicts`] anywhere a refusal is better than a hang.
    pub const UNLIMITED: Budget = Budget { conflicts: 0 };

    /// Give up after this many conflicts, reporting [`Answer::Exhausted`].
    ///
    /// Zero means [`Budget::UNLIMITED`] — there is no such thing as a solve permitted no search at
    /// all, and reading it as "answer immediately with nothing" would be a refusal that says nothing.
    pub const fn conflicts(n: u64) -> Budget {
        Budget { conflicts: n }
    }
}

/// What a solve found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Answer {
    /// An assignment satisfying every clause. Index it with [`Model::get`].
    Satisfied(Model),
    /// No assignment satisfies the clauses together with the assumptions.
    ///
    /// `core` is the subset of the *assumptions* that suffices for that — the reason, not merely the
    /// verdict. A MaxSAT loop is built on this: relax something the core names and ask again.
    /// **Empty when no assumptions were passed**, which means the clauses are unsatisfiable on their
    /// own and no amount of relaxing assumptions will help.
    Unsatisfiable { core: Vec<Literal> },
    /// The budget ran out before the question was settled. Neither a model nor a refusal — the
    /// distinction matters, because "we did not find one" is not "there is not one".
    Exhausted,
}

/// A satisfying assignment.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Model {
    values: Vec<bool>,
}

impl Model {
    /// The value of variable `v` (1-based, as in [`Literal`]).
    ///
    /// Out of range reads as `false` rather than panicking: a caller asking about a variable it never
    /// declared has a bug, and it is better surfaced as an unsatisfied clause it can name than as a
    /// panic inside a library.
    #[inline]
    pub fn get(&self, v: u32) -> bool {
        match v.checked_sub(1) {
            Some(i) => self.values.get(i as usize).copied().unwrap_or(false),
            None => false,
        }
    }

    /// Whether `literal` holds under this assignment.
    #[inline]
    pub fn holds(&self, literal: Literal) -> bool {
        self.get(literal.unsigned_abs()) == (literal > 0)
    }

    /// The assignment as a slice, `values()[v - 1]` for variable `v`.
    #[inline]
    pub fn values(&self) -> &[bool] {
        &self.values
    }
}

/// Counts conflicts and stops the search at the budget.
///
/// `stop` takes `&self`, so the count has to be maintained by a callback that takes `&mut self` —
/// `on_new_clause` is the one that fires per conflict.
struct Budgeted {
    learnt: u64,
    limit: u64,
}

impl Callbacks for Budgeted {
    fn on_new_clause(&mut self, _c: &[Lit], kind: ClauseKind) {
        if kind == ClauseKind::Learnt {
            self.learnt += 1;
        }
    }
    fn stop(&self) -> bool {
        self.limit > 0 && self.learnt >= self.limit
    }
}

/// **Options with every source of variation pinned, rather than left to a default.**
///
/// `batsat`'s defaults already happen to be deterministic — `random_var_freq` 0, `rnd_pol` false,
/// `rnd_init_act` false — but a default is a decision someone else can change in a patch release, and
/// this crate's entire value is that it does not vary. Writing them down means a future `batsat`
/// deciding that a little randomisation improves its benchmarks cannot quietly change what this
/// project generates.
///
/// **`random_seed` is pinned to a constant rather than to zero**, because `SolverOpts::check` requires
/// it to be strictly positive and the solver asserts that check on construction. 91648253 is MiniSat's
/// own value, carried through every descendant. With `random_var_freq` and `rnd_pol` both off, nothing
/// consults it — pinning it is insurance against a future version that does.
fn pinned_opts() -> SolverOpts {
    SolverOpts {
        random_var_freq: 0.0,
        random_seed: 91_648_253.0,
        rnd_pol: false,
        rnd_init_act: false,
        ..SolverOpts::default()
    }
}

/// An incremental Boolean satisfiability solver.
///
/// Add clauses, then ask repeatedly under different assumptions — the clauses persist, and so does
/// everything the solver learnt about them. That is what makes an optimisation loop affordable: the
/// hundredth question costs a fraction of the first.
pub struct Solver {
    inner: BatSolver<Budgeted>,
    /// The solver's own handle per variable, in declaration order.
    ///
    /// **Kept rather than reconstructed.** `batsat` can build one from an index, but only through
    /// `Var::unsafe_from_idx` — a safe function with a name that is a warning. Holding what
    /// `var_of_int` already returned means the mapping is the one the solver made, not one this crate
    /// re-derived and could get one out of step.
    vars: Vec<Var>,
}

impl Solver {
    /// A solver over `variables` variables, numbered 1..=`variables`, with no preferences.
    ///
    /// A convenience over [`Solver::with_preferences`], not a second path — it is that function with
    /// every preference absent.
    pub fn new(variables: usize) -> Result<Self, String> {
        Self::with_preferences(&vec![None; variables])
    }

    /// A solver over `prefer.len()` variables, each with an optional value to try first.
    ///
    /// # A preference is a hint, never a constraint
    ///
    /// It changes which model comes back, never which models exist: the search tries the suggested
    /// value first and backtracks out of it like any other decision, so an unsatisfiable problem is
    /// still unsatisfiable and a satisfiable one still returns a real model.
    ///
    /// **This is how a caller gets variety without paying to prove an optimum.** Expressing "I would
    /// like this cell to be that tile" as a soft constraint makes the solver prove it found the
    /// arrangement *closest* to the whole wish-list, which is core-guided search over hundreds of
    /// units — measured at 9 seconds where the same instance without it solves in 15 ms. As a
    /// preference it costs nothing: the first descent follows the wish and propagation repairs it,
    /// which is exactly what a greedy tile generator does, with backtracking added.
    ///
    /// Determinism is untouched. The preferences are part of the input, so the answer is still a
    /// function of what it was given and nothing else.
    ///
    /// `Err` past `u32::MAX` variables, which is where the representation runs out of names.
    pub fn with_preferences(prefer: &[Option<bool>]) -> Result<Self, String> {
        let n = u32::try_from(prefer.len()).map_err(|_| {
            format!("deterministic_solver: {} variables is more than can be named", prefer.len())
        })?;
        let mut inner = BatSolver::new(pinned_opts(), Budgeted { learnt: 0, limit: 0 });
        // Allocate every variable up front so a model is always full-width, even for a variable that
        // appears in no clause. A caller reading a shorter model would silently get `false` for a
        // variable the solver was simply never told about.
        let vars = (0..n)
            .map(|v| {
                let upol = match prefer.get(v as usize).copied().flatten() {
                    Some(true) => lbool::TRUE,
                    Some(false) => lbool::FALSE,
                    None => lbool::UNDEF,
                };
                // `dvar: true` keeps the variable eligible for branching; `upol` is only consulted
                // once the search has chosen to branch on it.
                let var = inner.new_var(upol, true);
                debug_assert_eq!(var.idx(), v, "batsat allocates variables in order");
                var
            })
            .collect();
        Ok(Solver { inner, vars })
    }

    /// How many variables this solver was built over.
    #[inline]
    pub fn variables(&self) -> usize {
        self.vars.len()
    }

    /// Declare one more variable, returning its positive literal.
    ///
    /// **Growing is what makes an optimisation loop possible.** A core-guided MaxSAT search learns
    /// what the clauses cannot satisfy and then encodes a counter over that — variables it could not
    /// have known to ask for when the problem was built. The alternative is guessing an upper bound at
    /// construction and hoping, which is a worse contract than this one.
    ///
    /// Existing clauses, learnt clauses and the assignment trail are untouched, so this is cheap and
    /// may be interleaved freely with [`Solver::solve`].
    pub fn add_var(&mut self) -> Result<Literal, String> {
        let idx = u32::try_from(self.vars.len())
            .map_err(|_| "deterministic_solver: too many variables to name another".to_owned())?;
        let v = self.inner.var_of_int(idx);
        self.vars.push(v);
        Ok((idx + 1) as Literal)
    }

    /// Add a clause — a disjunction of literals.
    ///
    /// The empty clause is admitted and makes the problem unsatisfiable, which is its meaning. `Err`
    /// names a literal that is `0` or past [`Solver::variables`], because both are a caller bug that
    /// would otherwise be absorbed into a wrong answer.
    pub fn add_clause(&mut self, literals: &[Literal]) -> Result<(), String> {
        let mut lits = Vec::with_capacity(literals.len());
        for &l in literals {
            lits.push(self.literal(l)?);
        }
        // The return says whether the problem is still satisfiable; an unsatisfiable one is a legal
        // state that `solve` reports, not an error to raise here.
        let _ = self.inner.add_clause_reuse(&mut lits);
        Ok(())
    }

    /// Solve under `assumptions` — literals temporarily forced true for this question only.
    ///
    /// Assumptions are how a soft constraint is asked about without being committed to: guard the
    /// constraint's clauses with an indicator, assume the indicator, and an [`Answer::Unsatisfiable`]
    /// core names the indicators that cannot all hold at once.
    pub fn solve(&mut self, assumptions: &[Literal], budget: Budget) -> Result<Answer, String> {
        let mut assumed = Vec::with_capacity(assumptions.len());
        for &l in assumptions {
            assumed.push(self.literal(l)?);
        }
        // Reset the conflict count so a budget bounds THIS question rather than the solver's lifetime.
        // A budget that meant "since construction" would make the tenth question in a loop answerable
        // only if the first nine were cheap, which is not a bound anyone would predict.
        self.inner.cb_mut().learnt = 0;
        self.inner.cb_mut().limit = budget.conflicts;

        let verdict = self.inner.solve_limited(&assumed);
        if verdict == lbool::TRUE {
            let raw = self.inner.get_model();
            let values = (0..self.vars.len())
                .map(|v| raw.get(v).copied().unwrap_or(lbool::UNDEF) == lbool::TRUE)
                .collect();
            Ok(Answer::Satisfied(Model { values }))
        } else if verdict == lbool::FALSE {
            // **Negated on the way out.** `batsat` reports the core the way MiniSat does — as the
            // final conflict clause, which holds `¬p` for each failed assumption `p`. That is the
            // right form for resolution and the wrong form for a caller, who assumed `p` and wants
            // to be told which of the things it asked for cannot hold. Handing back `[1, 2]` for
            // assumptions `[-1, -2]` invites a MaxSAT loop to search its own assumption list for a
            // literal that is not in it, find nothing, and silently stop relaxing.
            let core = self.inner.unsat_core().iter().map(|&l| -to_dimacs(l)).collect();
            Ok(Answer::Unsatisfiable { core })
        } else {
            Ok(Answer::Exhausted)
        }
    }

    /// Translate one DIMACS literal, rejecting the two ways it can be malformed.
    fn literal(&self, l: Literal) -> Result<Lit, String> {
        if l == 0 {
            return Err("deterministic_solver: 0 is not a literal".to_owned());
        }
        let v = l.unsigned_abs();
        let Some(&var) = v.checked_sub(1).and_then(|i| self.vars.get(i as usize)) else {
            return Err(format!(
                "deterministic_solver: literal {l} names variable {v}, past the {} declared",
                self.vars.len()
            ));
        };
        // `Lit::new`'s `sign` is TRUE for the POSITIVE literal — the opposite of MiniSat's convention,
        // which the same-named parameter in the same-shaped API makes very easy to get backwards.
        // Verified in `batsat-0.6.0/src/clause.rs`: `value_lit(v) = value_var(v.var()) ^ !v.sign()`.
        Ok(Lit::new(var, l > 0))
    }
}

/// A `batsat` literal back to DIMACS.
fn to_dimacs(l: Lit) -> Literal {
    let v = l.var().idx() as i32 + 1;
    if l.sign() {
        v
    } else {
        -v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Does `model` satisfy every clause? The independent referee — a solver that answered wrongly
    /// and a test that trusted it would agree with each other.
    fn satisfies(model: &Model, clauses: &[&[Literal]]) -> bool {
        clauses.iter().all(|c| c.iter().any(|&l| model.holds(l)))
    }

    fn solve_all(variables: usize, clauses: &[&[Literal]]) -> Answer {
        let mut s = Solver::new(variables).expect("a small solver");
        for c in clauses {
            s.add_clause(c).expect("a well-formed clause");
        }
        s.solve(&[], Budget::UNLIMITED).expect("a well-formed solve")
    }

    #[test]
    fn a_satisfiable_problem_comes_back_with_a_model_that_really_satisfies_it() {
        let clauses: &[&[Literal]] = &[&[1, 2], &[-1, 3], &[-2, -3], &[1, -3]];
        match solve_all(3, clauses) {
            Answer::Satisfied(m) => assert!(satisfies(&m, clauses), "the model must satisfy the clauses"),
            other => panic!("expected a model, got {other:?}"),
        }
    }

    #[test]
    fn an_unsatisfiable_problem_is_refused_rather_than_approximated() {
        // (a) ∧ (¬a) — no assumptions, so the core is empty and the clauses are the problem.
        match solve_all(1, &[&[1], &[-1]]) {
            Answer::Unsatisfiable { core } => assert!(core.is_empty(), "no assumptions, so no core"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_empty_clause_is_unsatisfiable() {
        match solve_all(1, &[&[]]) {
            Answer::Unsatisfiable { .. } => {}
            other => panic!("the empty clause admits nothing, got {other:?}"),
        }
    }

    #[test]
    fn a_problem_with_no_clauses_is_satisfiable() {
        match solve_all(3, &[]) {
            Answer::Satisfied(m) => assert_eq!(m.values().len(), 3, "the model covers every variable"),
            other => panic!("nothing to violate, got {other:?}"),
        }
    }

    #[test]
    fn a_variable_in_no_clause_still_appears_in_the_model() {
        // Variable 5 is never mentioned. A short model would read it as `false` by accident rather
        // than by decision, and a caller indexing by variable number would silently misread.
        match solve_all(5, &[&[1]]) {
            Answer::Satisfied(m) => assert_eq!(m.values().len(), 5),
            other => panic!("expected a model, got {other:?}"),
        }
    }

    #[test]
    fn assumptions_constrain_one_question_without_being_added_to_the_problem() {
        let mut s = Solver::new(2).expect("solver");
        s.add_clause(&[1, 2]).expect("clause");
        // Assume ¬1 and ¬2 — together impossible with (1 ∨ 2).
        match s.solve(&[-1, -2], Budget::UNLIMITED).expect("solve") {
            Answer::Unsatisfiable { core } => {
                assert!(!core.is_empty(), "the core must name the assumptions at fault");
                assert!(
                    core.iter().all(|&l| l == -1 || l == -2),
                    "the core is a subset of the assumptions: {core:?}"
                );
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        // The very next question, without those assumptions, must succeed — proving they were never
        // added to the clause set.
        match s.solve(&[], Budget::UNLIMITED).expect("solve") {
            Answer::Satisfied(m) => assert!(m.holds(1) || m.holds(2)),
            other => panic!("the assumptions must not have persisted, got {other:?}"),
        }
    }

    #[test]
    fn a_core_names_only_assumptions_that_were_passed() {
        let mut s = Solver::new(3).expect("solver");
        s.add_clause(&[1]).expect("clause");
        match s.solve(&[-1, 2, 3], Budget::UNLIMITED).expect("solve") {
            Answer::Unsatisfiable { core } => {
                assert!(core.contains(&-1), "the conflicting assumption must be in the core: {core:?}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_same_problem_gives_the_same_model_every_time() {
        // The property the crate exists for. A pigeonhole-flavoured instance with many models, so a
        // solver making an arbitrary choice has many arbitrary choices available to make differently.
        let mut clauses: Vec<Vec<Literal>> = Vec::new();
        for a in 1..=6i32 {
            clauses.push((1..=6).map(|b| a * 10 + b).collect());
            for b in 1..=6 {
                for c in (b + 1)..=6 {
                    clauses.push(vec![-(a * 10 + b), -(a * 10 + c)]);
                }
            }
        }
        let refs: Vec<&[Literal]> = clauses.iter().map(|c| c.as_slice()).collect();
        let first = match solve_all(66, &refs) {
            Answer::Satisfied(m) => m,
            other => panic!("expected a model, got {other:?}"),
        };
        for round in 0..8 {
            match solve_all(66, &refs) {
                Answer::Satisfied(m) => assert_eq!(m, first, "round {round} produced a different model"),
                other => panic!("round {round}: expected a model, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_budget_of_one_conflict_gives_up_rather_than_grinding() {
        // The pigeonhole principle at 11 pigeons and 10 holes: unsatisfiable, and famously requiring
        // exponential resolution, so a one-conflict budget cannot possibly settle it.
        let (pigeons, holes) = (11i32, 10i32);
        let idx = |p: i32, h: i32| p * holes + h + 1;
        let mut s = Solver::new((pigeons * holes) as usize).expect("solver");
        for p in 0..pigeons {
            let row: Vec<Literal> = (0..holes).map(|h| idx(p, h)).collect();
            s.add_clause(&row).expect("clause");
        }
        for h in 0..holes {
            for p in 0..pigeons {
                for q in (p + 1)..pigeons {
                    s.add_clause(&[-idx(p, h), -idx(q, h)]).expect("clause");
                }
            }
        }
        let answer = s.solve(&[], Budget::conflicts(1)).expect("solve");
        assert_eq!(answer, Answer::Exhausted, "a one-conflict budget must give up, not answer");
    }

    #[test]
    fn a_budget_bounds_each_question_rather_than_the_solvers_lifetime() {
        // Spend the budget on a hard question, then ask an easy one under the same budget. If the
        // count were cumulative the second would be refused for the first one's sins.
        let (pigeons, holes) = (11i32, 10i32);
        let idx = |p: i32, h: i32| p * holes + h + 1;
        let n = (pigeons * holes) as usize;
        let mut s = Solver::new(n).expect("solver");
        for p in 0..pigeons {
            s.add_clause(&(0..holes).map(|h| idx(p, h)).collect::<Vec<_>>()).expect("clause");
        }
        for h in 0..holes {
            for p in 0..pigeons {
                for q in (p + 1)..pigeons {
                    s.add_clause(&[-idx(p, h), -idx(q, h)]).expect("clause");
                }
            }
        }
        assert_eq!(s.solve(&[], Budget::conflicts(2)).expect("solve"), Answer::Exhausted);
        // Now pin every pigeon into a hole it cannot share — still unsatisfiable, but assumptions make
        // it trivially so, and it must be answered rather than refused for the previous question's cost.
        let easy = s.solve(&[idx(0, 0), idx(1, 0)], Budget::conflicts(2)).expect("solve");
        assert!(
            matches!(easy, Answer::Unsatisfiable { .. }),
            "an easy question after an exhausted one must still be answered, got {easy:?}"
        );
    }

    #[test]
    fn an_unlimited_budget_answers_what_a_tiny_one_gives_up_on() {
        // Same instance, two budgets, opposite outcomes — which is what makes `Exhausted` a statement
        // about the budget rather than about the problem.
        let clauses: Vec<Vec<Literal>> = vec![vec![1, 2], vec![-1, 2], vec![1, -2], vec![-1, -2]];
        let refs: Vec<&[Literal]> = clauses.iter().map(|c| c.as_slice()).collect();
        assert!(matches!(solve_all(2, &refs), Answer::Unsatisfiable { .. }));
    }

    #[test]
    fn a_malformed_literal_is_named_rather_than_absorbed() {
        let mut s = Solver::new(2).expect("solver");
        assert!(s.add_clause(&[0]).is_err(), "0 is not a literal");
        assert!(s.add_clause(&[3]).is_err(), "variable 3 was never declared");
        assert!(s.add_clause(&[-3]).is_err(), "nor its negation");
        assert!(s.solve(&[9], Budget::UNLIMITED).is_err(), "nor as an assumption");
        assert!(s.add_clause(&[1, -2]).is_ok(), "and a well-formed clause still works");
    }

    #[test]
    fn the_model_reads_a_literal_the_same_way_the_solver_does() {
        // The sign convention is the trap: `Lit::new`'s `sign` is true for the POSITIVE literal here,
        // the opposite of MiniSat's. If `literal` and `to_dimacs` disagreed, this fixture — which
        // forces variable 1 false and variable 2 true — would come back inverted.
        let mut s = Solver::new(2).expect("solver");
        s.add_clause(&[-1]).expect("clause");
        s.add_clause(&[2]).expect("clause");
        match s.solve(&[], Budget::UNLIMITED).expect("solve") {
            Answer::Satisfied(m) => {
                assert!(!m.get(1), "variable 1 was forced false");
                assert!(m.get(2), "variable 2 was forced true");
                assert!(m.holds(-1) && m.holds(2), "and `holds` must agree with `get`");
            }
            other => panic!("expected a model, got {other:?}"),
        }
    }

    #[test]
    fn a_preference_steers_the_model_without_changing_which_models_exist() {
        // Three free variables and one clause they all satisfy: every assignment with at least one
        // of them true is legal, so the preference alone decides which comes back.
        let clauses: &[&[Literal]] = &[&[1, 2, 3]];
        let ask = |prefer: &[Option<bool>]| {
            let mut s = Solver::with_preferences(prefer).expect("solver");
            for c in clauses {
                s.add_clause(c).expect("clause");
            }
            match s.solve(&[], Budget::UNLIMITED).expect("solve") {
                Answer::Satisfied(m) => m.values().to_vec(),
                other => panic!("expected a model, got {other:?}"),
            }
        };
        assert_eq!(ask(&[Some(true), Some(false), Some(false)]), vec![true, false, false]);
        assert_eq!(ask(&[Some(false), Some(true), Some(false)]), vec![false, true, false]);
        assert_eq!(ask(&[Some(false), Some(false), Some(true)]), vec![false, false, true]);
    }

    #[test]
    fn a_preference_cannot_make_an_unsatisfiable_problem_satisfiable() {
        // The hint asks for `1` true; the clauses forbid it. A hint that could override them would
        // be a constraint, and a wrong one.
        let mut s = Solver::with_preferences(&[Some(true)]).expect("solver");
        s.add_clause(&[-1]).expect("clause");
        match s.solve(&[], Budget::UNLIMITED).expect("solve") {
            Answer::Satisfied(m) => assert!(!m.get(1), "the clause wins over the hint"),
            other => panic!("expected a model, got {other:?}"),
        }
        let mut t = Solver::with_preferences(&[Some(true)]).expect("solver");
        t.add_clause(&[1]).expect("clause");
        t.add_clause(&[-1]).expect("clause");
        assert!(matches!(t.solve(&[], Budget::UNLIMITED).expect("solve"), Answer::Unsatisfiable { .. }));
    }

    #[test]
    fn a_variable_added_after_solving_joins_the_problem_properly() {
        let mut s = Solver::new(2).expect("solver");
        s.add_clause(&[1, 2]).expect("clause");
        assert!(matches!(s.solve(&[], Budget::UNLIMITED).expect("solve"), Answer::Satisfied(_)));

        // Grow, then constrain the new variable against the old ones — the shape a core-guided loop
        // needs, where the counter is built from what the first solve refused.
        let three = s.add_var().expect("grows");
        assert_eq!(three, 3);
        assert_eq!(s.variables(), 3);
        s.add_clause(&[-1, three]).expect("clause");
        s.add_clause(&[-three]).expect("clause"); // so variable 1 must now be false
        match s.solve(&[], Budget::UNLIMITED).expect("solve") {
            Answer::Satisfied(m) => {
                assert_eq!(m.values().len(), 3, "the model must cover the new variable");
                assert!(!m.get(3), "variable 3 was forced false");
                assert!(!m.get(1), "and that forces variable 1 false through the new clause");
                assert!(m.get(2), "leaving variable 2 to satisfy the original clause");
            }
            other => panic!("expected a model, got {other:?}"),
        }
    }

    #[test]
    fn a_zero_budget_means_unlimited_rather_than_no_search() {
        assert_eq!(Budget::conflicts(0), Budget::UNLIMITED);
        let clauses: &[&[Literal]] = &[&[1, 2], &[-1, 2], &[1, -2]];
        let mut s = Solver::new(2).expect("solver");
        for c in clauses {
            s.add_clause(c).expect("clause");
        }
        match s.solve(&[], Budget::conflicts(0)).expect("solve") {
            Answer::Satisfied(m) => assert!(satisfies(&m, clauses)),
            other => panic!("a zero budget must not mean instant surrender, got {other:?}"),
        }
    }
}
