//! Randomly assembles a WalkStrategy/MelodyStrategy/RhythmStrategy trio,
//! all three driven from one seed, plus a human-readable `Choice`
//! describing exactly what was picked -- so "the whole thing is seeded"
//! covers every axis of the pipeline, not just the harmonic walk.
//!
//! `AnyWalk`/`AnyMelody`/`AnyRhythm` exist because `Pipeline<W, M, R>` is
//! generic over concrete types chosen at compile time, but which concrete
//! strategy to run is a runtime decision here -- these enums give each
//! trait exactly one concrete type to be generic over, dispatching to
//! whichever variant was picked via a plain `match` (no `Box<dyn _>`
//! needed, since the full set of choices is fixed and small).

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use tonnetz_core::{
    CycleConfinedWalk, Euclidean, FixedPulse, FreeWalk, HamiltonianCycleWalk, MelodyStrategy, MovingVoice,
    PitchClass, Pipeline, RhythmStrategy, RollingWindowScale, System, SystemFixedScale, TightScale, Triad, Utt,
    WalkStrategy, WindowedDurations, WindowedTabuWalk,
};

pub enum AnyWalk {
    Free(FreeWalk<StdRng>),
    Tabu(WindowedTabuWalk<StdRng>),
    Hamiltonian(HamiltonianCycleWalk),
    Confined(CycleConfinedWalk<StdRng>),
}

impl WalkStrategy for AnyWalk {
    fn next(&mut self, current: Triad, history: &[Triad]) -> (Triad, Utt) {
        match self {
            AnyWalk::Free(w) => w.next(current, history),
            AnyWalk::Tabu(w) => w.next(current, history),
            AnyWalk::Hamiltonian(w) => w.next(current, history),
            AnyWalk::Confined(w) => w.next(current, history),
        }
    }

    fn current_system(&self) -> Option<System> {
        match self {
            AnyWalk::Confined(w) => w.current_system(),
            _ => None,
        }
    }
}

pub enum AnyMelody {
    Moving(MovingVoice),
    Tight(TightScale),
    Rolling(RollingWindowScale),
    SystemFixed(SystemFixedScale),
}

impl MelodyStrategy for AnyMelody {
    fn notes(&mut self, prev: Triad, next: Triad, op: Utt, history: &[Triad]) -> Vec<PitchClass> {
        match self {
            AnyMelody::Moving(m) => m.notes(prev, next, op, history),
            AnyMelody::Tight(m) => m.notes(prev, next, op, history),
            AnyMelody::Rolling(m) => m.notes(prev, next, op, history),
            AnyMelody::SystemFixed(m) => m.notes(prev, next, op, history),
        }
    }

    fn set_system(&mut self, system: System) {
        if let AnyMelody::SystemFixed(m) = self {
            m.set_system(system);
        }
    }
}

pub enum AnyRhythm {
    Fixed(FixedPulse),
    Euclid(Euclidean),
    Windowed(Box<WindowedDurations<StdRng>>),
}

impl RhythmStrategy for AnyRhythm {
    fn timing(&mut self, event_index: usize) -> (f64, f64) {
        match self {
            AnyRhythm::Fixed(r) => r.timing(event_index),
            AnyRhythm::Euclid(r) => r.timing(event_index),
            AnyRhythm::Windowed(r) => r.timing(event_index),
        }
    }
}

/// What `random_strategies` picked, in enough detail to fully reconstruct
/// the run by eye -- printed alongside the seed so a run is documented
/// even without re-running it.
pub struct Choice {
    pub walk: String,
    pub melody: String,
    pub rhythm: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "  walk:   {}", self.walk)?;
        writeln!(f, "  melody: {}", self.melody)?;
        write!(f, "  rhythm: {}", self.rhythm)
    }
}

/// A fresh, independently-seeded `StdRng` derived deterministically from
/// `rng` -- lets each strategy that needs its own `Rng` (rather than just
/// drawing a parameter) get one without correlating its output with
/// whatever else draws from `rng` afterward.
fn sub_rng(rng: &mut StdRng) -> StdRng {
    StdRng::seed_from_u64(rng.random())
}

/// Builds a fully random -- but fully reproducible -- walk/melody/rhythm
/// trio from a single seed.
///
/// Draw order from the seed (fixed; changing it changes what a
/// previously-recorded seed reproduces): walk kind, then walk params, then
/// melody kind (from a pool that includes `SystemFixedScale` only when the
/// walk just picked is `CycleConfinedWalk` -- it's only meaningful paired
/// with that walk, see `tonnetz_core::melody`'s doc comment), then melody
/// params, then rhythm kind, then rhythm params.
pub fn random_strategies(seed: u64) -> (AnyWalk, AnyMelody, AnyRhythm, Choice) {
    let mut rng = StdRng::seed_from_u64(seed);

    let (walk, walk_desc) = match rng.random_range(0..4) {
        0 => (AnyWalk::Free(FreeWalk::with_rng(sub_rng(&mut rng))), "FreeWalk".to_string()),
        1 => {
            let window = rng.random_range(2..=5);
            (
                AnyWalk::Tabu(WindowedTabuWalk::with_rng(window, sub_rng(&mut rng))),
                format!("WindowedTabuWalk {{ window: {window} }}"),
            )
        }
        2 => (AnyWalk::Hamiltonian(HamiltonianCycleWalk::new()), "HamiltonianCycleWalk".to_string()),
        _ => {
            let system = if rng.random_bool(0.5) { System::Hexatonic } else { System::Octatonic };
            let escape_probability = rng.random_range(0.05..=0.35);
            (
                AnyWalk::Confined(CycleConfinedWalk::with_rng(system, escape_probability, sub_rng(&mut rng))),
                format!("CycleConfinedWalk {{ system: {system:?}, escape_probability: {escape_probability:.2} }}"),
            )
        }
    };

    let melody_pool = if matches!(walk, AnyWalk::Confined(_)) { 4 } else { 3 };
    let (melody, melody_desc) = match rng.random_range(0..melody_pool) {
        0 => (AnyMelody::Moving(MovingVoice), "MovingVoice".to_string()),
        1 => (AnyMelody::Tight(TightScale), "TightScale".to_string()),
        2 => {
            let window = rng.random_range(2..=5);
            (AnyMelody::Rolling(RollingWindowScale { window }), format!("RollingWindowScale {{ window: {window} }}"))
        }
        _ => (AnyMelody::SystemFixed(SystemFixedScale::new()), "SystemFixedScale".to_string()),
    };

    let (rhythm, rhythm_desc) = match rng.random_range(0..3) {
        0 => {
            let beat = rng.random_range(0.5..=1.5_f64);
            (AnyRhythm::Fixed(FixedPulse { beat }), format!("FixedPulse {{ beat: {beat:.2} }}"))
        }
        1 => {
            let steps = rng.random_range(5..=12);
            let pulses = rng.random_range(1..=steps);
            (
                AnyRhythm::Euclid(Euclidean::new(pulses, steps)),
                format!("Euclidean {{ pulses: {pulses}, steps: {steps} }}"),
            )
        }
        _ => {
            let window = rng.random_range(2..=4);
            let palette = vec![1.0, 1.5, 2.0, 3.0];
            let desc = format!("WindowedDurations {{ window: {window}, palette: {palette:?} }}");
            let rhythm = Box::new(WindowedDurations::with_rng(window, palette, sub_rng(&mut rng)));
            (AnyRhythm::Windowed(rhythm), desc)
        }
    };

    (walk, melody, rhythm, Choice { walk: walk_desc, melody: melody_desc, rhythm: rhythm_desc })
}

/// `random_strategies` plus the `Pipeline` built from its result.
pub fn build_pipeline(seed: u64, start: Triad) -> (Pipeline<AnyWalk, AnyMelody, AnyRhythm>, Choice) {
    let (walk, melody, rhythm, choice) = random_strategies(seed);
    (Pipeline::new(walk, melody, rhythm, start), choice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonnetz_core::{Event, Mode};

    #[test]
    fn same_seed_produces_the_same_progression() {
        let start = Triad::new(0, Mode::Major);
        let (pipeline_a, choice_a) = build_pipeline(42, start);
        let (pipeline_b, choice_b) = build_pipeline(42, start);
        let a: Vec<Event> = pipeline_a.take(24).collect();
        let b: Vec<Event> = pipeline_b.take(24).collect();
        assert_eq!(a, b);
        assert_eq!(choice_a.to_string(), choice_b.to_string());
    }

    #[test]
    fn different_seeds_diverge() {
        let start = Triad::new(0, Mode::Major);
        let (pipeline_a, _) = build_pipeline(1, start);
        let (pipeline_b, _) = build_pipeline(2, start);
        let a: Vec<Event> = pipeline_a.take(24).collect();
        let b: Vec<Event> = pipeline_b.take(24).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn system_fixed_scale_is_only_ever_paired_with_cycle_confined_walk() {
        for seed in 0..500 {
            let (walk, melody, _, _) = random_strategies(seed);
            if matches!(melody, AnyMelody::SystemFixed(_)) {
                assert!(matches!(walk, AnyWalk::Confined(_)), "seed {seed} paired SystemFixedScale with a non-confined walk");
            }
        }
    }
}
