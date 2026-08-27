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
const PIANO_CHANNEL: i32 = 0; // == chord_channel below
/// Not committed (see .gitignore) -- fetch via
/// assets/soundfonts/fetch-salamander-piano.sh. Falls back to
/// GeneralUser GS's piano when absent.
const SALAMANDER_PATH: &str = "assets/soundfonts/salamander/SalamanderGrandPiano-V3+20200602.sf2";

fn piano_override_path() -> Option<&'static str> {
    if std::path::Path::new(SALAMANDER_PATH).exists() {
        Some(SALAMANDER_PATH)
    } else {
        eprintln!(
            "note: for a better piano, run assets/soundfonts/fetch-salamander-piano.sh (falling back to GeneralUser GS)"
        );
        None
    }
}

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
        if event.is_fill {
            print!(" ~{}~", event.notes[0]);
        } else {
            print!(" -{}-> {}", op_name(event.op), event.triad);
        }
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
                let digits = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")).unwrap_or(&value);
                seed = Some(u64::from_str_radix(digits, 16).expect("--seed must be a hexadecimal u64"));
            }
            other => eprintln!("warning: ignoring unknown argument '{other}'"),
        }
    }
    Args { backend, out, seed }
}

fn build_renderer(backend: &str, out: Option<String>) -> Result<Box<dyn Renderer>, Box<dyn Error>> {
    match backend {
        "sound" => {
            let config = SynthRendererConfig {
                chord_channel: PIANO_CHANNEL,
                chord_program: 0, // Acoustic Grand Piano
                chord_root_midi: 60,
                chord_velocity: 90,
                melody_channel: 1,
                melody_program: 73, // Flute
                melody_start_midi: 72,
                melody_velocity: 110,
            };
            let backend = match piano_override_path() {
                Some(piano_path) => SoundBackend::with_piano_override(SOUNDFONT_PATH, piano_path, PIANO_CHANNEL)?,
                None => SoundBackend::new(SOUNDFONT_PATH)?,
            };
            Ok(Box::new(SynthRenderer::new(backend, config)))
        }
        "text" => Ok(Box::new(TextRenderer)),
        "wav" => {
            let out_path = out.unwrap_or_else(|| "output.wav".to_string()).into();
            let config = WavRendererConfig {
                chord_channel: PIANO_CHANNEL,
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
            };
            let renderer = match piano_override_path() {
                Some(piano_path) => WavRenderer::with_piano_override(SOUNDFONT_PATH, piano_path, PIANO_CHANNEL, config)?,
                None => WavRenderer::new(SOUNDFONT_PATH, config)?,
            };
            Ok(Box::new(renderer))
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

    eprintln!("seed: {seed:#x} (pass --seed {seed:#x} to reproduce this run)");
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
