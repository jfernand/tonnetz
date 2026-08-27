//! A primary `Synthesizer` plus an optional second one that overrides a
//! single channel -- e.g. a nicer piano soundfont standing in for the
//! chord channel's default GM piano, without needing a full second GM
//! bank just to get one better instrument. Shared by `SoundBackend`
//! (live playback) and `WavRenderer` (offline rendering), since both
//! need the exact same "route by channel, sum when rendering" logic.

use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

pub(crate) fn load_synthesizer(path: impl AsRef<Path>, sample_rate: i32) -> Result<Synthesizer, Box<dyn Error>> {
    let mut file = File::open(path)?;
    let sound_font = Arc::new(SoundFont::new(&mut file)?);
    let settings = SynthesizerSettings::new(sample_rate);
    Ok(Synthesizer::new(&sound_font, &settings)?)
}

pub(crate) struct DualSynth {
    main: Synthesizer,
    over: Option<(i32, Synthesizer)>,
    scratch_left: Vec<f32>,
    scratch_right: Vec<f32>,
}

impl DualSynth {
    pub(crate) fn new(main: Synthesizer, over: Option<(i32, Synthesizer)>) -> Self {
        DualSynth {
            main,
            over,
            scratch_left: Vec::new(),
            scratch_right: Vec::new(),
        }
    }

    fn target(&mut self, channel: i32) -> &mut Synthesizer {
        match &mut self.over {
            Some((over_channel, synth)) if *over_channel == channel => synth,
            _ => &mut self.main,
        }
    }

    pub(crate) fn note_on(&mut self, channel: i32, key: i32, velocity: i32) {
        self.target(channel).note_on(channel, key, velocity);
    }

    pub(crate) fn note_off(&mut self, channel: i32, key: i32) {
        self.target(channel).note_off(channel, key);
    }

    pub(crate) fn program_change(&mut self, channel: i32, program: i32) {
        self.target(channel).process_midi_message(channel, 0xC0, program, 0);
    }

    /// Renders the primary synth into `left`/`right`, then, if there's an
    /// override, renders it into scratch buffers and sums it in -- the two
    /// soundfonts are independent audio sources being layered, not a
    /// single signal chain.
    pub(crate) fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.main.render(left, right);
        if let Some((_, synth)) = &mut self.over {
            if self.scratch_left.len() != left.len() {
                self.scratch_left.resize(left.len(), 0.0);
                self.scratch_right.resize(right.len(), 0.0);
            }
            synth.render(&mut self.scratch_left, &mut self.scratch_right);
            for i in 0..left.len() {
                left[i] += self.scratch_left[i];
                right[i] += self.scratch_right[i];
            }
        }
    }
}
