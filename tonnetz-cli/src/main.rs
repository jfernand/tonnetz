mod random_pipeline;

use std::error::Error;
use std::thread::sleep;
use std::time::Duration;

use rand::RngExt;
use tonnetz_core::{Event, Mode, Renderer, Triad, Utt};
use tonnetz_midi::{MidiRenderer, MidiRendererConfig};
use tonnetz_sound::{SoundBackend, SynthRenderer, SynthRendererConfig, WavRenderer, WavRendererConfig};

const UNIT_SECONDS: f64 = 0.3; // one Euclidean rhythm "step"
const STEPS: usize = 24;
const SOUNDFONT_PATH: &str = "assets/soundfonts/GeneralUser-GS.sf2";

fn op_name(op: Utt) -> &'static str {
    match op {
        Utt::P => "P",
        Utt::L => "L",
        Utt::R => "R",
        _ => "?",
    }
}

/// Formalizes the plain trace this CLI always used to print alongside live
/// sound into a real `Renderer` -- proving the abstraction covers existing
/// behavior, not just the new file-writing backends. Choosing `--backend
/// sound` now means the trace no longer prints; only `--backend text` does.
struct TextRenderer;

impl Renderer for TextRenderer {
    fn start(&mut self, triad: Triad) {
        print!("{triad}");
    }

    fn render(&mut self, event: &Event) {
        print!(" -{}-> {}", op_name(event.op), event.triad);
    }

    fn finish(&mut self) -> Result<(), Box<dyn Error>> {
        println!();
        Ok(())
    }
}

struct Args {
    backend: String,
    out: Option<String>,
    seed: Option<u64>,
}

fn parse_args() -> Args {
    let mut backend = "sound".to_string();
    let mut out = None;
    let mut seed = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--backend" => backend = args.next().expect("--backend requires a value"),
            "--out" => out = Some(args.next().expect("--out requires a value")),
            "--seed" => {
                let value = args.next().expect("--seed requires a value");
                seed = Some(value.parse().expect("--seed must be a u64"));
            }
            other => eprintln!("warning: ignoring unknown argument '{other}'"),
        }
    }
    Args { backend, out, seed }
}

fn build_renderer(backend: &str, out: Option<String>) -> Result<Box<dyn Renderer>, Box<dyn Error>> {
    match backend {
        "sound" => {
            let backend = SoundBackend::new(SOUNDFONT_PATH)?;
            Ok(Box::new(SynthRenderer::new(
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
            )))
        }
        "text" => Ok(Box::new(TextRenderer)),
        "wav" => {
            let out_path = out.unwrap_or_else(|| "output.wav".to_string()).into();
            Ok(Box::new(WavRenderer::new(
                SOUNDFONT_PATH,
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
                    unit_seconds: UNIT_SECONDS,
                    release_seconds: 1.0,
                    out_path,
                },
            )?))
        }
        "midi" => {
            let out_path = out.unwrap_or_else(|| "output.mid".to_string()).into();
            Ok(Box::new(MidiRenderer::new(MidiRendererConfig {
                chord_channel: 0,
                chord_program: 0,
                chord_root_midi: 60,
                chord_velocity: 90,
                melody_channel: 1,
                melody_program: 73,
                melody_start_midi: 72,
                melody_velocity: 110,
                ticks_per_unit: 480,
                unit_seconds: UNIT_SECONDS,
                out_path,
            })))
        }
        other => Err(format!("unknown backend '{other}' (expected sound|text|midi|wav)").into()),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args();
    let seed = args.seed.unwrap_or_else(|| rand::rng().random());

    let mut renderer = build_renderer(&args.backend, args.out)?;

    let start = Triad::new(0, Mode::Major);
    let (mut pipeline, choice) = random_pipeline::build_pipeline(seed, start);

    eprintln!("seed: {seed} (pass --seed {seed} to reproduce this run)");
    eprintln!("{choice}");

    renderer.start(start);

    let mut last_duration = 0.0;
    for event in pipeline.by_ref().take(STEPS) {
        if renderer.wants_realtime_pacing() {
            sleep(Duration::from_secs_f64(event.duration * UNIT_SECONDS));
        }
        renderer.render(&event);
        last_duration = event.duration;
    }

    if renderer.wants_realtime_pacing() {
        sleep(Duration::from_secs_f64(last_duration * UNIT_SECONDS));
    }
    renderer.finish()?;

    Ok(())
}
