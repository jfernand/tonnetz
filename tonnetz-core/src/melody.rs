//! Melody strategies (CONCEPT.md section 5).

use std::collections::HashSet;

use crate::{PitchClass, System, Triad, Utt};

/// Derives melody notes from a walk step. `history` holds every triad
/// visited before `next` (i.e. `prev == *history.last().unwrap()` after
/// the first step), mirroring `WalkStrategy::next`'s history parameter --
/// needed for window-based strategies like `RollingWindowScale`, which
/// CONCEPT.md's original two-triad-only signature couldn't support.
pub trait MelodyStrategy {
    fn notes(&mut self, prev: Triad, next: Triad, op: Utt, history: &[Triad]) -> Vec<PitchClass>;

    /// Called by `Pipeline` right after each walk step, whenever the
    /// active `WalkStrategy::current_system` reports one (i.e. only when
    /// driven by `CycleConfinedWalk`). Default no-op; only
    /// `SystemFixedScale` needs it, and every other strategy is free to
    /// ignore it. Kept off `notes`'s own signature so strategies that
    /// don't care about the walk's system never see it.
    fn set_system(&mut self, _system: System) {}
}

/// The single voice that every P/L/R step moves by step, tracked across
/// the walk: a complete, correctly-voiced melodic line with no extra
/// logic needed. The cheapest strategy, and the suggested default.
pub struct MovingVoice;

impl MelodyStrategy for MovingVoice {
    fn notes(&mut self, prev: Triad, next: Triad, _op: Utt, _history: &[Triad]) -> Vec<PitchClass> {
        let prev_pcs: HashSet<PitchClass> = prev.pitch_classes().into_iter().collect();
        next.pitch_classes()
            .into_iter()
            .filter(|pc| !prev_pcs.contains(pc))
            .collect()
    }
}

/// The current triad's 3 notes only: an arpeggio, harmonically locked to
/// the chord just arrived at.
pub struct TightScale;

impl MelodyStrategy for TightScale {
    fn notes(&mut self, _prev: Triad, next: Triad, _op: Utt, _history: &[Triad]) -> Vec<PitchClass> {
        next.pitch_classes().to_vec()
    }
}

/// Union of pitch classes across the last `window` triads (including the
/// one just arrived at). `window == 1` reduces to `TightScale`.
pub struct RollingWindowScale {
    pub window: usize,
}

impl MelodyStrategy for RollingWindowScale {
    fn notes(&mut self, _prev: Triad, next: Triad, _op: Utt, history: &[Triad]) -> Vec<PitchClass> {
        let start = history.len().saturating_sub(self.window.saturating_sub(1));
        let mut pcs: HashSet<PitchClass> = history[start..]
            .iter()
            .flat_map(|t| t.pitch_classes())
            .collect();
        pcs.extend(next.pitch_classes());
        let mut notes: Vec<PitchClass> = pcs.into_iter().collect();
        notes.sort_by_key(|pc| pc.0);
        notes
    }
}

/// The current hexatonic/octatonic collection (CONCEPT.md section 3) while
/// a `CycleConfinedWalk` holds the walk on one system. Only meaningful
/// paired with that walk strategy -- `Pipeline` feeds it the active system
/// via `set_system` each step, since a single `(prev, next, op)` step can't
/// disambiguate it (`op == P` alone appears in both systems).
///
/// Before the first `set_system` call (i.e. not actually paired with
/// `CycleConfinedWalk`), falls back to the arrived-at triad's own 3 notes,
/// matching `TightScale` rather than panicking.
#[derive(Default)]
pub struct SystemFixedScale {
    system: Option<System>,
}

impl SystemFixedScale {
    pub fn new() -> Self {
        SystemFixedScale { system: None }
    }
}

impl MelodyStrategy for SystemFixedScale {
    fn notes(&mut self, _prev: Triad, next: Triad, _op: Utt, _history: &[Triad]) -> Vec<PitchClass> {
        match self.system {
            Some(system) => system.pitch_classes(next),
            None => next.pitch_classes().to_vec(),
        }
    }

    fn set_system(&mut self, system: System) {
        self.system = Some(system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mode;

    #[test]
    fn moving_voice_returns_exactly_the_note_that_changed() {
        let c_major = Triad::new(0, Mode::Major);
        let a_minor = Utt::R.apply(c_major);
        let mut strategy = MovingVoice;
        let notes = strategy.notes(c_major, a_minor, Utt::R, &[]);
        // C major = {C, E, G}; A minor = {A, E, C}; only A is new.
        assert_eq!(notes, vec![PitchClass::new(9)]);
    }

    #[test]
    fn tight_scale_returns_the_new_triad() {
        let c_major = Triad::new(0, Mode::Major);
        let mut strategy = TightScale;
        let mut notes = strategy.notes(c_major, c_major, Utt::IDENTITY, &[]);
        notes.sort_by_key(|pc| pc.0);
        assert_eq!(notes, vec![PitchClass::new(0), PitchClass::new(4), PitchClass::new(7)]);
    }

    #[test]
    fn rolling_window_scale_of_1_matches_tight_scale() {
        let c_major = Triad::new(0, Mode::Major);
        let mut strategy = RollingWindowScale { window: 1 };
        let mut notes = strategy.notes(c_major, c_major, Utt::IDENTITY, &[]);
        notes.sort_by_key(|pc| pc.0);
        assert_eq!(notes, vec![PitchClass::new(0), PitchClass::new(4), PitchClass::new(7)]);
    }

    #[test]
    fn system_fixed_scale_falls_back_to_tight_scale_before_set_system() {
        let c_major = Triad::new(0, Mode::Major);
        let mut strategy = SystemFixedScale::new();
        let mut notes = strategy.notes(c_major, c_major, Utt::IDENTITY, &[]);
        notes.sort_by_key(|pc| pc.0);
        assert_eq!(notes, vec![PitchClass::new(0), PitchClass::new(4), PitchClass::new(7)]);
    }

    #[test]
    fn system_fixed_scale_uses_the_pushed_system_once_set() {
        let c_major = Triad::new(0, Mode::Major);
        let mut strategy = SystemFixedScale::new();
        strategy.set_system(crate::System::Hexatonic);
        let mut notes = strategy.notes(c_major, c_major, Utt::IDENTITY, &[]);
        notes.sort_by_key(|pc| pc.0);
        let expected: Vec<PitchClass> = [0, 3, 4, 7, 8, 11].map(PitchClass::new).to_vec();
        assert_eq!(notes, expected);
    }

    #[test]
    fn rolling_window_scale_unions_across_the_window() {
        // C major -R-> A minor -L-> F major: window 2 at the F-major step
        // should union F major's and A minor's pitch classes only.
        let c_major = Triad::new(0, Mode::Major);
        let a_minor = Utt::R.apply(c_major);
        let f_major = Utt::L.apply(a_minor);
        let mut strategy = RollingWindowScale { window: 2 };
        let mut notes = strategy.notes(a_minor, f_major, Utt::L, &[c_major, a_minor]);
        notes.sort_by_key(|pc| pc.0);
        // A minor = {A, C, E}, F major = {F, A, C}. Union = {A, C, E, F}, C major's G is excluded.
        assert_eq!(
            notes,
            vec![PitchClass::new(0), PitchClass::new(4), PitchClass::new(5), PitchClass::new(9)]
        );
    }
}
