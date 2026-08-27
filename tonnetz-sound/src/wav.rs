//! Offline WAV rendering: drives the same `rustysynth::Synthesizer` as the
//! live `SynthRenderer`, but with no cpal stream -- samples are rendered
//! directly into an in-memory buffer and written to a file at `finish()`,
//! so it's the first `Renderer` runnable with no audio device at all.
//!
//! Unlike the live renderer, timing here comes entirely from `event.onset`/
//! `event.duration` (converted to a sample-frame count via `unit_seconds`
//! and `sample_rate`), not from real wall-clock pacing -- `render` should
//! be called as fast as possible; `wants_realtime_pacing` says so.

use std::error::Error;
use std::path::{Path, PathBuf};

use crate::dual_synth::{DualSynth, load_synthesizer};
use tonnetz_core::{Event, NoteChange, Renderer, Triad, VoiceTracker};

/// Configuration for `WavRenderer`: the same chord/melody channel-routing
/// knobs as `SynthRendererConfig`, plus the sample rate, the
/// abstract-time-unit-to-seconds scale (kept the same across every
/// backend so switching backends doesn't change the perceived tempo), a
/// short release tail so the last note doesn't cut off abruptly, and the
/// output file path.
pub struct WavRendererConfig {
    pub chord_channel: i32,
    pub chord_program: i32,
    pub chord_root_midi: i32,
    pub chord_velocity: i32,
    pub melody_channel: i32,
    pub melody_program: i32,
    pub melody_start_midi: i32,
    pub melody_velocity: i32,
    pub sample_rate: i32,
    pub unit_seconds: f64,
    pub release_seconds: f64,
    pub out_path: PathBuf,
}

pub struct WavRenderer {
    synthesizer: DualSynth,
    config: WavRendererConfig,
    voice: VoiceTracker,
    samples_per_unit: f64,
    rendered_samples: usize,
    last_event_end_units: f64,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl WavRenderer {
    pub fn new(soundfont_path: impl AsRef<Path>, config: WavRendererConfig) -> Result<Self, Box<dyn Error>> {
        Self::build(soundfont_path, None, config)
    }

    /// Like `new`, but a single `channel` is instead served by
    /// `piano_soundfont_path` -- see `SoundBackend::with_piano_override`,
    /// which this mirrors for offline rendering.
    pub fn with_piano_override(
        soundfont_path: impl AsRef<Path>,
        piano_soundfont_path: impl AsRef<Path>,
        channel: i32,
        config: WavRendererConfig,
    ) -> Result<Self, Box<dyn Error>> {
        Self::build(soundfont_path, Some((piano_soundfont_path.as_ref(), channel)), config)
    }

    fn build(
        soundfont_path: impl AsRef<Path>,
        piano_override: Option<(&Path, i32)>,
        config: WavRendererConfig,
    ) -> Result<Self, Box<dyn Error>> {
        let main = load_synthesizer(soundfont_path, config.sample_rate)?;
        let over = match piano_override {
            Some((path, channel)) => Some((channel, load_synthesizer(path, config.sample_rate)?)),
            None => None,
        };
        let mut synthesizer = DualSynth::new(main, over);
        synthesizer.program_change(config.chord_channel, config.chord_program);
        synthesizer.program_change(config.melody_channel, config.melody_program);

        let voice = VoiceTracker::new(config.chord_root_midi, config.melody_start_midi);
        let samples_per_unit = config.sample_rate as f64 * config.unit_seconds;

        Ok(WavRenderer {
            synthesizer,
            config,
            voice,
            samples_per_unit,
            rendered_samples: 0,
            last_event_end_units: 0.0,
            left: Vec::new(),
            right: Vec::new(),
        })
    }

    /// Renders forward, sample by sample, up to `target_sample`. A no-op
    /// if already there or past it (e.g. two events sharing an onset).
    fn advance_to(&mut self, target_sample: usize) {
        if target_sample <= self.rendered_samples {
            return;
        }
        let n = target_sample - self.rendered_samples;
        let mut left = vec![0.0f32; n];
        let mut right = vec![0.0f32; n];
        self.synthesizer.render(&mut left, &mut right);
        self.left.extend_from_slice(&left);
        self.right.extend_from_slice(&right);
        self.rendered_samples = target_sample;
    }

    fn apply(&mut self, change: NoteChange) {
        if let Some(notes) = change.chord_off {
            for note in notes {
                self.synthesizer.note_off(self.config.chord_channel, note);
            }
        }
        if let Some(midi) = change.melody_off {
            self.synthesizer.note_off(self.config.melody_channel, midi);
        }
        if let Some(notes) = change.chord_on {
            for note in notes {
                self.synthesizer
                    .note_on(self.config.chord_channel, note, self.config.chord_velocity);
            }
        }
        if let Some(midi) = change.melody_on {
            self.synthesizer
                .note_on(self.config.melody_channel, midi, self.config.melody_velocity);
        }
    }

    fn units_to_samples(&self, units: f64) -> usize {
        (units * self.samples_per_unit).round() as usize
    }
}

impl Renderer for WavRenderer {
    fn start(&mut self, triad: Triad) {
        let change = self.voice.start(triad);
        self.apply(change);
    }

    fn render(&mut self, event: &Event) {
        self.advance_to(self.units_to_samples(event.onset));
        let change = if event.is_fill {
            self.voice.advance_fill(event.notes[0])
        } else {
            self.voice.advance(event)
        };
        self.apply(change);
        self.last_event_end_units = event.onset + event.duration;
    }

    fn finish(&mut self) -> Result<(), Box<dyn Error>> {
        self.advance_to(self.units_to_samples(self.last_event_end_units));
        let change = self.voice.finish();
        self.apply(change);

        let release_samples = (self.config.release_seconds * self.config.sample_rate as f64).round() as usize;
        self.advance_to(self.rendered_samples + release_samples);

        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: self.config.sample_rate as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&self.config.out_path, spec)?;
        for (&l, &r) in self.left.iter().zip(self.right.iter()) {
            writer.write_sample(l)?;
            writer.write_sample(r)?;
        }
        writer.finalize()?;
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

    fn soundfont_path() -> String {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/soundfonts/GeneralUser-GS.sf2").to_string()
    }

    fn test_config(out_path: PathBuf) -> WavRendererConfig {
        WavRendererConfig {
            chord_channel: 0,
            chord_program: 0,
            chord_root_midi: 60,
            chord_velocity: 90,
            melody_channel: 1,
            melody_program: 73,
            melody_start_midi: 72,
            melody_velocity: 110,
            sample_rate: 44100,
            unit_seconds: 0.1,
            release_seconds: 0.2,
            out_path,
        }
    }

    #[test]
    fn wav_renderer_writes_a_non_silent_file_of_the_expected_length() {
        let out_path = std::env::temp_dir().join("tonnetz_wav_renderer_test.wav");
        let config = test_config(out_path.clone());
        let unit_seconds = config.unit_seconds;
        let sample_rate = config.sample_rate;
        let mut renderer = WavRenderer::new(soundfont_path(), config).expect("renderer should construct");

        let start = Triad::new(0, Mode::Major);
        let mut pipeline = Pipeline::new(FreeWalk::new(), MovingVoice, Euclidean::new(3, 8), NoFill, start);
        renderer.start(start);
        let mut last_end = 0.0;
        for event in pipeline.by_ref().take(8) {
            last_end = event.onset + event.duration;
            renderer.render(&event);
        }
        renderer.finish().expect("finish should write the file");

        let reader = hound::WavReader::open(&out_path).expect("file should be a valid WAV");
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, sample_rate as u32);
        assert_eq!(spec.sample_format, hound::SampleFormat::Float);

        let samples: Vec<f32> = reader.into_samples::<f32>().map(|s| s.unwrap()).collect();
        assert!(!samples.is_empty());

        let expected_seconds = last_end * unit_seconds + 0.2;
        let actual_seconds = (samples.len() / 2) as f64 / sample_rate as f64;
        assert!(
            (actual_seconds - expected_seconds).abs() < 0.01,
            "expected ~{expected_seconds}s, got {actual_seconds}s"
        );

        let max_amplitude = samples.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
        assert!(max_amplitude > 0.01, "expected audible output, max amplitude was {max_amplitude}");

        let _ = std::fs::remove_file(&out_path);
    }
}
