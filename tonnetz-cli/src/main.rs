use std::error::Error;
use std::thread::sleep;
use std::time::Duration;

use tonnetz_core::{FreeWalk, Mode, Triad, Utt, WalkStrategy};
use tonnetz_sound::SoundBackend;

const ROOT_MIDI: i32 = 60; // middle C
const VELOCITY: i32 = 100;
const STEP: Duration = Duration::from_millis(900);

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
    backend.set_program(0, 0); // Acoustic Grand Piano

    let mut walk = FreeWalk::new();
    let mut triad = Triad::new(0, Mode::Major);

    print!("{triad}");
    backend.play_triad(triad, ROOT_MIDI, VELOCITY);
    sleep(STEP);

    for _ in 0..7 {
        backend.stop_triad(triad, ROOT_MIDI);
        let (next, op) = walk.next(triad, &[]);
        print!(" -{}-> {next}", op_name(op));
        triad = next;
        backend.play_triad(triad, ROOT_MIDI, VELOCITY);
        sleep(STEP);
    }
    backend.stop_triad(triad, ROOT_MIDI);
    println!();

    Ok(())
}
