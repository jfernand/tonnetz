//! MIDI-note math and the voice-leading state machine shared by every
//! `Renderer` backend (live synth, offline WAV render, MIDI file). None of
//! this depends on any particular audio engine or file format -- it only
//! decides which MIDI note numbers should turn on/off as a `Pipeline`
//! advances, leaving how "note on"/"note off" actually gets realized to
//! the caller.

use crate::{Event, Mode, PitchClass, Triad};

/// MIDI note number for a triad's root, third, and fifth in close
/// position, given the root's own MIDI note number. Always adds upward
/// (major third = +4, minor third = +3, fifth = +7), so it never needs to
/// wrap an octave the way raw pitch-class arithmetic would.
pub fn triad_midi_notes(triad: Triad, root_midi: i32) -> [i32; 3] {
    let third = match triad.mode {
        Mode::Major => 4,
        Mode::Minor => 3,
    };
    [root_midi, root_midi + third, root_midi + 7]
}

/// The MIDI note matching `pitch_class` that's closest to `reference_midi`
/// (ties break downward). This is what makes a `MovingVoice`-style melody
/// actually sound like a continuous line: `PitchClass` alone has no
/// octave, so re-deriving a MIDI note from scratch every step could leap
/// by up to 11 semitones even though the underlying voice only ever moves
/// by a step; anchoring each new note to the previous one keeps the
/// motion minimal, in whichever direction is actually shorter.
pub fn nearest_midi_note(reference_midi: i32, pitch_class: PitchClass) -> i32 {
    let same_octave = reference_midi - reference_midi.rem_euclid(12) + pitch_class.0 as i32;
    [same_octave - 12, same_octave, same_octave + 12]
        .into_iter()
        .min_by_key(|&m| (m - reference_midi).abs())
        .expect("array is non-empty")
}

/// Which MIDI notes should turn off and which should turn on for one step.
/// Chord and melody are kept separate (rather than one flat note list)
/// since channel/program routing between them is a per-renderer config
/// concern, not `VoiceTracker`'s.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoteChange {
    pub chord_off: Option<[i32; 3]>,
    pub chord_on: Option<[i32; 3]>,
    pub melody_off: Option<i32>,
    pub melody_on: Option<i32>,
}

/// Tracks the currently-sounding chord + melody voice across a `Pipeline`
/// walk, translating each `Event` into a `NoteChange` without knowing how
/// the caller actually realizes "note on"/"note off" (a live synth call, a
/// buffered audio sample, or a MIDI file event). Extracted from what used
/// to be `tonnetz_sound::SynthRenderer`'s only implementation of this
/// logic, so every backend shares one state machine instead of
/// reimplementing it.
///
/// Only ever tracks a single melody voice: like every current `Renderer`,
/// this only supports `event.notes.first()` -- multi-note melody
/// strategies (`TightScale`, `RollingWindowScale`) aren't fully supported
/// yet (see `Renderer`'s doc comment in `pipeline.rs`).
pub struct VoiceTracker {
    chord_root_midi: i32,
    melody_start_midi: i32,
    current_chord: Option<[i32; 3]>,
    current_melody: Option<i32>,
}

impl VoiceTracker {
    pub fn new(chord_root_midi: i32, melody_start_midi: i32) -> Self {
        VoiceTracker {
            chord_root_midi,
            melody_start_midi,
            current_chord: None,
            current_melody: None,
        }
    }

    /// A seed triad with no preceding event, anchoring the melody voice on
    /// its root. Use this once before feeding a `Pipeline`'s events
    /// through `advance` -- a `Pipeline` yields one `Event` per *step*, so
    /// it never produces one for its own starting triad.
    pub fn start(&mut self, triad: Triad) -> NoteChange {
        let chord = triad_midi_notes(triad, self.chord_root_midi);
        self.current_chord = Some(chord);

        let midi = nearest_midi_note(self.melody_start_midi, triad.root);
        self.current_melody = Some(midi);

        NoteChange {
            chord_off: None,
            chord_on: Some(chord),
            melody_off: None,
            melody_on: Some(midi),
        }
    }

    /// Stops whatever chord/melody note was last sounding and starts
    /// `event`'s, anchoring the new melody note on the previous one via
    /// `nearest_midi_note` regardless of whether `event.notes` is empty.
    pub fn advance(&mut self, event: &Event) -> NoteChange {
        let chord_off = self
            .current_chord
            .take();
        let melody_off = self.current_melody;

        let chord = triad_midi_notes(event.triad, self.chord_root_midi);
        self.current_chord = Some(chord);

        let anchor = self
            .current_melody
            .unwrap_or(self.melody_start_midi);
        let melody_on = event
            .notes
            .first()
            .map(|&pc| nearest_midi_note(anchor, pc));
        self.current_melody = melody_on;

        NoteChange {
            chord_off,
            chord_on: Some(chord),
            melody_off,
            melody_on,
        }
    }

    /// Like `advance`, but for a `FillStrategy`-produced event: only the
    /// melody voice moves (anchored on the previous melody note, same as
    /// `advance`); the chord is left exactly as it is, since a fill
    /// doesn't move the harmonic walk.
    pub fn advance_fill(&mut self, pitch: PitchClass) -> NoteChange {
        let melody_off = self.current_melody;

        let anchor = self
            .current_melody
            .unwrap_or(self.melody_start_midi);
        let midi = nearest_midi_note(anchor, pitch);
        self.current_melody = Some(midi);

        NoteChange {
            chord_off: None,
            chord_on: None,
            melody_off,
            melody_on: Some(midi),
        }
    }

    /// Stops whatever was last sounding, with nothing new to start. Call
    /// after consuming a pipeline, since neither `Renderer::render` nor
    /// `advance` has a "this is the last event" signal of their own.
    pub fn finish(&mut self) -> NoteChange {
        NoteChange {
            chord_off: self
                .current_chord
                .take(),
            chord_on: None,
            melody_off: self
                .current_melody
                .take(),
            melody_on: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Utt;

    #[test]
    fn triad_midi_notes_never_wrap_below_the_root() {
        for root in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let notes = triad_midi_notes(Triad::new(root, mode), 60);
                assert!(notes[0] < notes[1] && notes[1] < notes[2]);
                assert_eq!(notes[2] - notes[0], 7);
            }
        }
    }

    #[test]
    fn nearest_midi_note_matches_the_pitch_class() {
        for reference in 40..90 {
            for pc in 0..12 {
                let note = nearest_midi_note(reference, PitchClass::new(pc));
                assert_eq!(note.rem_euclid(12), pc.rem_euclid(12));
            }
        }
    }

    #[test]
    fn nearest_midi_note_never_moves_more_than_a_tritone() {
        for reference in 40..90 {
            for pc in 0..12 {
                let note = nearest_midi_note(reference, PitchClass::new(pc));
                assert!(
                    (note - reference).abs() <= 6,
                    "{note} too far from {reference}"
                );
            }
        }
    }

    #[test]
    fn nearest_midi_note_examples() {
        // B is the nearest occurrence of pitch class 11 below middle C.
        assert_eq!(nearest_midi_note(72, PitchClass::new(11)), 71);
        // C# a semitone above middle C, not a semitone below.
        assert_eq!(nearest_midi_note(60, PitchClass::new(1)), 61);
    }

    #[test]
    fn voice_tracker_start_turns_on_with_no_offs() {
        let mut tracker = VoiceTracker::new(60, 72);
        let change = tracker.start(Triad::new(0, Mode::Major));
        assert_eq!(change.chord_off, None);
        assert_eq!(change.melody_off, None);
        assert_eq!(change.chord_on, Some([60, 64, 67]));
        assert_eq!(
            change.melody_on,
            Some(nearest_midi_note(72, PitchClass::new(0)))
        );
    }

    #[test]
    fn voice_tracker_advance_turns_off_previous_and_on_next() {
        let mut tracker = VoiceTracker::new(60, 72);
        let start = tracker.start(Triad::new(0, Mode::Major));

        let c_major = Triad::new(0, Mode::Major);
        let a_minor = Utt::R.apply(c_major);
        let event = Event {
            triad: a_minor,
            op: Utt::R,
            notes: vec![PitchClass::new(9)], // the new A that MovingVoice would report
            onset: 0.0,
            duration: 1.0,
            is_fill: false,
        };
        let change = tracker.advance(&event);

        assert_eq!(change.chord_off, start.chord_on);
        assert_eq!(change.melody_off, start.melody_on);
        assert_eq!(change.chord_on, Some(triad_midi_notes(a_minor, 60)));
        // Anchored on the previous melody note (72's octave of pitch class 0),
        // not re-derived from scratch.
        let anchor = start
            .melody_on
            .unwrap();
        assert_eq!(
            change.melody_on,
            Some(nearest_midi_note(anchor, PitchClass::new(9)))
        );
    }

    #[test]
    fn voice_tracker_advance_with_no_melody_notes_turns_off_without_turning_on() {
        let mut tracker = VoiceTracker::new(60, 72);
        tracker.start(Triad::new(0, Mode::Major));

        let event = Event {
            triad: Triad::new(0, Mode::Minor),
            op: Utt::P,
            notes: vec![],
            onset: 0.0,
            duration: 1.0,
            is_fill: false,
        };
        let change = tracker.advance(&event);
        assert!(
            change
                .melody_off
                .is_some()
        );
        assert_eq!(change.melody_on, None);
    }

    #[test]
    fn voice_tracker_finish_turns_off_whatever_was_last_playing() {
        let mut tracker = VoiceTracker::new(60, 72);
        let start = tracker.start(Triad::new(0, Mode::Major));
        let change = tracker.finish();
        assert_eq!(change.chord_off, start.chord_on);
        assert_eq!(change.melody_off, start.melody_on);
        assert_eq!(change.chord_on, None);
        assert_eq!(change.melody_on, None);

        // A second finish() has nothing left to turn off.
        let again = tracker.finish();
        assert_eq!(again.chord_off, None);
        assert_eq!(again.melody_off, None);
    }

    #[test]
    fn advance_fill_leaves_the_chord_untouched() {
        let mut tracker = VoiceTracker::new(60, 72);
        let start = tracker.start(Triad::new(0, Mode::Major));

        let change = tracker.advance_fill(PitchClass::new(7));
        assert_eq!(change.chord_off, None);
        assert_eq!(change.chord_on, None);
        assert_eq!(change.melody_off, start.melody_on);
        // Anchored on the previous melody note, same as `advance`.
        let anchor = start
            .melody_on
            .unwrap();
        assert_eq!(
            change.melody_on,
            Some(nearest_midi_note(anchor, PitchClass::new(7)))
        );
    }

    #[test]
    fn advance_fill_chains_off_the_previous_fill() {
        let mut tracker = VoiceTracker::new(60, 72);
        tracker.start(Triad::new(0, Mode::Major));

        let first = tracker.advance_fill(PitchClass::new(7));
        let second = tracker.advance_fill(PitchClass::new(4));
        assert_eq!(second.melody_off, first.melody_on);
        assert_eq!(second.chord_off, None);
        assert_eq!(second.chord_on, None);
    }
}
