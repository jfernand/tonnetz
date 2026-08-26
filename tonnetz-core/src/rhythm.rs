//! Rhythm strategies (CONCEPT.md section 6). Deliberately independent of
//! `Triad`/`Utt`/`WalkStrategy` -- a walk step is an event (chord + melody
//! notes), and rhythm only decides when events sound.

use rand::rngs::ThreadRng;
use rand::{Rng, RngExt};

pub trait RhythmStrategy {
    /// Given the event index, return its onset time and duration. Onset
    /// grows monotonically with `event_index`; strategies with internal
    /// state (`WindowedDurations`) require sequential calls starting at 0.
    fn timing(&mut self, event_index: usize) -> (f64, f64);
}

/// One event per beat. Simplest baseline.
pub struct FixedPulse {
    pub beat: f64,
}

impl RhythmStrategy for FixedPulse {
    fn timing(&mut self, event_index: usize) -> (f64, f64) {
        (event_index as f64 * self.beat, self.beat)
    }
}

/// Bjorklund's algorithm: distributes `pulses` onsets as evenly as
/// possible across `steps` slots (Toussaint, "The Euclidean Algorithm
/// Generates Traditional Musical Rhythms," 2005). Returns one `bool` per
/// slot, `true` where an onset falls.
pub fn bjorklund(pulses: usize, steps: usize) -> Vec<bool> {
    assert!(1 <= pulses && pulses <= steps, "need 1 <= pulses <= steps");
    if pulses == steps {
        return vec![true; steps];
    }
    let mut a: Vec<Vec<bool>> = vec![vec![true]; pulses];
    let mut b: Vec<Vec<bool>> = vec![vec![false]; steps - pulses];
    while b.len() > 1 {
        let n = a.len().min(b.len());
        let new_a: Vec<Vec<bool>> = (0..n)
            .map(|i| a[i].iter().chain(b[i].iter()).copied().collect())
            .collect();
        let rem_a = if a.len() > n { a.split_off(n) } else { Vec::new() };
        let rem_b = if b.len() > n { b.split_off(n) } else { Vec::new() };
        a = new_a;
        b = if !rem_a.is_empty() { rem_a } else { rem_b };
    }
    a.into_iter().chain(b).flatten().collect()
}

/// A standard Euclidean rhythm: `pulses` onsets spread over `steps` slots,
/// each slot one time unit long, reused cyclically as a duration/onset
/// pattern. `event_index` is the index of the onset overall (not the
/// slot), so `Euclidean::new(3, 8)`'s events land at times 0, 3, 6, 8, 11,
/// 14, 16, ... with durations 3, 3, 2, 3, 3, 2, ...
pub struct Euclidean {
    steps: usize,
    onsets: Vec<usize>,
    durations: Vec<f64>,
}

impl Euclidean {
    pub fn new(pulses: usize, steps: usize) -> Self {
        let pattern = bjorklund(pulses, steps);
        let onsets: Vec<usize> = pattern
            .iter()
            .enumerate()
            .filter_map(|(i, &on)| on.then_some(i))
            .collect();
        let durations = onsets
            .windows(2)
            .map(|w| (w[1] - w[0]) as f64)
            .chain(std::iter::once((steps - onsets[onsets.len() - 1] + onsets[0]) as f64))
            .collect();
        Euclidean { steps, onsets, durations }
    }
}

impl RhythmStrategy for Euclidean {
    fn timing(&mut self, event_index: usize) -> (f64, f64) {
        let pulses = self.onsets.len();
        let cycle = event_index / pulses;
        let slot = event_index % pulses;
        let onset = (cycle * self.steps) as f64 + self.onsets[slot] as f64;
        (onset, self.durations[slot])
    }
}

/// Its own small tabu process over a duration palette (mirroring
/// `WindowedTabuWalk`, but independent of the harmonic walk's window --
/// see CONCEPT.md section 6). Forbids reusing any of the last `window`
/// durations, relaxing to the least-recently-used one once the whole
/// palette is tabu. Requires sequential calls starting at event index 0.
pub struct WindowedDurations<R: Rng> {
    rng: R,
    window: usize,
    palette: Vec<f64>,
    chosen: Vec<usize>,
    elapsed: f64,
}

impl WindowedDurations<ThreadRng> {
    pub fn new(window: usize, palette: Vec<f64>) -> Self {
        assert!(!palette.is_empty());
        WindowedDurations {
            rng: rand::rng(),
            window,
            palette,
            chosen: Vec::new(),
            elapsed: 0.0,
        }
    }
}

impl<R: Rng> WindowedDurations<R> {
    pub fn with_rng(window: usize, palette: Vec<f64>, rng: R) -> Self {
        assert!(!palette.is_empty());
        WindowedDurations {
            rng,
            window,
            palette,
            chosen: Vec::new(),
            elapsed: 0.0,
        }
    }
}

impl<R: Rng> RhythmStrategy for WindowedDurations<R> {
    fn timing(&mut self, event_index: usize) -> (f64, f64) {
        assert_eq!(event_index, self.chosen.len(), "requires sequential access from 0");
        let tabu = &self.chosen[self.chosen.len().saturating_sub(self.window)..];
        let legal: Vec<usize> = (0..self.palette.len()).filter(|i| !tabu.contains(i)).collect();
        let index = if legal.is_empty() {
            (0..self.palette.len())
                .min_by_key(|i| tabu.iter().rposition(|t| t == i).unwrap_or(0))
                .expect("palette is non-empty")
        } else {
            legal[self.rng.random_range(0..legal.len())]
        };
        let duration = self.palette[index];
        let onset = self.elapsed;
        self.elapsed += duration;
        self.chosen.push(index);
        (onset, duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn fixed_pulse_is_evenly_spaced() {
        let mut rhythm = FixedPulse { beat: 0.5 };
        assert_eq!(rhythm.timing(0), (0.0, 0.5));
        assert_eq!(rhythm.timing(1), (0.5, 0.5));
        assert_eq!(rhythm.timing(4), (2.0, 0.5));
    }

    #[test]
    fn bjorklund_matches_known_patterns() {
        // E(3,8) is the tresillo, a standard reference case (Toussaint 2005).
        assert_eq!(
            bjorklund(3, 8),
            vec![true, false, false, true, false, false, true, false]
        );
        // E(2,5).
        assert_eq!(bjorklund(2, 5), vec![true, false, true, false, false]);
    }

    #[test]
    fn bjorklund_always_places_exactly_pulses_onsets() {
        for steps in 1..=32 {
            for pulses in 1..=steps {
                let pattern = bjorklund(pulses, steps);
                assert_eq!(pattern.len(), steps);
                assert_eq!(pattern.iter().filter(|&&b| b).count(), pulses);
            }
        }
    }

    #[test]
    fn euclidean_3_8_cycles_with_the_tresillo_gaps() {
        let mut rhythm = Euclidean::new(3, 8);
        assert_eq!(rhythm.timing(0), (0.0, 3.0));
        assert_eq!(rhythm.timing(1), (3.0, 3.0));
        assert_eq!(rhythm.timing(2), (6.0, 2.0));
        assert_eq!(rhythm.timing(3), (8.0, 3.0)); // second cycle
        assert_eq!(rhythm.timing(5), (14.0, 2.0));
    }

    #[test]
    fn windowed_durations_never_repeats_within_the_window() {
        let palette = vec![0.25, 0.5, 1.0];
        let mut rhythm = WindowedDurations::with_rng(2, palette.clone(), StdRng::seed_from_u64(1));
        let mut history = Vec::new();
        let mut onset = 0.0;
        for i in 0..100 {
            let (o, d) = rhythm.timing(i);
            assert_eq!(o, onset);
            onset += d;
            history.push(d);
            if history.len() >= 2 {
                assert_ne!(history[history.len() - 1], history[history.len() - 2]);
            }
        }
    }
}
