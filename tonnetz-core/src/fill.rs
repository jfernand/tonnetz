//! Fill strategies: an optional, denser sub-rhythm for the melody voice
//! alone, layered into the gap between two main beats without touching the
//! chord or the harmonic walk. Kept as a separate axis from
//! `RhythmStrategy` because the chord and melody need independently
//! shrinkable durations within one step -- a single shared
//! `(onset, duration)` per step (what `RhythmStrategy` produces) can't
//! express "the chord keeps ringing, but the melody's own note stops
//! early so a fill can take over before the next chord."

use crate::{PitchClass, Triad};

/// Given the triad the main event just arrived at, and that event's own
/// onset-to-onset `duration`, return 0+ extra notes to interleave before
/// the next main event -- each paired with its onset as a fraction of
/// `duration`, strictly in (0.0, 1.0), ascending. Only the melody voice
/// plays these; the chord keeps ringing through them untouched (see
/// `VoiceTracker::advance_fill`).
pub trait FillStrategy {
    fn fills(&mut self, triad: Triad, duration: f64) -> Vec<(f64, PitchClass)>;
}

/// No subdivision -- today's behavior, chord and melody share the main
/// beat alone. The default/off option in the fill pool.
pub struct NoFill;

impl FillStrategy for NoFill {
    fn fills(&mut self, _triad: Triad, _duration: f64) -> Vec<(f64, PitchClass)> {
        vec![]
    }
}

/// Arpeggiates the current triad's tones at `count` evenly-spaced points
/// within the gap, cycling third, fifth, root, third, ... -- starting from
/// the third rather than the root, since the main melody note (from
/// `MelodyStrategy`) is often already close to the root.
pub struct ArpeggioFill {
    pub count: usize,
}

impl FillStrategy for ArpeggioFill {
    fn fills(&mut self, triad: Triad, _duration: f64) -> Vec<(f64, PitchClass)> {
        let pcs = triad.pitch_classes();
        (1..=self.count)
            .map(|i| (i as f64 / (self.count + 1) as f64, pcs[i % 3]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mode;

    #[test]
    fn no_fill_never_produces_anything() {
        let mut fill = NoFill;
        assert_eq!(fill.fills(Triad::new(0, Mode::Major), 1.0), vec![]);
    }

    #[test]
    fn arpeggio_fill_cycles_third_fifth_root() {
        let c_major = Triad::new(0, Mode::Major); // {C=0, E=4, G=7}
        let mut fill = ArpeggioFill { count: 3 };
        let fills = fill.fills(c_major, 1.0);
        let pitches: Vec<PitchClass> = fills.iter().map(|&(_, pc)| pc).collect();
        assert_eq!(pitches, vec![PitchClass::new(4), PitchClass::new(7), PitchClass::new(0)]);
    }

    #[test]
    fn arpeggio_fill_places_count_points_evenly_and_ascending() {
        let mut fill = ArpeggioFill { count: 2 };
        let fills = fill.fills(Triad::new(0, Mode::Major), 1.0);
        let fractions: Vec<f64> = fills.iter().map(|&(f, _)| f).collect();
        assert_eq!(fractions, vec![1.0 / 3.0, 2.0 / 3.0]);
        assert!(fractions.iter().all(|&f| f > 0.0 && f < 1.0));
        assert!(fractions.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn arpeggio_fill_with_zero_count_produces_nothing() {
        let mut fill = ArpeggioFill { count: 0 };
        assert_eq!(fill.fills(Triad::new(0, Mode::Major), 1.0), vec![]);
    }
}
