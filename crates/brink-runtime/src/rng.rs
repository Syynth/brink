//! Pluggable PRNG for story randomization.
//!
//! Two built-in implementations:
//! - [`FastRng`]: xorshift32, the default for production use.
//! - [`DotNetRng`]: port of .NET `System.Random` (Knuth subtractive generator),
//!   used for corpus tests requiring exact match with the reference C# runtime.

/// Trait for story-level random number generation.
///
/// Implementations must be seedable from an `i32` and produce non-negative
/// `i32` values. The runtime constructs a fresh RNG for each random operation
/// using a deterministic seed derived from story state.
pub trait StoryRng {
    /// Create a new RNG from the given seed.
    fn from_seed(seed: i32) -> Self;
    /// Return a non-negative random `i32`.
    fn next_int(&mut self) -> i32;
}

// ── FastRng ─────────────────────────────────────────────────────────────────

/// Xorshift32-based PRNG. Fast, decent distribution, not .NET-compatible.
#[derive(Clone)]
pub struct FastRng {
    state: u32,
}

impl StoryRng for FastRng {
    #[expect(clippy::cast_sign_loss)]
    fn from_seed(seed: i32) -> Self {
        // Avoid zero state (xorshift32 fixpoint).
        let s = seed as u32;
        Self {
            state: if s == 0 { 1 } else { s },
        }
    }

    #[expect(clippy::cast_possible_wrap)]
    fn next_int(&mut self) -> i32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        // Mask off sign bit to guarantee non-negative.
        (x & 0x7FFF_FFFF) as i32
    }
}

// ── DotNetRng ───────────────────────────────────────────────────────────────

const MBIG: i32 = i32::MAX; // 2147483647
const MSEED: i32 = 161_803_398;
const SEED_ARRAY_LEN: usize = 56; // index 0 unused, 1..55 active

/// Port of .NET `System.Random` (Knuth subtractive generator).
///
/// Reproduces the exact sequence of the reference ink C# runtime so that
/// corpus tests can match expected transcripts.
#[derive(Clone)]
pub struct DotNetRng {
    seed_array: [i32; SEED_ARRAY_LEN],
    inext: i32,
    inextp: i32,
}

impl StoryRng for DotNetRng {
    fn from_seed(seed: i32) -> Self {
        let mut seed_array = [0i32; SEED_ARRAY_LEN];

        // .NET constructor logic (mscorlib System.Random)
        let subtraction = if seed == i32::MIN {
            i32::MAX
        } else {
            seed.abs()
        };
        let mut mj = MSEED.wrapping_sub(subtraction);
        seed_array[55] = mj;
        let mut mk: i32 = 1;

        for i in 1..55 {
            // Scatter: map i → index in [1..55] via (21*i) % 55
            let ii = (21_usize.wrapping_mul(i)) % 55;
            seed_array[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(MBIG);
            }
            mj = seed_array[ii];
        }

        for _k in 1..5 {
            for i in 1..56 {
                let idx = 1 + (i + 30) % 55;
                seed_array[i] = seed_array[i].wrapping_sub(seed_array[idx]);
                if seed_array[i] < 0 {
                    seed_array[i] = seed_array[i].wrapping_add(MBIG);
                }
            }
        }

        Self {
            seed_array,
            inext: 0,
            inextp: 21,
        }
    }

    #[expect(clippy::cast_sign_loss)]
    fn next_int(&mut self) -> i32 {
        let mut inext = self.inext + 1;
        if inext >= 56 {
            inext = 1;
        }
        let mut inextp = self.inextp + 1;
        if inextp >= 56 {
            inextp = 1;
        }

        let mut num =
            self.seed_array[inext as usize].wrapping_sub(self.seed_array[inextp as usize]);
        if num == MBIG {
            num -= 1;
        }
        if num < 0 {
            num = num.wrapping_add(MBIG);
        }

        self.seed_array[inext as usize] = num;
        self.inext = inext;
        self.inextp = inextp;

        num
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Validate `DotNetRng` against known .NET System.Random output.
    /// Seed = 0: first 5 values from .NET are:
    ///   1559595546, 1755192844, 1649316166, 1198642031, 442452829
    #[test]
    fn dotnet_rng_seed_0_sequence() {
        let mut rng = DotNetRng::from_seed(0);
        assert_eq!(rng.next_int(), 1_559_595_546);
        assert_eq!(rng.next_int(), 1_755_192_844);
        assert_eq!(rng.next_int(), 1_649_316_166);
        assert_eq!(rng.next_int(), 1_198_642_031);
        assert_eq!(rng.next_int(), 442_452_829);
    }

    /// Validate `DotNetRng` negative seed (`i32::MIN` edge case).
    #[test]
    fn dotnet_rng_negative_seed() {
        let mut rng = DotNetRng::from_seed(-1);
        let v = rng.next_int();
        assert!(
            v >= 0,
            "negative seed should still produce non-negative values"
        );
    }

    /// All values must be non-negative.
    #[test]
    fn dotnet_rng_all_non_negative() {
        let mut rng = DotNetRng::from_seed(42);
        for _ in 0..1000 {
            assert!(rng.next_int() >= 0);
        }
    }

    /// `FastRng` should produce non-negative values.
    #[test]
    fn fast_rng_all_non_negative() {
        let mut rng = FastRng::from_seed(42);
        for _ in 0..1000 {
            assert!(rng.next_int() >= 0);
        }
    }

    /// `FastRng` with seed 0 should not get stuck (zero-state avoidance).
    #[test]
    fn fast_rng_seed_zero_not_stuck() {
        let mut rng = FastRng::from_seed(0);
        let first = rng.next_int();
        let second = rng.next_int();
        assert_ne!(first, 0);
        assert_ne!(first, second);
    }
}
