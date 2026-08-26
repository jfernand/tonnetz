use std::error::Error;
use std::thread::sleep;
use std::time::Duration;

use tonnetz_core::{
    Euclidean, FreeWalk, MelodyStrategy, Mode, MovingVoice, RhythmStrategy, Triad, Utt,
    WalkStrategy,
};
use tonnetz_sound::{SoundBackend, nearest_midi_note};

const CHORD_CHANNEL: i32 = 0;
const MELODY_CHANNEL: i32 = 1;
const ROOT_MIDI: i32 = 60; // middle C
const MELODY_START_MIDI: i32 = 72; // an octave above, so it sits above the chord
const CHORD_VELOCITY: i32 = 90;
const MELODY_VELOCITY: i32 = 110;
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
    backend.set_program(CHORD_CHANNEL, 0); // Acoustic Grand Piano
    backend.set_program(MELODY_CHANNEL, 73); // Flute

    let mut walk = FreeWalk::new();
    let mut melody = MovingVoice;
    let mut rhythm = Euclidean::new(3, 8);

    let mut triad = Triad::new(0, Mode::Major);
    let mut history = vec![triad];
    let mut melody_midi = nearest_midi_note(MELODY_START_MIDI, triad.root);

    print!("{triad}");
    backend.play_triad(triad, ROOT_MIDI, CHORD_VELOCITY);
    backend.note_on(MELODY_CHANNEL, melody_midi, MELODY_VELOCITY);

    for i in 0..STEPS {
        let (_, duration) = rhythm.timing(i);
        sleep(Duration::from_secs_f64(duration * UNIT_SECONDS));

        backend.stop_triad(triad, ROOT_MIDI);
        let prev = triad;
        let (next, op) = walk.next(prev, &history);
        let notes = melody.notes(prev, next, op, &history);
        history.push(next);
        triad = next;

        backend.note_off(MELODY_CHANNEL, melody_midi);
        if let Some(&pc) = notes.first() {
            melody_midi = nearest_midi_note(melody_midi, pc);
            backend.note_on(MELODY_CHANNEL, melody_midi, MELODY_VELOCITY);
        }

        print!(" -{}-> {triad}", op_name(op));
        backend.play_triad(triad, ROOT_MIDI, CHORD_VELOCITY);
    }

    let (_, last_duration) = rhythm.timing(STEPS);
    sleep(Duration::from_secs_f64(last_duration * UNIT_SECONDS));
    backend.stop_triad(triad, ROOT_MIDI);
    backend.note_off(MELODY_CHANNEL, melody_midi);
    println!();

    Ok(())
}
