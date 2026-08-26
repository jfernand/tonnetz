// Smoke test for the tonnetz-sound backend: loads the bundled GeneralUser
// GS SoundFont and plays a C major triad for a few seconds.
// Run with: cargo run --example synth_smoke_test

use tonnetz_core::{Mode, Triad};
use tonnetz_sound::SoundBackend;

fn main() {
    let backend =
        SoundBackend::new("assets/soundfonts/GeneralUser-GS.sf2").expect("start sound backend");
    backend.set_program(0, 0); // Acoustic Grand Piano

    backend.play_triad(Triad::new(0, Mode::Major), 60, 100);
    std::thread::sleep(std::time::Duration::from_secs(5));
}
