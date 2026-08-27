//! Writes a `Pipeline`'s events to a Standard MIDI File instead of playing
//! them live -- the same `VoiceTracker` state machine `tonnetz-sound` uses,
//! just realized as buffered MIDI events instead of live synth calls.
//!
//! Like `tonnetz_sound::WavRenderer`, timing here comes entirely from
//! `event.onset`/`event.duration` (converted to a tick count via
//! `ticks_per_unit`), not real wall-clock pacing -- `render` should be
//! called as fast as possible; `wants_realtime_pacing` says so. Tempo is
//! derived from the same `unit_seconds` every other backend uses, so
//! switching backends doesn't change the perceived tempo: one abstract
//! time unit is treated as one MIDI quarter note, so
//! `tempo_us_per_quarter = unit_seconds * 1_000_000`.

use std::error::Error;
use std::path::PathBuf;

use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use tonnetz_core::{Event, NoteChange, Renderer, Triad, VoiceTracker};

/// Configuration for `MidiRenderer`: the same chord/melody channel-routing
/// knobs as `SynthRendererConfig`/`WavRendererConfig`, plus the ticks-per-
/// unit resolution (also used as the file's ticks-per-quarter-note, since
/// one abstract time unit is treated as one quarter note), the
/// abstract-time-to-seconds scale (kept the same across every backend),
/// and the output file path.
pub struct MidiRendererConfig {
    pub chord_channel: u8,
    pub chord_program: u8,
    pub chord_root_midi: i32,
    pub chord_velocity: u8,
    pub melody_channel: u8,
    pub melody_program: u8,
    pub melody_start_midi: i32,
    pub melody_velocity: u8,
    pub ticks_per_unit: u16,
    pub unit_seconds: f64,
    pub out_path: PathBuf,
}

/// One buffered MIDI channel message at an absolute tick, before
/// delta-encoding. `is_off` breaks ties so that a note-off due at the same
/// tick as a note-on (e.g. one voice releasing exactly as another starts)
/// is written first -- writing them in the other order would mean, for an
/// instant, a synth voice that should be silent is still sounding.
struct AbsEvent {
    tick: u32,
    is_off: bool,
    channel: u8,
    message: MidiMessage,
}

pub struct MidiRenderer {
    config: MidiRendererConfig,
    voice: VoiceTracker,
    events: Vec<AbsEvent>,
    last_event_end_units: f64,
}

impl MidiRenderer {
    pub fn new(config: MidiRendererConfig) -> Self {
        let voice = VoiceTracker::new(config.chord_root_midi, config.melody_start_midi);
        let events = vec![
            AbsEvent {
                tick: 0,
                is_off: false,
                channel: config.chord_channel,
                message: MidiMessage::ProgramChange {
                    program: u7::new(config.chord_program),
                },
            },
            AbsEvent {
                tick: 0,
                is_off: false,
                channel: config.melody_channel,
                message: MidiMessage::ProgramChange {
                    program: u7::new(config.melody_program),
                },
            },
        ];
        MidiRenderer {
            config,
            voice,
            events,
            last_event_end_units: 0.0,
        }
    }

    fn units_to_ticks(&self, units: f64) -> u32 {
        (units * self.config.ticks_per_unit as f64).round() as u32
    }

    fn push_change(&mut self, tick: u32, change: NoteChange) {
        if let Some(notes) = change.chord_off {
            for note in notes {
                self.push_note(tick, true, self.config.chord_channel, note, 0);
            }
        }
        if let Some(midi) = change.melody_off {
            self.push_note(tick, true, self.config.melody_channel, midi, 0);
        }
        if let Some(notes) = change.chord_on {
            for note in notes {
                self.push_note(tick, false, self.config.chord_channel, note, self.config.chord_velocity);
            }
        }
        if let Some(midi) = change.melody_on {
            self.push_note(tick, false, self.config.melody_channel, midi, self.config.melody_velocity);
        }
    }

    fn push_note(&mut self, tick: u32, is_off: bool, channel: u8, key: i32, velocity: u8) {
        let message = if is_off {
            MidiMessage::NoteOff {
                key: u7::new(key as u8),
                vel: u7::new(velocity),
            }
        } else {
            MidiMessage::NoteOn {
                key: u7::new(key as u8),
                vel: u7::new(velocity),
            }
        };
        self.events.push(AbsEvent { tick, is_off, channel, message });
    }
}

impl Renderer for MidiRenderer {
    fn start(&mut self, triad: Triad) {
        let change = self.voice.start(triad);
        self.push_change(0, change);
    }

    fn render(&mut self, event: &Event) {
        let tick = self.units_to_ticks(event.onset);
        let change = if event.is_fill {
            self.voice.advance_fill(event.notes[0])
        } else {
            self.voice.advance(event)
        };
        self.push_change(tick, change);
        self.last_event_end_units = event.onset + event.duration;
    }

    fn finish(&mut self) -> Result<(), Box<dyn Error>> {
        let tick = self.units_to_ticks(self.last_event_end_units);
        let change = self.voice.finish();
        self.push_change(tick, change);

        // Stable sort: note-offs before note-ons at the same tick (see
        // `AbsEvent::is_off`'s doc comment), otherwise input order, which
        // is already tick-ascending since every event is pushed in the
        // order `render`/`finish` computed it.
        self.events.sort_by_key(|e| (e.tick, !e.is_off));

        let tempo_us_per_quarter = (self.config.unit_seconds * 1_000_000.0).round() as u32;
        let mut track = vec![TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(tempo_us_per_quarter))),
        }];

        let mut last_tick = 0u32;
        for e in &self.events {
            track.push(TrackEvent {
                delta: u28::new(e.tick.saturating_sub(last_tick)),
                kind: TrackEventKind::Midi {
                    channel: u4::new(e.channel),
                    message: e.message,
                },
            });
            last_tick = e.tick;
        }
        track.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });

        let header = Header::new(Format::SingleTrack, Timing::Metrical(u15::new(self.config.ticks_per_unit)));
        let smf = Smf { header, tracks: vec![track] };
        smf.save(&self.config.out_path)?;
        Ok(())
    }

    fn wants_realtime_pacing(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonnetz_core::{Euclidean, FreeWalk, Mode, MovingVoice, NoFill, Pipeline};

    fn test_config(out_path: PathBuf) -> MidiRendererConfig {
        MidiRendererConfig {
            chord_channel: 0,
            chord_program: 0,
            chord_root_midi: 60,
            chord_velocity: 90,
            melody_channel: 1,
            melody_program: 73,
            melody_start_midi: 72,
            melody_velocity: 110,
            ticks_per_unit: 480,
            unit_seconds: 0.3,
            out_path,
        }
    }

    #[test]
    fn midi_renderer_round_trips_through_a_real_parse() {
        let out_path = std::env::temp_dir().join("tonnetz_midi_renderer_test.mid");
        let mut renderer = MidiRenderer::new(test_config(out_path.clone()));

        let start = Triad::new(0, Mode::Major);
        let mut pipeline = Pipeline::new(FreeWalk::new(), MovingVoice, Euclidean::new(3, 8), NoFill, start);
        renderer.start(start);
        for event in pipeline.by_ref().take(8) {
            renderer.render(&event);
        }
        renderer.finish().expect("finish should write the file");

        let bytes = std::fs::read(&out_path).expect("file should exist");
        let smf = Smf::parse(&bytes).expect("file should be a valid SMF");

        assert_eq!(smf.header.format, Format::SingleTrack);
        assert_eq!(smf.header.timing, Timing::Metrical(u15::new(480)));
        assert_eq!(smf.tracks.len(), 1);

        let track = &smf.tracks[0];
        let note_on_count = track
            .iter()
            .filter(|e| matches!(e.kind, TrackEventKind::Midi { message: MidiMessage::NoteOn { .. }, .. }))
            .count();
        assert!(note_on_count > 0, "expected at least one NoteOn event");

        let has_tempo = track
            .iter()
            .any(|e| matches!(e.kind, TrackEventKind::Meta(MetaMessage::Tempo(_))));
        assert!(has_tempo, "expected a tempo meta event");

        let has_end_of_track = matches!(track.last().map(|e| &e.kind), Some(TrackEventKind::Meta(MetaMessage::EndOfTrack)));
        assert!(has_end_of_track, "expected the track to end with EndOfTrack");

        // Absolute ticks (running sum of deltas) must be monotonically
        // non-decreasing -- this is what actually proves the sort in
        // `finish` produced a well-formed, playable event order.
        let mut tick = 0u32;
        let mut last_tick = 0u32;
        for e in track {
            tick += u32::from(e.delta);
            assert!(tick >= last_tick, "ticks must never go backwards");
            last_tick = tick;
        }

        let _ = std::fs::remove_file(&out_path);
    }
}
