// Smoke test for the rustysynth + cpal audio pipeline: loads the bundled
// GeneralUser GS SoundFont and plays a C major triad for a couple seconds.
// Run with: cargo run --example synth_smoke_test

use std::fs::File;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

fn main() {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no default output device");
    let config = device
        .default_output_config()
        .expect("no default output config");
    let sample_rate = config.sample_rate() as i32;
    let channels = config.channels() as usize;
    let stream_config = config.config();

    let mut sf2 = File::open("assets/soundfonts/GeneralUser-GS.sf2").expect("open soundfont");
    let sound_font = Arc::new(SoundFont::new(&mut sf2).expect("parse soundfont"));

    let settings = SynthesizerSettings::new(sample_rate);
    let synthesizer = Arc::new(Mutex::new(
        Synthesizer::new(&sound_font, &settings).expect("create synthesizer"),
    ));

    // Program 0 = Acoustic Grand Piano. C major triad: C4, E4, G4.
    {
        let mut synth = synthesizer.lock().unwrap();
        synth.process_midi_message(0, 0xC0, 0, 0); // program change
        synth.note_on(0, 60, 100);
        synth.note_on(0, 64, 100);
        synth.note_on(0, 67, 100);
    }

    let mut left = vec![0f32; 0];
    let mut right = vec![0f32; 0];

    let stream = device
        .build_output_stream(
            stream_config,
            move |data: &mut [f32], _| {
                let frames = data.len() / channels;
                if left.len() != frames {
                    left.resize(frames, 0.0);
                    right.resize(frames, 0.0);
                }
                synthesizer.lock().unwrap().render(&mut left, &mut right);
                for (i, frame) in data.chunks_mut(channels).enumerate() {
                    for sample in frame.iter_mut() {
                        *sample = left[i]; // mono-mixed for simplicity
                    }
                    let _ = right[i];
                }
            },
            |err| eprintln!("stream error: {err}"),
            None,
        )
        .expect("build output stream");

    stream.play().expect("play stream");
    std::thread::sleep(std::time::Duration::from_secs(5));
}
