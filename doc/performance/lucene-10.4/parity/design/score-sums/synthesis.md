# Default Boolean score accumulation

The strict native comparison exposed one ranking failure: identical documents in
`merged_missing_deleted_fieldnormsFalse/minimum_two` received neighboring float
scores depending on heap traversal order. Individual term scores already matched
the pinned Lucene 10.4 reference. With field population 147, tokens 1583 and term
document frequencies 108/108/53, the three child scores have bits `3e65c6cb`, `3e65c6cb`,
`3f5a91a8`. Recursive float addition yields either 1.3025672435760498 or
1.3025673627853394. Lucene's `DisjunctionSumScorer` sums double and casts once,
yielding the latter. A real index regression was committed red before the fix.

## Contract and compatibility

The default `SumCombiner` now accumulates child float scores in double and rounds
once when read. Its public name, constructor and trait methods remain available;
its private accumulator grows from four bytes to eight. Default Boolean scores
can therefore change their last bits compared with recursive float addition,
including when a historical custom BM25 provider supplies the child scores.
Those providers' scalar BM25 convention and statistics rounding are unchanged.

Intersection uses the same default sum. Buffered unions and minimum-should heap
disjunctions already carry the generic combiner and inherit its arithmetic.
Required/optional still joins two already-rounded child groups; double addition
and one float conversion of two float values give the same result as Lucene's
float two-group join. Nested query boundaries continue returning float scores.
No flattening of nested queries or normalization of token graphs is introduced.

Custom `ScoreCombiner` implementations retain their supplied arithmetic. An
additive `SUMS_IN_F64` capability defaults false; optimized union summation needs
both it and the existing `SUPPORTS_BLOCK_WAND` flag. Existing float-summing custom
combiners use their own generic scorer, including when they already declared
`SUPPORTS_BLOCK_WAND=true`. They may lose that specialization, while retaining
their exact score behavior. Opting into `SUMS_IN_F64` declares the double/final
float sum contract; it is not implied by sum eligibility alone. Disjunction-max
and other custom combiner operations are unchanged.

## Bounds and proof

Exact document sums and every optimized union/intersection bound use compatible
arithmetic. A float bound cannot safely prune a document whose actual sum now
rounds only once. Merely summing the bounds in double is also insufficient when
their order differs from document accumulation: leaves
`[1, 2^-24, 2^-53, 2^-53]` produce different final float values in different
recursive double orders at a float midpoint. This is a regression test.

For nonnegative leaves and k additions, the standard recursive-sum error bound is
`gamma = k*u/(1-k*u)`, with double unit roundoff `u=2^-53`. If one order of
upper-bound leaves gives S, their exact sum is at most `S/(1-gamma)` and any other
order is at most that exact sum times `1+gamma`. The helper therefore bounds by
`S*(1+gamma)/(1-gamma)`, rounding each coefficient operation and the final
product outward. It computes the coefficient once per optimized query kernel.
Float conversion is monotone, so the final rounded bound safely compares with
the float score threshold. Nonfinite bounds disable pruning. For two operands
the addition is commutative and no order allowance is required.

Grouped prefix/suffix sums retain the original leaf count. Signed actual scores
are accumulated separately from nonnegative pruning leaves: each known
contribution is replaced by `max(score,0)` for the bound. Pointwise monotonic
addition bounds the signed actual sum by this positive counterfactual; the
positive-sum error proof then applies. Returned document scores receive no error
allowance. Candidate filtering compares the forward rounded total bound;
subtracting a suffix from the threshold is not the inverse of that comparison.

This matches Lucene's double score operation rather than claiming floating-point
addition is mathematically associative. Pinned local source evidence is
`releases/lucene/10.4.0`: `DisjunctionSumScorer`, `ConjunctionScorer`, `WANDScorer`,
`ReqOptSumScorer`, and `MathUtil.sumUpperBound`. Lucene also explicitly allows for
order error in its disjunction bounds; this implementation uses the conservative
gamma ratio with outward rounding rather than its approximate coefficient.

## Design decision and verification

Candidate A changes the default sum once. Candidate B preserves every historical
intermediate float rounding through a new statistics-provider aggregation policy,
a private native combiner, and mode propagation through intersections and WAND.
The independent judge selected A after the default rounding change was accepted:
it keeps arithmetic ownership together without adding another provider policy.
B's bound proof, forward comparison and custom-combiner safeguards were retained.
Full candidate packages and judgment are in `target/score-sum-receipts-oct03`.

The committed red index test recreates the exact scalar statistics with 147
documents and verifies 53 identical documents across clause orders, minimum-should,
OR and all-MUST. It checks exhaustive scores, TopDocs and pruning at the preceding,
equal and following float thresholds. A custom recursive-float combiner with
`SUPPORTS_BLOCK_WAND=true` retains its distinct lower score through its own update
path. Helper tests cover the midpoint witness, signed pointwise replacements,
subnormal leaves, zero, maximum finite values and nonfinite bounds. The existing
exhaustive WAND reference adopts the new default double-sum contract without
loosening its checks. Library/release receipts accompany the fix report.

The parent owns the strict cross-engine gate and comparative performance runs.
This unit does not change their ruler or claim a measured speed improvement.
