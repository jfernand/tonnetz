//! A rustysynth + cpal sound backend: loads a SoundFont, opens a real
//! audio output device, and plays MIDI notes or `tonnetz_core::Triad`s
//! through it. Extracted from tonnetz-cli's `synth_smoke_test` example,
//! which now just calls into this crate.
//!
//! `tonnetz_core::Triad` has no octave (CONCEPT.md section 8 leaves
//! pitch-class-vs-absolute-frequency as an open question) -- this crate
//! is where that question actually gets answered, by picking a MIDI note
//! for the root and building the triad up from there in close position.

mod dual_synth;
mod wav;
pub use wav::{WavRenderer, WavRendererConfig};

use std::error::Error;
use std::path::Path;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dual_synth::{load_synthesizer, DualSynth};
use tonnetz_core::{triad_midi_notes, Event, Renderer, Triad, VoiceTracker};

/// An open audio output stream backed by a SoundFont synthesizer (plus,
/// optionally, a second one overriding a single channel -- see
/// `with_piano_override`). Dropping it stops playback and releases the
/// device.
pub struct SoundBackend {
    synth: Arc<Mutex<DualSynth>>,
    _stream: cpal::Stream,
}

impl SoundBackend {
    pub fn new(soundfont_path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        Self::build(soundfont_path, None)
    }

    /// Like `new`, but a single `channel` is instead served by
    /// `piano_soundfont_path` -- e.g. swapping in a nicer piano sample
    /// library for the chord channel while everything else stays on the
    /// main General MIDI bank.
    pub fn with_piano_override(
        soundfont_path: impl AsRef<Path>,
        piano_soundfont_path: impl AsRef<Path>,
        channel: i32,
    ) -> Result<Self, Box<dyn Error>> {
        Self::build(soundfont_path, Some((piano_soundfont_path.as_ref(), channel)))
    }

    fn build(soundfont_path: impl AsRef<Path>, piano_override: Option<(&Path, i32)>) -> Result<Self, Box<dyn Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default audio output device")?;
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate() as i32;
        let channels = config.channels() as usize;
        let stream_config = config.config();

        let main = load_synthesizer(soundfont_path, sample_rate)?;
        let over = match piano_override {
            Some((path, channel)) => Some((channel, load_synthesizer(path, sample_rate)?)),
            None => None,
        };
        let synth = Arc::new(Mutex::new(DualSynth::new(main, over)));

        let render_synth = synth.clone();
        let mut left = Vec::new();
        let mut right = Vec::new();
        let stream = device.build_output_stream(
            stream_config,
            move |data: &mut [f32], _| {
                let frames = data.len() / channels;
                if left.len() != frames {
                    left.resize(frames, 0.0);
                    right.resize(frames, 0.0);
                }
                render_synth.lock().unwrap().render(&mut left, &mut right);
                for (i, frame) in data.chunks_mut(channels).enumerate() {
                    for sample in frame.iter_mut() {
                        *sample = left[i]; // mono-mixed for simplicity
                    }
                    let _ = right[i];
                }
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )?;
        stream.play()?;

        Ok(SoundBackend { synth, _stream: stream })
    }

    /// Selects a General MIDI program (instrument) on the given channel.
    pub fn set_program(&self, channel: i32, program: i32) {
        self.synth.lock().unwrap().program_change(channel, program);
    }

    pub fn note_on(&self, channel: i32, key: i32, velocity: i32) {
        self.synth.lock().unwrap().note_on(channel, key, velocity);
    }

    pub fn note_off(&self, channel: i32, key: i32) {
        self.synth.lock().unwrap().note_off(channel, key);
    }

    /// Starts a triad's three notes (root, third, fifth in close position
    /// above `root_midi`) sounding on channel 0.
    pub fn play_triad(&self, triad: Triad, root_midi: i32, velocity: i32) {
        for note in triad_midi_notes(triad, root_midi) {
            self.note_on(0, note, velocity);
        }
    }

    /// Stops the same three notes `play_triad` would have started.
    pub fn stop_triad(&self, triad: Triad, root_midi: i32) {
        for note in triad_midi_notes(triad, root_midi) {
            self.note_off(0, note);
        }
    }
}

/// Configuration for `SynthRenderer`: which channel/program/register/
/// velocity to use for the chord and for the (single-voice) melody.
pub struct SynthRendererConfig {
    pub chord_channel: i32,
    pub chord_program: i32,
    pub chord_root_midi: i32,
    pub chord_velocity: i32,
    pub melody_channel: i32,
    pub melody_program: i32,
    pub melody_start_midi: i32,
    pub melody_velocity: i32,
}

/// A `tonnetz_core::Renderer` over `SoundBackend`: stops whatever it last
/// played, starts the new event's chord, and tracks a single continuous
/// melody voice via `nearest_midi_note`. Only `event.notes.first()` is
/// rendered -- multi-note melody strategies (`TightScale`,
/// `RollingWindowScale`) aren't fully supported by this single-voice
/// renderer yet (see `Renderer`'s doc comment in tonnetz-core).
pub struct SynthRenderer {
    backend: SoundBackend,
    config: SynthRendererConfig,
    voice: VoiceTracker,
}

impl SynthRenderer {
    pub fn new(backend: SoundBackend, config: SynthRendererConfig) -> Self {
        backend.set_program(config.chord_channel, config.chord_program);
        backend.set_program(config.melody_channel, config.melody_program);
        let voice = VoiceTracker::new(config.chord_root_midi, config.melody_start_midi);
        SynthRenderer { backend, config, voice }
    }

    fn apply(&self, change: tonnetz_core::NoteChange) {
        if let Some(notes) = change.chord_off {
            for note in notes {
                self.backend.note_off(self.config.chord_channel, note);
            }
        }
        if let Some(midi) = change.melody_off {
            self.backend.note_off(self.config.melody_channel, midi);
        }
        if let Some(notes) = change.chord_on {
            for note in notes {
                self.backend.note_on(self.config.chord_channel, note, self.config.chord_velocity);
            }
        }
        if let Some(midi) = change.melody_on {
            self.backend.note_on(self.config.melody_channel, midi, self.config.melody_velocity);
        }
    }

}

impl Renderer for SynthRenderer {
    fn start(&mut self, triad: Triad) {
        let change = self.voice.start(triad);
        self.apply(change);
    }

    fn render(&mut self, event: &Event) {
        let change = self.voice.advance(event);
        self.apply(change);
    }

    fn finish(&mut self) -> Result<(), Box<dyn Error>> {
        let change = self.voice.finish();
        self.apply(change);
        Ok(())
    }
}
