mod random_pipeline;

use std::error::Error;
use std::io::{self, Write};
use std::thread::sleep;
use std::time::Duration;

use rand::RngExt;
use tonnetz_core::{Event, Mode, Renderer, Triad, Utt};
use tonnetz_midi::{MidiRenderer, MidiRendererConfig};
use tonnetz_sound::{
    SoundBackend, SynthRenderer, SynthRendererConfig, WavRenderer, WavRendererConfig,
};

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

/// Widest a `PitchClass`'s `Display` ever gets, e.g. "A#" -- used to pad
/// note columns so consecutive lines line up regardless of which notes
/// appear.
const PITCH_WIDTH: usize = 2;

/// A triad as its three notes, root first (`Triad::pitch_classes()`'s own
/// order) -- unlike `Triad`'s bare `Display` (e.g. "C" for C major, with no
/// suffix), this can never be mistaken for a single melody note. Always
/// exactly 10 characters, so triad columns line up across lines without
/// needing their own padding.
fn triad_notes(triad: Triad) -> String {
    let pcs = triad.pitch_classes();
    format!("[{:<PITCH_WIDTH$} {:<PITCH_WIDTH$} {:<PITCH_WIDTH$}]", pcs[0], pcs[1], pcs[2])
}

/// Prints the seed triad alone -- called once before the loop, independent
/// of which `Renderer` is actually producing sound/a file, so the notes
/// being played are always visible regardless of `--backend`.
///
/// `print!`/`println!` alone don't make text show up as it's written:
/// stdout is only line-buffered (flushing on `\n`) when connected to a
/// terminal, and fully block-buffered otherwise (e.g. piped to `tee`) --
/// either way, an explicit `flush()` is the only thing that's guaranteed
/// to make each line appear immediately rather than all at once at exit.
fn print_start(triad: Triad) {
    println!("{}", triad_notes(triad));
    let _ = io::stdout().flush();
}

/// Prints one event on its own line as it's triggered: a fill shows just
/// its own pitch; a main event shows `prev -op-> new +melody note(s)` --
/// `prev` is the previous *main* event's triad (fills don't move it,
/// mirroring `Pipeline`'s own "fills don't move the harmonic walk" rule).
/// Every field is padded to its widest possible value so the columns line
/// up across lines.
fn print_event(prev: Triad, event: &Event) {
    if event.is_fill {
        println!("  ~{:<PITCH_WIDTH$}~", event.notes[0]);
    } else {
        print!("{} -{}-> {}", triad_notes(prev), op_name(event.op), triad_notes(event.triad));
        for pc in &event.notes {
            print!(" +{pc:<PITCH_WIDTH$}");
        }
        println!();
    }
    let _ = io::stdout().flush();
}

/// A `Renderer` that produces no audio or file -- pairs with the universal
/// `print_start`/`print_event` trace above (which runs for every backend)
/// to give a "just show me the notes, don't open a device or write a file"
/// option.
struct TextRenderer;

impl Renderer for TextRenderer {
    fn render(&mut self, _event: &Event) {}
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
            "--backend" => {
                backend = args
                    .next()
                    .expect("--backend requires a value")
            }
            "--out" => {
                out = Some(
                    args.next()
                        .expect("--out requires a value"),
                )
            }
            "--seed" => {
                let value = args
                    .next()
                    .expect("--seed requires a value");
                let digits = value
                    .strip_prefix("0x")
                    .or_else(|| value.strip_prefix("0X"))
                    .unwrap_or(&value);
                seed = Some(
                    u64::from_str_radix(digits, 16).expect("--seed must be a hexadecimal u64"),
                );
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
                Some(piano_path) => {
                    SoundBackend::with_piano_override(SOUNDFONT_PATH, piano_path, PIANO_CHANNEL)?
                }
                None => SoundBackend::new(SOUNDFONT_PATH)?,
            };
            Ok(Box::new(SynthRenderer::new(backend, config)))
        }
        "text" => Ok(Box::new(TextRenderer)),
        "wav" => {
            let out_path = out
                .unwrap_or_else(|| "output.wav".to_string())
                .into();
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
                Some(piano_path) => WavRenderer::with_piano_override(
                    SOUNDFONT_PATH,
                    piano_path,
                    PIANO_CHANNEL,
                    config,
                )?,
                None => WavRenderer::new(SOUNDFONT_PATH, config)?,
            };
            Ok(Box::new(renderer))
        }
        "midi" => {
            let out_path = out
                .unwrap_or_else(|| "output.mid".to_string())
                .into();
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
    let seed = args
        .seed
        .unwrap_or_else(|| rand::rng().random());

    let mut renderer = build_renderer(&args.backend, args.out)?;

    let start = Triad::new(0, Mode::Major);
    let (mut pipeline, choice) = random_pipeline::build_pipeline(seed, start);

    eprintln!("seed: {seed:#x} (pass --seed {seed:#x} to reproduce this run)");
    eprintln!("{choice}");

    print_start(start);
    renderer.start(start);

    let mut last_duration = 0.0;
    let mut prev_triad = start;
    for event in pipeline
        .by_ref()
        .take(STEPS)
    {
        if renderer.wants_realtime_pacing() {
            sleep(Duration::from_secs_f64(event.duration * UNIT_SECONDS));
        }
        print_event(prev_triad, &event);
        if !event.is_fill {
            prev_triad = event.triad;
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
