use tonnetz_core::{Mode, Triad, Utt};

fn main() {
    let mut triad = Triad::new(0, Mode::Major);
    print!("{triad}");
    for op in [Utt::R, Utt::L, Utt::R, Utt::L] {
        triad = op.apply(triad);
        print!(" -> {triad}");
    }
    println!();
}
