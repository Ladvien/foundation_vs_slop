//! **Fairness / exploitability** — is the encounter beatable by a *simple dominant trick*, or does surviving
//! it demand skilled, varied play?
//!
//! This is the fourth Phase-1 tone proxy, but unlike dread/loneliness/pacing it cannot be read off a single
//! belief series: exploitability is a property of *how well an agent can play the config*, so it is computed
//! from a **playtester** (Phase 4). The RL-testing literature is explicit that a reward-driven agent will
//! surface unintended dominant strategies and exploits (Bergdahl et al., "Augmenting Automated Game Testing
//! with Deep Reinforcement Learning", CoG 2020, DOI 10.1109/cog47356.2020.9231552), and that what matters for
//! evaluation is not merely *winning* but the *style* by which it wins (Zhao et al., "Winning Is Not
//! Everything", IEEE TG 2020, DOI 10.1109/tg.2020.2990865): a high win-rate reached by one monotone tactic is
//! an exploit, the same win-rate reached by adaptive, varied play is fair difficulty. Difficulty itself is
//! read from the strongest achievable play (Shin et al., DOI 10.1109/access.2020.2980380; Roohi et al.,
//! DOI 10.1145/3474658 — the agent's *best-case* run predicts human difficulty better than its average).
//!
//! So exploitability combines two measured quantities from the best playtester found:
//! - **competence** — how well it kept the squad alive (survival fraction), the difficulty gauge; and
//! - **strategy concentration** — how much its play collapsed onto a single mode (a Herfindahl/Simpson index
//!   over the squad's mode histogram): `1` = one tactic did everything, low = broad, adaptive play.
//!
//! `exploitability = competence · concentration` is high only when a *concentrated* strategy *dominates*; and
//! `fairness = 1 − exploitability` is high when the config is either appropriately hard (low competence) or
//! demands varied play to win (low concentration). This is a *proxy the optimizer can penalise*, not a hard
//! gate — a search that only ever produced trivially-exploitable configs should be steered away from them.

/// The playtester's **competence** on a config: the fraction of the squad it kept alive. `0` when the squad
/// was empty (nothing to keep alive). The difficulty gauge — a config a strong player clears with the whole
/// squad intact is easy; one that costs units even under best play is hard.
pub fn survival_competence(survivors: u32, squad_size: u32) -> f32 {
    if squad_size == 0 {
        return 0.0;
    }
    (survivors as f32 / squad_size as f32).clamp(0.0, 1.0)
}

/// **Strategy concentration** — the Herfindahl/Simpson index `Σ p_i²` over a mode-usage histogram, where
/// `p_i` is mode `i`'s share of the decisions. `1.0` when one mode did everything (a single dominant tactic),
/// falling toward `1/k` as play spreads evenly over `k` modes. `0.0` for an empty histogram (no play to
/// characterise). This is the "style" axis: a concentrated style winning is the exploit signal.
pub fn mode_concentration(counts: &[u32]) -> f32 {
    let total: u64 = counts.iter().map(|&c| u64::from(c)).sum();
    if total == 0 {
        return 0.0;
    }
    let mut h = 0.0f64;
    for &c in counts {
        let p = f64::from(c) / total as f64;
        h += p * p;
    }
    (h as f32).clamp(0.0, 1.0)
}

/// **Exploitability** `= competence · concentration`: high only when a *concentrated* strategy *dominates*
/// the config. Both inputs in `[0,1]`; result in `[0,1]`.
pub fn exploitability(competence: f32, concentration: f32) -> f32 {
    (competence.clamp(0.0, 1.0) * concentration.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

/// **Fairness** `= 1 − exploitability`: high when the config is appropriately hard (low competence) *or*
/// demands varied play to win (low concentration), low when a single dominant trick clears it.
pub fn fairness(competence: f32, concentration: f32) -> f32 {
    (1.0 - exploitability(competence, concentration)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn competence_is_the_survival_fraction() {
        assert_eq!(survival_competence(5, 5), 1.0);
        assert_eq!(survival_competence(0, 5), 0.0);
        assert!((survival_competence(3, 4) - 0.75).abs() < 1e-6);
        // An empty squad has no competence to report rather than a divide-by-zero.
        assert_eq!(survival_competence(0, 0), 0.0);
    }

    #[test]
    fn concentration_is_one_for_a_single_tactic() {
        assert_eq!(mode_concentration(&[100, 0, 0, 0]), 1.0);
        assert_eq!(mode_concentration(&[]), 0.0);
        assert_eq!(mode_concentration(&[0, 0, 0]), 0.0);
    }

    #[test]
    fn concentration_falls_as_play_spreads() {
        let one = mode_concentration(&[100, 0, 0, 0]);
        let two = mode_concentration(&[50, 50, 0, 0]);
        let four = mode_concentration(&[25, 25, 25, 25]);
        assert!(one > two && two > four, "concentration must fall as play spreads: {one} {two} {four}");
        // Even over k modes → exactly 1/k.
        assert!((four - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_dominant_simple_strategy_is_exploitable_and_unfair() {
        // High survival reached by one tactic — the exploit the playtester exists to find.
        let comp = survival_competence(5, 5); // full survival
        let conc = mode_concentration(&[200, 0, 0, 0]); // one mode
        assert!(exploitability(comp, conc) > 0.9, "a dominant simple win must read as exploitable");
        assert!(fairness(comp, conc) < 0.1, "and therefore unfair");
    }

    #[test]
    fn hard_or_varied_play_is_fair() {
        // Appropriately hard: even best play loses units → low competence → fair regardless of style.
        assert!(fairness(survival_competence(2, 5), 1.0) > 0.5, "a costly config is fair even if won simply");
        // Easy but demands broad play: full survival but spread over many modes → not a single-trick exploit.
        let varied = mode_concentration(&[20, 20, 20, 20, 20]);
        assert!(fairness(survival_competence(5, 5), varied) > 0.5, "varied winning play is fair");
    }

    #[test]
    fn all_outputs_are_bounded_unit_interval() {
        for (c, k) in [(0.0, 0.0), (1.0, 1.0), (0.5, 0.3), (0.9, 0.7)] {
            for v in [exploitability(c, k), fairness(c, k)] {
                assert!((0.0..=1.0).contains(&v), "out of range for ({c},{k})");
            }
        }
    }
}
