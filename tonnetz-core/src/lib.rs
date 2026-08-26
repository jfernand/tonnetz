//! Core neo-Riemannian harmonic types: pitch classes, triads, and the
//! transformations that move between them.
//!
//! `Utt` implements Hook's uniform triadic transformations (Hook, "Uniform
//! Triadic Transformations," Journal of Music Theory 46, 2002), of which
//! the classic P/L/R neo-Riemannian operations (Cohn, "Introduction to
//! Neo-Riemannian Theory," Journal of Music Theory 42, 1998) are three
//! named instances. See CONCEPT.md at the repo root for the full design
//! rationale.

mod melody;
mod rhythm;
mod walk;
pub use melody::{MelodyStrategy, MovingVoice, RollingWindowScale, TightScale};
pub use rhythm::{Euclidean, FixedPulse, RhythmStrategy, WindowedDurations, bjorklund};
pub use walk::{CycleConfinedWalk, FreeWalk, HamiltonianCycleWalk, System, WalkStrategy, WindowedTabuWalk, PLR};

/// A pitch class, 0-11, where 0 = C. Arithmetic wraps mod 12.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PitchClass(pub u8);

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

impl PitchClass {
    pub fn new(value: i32) -> Self {
        PitchClass(value.rem_euclid(12) as u8)
    }

    pub fn name(self) -> &'static str {
        NOTE_NAMES[self.0 as usize]
    }
}

impl std::ops::Add<i32> for PitchClass {
    type Output = PitchClass;
    fn add(self, rhs: i32) -> PitchClass {
        PitchClass::new(self.0 as i32 + rhs)
    }
}

impl std::fmt::Display for PitchClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mode {
    Major,
    Minor,
}

impl Mode {
    pub fn flip(self) -> Mode {
        match self {
            Mode::Major => Mode::Minor,
            Mode::Minor => Mode::Major,
        }
    }
}

/// A major or minor triad, identified by root and mode alone (24 total).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Triad {
    pub root: PitchClass,
    pub mode: Mode,
}

impl Triad {
    pub fn new(root: i32, mode: Mode) -> Self {
        Triad {
            root: PitchClass::new(root),
            mode,
        }
    }

    pub fn apply(self, utt: Utt) -> Triad {
        utt.apply(self)
    }

    /// The triad's three pitch classes: root, third, fifth.
    pub fn pitch_classes(self) -> [PitchClass; 3] {
        let third = match self.mode {
            Mode::Major => 4,
            Mode::Minor => 3,
        };
        [self.root, self.root + third, self.root + 7]
    }
}

impl std::fmt::Display for Triad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let suffix = match self.mode {
            Mode::Major => "",
            Mode::Minor => "m",
        };
        write!(f, "{}{}", self.root, suffix)
    }
}

/// Whether a `Utt` preserves (`Plus`) or flips (`Minus`) triad mode.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sign {
    Plus,
    Minus,
}

impl Sign {
    fn xor(self, other: Sign) -> Sign {
        if self == other { Sign::Plus } else { Sign::Minus }
    }
}

/// A uniform triadic transformation: `<sign, m, n>` sends `(root, Major)`
/// to `(root + m, mode')` and `(root, Minor)` to `(root + n, mode')`, where
/// `mode'` flips iff `sign` is `Minus`. P, L, and R below are the three
/// UTTs that generate the dihedral-24 neo-Riemannian group; the full UTT
/// group has order 288 (2 x 12 x 12) and is a strict superset, so new
/// non-PLR transformations can be added as more `Utt` values without
/// changing this type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Utt {
    pub sign: Sign,
    pub m: i32,
    pub n: i32,
}

impl Utt {
    pub const IDENTITY: Utt = Utt {
        sign: Sign::Plus,
        m: 0,
        n: 0,
    };

    /// Parallel: (root, Major) <-> (root, Minor). Fixes the perfect-fifth edge.
    pub const P: Utt = Utt {
        sign: Sign::Minus,
        m: 0,
        n: 0,
    };

    /// Leading-tone exchange: (root, Major) -> (root+4, Minor); fixes the minor-third edge.
    pub const L: Utt = Utt {
        sign: Sign::Minus,
        m: 4,
        n: 8,
    };

    /// Relative: (root, Major) -> (root+9, Minor); fixes the major-third edge.
    pub const R: Utt = Utt {
        sign: Sign::Minus,
        m: 9,
        n: 3,
    };

    /// Mode-preserving transposition by `k` semitones.
    pub fn transpose(k: i32) -> Utt {
        Utt {
            sign: Sign::Plus,
            m: k,
            n: k,
        }
    }

    pub fn apply(self, triad: Triad) -> Triad {
        let (offset, flips) = match triad.mode {
            Mode::Major => (self.m, self.sign == Sign::Minus),
            Mode::Minor => (self.n, self.sign == Sign::Minus),
        };
        let mode = if flips { triad.mode.flip() } else { triad.mode };
        Triad {
            root: triad.root + offset,
            mode,
        }
    }

    /// The UTT equivalent to applying `other` then `self`:
    /// `self.compose(other).apply(t) == self.apply(other.apply(t))`.
    pub fn compose(self, other: Utt) -> Utt {
        let sign = self.sign.xor(other.sign);
        // When `other` flips mode, the triad's mode at the point `self` is
        // applied has already been swapped, so `self`'s major/minor offsets
        // must be swapped too before combining with `other`'s.
        let (self_m, self_n) = match other.sign {
            Sign::Minus => (self.n, self.m),
            Sign::Plus => (self.m, self.n),
        };
        Utt {
            sign,
            m: (other.m + self_m).rem_euclid(12),
            n: (other.n + self_n).rem_euclid(12),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_triads() -> Vec<Triad> {
        (0..12)
            .flat_map(|r| [Triad::new(r, Mode::Major), Triad::new(r, Mode::Minor)])
            .collect()
    }

    // CONCEPT.md's root-arithmetic table, checked row by row before trusting it.
    #[test]
    fn plr_matches_cohn_table() {
        for r in 0..12 {
            assert_eq!(Utt::P.apply(Triad::new(r, Mode::Major)), Triad::new(r, Mode::Minor));
            assert_eq!(Utt::P.apply(Triad::new(r, Mode::Minor)), Triad::new(r, Mode::Major));
            assert_eq!(Utt::L.apply(Triad::new(r, Mode::Major)), Triad::new(r + 4, Mode::Minor));
            assert_eq!(Utt::L.apply(Triad::new(r, Mode::Minor)), Triad::new(r - 4, Mode::Major));
            assert_eq!(Utt::R.apply(Triad::new(r, Mode::Major)), Triad::new(r + 9, Mode::Minor));
            assert_eq!(Utt::R.apply(Triad::new(r, Mode::Minor)), Triad::new(r + 3, Mode::Major));
        }
    }

    // Cohn's own worked examples (1998, p. 172).
    #[test]
    fn verified_examples() {
        let c_minor = Triad::new(0, Mode::Minor);
        assert_eq!(Utt::L.apply(c_minor), Triad::new(8, Mode::Major)); // Ab major
        assert_eq!(Utt::R.apply(c_minor), Triad::new(3, Mode::Major)); // Eb major
        assert_eq!(Utt::P.apply(Triad::new(0, Mode::Major)), c_minor);
    }

    #[test]
    fn p_l_r_are_involutions() {
        for t in all_triads() {
            assert_eq!(Utt::P.apply(Utt::P.apply(t)), t);
            assert_eq!(Utt::L.apply(Utt::L.apply(t)), t);
            assert_eq!(Utt::R.apply(Utt::R.apply(t)), t);
        }
    }

    fn cycle_length(start: Triad, ops: [Utt; 2]) -> usize {
        let mut t = start;
        let mut seen = Vec::new();
        let mut step = 0;
        loop {
            t = ops[step % 2].apply(t);
            step += 1;
            if t == start {
                return step;
            }
            assert!(!seen.contains(&t), "cycle repeated before returning home");
            seen.push(t);
        }
    }

    #[test]
    fn hexatonic_cycle_has_length_6() {
        assert_eq!(cycle_length(Triad::new(0, Mode::Major), [Utt::P, Utt::L]), 6);
    }

    #[test]
    fn octatonic_cycle_has_length_8() {
        assert_eq!(cycle_length(Triad::new(0, Mode::Major), [Utt::P, Utt::R]), 8);
    }

    #[test]
    fn hamiltonian_cycle_has_length_24() {
        assert_eq!(cycle_length(Triad::new(0, Mode::Major), [Utt::R, Utt::L]), 24);
    }

    // Alternating R, L from C major, as attested in Beethoven's 9th Symphony,
    // 2nd movement, mm. 143-176 (cited independently across multiple papers
    // in docs/). Beethoven stops after 19 steps; this fixture runs the full
    // 24-step Hamiltonian cycle back home.
    #[test]
    fn beethoven_ninth_hamiltonian_cycle() {
        let expected = [
            (0, Mode::Major),  // C
            (9, Mode::Minor),  // a
            (5, Mode::Major),  // F
            (2, Mode::Minor),  // d
            (10, Mode::Major), // Bb
            (7, Mode::Minor),  // g
            (3, Mode::Major),  // Eb
            (0, Mode::Minor),  // c
            (8, Mode::Major),  // Ab
            (5, Mode::Minor),  // f
            (1, Mode::Major),  // Db
            (10, Mode::Minor), // bb
            (6, Mode::Major),  // Gb
            (3, Mode::Minor),  // eb
            (11, Mode::Major), // B
            (8, Mode::Minor),  // g#
            (4, Mode::Major),  // E
            (1, Mode::Minor),  // c#
            (9, Mode::Major),  // A
            (6, Mode::Minor),  // f#
            (2, Mode::Major),  // D
            (11, Mode::Minor), // b
            (7, Mode::Major),  // G
            (4, Mode::Minor),  // e
        ];

        let mut t = Triad::new(0, Mode::Major);
        for (i, &(root, mode)) in expected.iter().enumerate() {
            assert_eq!(t, Triad::new(root, mode), "step {i}");
            t = if i % 2 == 0 { Utt::R.apply(t) } else { Utt::L.apply(t) };
        }
        assert_eq!(t, Triad::new(0, Mode::Major)); // back home after 24 steps
    }

    // Riemann's dualism: the non-PLR dominant transposition D = T7 equals
    // "apply L then R" on major triads but "apply R then L" on minor triads
    // (Crans, Fiore & Satyendra 2008, footnote 16) -- the two modes compose
    // in opposite order.
    #[test]
    fn dominant_is_l_then_r_on_major_and_r_then_l_on_minor() {
        let major = Triad::new(0, Mode::Major);
        let minor = Triad::new(0, Mode::Minor);
        let dominant_major = Triad::new(7, Mode::Major);
        let dominant_minor = Triad::new(7, Mode::Minor);
        assert_eq!(Utt::R.apply(Utt::L.apply(major)), dominant_major);
        assert_eq!(Utt::L.apply(Utt::R.apply(minor)), dominant_minor);
    }

    #[test]
    fn compose_matches_sequential_apply() {
        let ops = [Utt::IDENTITY, Utt::P, Utt::L, Utt::R, Utt::transpose(5)];
        for &a in &ops {
            for &b in &ops {
                let composed = a.compose(b);
                for t in all_triads() {
                    assert_eq!(composed.apply(t), a.apply(b.apply(t)), "{a:?} . {b:?} on {t:?}");
                }
            }
        }
    }

    #[test]
    fn p_l_r_commute_with_transposition() {
        let t7 = Utt::transpose(7);
        for op in [Utt::P, Utt::L, Utt::R] {
            for t in all_triads() {
                assert_eq!(op.apply(t7.apply(t)), t7.apply(op.apply(t)));
            }
        }
    }
}
