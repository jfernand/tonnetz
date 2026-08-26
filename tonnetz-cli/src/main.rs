use std::error::Error;
use std::thread::sleep;
use std::time::Duration;

use tonnetz_core::{Euclidean, FreeWalk, Mode, MovingVoice, Pipeline, Renderer, Triad, Utt};
use tonnetz_sound::{SoundBackend, SynthRenderer, SynthRendererConfig};

const UNIT_SECONDS: f64 = 0.3; // one Euclidean rhythm "step"
const STEPS: usize = 24;

fn op_name(op: Utt) -> &'static str {
    match op {
        Utt::P => "P",
        Utt::L => "L",
        Utt::R => "R",
        _ => "?",
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let backend = SoundBackend::new("assets/soundfonts/GeneralUser-GS.sf2")?;
    let mut renderer = SynthRenderer::new(
        backend,
        SynthRendererConfig {
            chord_channel: 0,
            chord_program: 0, // Acoustic Grand Piano
            chord_root_midi: 60,
            chord_velocity: 90,
            melody_channel: 1,
            melody_program: 73, // Flute
            melody_start_midi: 72,
            melody_velocity: 110,
        },
    );

    let start = Triad::new(0, Mode::Major);
    let mut pipeline = Pipeline::new(FreeWalk::new(), MovingVoice, Euclidean::new(3, 8), start);

    print!("{start}");
    renderer.start(start);

    let mut last_duration = 0.0;
    for event in pipeline.by_ref().take(STEPS) {
        sleep(Duration::from_secs_f64(event.duration * UNIT_SECONDS));
        print!(" -{}-> {}", op_name(event.op), event.triad);
        renderer.render(&event);
        last_duration = event.duration;
    }

    sleep(Duration::from_secs_f64(last_duration * UNIT_SECONDS));
    renderer.silence();
    println!();

    Ok(())
}
