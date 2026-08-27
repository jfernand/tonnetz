//! Walk strategies over the 24-triad graph (CONCEPT.md section 4).

use rand::rngs::ThreadRng;
use rand::{Rng, RngExt};

use crate::{PitchClass, Triad, Utt};

/// The three neo-Riemannian moves, in a fixed order used for iteration.
pub const PLR: [Utt; 3] = [Utt::P, Utt::L, Utt::R];

/// Something that walks the triad graph one step at a time. Returns both
/// the next triad and the operation used to reach it, since melody
/// strategies need to know which voice moved (CONCEPT.md section 5).
pub trait WalkStrategy {
    fn next(&mut self, current: Triad, history: &[Triad]) -> (Triad, Utt);

    /// The hexatonic/octatonic system this walk is currently confined to,
    /// if any. Default `None`; only `CycleConfinedWalk` overrides it. This
    /// lets `Pipeline` push the active system into `MelodyStrategy::
    /// set_system` (for `SystemFixedScale`) without needing to know the
    /// concrete walk type -- CONCEPT.md section 5's plumbing question.
    fn current_system(&self) -> Option<System> {
        None
    }
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
        let candidates: Vec<Utt> = PLR
            .into_iter()
            .filter(|&op| !exclude(op))
            .collect();
        debug_assert!(!candidates.is_empty());
        candidates[self
            .rng
            .random_range(0..candidates.len())]
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
        let tabu = &history[history
            .len()
            .saturating_sub(self.window)..];
        let legal: Vec<Utt> = PLR
            .into_iter()
            .filter(|op| !tabu.contains(&op.apply(current)))
            .collect();

        let op = if legal.is_empty() {
            // All neighbors are tabu: relax to the least-recently-visited one.
            *PLR.iter()
                .min_by_key(|op| {
                    let candidate = op.apply(current);
                    tabu.iter()
                        .rposition(|&t| t == candidate)
                        .unwrap_or(0)
                })
                .expect("PLR is non-empty")
        } else {
            legal[self
                .rng
                .random_range(0..legal.len())]
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
        let op = if self
            .step
            .is_multiple_of(2)
        {
            Utt::R
        } else {
            Utt::L
        };
        self.step += 1;
        (op.apply(current), op)
    }
}

/// One of the two named systems from CONCEPT.md section 3: hexatonic
/// (alternating P, L) or octatonic (alternating P, R). P is common to
/// both, so it's always the op used to resume alternation right after an
/// escape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum System {
    Hexatonic,
    Octatonic,
}

impl System {
    fn pair(self) -> [Utt; 2] {
        match self {
            System::Hexatonic => [Utt::P, Utt::L],
            System::Octatonic => [Utt::P, Utt::R],
        }
    }

    /// The op that isn't one of this system's two defining ops --
    /// applying it once is a system modulation (CONCEPT.md section 3).
    fn third_op(self) -> Utt {
        match self {
            System::Hexatonic => Utt::R,
            System::Octatonic => Utt::L,
        }
    }

    fn toggle(self) -> System {
        match self {
            System::Hexatonic => System::Octatonic,
            System::Octatonic => System::Hexatonic,
        }
    }

    /// The fixed 6-note (hexatonic) or 8-note (octatonic) pitch-class
    /// collection `triad` belongs to (CONCEPT.md section 3). `triad.root
    /// mod 4` (hexatonic) / `mod 3` (octatonic) is invariant under that
    /// system's own two defining ops -- P never changes root, and L/R only
    /// move it by multiples of 4/3 respectively -- so the collection is
    /// fully determined by `triad.root` alone; no cycle identity (which of
    /// the 4 hexatonic / 3 octatonic systems) needs to be tracked
    /// separately.
    pub fn pitch_classes(self, triad: Triad) -> Vec<PitchClass> {
        let root = triad
            .root
            .0 as i32;
        let (modulus, offsets): (i32, &[i32]) = match self {
            System::Hexatonic => (4, &[0, 3, 4, 7, 8, 11]),
            System::Octatonic => (3, &[0, 1, 3, 4, 6, 7, 9, 10]),
        };
        let base = root.rem_euclid(modulus);
        offsets
            .iter()
            .map(|&o| PitchClass::new(base + o))
            .collect()
    }
}

/// Stay on the current hexatonic or octatonic cycle (alternate its two
/// defining ops) until a random escape fires, then take the third op once
/// to modulate onto the other kind of system, and continue.
///
/// Since a system's third op is, by definition, not one of its own pair,
/// and the continuation step always picks the pair member *other* than
/// the last op used, this walk never repeats the same op twice in a row
/// -- whether it just escaped or not.
pub struct CycleConfinedWalk<R: Rng> {
    rng: R,
    escape_probability: f32,
    system: System,
    last_op: Option<Utt>,
}

impl CycleConfinedWalk<ThreadRng> {
    pub fn new(system: System, escape_probability: f32) -> Self {
        CycleConfinedWalk {
            rng: rand::rng(),
            escape_probability,
            system,
            last_op: None,
        }
    }
}

impl<R: Rng> CycleConfinedWalk<R> {
    pub fn with_rng(system: System, escape_probability: f32, rng: R) -> Self {
        CycleConfinedWalk {
            rng,
            escape_probability,
            system,
            last_op: None,
        }
    }
}

impl<R: Rng> WalkStrategy for CycleConfinedWalk<R> {
    fn next(&mut self, current: Triad, _history: &[Triad]) -> (Triad, Utt) {
        let op = if self
            .rng
            .random::<f32>()
            < self.escape_probability
        {
            let op = self
                .system
                .third_op();
            self.system = self
                .system
                .toggle();
            op
        } else {
            let pair = self
                .system
                .pair();
            if self.last_op == Some(pair[0]) {
                pair[1]
            } else {
                pair[0]
            }
        };
        self.last_op = Some(op);
        (op.apply(current), op)
    }

    fn current_system(&self) -> Option<System> {
        Some(self.system)
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
            let tabu = &history[history
                .len()
                .saturating_sub(3)..];
            // Only guaranteed when a legal (non-tabu) neighbor existed;
            // the relax-to-least-recent fallback can otherwise revisit.
            if PLR
                .iter()
                .any(|op| !tabu.contains(&op.apply(t)))
            {
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
        assert_eq!(
            seen[..24]
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            24
        );
    }

    #[test]
    fn cycle_confined_walk_with_zero_escape_stays_on_the_hexatonic_6_cycle() {
        let mut walk =
            CycleConfinedWalk::with_rng(System::Hexatonic, 0.0, StdRng::seed_from_u64(1));
        let start = Triad::new(0, Mode::Major);
        let mut t = start;
        let mut seen = vec![t];
        for _ in 0..6 {
            let (next, op) = walk.next(t, &seen);
            assert!(
                op == Utt::P || op == Utt::L,
                "hexatonic confinement must not use R"
            );
            t = next;
            seen.push(t);
        }
        assert_eq!(t, start);
        assert_eq!(
            seen[..6]
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            6
        );
    }

    #[test]
    fn cycle_confined_walk_with_zero_escape_stays_on_the_octatonic_8_cycle() {
        let mut walk =
            CycleConfinedWalk::with_rng(System::Octatonic, 0.0, StdRng::seed_from_u64(1));
        let start = Triad::new(0, Mode::Major);
        let mut t = start;
        let mut seen = vec![t];
        for _ in 0..8 {
            let (next, op) = walk.next(t, &seen);
            assert!(
                op == Utt::P || op == Utt::R,
                "octatonic confinement must not use L"
            );
            t = next;
            seen.push(t);
        }
        assert_eq!(t, start);
        assert_eq!(
            seen[..8]
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            8
        );
    }

    #[test]
    fn cycle_confined_walk_with_full_escape_matches_hamiltonian_cycle_walk() {
        // Escaping every single step alternates R (hexatonic's third op)
        // and L (octatonic's third op) while toggling system each time --
        // exactly the R/L Hamiltonian walk, starting from Hexatonic.
        let mut confined =
            CycleConfinedWalk::with_rng(System::Hexatonic, 1.0, StdRng::seed_from_u64(99));
        let mut hamiltonian = HamiltonianCycleWalk::new();
        let mut t = Triad::new(0, Mode::Major);
        for _ in 0..24 {
            let (a, op_a) = confined.next(t, &[]);
            let (b, op_b) = hamiltonian.next(t, &[]);
            assert_eq!(a, b);
            assert_eq!(op_a, op_b);
            t = a;
        }
    }

    #[test]
    fn free_walk_reports_no_current_system() {
        let walk = FreeWalk::with_rng(StdRng::seed_from_u64(1));
        assert_eq!(walk.current_system(), None);
    }

    #[test]
    fn cycle_confined_walk_reports_its_current_system() {
        let mut walk =
            CycleConfinedWalk::with_rng(System::Hexatonic, 1.0, StdRng::seed_from_u64(3));
        assert_eq!(walk.current_system(), Some(System::Hexatonic));
        let t = Triad::new(0, Mode::Major);
        walk.next(t, &[]); // full escape probability guarantees a toggle
        assert_eq!(walk.current_system(), Some(System::Octatonic));
    }

    // Hand-computed by alternating P/L from C major: C, Cm, Ab, Abm, E, Em.
    #[test]
    fn hexatonic_pitch_classes_match_the_c_major_augmented_collection() {
        let mut pcs: Vec<u8> = System::Hexatonic
            .pitch_classes(Triad::new(0, Mode::Major))
            .into_iter()
            .map(|pc| pc.0)
            .collect();
        pcs.sort();
        assert_eq!(pcs, vec![0, 3, 4, 7, 8, 11]);
    }

    // Same collection from any triad on that cycle (root mod 4 == 0).
    #[test]
    fn hexatonic_pitch_classes_are_shared_across_the_whole_cycle() {
        let from_ab_major = System::Hexatonic.pitch_classes(Triad::new(8, Mode::Major));
        let from_c_major = System::Hexatonic.pitch_classes(Triad::new(0, Mode::Major));
        let mut a: Vec<u8> = from_ab_major
            .into_iter()
            .map(|pc| pc.0)
            .collect();
        let mut b: Vec<u8> = from_c_major
            .into_iter()
            .map(|pc| pc.0)
            .collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    // Hand-computed by alternating P/R from C major: C, Cm, Eb, Ebm, F#, F#m, A,
    // Am.
    #[test]
    fn octatonic_pitch_classes_match_the_c_major_diminished_collection() {
        let mut pcs: Vec<u8> = System::Octatonic
            .pitch_classes(Triad::new(0, Mode::Major))
            .into_iter()
            .map(|pc| pc.0)
            .collect();
        pcs.sort();
        assert_eq!(pcs, vec![0, 1, 3, 4, 6, 7, 9, 10]);
    }

    #[test]
    fn cycle_confined_walk_never_repeats_the_last_op() {
        let mut walk =
            CycleConfinedWalk::with_rng(System::Hexatonic, 0.3, StdRng::seed_from_u64(2024));
        let mut t = Triad::new(0, Mode::Major);
        let mut last_op = None;
        for _ in 0..500 {
            let (next, op) = walk.next(t, &[]);
            assert_ne!(Some(op), last_op);
            last_op = Some(op);
            t = next;
        }
    }
}
