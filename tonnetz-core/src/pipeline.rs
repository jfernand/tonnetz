//! The pipeline from CONCEPT.md section 7: a `WalkStrategy` drives a
//! triad sequence, a `MelodyStrategy` and `RhythmStrategy` each derive
//! their half of an event from that same sequence, and a `Renderer` turns
//! the combined event into sound (or a MIDI file, or anything else).
//!
//! `WalkStrategy`, `MelodyStrategy`, and `RhythmStrategy` were designed
//! independently and CONCEPT.md never actually specified how they get
//! driven together, so `Pipeline` and `Renderer` are new, not a
//! transcription of something already in the doc.

use crate::{MelodyStrategy, PitchClass, RhythmStrategy, Triad, Utt, WalkStrategy};

/// One fully-resolved step: the chord just arrived at, the op that
/// produced it, the melody notes for this step, and its abstract timing.
/// Onset/duration are in whatever units the `RhythmStrategy` used (e.g.
/// Euclidean's "one step" units) -- converting those to real time is a
/// `Renderer`/player concern, not the pipeline's.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub triad: Triad,
    pub op: Utt,
    pub notes: Vec<PitchClass>,
    pub onset: f64,
    pub duration: f64,
}

/// Drives a `WalkStrategy`, `MelodyStrategy`, and `RhythmStrategy`
/// together into a stream of `Event`s (CONCEPT.md section 7). Lazy
/// (`Iterator`) rather than building a `Vec<Event>` upfront, resolving
/// section 8's open question in favor of streaming: `WalkStrategy` has no
/// natural end (`FreeWalk` can run forever), and a consumer that wants an
/// offline batch can still `.take(n).collect()` this.
pub struct Pipeline<W, M, R> {
    walk: W,
    melody: M,
    rhythm: R,
    triad: Triad,
    history: Vec<Triad>,
    event_index: usize,
}

impl<W: WalkStrategy, M: MelodyStrategy, R: RhythmStrategy> Pipeline<W, M, R> {
    pub fn new(walk: W, melody: M, rhythm: R, start: Triad) -> Self {
        Pipeline {
            walk,
            melody,
            rhythm,
            triad: start,
            history: vec![start],
            event_index: 0,
        }
    }

    /// The triad this pipeline is currently sitting on (the seed triad
    /// before the first `next()` call, or the most recent event's triad
    /// after).
    pub fn current(&self) -> Triad {
        self.triad
    }
}

impl<W: WalkStrategy, M: MelodyStrategy, R: RhythmStrategy> Iterator for Pipeline<W, M, R> {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        let prev = self.triad;
        let (next, op) = self.walk.next(prev, &self.history);
        let notes = self.melody.notes(prev, next, op, &self.history);
        let (onset, duration) = self.rhythm.timing(self.event_index);

        self.history.push(next);
        self.triad = next;
        self.event_index += 1;

        Some(Event {
            triad: next,
            op,
            notes,
            onset,
            duration,
        })
    }
}

/// Turns a `Pipeline`'s `Event`s into sound, a file, or anything else.
/// Deliberately minimal: how a `Renderer` maps an arbitrary-length
/// `notes` onto actual voices (one continuously-tracked melodic line vs.
/// a full chord-scale) is left to the implementation, since strategies
/// like `MovingVoice` (always 1 note) and `TightScale`/`RollingWindowScale`
/// (several at once) need genuinely different rendering approaches.
pub trait Renderer {
    fn render(&mut self, event: &Event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FreeWalk, HamiltonianCycleWalk, Mode, MovingVoice, Utt};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    struct ConstantRhythm;
    impl RhythmStrategy for ConstantRhythm {
        fn timing(&mut self, event_index: usize) -> (f64, f64) {
            (event_index as f64, 1.0)
        }
    }

    #[test]
    fn pipeline_matches_manual_step_by_step_composition() {
        let start = Triad::new(0, Mode::Major);
        let mut pipeline = Pipeline::new(
            FreeWalk::with_rng(StdRng::seed_from_u64(11)),
            MovingVoice,
            ConstantRhythm,
            start,
        );

        let mut manual_walk = FreeWalk::with_rng(StdRng::seed_from_u64(11));
        let mut manual_melody = MovingVoice;
        let mut manual_history = vec![start];
        let mut manual_triad = start;

        for i in 0..20 {
            let event = pipeline.next().unwrap();

            let (next, op) = manual_walk.next(manual_triad, &manual_history);
            let notes = manual_melody.notes(manual_triad, next, op, &manual_history);
            manual_history.push(next);
            manual_triad = next;

            assert_eq!(event.triad, next);
            assert_eq!(event.op, op);
            assert_eq!(event.notes, notes);
            assert_eq!(event.onset, i as f64);
            assert_eq!(event.duration, 1.0);
        }
        assert_eq!(pipeline.current(), manual_triad);
    }

    #[test]
    fn pipeline_moving_voice_always_yields_exactly_one_note() {
        let mut pipeline = Pipeline::new(
            HamiltonianCycleWalk::new(),
            MovingVoice,
            ConstantRhythm,
            Triad::new(0, Mode::Major),
        );
        for event in pipeline.by_ref().take(24) {
            assert_eq!(event.notes.len(), 1, "P/L/R always move exactly one voice");
            assert!(matches!(event.op, Utt::R | Utt::L));
        }
    }
}
