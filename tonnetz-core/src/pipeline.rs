//! The pipeline from CONCEPT.md section 7: a `WalkStrategy` drives a
//! triad sequence, a `MelodyStrategy` and `RhythmStrategy` each derive
//! their half of an event from that same sequence, and a `Renderer` turns
//! the combined event into sound (or a MIDI file, or anything else).
//!
//! `WalkStrategy`, `MelodyStrategy`, and `RhythmStrategy` were designed
//! independently and CONCEPT.md never actually specified how they get
//! driven together, so `Pipeline` and `Renderer` are new, not a
//! transcription of something already in the doc.

use std::collections::VecDeque;

use crate::{FillStrategy, MelodyStrategy, PitchClass, RhythmStrategy, Triad, Utt, WalkStrategy};

/// One fully-resolved step: the chord just arrived at, the op that
/// produced it, the melody notes for this step, and its abstract timing.
/// Onset/duration are in whatever units the `RhythmStrategy` used (e.g.
/// Euclidean's "one step" units) -- converting those to real time is a
/// `Renderer`/player concern, not the pipeline's.
///
/// `is_fill` marks an event produced by a `FillStrategy` rather than a
/// walk step: the harmonic walk didn't move (`triad`/`op` just repeat the
/// last main event's, with `op` always `Utt::IDENTITY`), and only the
/// melody voice should react to it -- see `VoiceTracker::advance_fill`.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub triad: Triad,
    pub op: Utt,
    pub notes: Vec<PitchClass>,
    pub onset: f64,
    pub duration: f64,
    pub is_fill: bool,
}

/// Drives a `WalkStrategy`, `MelodyStrategy`, `RhythmStrategy`, and
/// `FillStrategy` together into a stream of `Event`s (CONCEPT.md section
/// 7). Lazy (`Iterator`) rather than building a `Vec<Event>` upfront,
/// resolving section 8's open question in favor of streaming:
/// `WalkStrategy` has no natural end (`FreeWalk` can run forever), and a
/// consumer that wants an offline batch can still `.take(n).collect()`
/// this.
pub struct Pipeline<W, M, R, F> {
    walk: W,
    melody: M,
    rhythm: R,
    fill: F,
    triad: Triad,
    history: Vec<Triad>,
    event_index: usize,
    pending_fills: VecDeque<Event>,
}

impl<W: WalkStrategy, M: MelodyStrategy, R: RhythmStrategy, F: FillStrategy> Pipeline<W, M, R, F> {
    pub fn new(walk: W, melody: M, rhythm: R, fill: F, start: Triad) -> Self {
        Pipeline {
            walk,
            melody,
            rhythm,
            fill,
            triad: start,
            history: vec![start],
            event_index: 0,
            pending_fills: VecDeque::new(),
        }
    }

    /// The triad this pipeline is currently sitting on (the seed triad
    /// before the first `next()` call, or the most recent event's triad
    /// after).
    pub fn current(&self) -> Triad {
        self.triad
    }
}

impl<W: WalkStrategy, M: MelodyStrategy, R: RhythmStrategy, F: FillStrategy> Iterator
    for Pipeline<W, M, R, F>
{
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        if let Some(fill_event) = self
            .pending_fills
            .pop_front()
        {
            return Some(fill_event);
        }

        let prev = self.triad;
        let (next, op) = self
            .walk
            .next(prev, &self.history);
        if let Some(system) = self
            .walk
            .current_system()
        {
            self.melody
                .set_system(system);
        }
        let notes = self
            .melody
            .notes(prev, next, op, &self.history);
        let (onset, duration) = self
            .rhythm
            .timing(self.event_index);

        self.history
            .push(next);
        self.triad = next;
        self.event_index += 1;

        let mut fills = self
            .fill
            .fills(next, duration);
        fills.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .expect("fill fractions must not be NaN")
        });

        let main_end = onset + duration;
        let main_duration = fills
            .first()
            .map_or(duration, |&(frac, _)| frac * duration);

        for i in 0..fills.len() {
            let (frac, pitch) = fills[i];
            let fill_onset = onset + frac * duration;
            let next_onset = fills
                .get(i + 1)
                .map_or(main_end, |&(next_frac, _)| onset + next_frac * duration);
            self.pending_fills
                .push_back(Event {
                    triad: next,
                    op: Utt::IDENTITY,
                    notes: vec![pitch],
                    onset: fill_onset,
                    duration: next_onset - fill_onset,
                    is_fill: true,
                });
        }

        Some(Event {
            triad: next,
            op,
            notes,
            onset,
            duration: main_duration,
            is_fill: false,
        })
    }
}

/// Turns a `Pipeline`'s `Event`s into sound, a file, or anything else.
/// Deliberately minimal: how a `Renderer` maps an arbitrary-length
/// `notes` onto actual voices (one continuously-tracked melodic line vs.
/// a full chord-scale) is left to the implementation, since strategies
/// like `MovingVoice` (always 1 note) and `TightScale`/`RollingWindowScale`
/// (several at once) need genuinely different rendering approaches.
///
/// `start`/`finish` default to no-ops and `render` stays the only required
/// method, so every existing implementation keeps compiling unchanged; a
/// driver that wants a uniform lifecycle across backends (live sound, a
/// MIDI/WAV file, plain text) can still call all three generically through
/// `dyn Renderer` without knowing the concrete backend. `finish` is
/// fallible because file-writing backends can fail on the final flush;
/// `start`/`render` stay infallible since they only ever buffer in memory.
pub trait Renderer {
    /// Called once before the first `Event`, with the pipeline's starting
    /// triad (which `Pipeline` itself never emits as an `Event`).
    fn start(&mut self, _triad: Triad) {}

    fn render(&mut self, event: &Event);

    /// Called once after the last `Event` to flush/finalize -- write a
    /// file, stop lingering notes, etc.
    fn finish(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// Whether a driver should pace itself in real time (sleeping
    /// `event.duration` between calls to `render`) before advancing, or
    /// just run through every event as fast as possible. Live backends
    /// (sound, a text trace meant to be watched) want real-time pacing;
    /// file-writing backends (MIDI, WAV) don't -- they encode timing into
    /// the output itself via `event.onset`/`event.duration` and should
    /// render as fast as possible instead of sleeping in the caller.
    fn wants_realtime_pacing(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CycleConfinedWalk, FreeWalk, HamiltonianCycleWalk, Mode, MovingVoice, NoFill, System,
        SystemFixedScale, Utt,
    };
    use rand::SeedableRng;
    use rand::rngs::StdRng;

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
            NoFill,
            start,
        );

        let mut manual_walk = FreeWalk::with_rng(StdRng::seed_from_u64(11));
        let mut manual_melody = MovingVoice;
        let mut manual_history = vec![start];
        let mut manual_triad = start;

        for i in 0..20 {
            let event = pipeline
                .next()
                .unwrap();

            let (next, op) = manual_walk.next(manual_triad, &manual_history);
            let notes = manual_melody.notes(manual_triad, next, op, &manual_history);
            manual_history.push(next);
            manual_triad = next;

            assert_eq!(event.triad, next);
            assert_eq!(event.op, op);
            assert_eq!(event.notes, notes);
            assert_eq!(event.onset, i as f64);
            assert_eq!(event.duration, 1.0);
            assert!(!event.is_fill);
        }
        assert_eq!(pipeline.current(), manual_triad);
    }

    #[test]
    fn pipeline_pushes_the_walk_s_current_system_into_system_fixed_scale() {
        // escape_probability 0.0 stays confined to Hexatonic every step, so
        // every event's notes should be the fixed 6-note collection derived
        // from that step's own triad.
        let mut pipeline = Pipeline::new(
            CycleConfinedWalk::with_rng(System::Hexatonic, 0.0, StdRng::seed_from_u64(5)),
            SystemFixedScale::new(),
            ConstantRhythm,
            NoFill,
            Triad::new(0, Mode::Major),
        );
        for event in pipeline
            .by_ref()
            .take(6)
        {
            let mut expected: Vec<_> = System::Hexatonic.pitch_classes(event.triad);
            expected.sort_by_key(|pc| pc.0);
            let mut notes = event
                .notes
                .clone();
            notes.sort_by_key(|pc| pc.0);
            assert_eq!(notes, expected);
        }
    }

    #[test]
    fn pipeline_moving_voice_always_yields_exactly_one_note() {
        let mut pipeline = Pipeline::new(
            HamiltonianCycleWalk::new(),
            MovingVoice,
            ConstantRhythm,
            NoFill,
            Triad::new(0, Mode::Major),
        );
        for event in pipeline
            .by_ref()
            .take(24)
        {
            assert_eq!(
                event
                    .notes
                    .len(),
                1,
                "P/L/R always move exactly one voice"
            );
            assert!(matches!(event.op, Utt::R | Utt::L));
        }
    }

    struct FixedFill;
    impl crate::FillStrategy for FixedFill {
        fn fills(&mut self, _triad: Triad, _duration: f64) -> Vec<(f64, PitchClass)> {
            vec![(0.25, PitchClass::new(1)), (0.75, PitchClass::new(2))]
        }
    }

    #[test]
    fn fills_are_interleaved_with_chained_durations_between_main_events() {
        let mut pipeline = Pipeline::new(
            HamiltonianCycleWalk::new(),
            MovingVoice,
            ConstantRhythm, // onset = index, duration = 1.0
            FixedFill,      // fills at 0.25 and 0.75 of the gap
            Triad::new(0, Mode::Major),
        );

        // Step 0: main event's own duration shrinks to reach the first
        // fill (0.25); fills chain legato from there to the next main
        // event's onset (1.0).
        let main0 = pipeline
            .next()
            .unwrap();
        assert!(!main0.is_fill);
        assert_eq!((main0.onset, main0.duration), (0.0, 0.25));

        let fill0a = pipeline
            .next()
            .unwrap();
        assert!(fill0a.is_fill);
        assert_eq!(fill0a.op, Utt::IDENTITY);
        assert_eq!(
            fill0a.triad, main0.triad,
            "fills don't move the harmonic walk"
        );
        assert_eq!(fill0a.notes, vec![PitchClass::new(1)]);
        assert_eq!((fill0a.onset, fill0a.duration), (0.25, 0.5));

        let fill0b = pipeline
            .next()
            .unwrap();
        assert!(fill0b.is_fill);
        assert_eq!(fill0b.notes, vec![PitchClass::new(2)]);
        assert_eq!((fill0b.onset, fill0b.duration), (0.75, 0.25));

        // Next main event picks up exactly where the last fill left off,
        // with its own full duration shrunk the same way.
        let main1 = pipeline
            .next()
            .unwrap();
        assert!(!main1.is_fill);
        assert_eq!((main1.onset, main1.duration), (1.0, 0.25));
        assert_ne!(
            main1.triad, main0.triad,
            "the walk still advances on main events"
        );
    }
}
