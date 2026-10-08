use crate::query::Scorer;
use crate::Score;

/// The `ScoreCombiner` trait defines how to compute
/// an overall score given a list of scores.
pub trait ScoreCombiner: Default + Clone + Send + Copy + 'static {
    /// Whether Block-WAND may drive this combiner.
    ///
    /// Block-WAND prunes blocks by comparing the *sum* of the per-term block
    /// maxima against the threshold, and scores the surviving documents the
    /// same way. That is only the right answer for a combiner that sums. A
    /// combiner aggregating any other way (dis_max takes the maximum) has to
    /// see every matching term, so it must not be pruned this way.
    const SUPPORTS_BLOCK_WAND: bool = false;

    /// Whether this combiner sums child f32 scores in f64 and rounds once.
    /// Optimized summing scorers require this convention. Other combiners
    /// retain their own arithmetic through the generic scorer.
    const SUMS_IN_F64: bool = false;

    /// Whether `update` reads per-document scores. Bulk bitset fill for
    /// COUNT is valid when this is false.
    const NEEDS_PER_DOC_SCORES: bool = true;

    /// Aggregates the score combiner with the given scorer.
    ///
    /// The `ScoreCombiner` may decide to call `.scorer.score()`
    /// or not.
    fn update<TScorer: Scorer>(&mut self, scorer: &mut TScorer);

    /// Clears the score combiner state back to its initial state.
    fn clear(&mut self);

    /// Returns the aggregate score.
    fn score(&self) -> Score;
}

/// Just ignores scores. The `DoNothingCombiner` does not
/// even call the scorers `.score()` function.
///
/// It is useful to optimize the case when scoring is disabled.
#[derive(Default, Clone, Copy)] //< these should not be too much work :)
pub struct DoNothingCombiner;

impl ScoreCombiner for DoNothingCombiner {
    const NEEDS_PER_DOC_SCORES: bool = false;

    fn update<TScorer: Scorer>(&mut self, _scorer: &mut TScorer) {}

    fn clear(&mut self) {}

    #[inline]
    fn score(&self) -> Score {
        1.0
    }
}

/// Sums the score of different scorers.
///
/// Child scores accumulate in f64 and convert to f32 once when read. This
/// avoids intermediate f32 rounding changing ordinary ties with visit order.
#[derive(Default, Clone, Copy)]
pub struct SumCombiner {
    score: f64,
}

impl ScoreCombiner for SumCombiner {
    const SUPPORTS_BLOCK_WAND: bool = true;
    const SUMS_IN_F64: bool = true;

    fn update<TScorer: Scorer>(&mut self, scorer: &mut TScorer) {
        self.score += f64::from(scorer.score());
    }

    fn clear(&mut self) {
        self.score = 0.0;
    }

    #[inline]
    fn score(&self) -> Score {
        self.score as Score
    }
}

/// Bounds differently ordered f64 sums of nonnegative score contributions.
/// Document scores never use this error allowance.
pub(crate) struct ScoreSumUpperBound {
    factor: f64,
}

impl ScoreSumUpperBound {
    pub(crate) fn new(num_terms: usize) -> Self {
        if num_terms <= 2 {
            // Two-operand addition is commutative, so there is no order error.
            return Self { factor: 1.0 };
        }
        // For k additions, Higham's gamma_k bounds recursive summation's
        // relative error by k*u/(1-k*u), with unit roundoff u=2^-53.
        // If one order yields S, another is at most S*(1+gamma)/(1-gamma).
        // Round every bound-construction operation outward. Use the number
        // of original leaves even when prefixes/suffixes are grouped.
        let k = ((num_terms - 1) as f64).next_up();
        let rho = (k * (f64::EPSILON / 2.0)).next_up();
        let gamma = (rho / (1.0 - rho).next_down()).next_up();
        let factor = if rho >= 1.0 || gamma >= 1.0 {
            f64::INFINITY
        } else {
            ((1.0 + gamma).next_up() / (1.0 - gamma).next_down()).next_up()
        };
        Self { factor }
    }

    #[inline]
    pub(crate) fn score(&self, sum: f64) -> Score {
        if !sum.is_finite() || sum < 0.0 {
            return Score::INFINITY;
        }
        if sum == 0.0 || self.factor == 1.0 {
            return sum as Score;
        }
        // Float conversion is monotone: bound the double sum first, then
        // compare the rounded bound to the rounded document threshold.
        (sum * self.factor).next_up() as Score
    }
}

/// The same rounded bound for an admitted 3..=32-term nonnegative score sum.
/// Keep the generic helper for signed, exceptional, and unrestricted callers.
pub(crate) struct SmallScoreSumUpperBound {
    factor: f64,
}

impl SmallScoreSumUpperBound {
    pub(crate) fn new(num_terms: usize) -> Option<Self> {
        if !(3..=32).contains(&num_terms) {
            return None;
        }
        let factor = ScoreSumUpperBound::new(num_terms).factor;
        debug_assert!(factor.is_finite() && factor > 1.0 && factor < 2.0);
        Some(Self { factor })
    }

    /// `sum` must be finite, nonnegative, and at most 32 * f32::MAX. The admitted
    /// term caller establishes this domain before any cursor movement.
    #[inline]
    pub(crate) fn score(&self, sum: f64) -> Score {
        debug_assert!(sum.is_finite() && sum >= 0.0 && sum <= 32.0 * f64::from(Score::MAX));
        let inflated = sum * self.factor;
        // Positive finite products use exactly the next_up bit successor.
        // For either signed zero, this instead produces the corresponding
        // signed least subnormal f64; its f32 cast preserves the zero bits.
        f64::from_bits(inflated.to_bits() + 1) as Score
    }
}

/// Take max score of different scorers
/// and optionally sum it with other matches multiplied by `tie_breaker`
#[derive(Default, Clone, Copy)]
pub struct DisjunctionMaxCombiner {
    max: Score,
    sum: Score,
    tie_breaker: Score,
}

impl DisjunctionMaxCombiner {
    /// Creates `DisjunctionMaxCombiner` with tie breaker
    pub fn with_tie_breaker(tie_breaker: Score) -> DisjunctionMaxCombiner {
        DisjunctionMaxCombiner {
            max: 0.0,
            sum: 0.0,
            tie_breaker,
        }
    }
}

impl ScoreCombiner for DisjunctionMaxCombiner {
    fn update<TScorer: Scorer>(&mut self, scorer: &mut TScorer) {
        let score = scorer.score();
        self.max = Score::max(score, self.max);
        self.sum += score;
    }

    fn clear(&mut self) {
        self.max = 0.0;
        self.sum = 0.0;
    }

    #[inline]
    fn score(&self) -> Score {
        self.max + (self.sum - self.max) * self.tie_breaker
    }
}

#[cfg(test)]
mod tests {
    use super::{ScoreSumUpperBound, SmallScoreSumUpperBound};

    fn check_small_bound(generic: &ScoreSumUpperBound, small: &SmallScoreSumUpperBound, sum: f64) {
        assert_eq!(
            small.score(sum).to_bits(),
            generic.score(sum).to_bits(),
            "sum={sum:?} sum_bits={:#018x}",
            sum.to_bits()
        );
    }

    #[test]
    fn small_score_sum_bound_matches_generic_at_cast_boundaries() {
        for unsupported in [0, 1, 2, 33, usize::MAX] {
            assert!(SmallScoreSumUpperBound::new(unsupported).is_none());
        }
        let leaf_bits = [
            0,
            1,
            2,
            0x007f_fffe,
            0x007f_ffff,
            0x0080_0000,
            0x0080_0001,
            0x3f7f_ffff,
            0x3f80_0000,
            0x3f80_0001,
            0x7f7f_fffe,
            0x7f7f_ffff,
        ];
        for count in 3..=32 {
            let generic = ScoreSumUpperBound::new(count);
            let small = SmallScoreSumUpperBound::new(count).unwrap();
            assert_eq!(small.factor.to_bits(), generic.factor.to_bits());
            // Distinguish both signs: numeric equality alone would miss drift.
            check_small_bound(&generic, &small, 0.0);
            check_small_bound(&generic, &small, -0.0);
            assert_eq!(small.score(0.0).to_bits(), 0.0f32.to_bits());
            assert_eq!(small.score(-0.0).to_bits(), (-0.0f32).to_bits());
            check_small_bound(
                &generic,
                &small,
                f64::from(u32::try_from(count).unwrap()) * f64::from(f32::MAX),
            );
            for bits in leaf_bits {
                let leaf = f32::from_bits(bits);
                check_small_bound(&generic, &small, f64::from(leaf));
                // The final finite-to-infinite f32 cast boundary has the same
                // half-ULP tie as the adjacent finite representable values.
                let midpoint = if leaf == f32::MAX {
                    f64::from(leaf) + 2.0f64.powi(103)
                } else {
                    (f64::from(leaf) + f64::from(leaf.next_up())) * 0.5
                };
                // Reverse the unchanged factor to put products around each
                // f32 midpoint, then probe neighboring representable inputs.
                // The first midpoint also checks f32 cast underflow to zero.
                let input = midpoint / generic.factor;
                let mut below = input;
                let mut above = input;
                for _ in 0..4 {
                    below = below.next_down();
                    above = above.next_up();
                }
                for sum in [below, input.next_down(), input, input.next_up(), above] {
                    check_small_bound(&generic, &small, sum);
                }
                // Also prove both sides of the cast boundary were exercised,
                // including zero/subnormal and maximum/infinity transitions.
                assert_eq!(small.score(below).to_bits(), leaf.to_bits());
                assert_eq!(small.score(above).to_bits(), leaf.next_up().to_bits());
            }
            // Exceptional/signed values keep the generic conservative route.
            for sum in [-1.0, f64::NEG_INFINITY, f64::INFINITY, f64::NAN] {
                assert_eq!(generic.score(sum), f32::INFINITY);
            }
        }
        assert_eq!(
            ScoreSumUpperBound::new(2).score(3.0).to_bits(),
            3.0f32.to_bits()
        );
    }

    #[test]
    fn small_score_sum_bound_matches_generic_for_disjoint_leaf_sums() {
        for count in 3..=32 {
            let generic = ScoreSumUpperBound::new(count);
            let small = SmallScoreSumUpperBound::new(count).unwrap();
            for seed in 0..256u64 {
                let mut state = seed + 1;
                let leaves: Vec<_> = (0..count)
                    .map(|_| {
                        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                        let bits = u32::try_from(state >> 32).unwrap() % 0x7f80_0000;
                        f32::from_bits(bits)
                    })
                    .collect();
                let mut prefix = 0.0f64;
                // Every split mirrors disjoint known leaves plus a remaining
                // inclusive prefix, retaining the original count's allowance.
                for split in 0..=count {
                    let mut known = 0.0f64;
                    for &leaf in leaves[split..].iter().rev() {
                        known += f64::from(leaf);
                    }
                    check_small_bound(&generic, &small, prefix);
                    check_small_bound(&generic, &small, known + prefix);
                    if split < count {
                        prefix += f64::from(leaves[split]);
                    }
                }
            }
        }
        // Adversarial incoming orders straddle a normal f32 tie even though
        // every leaf is itself exactly representable in f32.
        let leaves = [1.0f32, 2.0f32.powi(-24), 2.0f32.powi(-53), 2.0f32.powi(-53)];
        let generic = ScoreSumUpperBound::new(leaves.len());
        let small = SmallScoreSumUpperBound::new(leaves.len()).unwrap();
        for order in [[0, 1, 2, 3], [2, 3, 1, 0], [1, 0, 3, 2]] {
            let mut sum = 0.0f64;
            for index in order {
                sum += f64::from(leaves[index]);
            }
            check_small_bound(&generic, &small, sum);
        }
    }

    #[test]
    fn score_sum_bound_covers_double_order_error_at_float_midpoint() {
        let leaves = [1.0f32, 2.0f32.powi(-24), 2.0f32.powi(-53), 2.0f32.powi(-53)];
        let sum = |order: [usize; 4]| {
            let mut result = 0.0f64;
            for i in order {
                result += f64::from(leaves[i]);
            }
            result
        };
        let lower = sum([0, 1, 2, 3]);
        let higher = sum([2, 3, 1, 0]);
        assert_eq!(lower as f32, 1.0);
        assert_eq!(higher as f32, 1.0f32.next_up());
        let bound = ScoreSumUpperBound::new(4);
        assert!(bound.score(lower) >= higher as f32);
        for threshold in [1.0f32.next_down(), 1.0, 1.0f32.next_up()] {
            if higher as f32 > threshold {
                assert!(bound.score(lower) > threshold);
            }
        }
        // Prefix/suffix grouping represents four original leaves, not two.
        let grouped = (f64::from(leaves[0]) + f64::from(leaves[1]))
            + (f64::from(leaves[2]) + f64::from(leaves[3]));
        assert!(bound.score(grouped) >= higher as f32);
    }

    #[test]
    fn score_sum_bound_handles_signed_pointwise_replacements_and_extremes() {
        let bound = ScoreSumUpperBound::new(4);
        for leaves in [
            [f32::MAX, -f32::MAX, 1.0, f32::from_bits(1)],
            [-1.0, 1.0, f32::from_bits(1), f32::from_bits(1)],
            [0.0; 4],
        ] {
            let mut actual = 0.0f64;
            let mut nonnegative = 0.0f64;
            for leaf in leaves {
                actual += f64::from(leaf);
                nonnegative += f64::from(leaf.max(0.0));
            }
            assert!(bound.score(nonnegative) >= actual as f32);
        }
        assert_eq!(bound.score(f64::INFINITY), f32::INFINITY);
        assert_eq!(bound.score(f64::NAN), f32::INFINITY);
        assert_eq!(ScoreSumUpperBound::new(2).score(3.0), 3.0);
    }
}
