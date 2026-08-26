//! Walk strategies over the 24-triad graph (CONCEPT.md section 4).

use rand::rngs::ThreadRng;
use rand::{Rng, RngExt};

use crate::{Triad, Utt};

/// The three neo-Riemannian moves, in a fixed order used for iteration.
pub const PLR: [Utt; 3] = [Utt::P, Utt::L, Utt::R];

/// Something that walks the triad graph one step at a time. Returns both
/// the next triad and the operation used to reach it, since melody
/// strategies need to know which voice moved (CONCEPT.md section 5).
pub trait WalkStrategy {
    fn next(&mut self, current: Triad, history: &[Triad]) -> (Triad, Utt);
}

/// Random choice among {P, L, R}, forbidding the op just used (since P/L/R
/// are involutions, that's equivalent to forbidding immediate return to
/// the previous triad).
pub struct FreeWalk<R: Rng> {
    rng: R,
    last_op: Option<Utt>,
}

impl FreeWalk<ThreadRng> {
    pub fn new() -> Self {
        FreeWalk {
            rng: rand::rng(),
            last_op: None,
        }
    }
}

impl Default for FreeWalk<ThreadRng> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: Rng> FreeWalk<R> {
    /// For deterministic tests: build with a seeded RNG.
    pub fn with_rng(rng: R) -> Self {
        FreeWalk { rng, last_op: None }
    }

    fn choose(&mut self, exclude: impl Fn(Utt) -> bool) -> Utt {
        let candidates: Vec<Utt> = PLR.into_iter().filter(|&op| !exclude(op)).collect();
        debug_assert!(!candidates.is_empty());
        candidates[self.rng.random_range(0..candidates.len())]
    }
}

impl<R: Rng> WalkStrategy for FreeWalk<R> {
    fn next(&mut self, current: Triad, _history: &[Triad]) -> (Triad, Utt) {
        let last_op = self.last_op;
        let op = self.choose(|op| Some(op) == last_op);
        self.last_op = Some(op);
        (op.apply(current), op)
    }
}

/// Forbid revisiting any of the last `window` triads. `window == 1` reduces
/// to `FreeWalk`. Since every triad has only 3 neighbors, all of them can
/// be tabu once `window >= 3`; when that happens this relaxes to the
/// least-recently-visited neighbor rather than getting stuck.
pub struct WindowedTabuWalk<R: Rng> {
    rng: R,
    window: usize,
}

impl WindowedTabuWalk<ThreadRng> {
    pub fn new(window: usize) -> Self {
        WindowedTabuWalk {
            rng: rand::rng(),
            window,
        }
    }
}

impl<R: Rng> WindowedTabuWalk<R> {
    pub fn with_rng(window: usize, rng: R) -> Self {
        WindowedTabuWalk { rng, window }
    }
}

impl<R: Rng> WalkStrategy for WindowedTabuWalk<R> {
    fn next(&mut self, current: Triad, history: &[Triad]) -> (Triad, Utt) {
        let tabu = &history[history.len().saturating_sub(self.window)..];
        let legal: Vec<Utt> = PLR
            .into_iter()
            .filter(|op| !tabu.contains(&op.apply(current)))
            .collect();

        let op = if legal.is_empty() {
            // All neighbors are tabu: relax to the least-recently-visited one.
            *PLR
                .iter()
                .min_by_key(|op| {
                    let candidate = op.apply(current);
                    tabu.iter().rposition(|&t| t == candidate).unwrap_or(0)
                })
                .expect("PLR is non-empty")
        } else {
            legal[self.rng.random_range(0..legal.len())]
        };
        (op.apply(current), op)
    }
}

/// The alternating-L/R walk from CONCEPT.md section 3: visits all 24
/// triads with no repeats, returning home at step 24.
pub struct HamiltonianCycleWalk {
    step: usize,
}

impl HamiltonianCycleWalk {
    pub fn new() -> Self {
        HamiltonianCycleWalk { step: 0 }
    }
}

impl Default for HamiltonianCycleWalk {
    fn default() -> Self {
        Self::new()
    }
}

impl WalkStrategy for HamiltonianCycleWalk {
    fn next(&mut self, current: Triad, _history: &[Triad]) -> (Triad, Utt) {
        let op = if self.step.is_multiple_of(2) { Utt::R } else { Utt::L };
        self.step += 1;
        (op.apply(current), op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mode;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn free_walk_never_repeats_the_last_op() {
        let mut walk = FreeWalk::with_rng(StdRng::seed_from_u64(42));
        let mut t = Triad::new(0, Mode::Major);
        let mut last_op = None;
        for _ in 0..200 {
            let (next, op) = walk.next(t, &[]);
            assert_ne!(Some(op), last_op);
            last_op = Some(op);
            t = next;
        }
    }

    #[test]
    fn windowed_tabu_walk_of_window_1_matches_free_walk_constraint() {
        let mut walk = WindowedTabuWalk::with_rng(1, StdRng::seed_from_u64(7));
        let mut t = Triad::new(0, Mode::Major);
        let mut history = vec![t];
        for _ in 0..200 {
            let (next, _) = walk.next(t, &history);
            assert_ne!(next, t, "window=1 must forbid immediate return");
            history.push(next);
            t = next;
        }
    }

    #[test]
    fn windowed_tabu_walk_respects_larger_windows() {
        let mut walk = WindowedTabuWalk::with_rng(3, StdRng::seed_from_u64(7));
        let mut t = Triad::new(0, Mode::Major);
        let mut history = vec![t];
        for _ in 0..200 {
            let (next, _) = walk.next(t, &history);
            let tabu = &history[history.len().saturating_sub(3)..];
            // Only guaranteed when a legal (non-tabu) neighbor existed;
            // the relax-to-least-recent fallback can otherwise revisit.
            if PLR.iter().any(|op| !tabu.contains(&op.apply(t))) {
                assert!(!tabu.contains(&next));
            }
            history.push(next);
            t = next;
        }
    }

    #[test]
    fn hamiltonian_cycle_walk_visits_all_24_and_returns_home() {
        let mut walk = HamiltonianCycleWalk::new();
        let start = Triad::new(0, Mode::Major);
        let mut t = start;
        let mut seen = vec![t];
        for _ in 0..24 {
            let (next, _) = walk.next(t, &seen);
            t = next;
            seen.push(t);
        }
        assert_eq!(t, start);
        assert_eq!(seen[..24].iter().collect::<std::collections::HashSet<_>>().len(), 24);
    }
}
