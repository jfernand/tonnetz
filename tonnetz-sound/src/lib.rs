//! A rustysynth + cpal sound backend: loads a SoundFont, opens a real
//! audio output device, and plays MIDI notes or `tonnetz_core::Triad`s
//! through it. Extracted from tonnetz-cli's `synth_smoke_test` example,
//! which now just calls into this crate.
//!
//! `tonnetz_core::Triad` has no octave (CONCEPT.md section 8 leaves
//! pitch-class-vs-absolute-frequency as an open question) -- this crate
//! is where that question actually gets answered, by picking a MIDI note
//! for the root and building the triad up from there in close position.

use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use tonnetz_core::{Mode, Triad};

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

/// An open audio output stream backed by a SoundFont synthesizer. Dropping
/// it stops playback and releases the device.
pub struct SoundBackend {
    synthesizer: Arc<Mutex<Synthesizer>>,
    _stream: cpal::Stream,
}

impl SoundBackend {
    pub fn new(soundfont_path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default audio output device")?;
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate() as i32;
        let channels = config.channels() as usize;
        let stream_config = config.config();

        let mut sf2 = File::open(soundfont_path)?;
        let sound_font = Arc::new(SoundFont::new(&mut sf2)?);
        let settings = SynthesizerSettings::new(sample_rate);
        let synthesizer = Arc::new(Mutex::new(Synthesizer::new(&sound_font, &settings)?));

        let render_synth = synthesizer.clone();
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

        Ok(SoundBackend {
            synthesizer,
            _stream: stream,
        })
    }

    /// Selects a General MIDI program (instrument) on the given channel.
    pub fn set_program(&self, channel: i32, program: i32) {
        self.synthesizer
            .lock()
            .unwrap()
            .process_midi_message(channel, 0xC0, program, 0);
    }

    pub fn note_on(&self, channel: i32, key: i32, velocity: i32) {
        self.synthesizer.lock().unwrap().note_on(channel, key, velocity);
    }

    pub fn note_off(&self, channel: i32, key: i32) {
        self.synthesizer.lock().unwrap().note_off(channel, key);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
