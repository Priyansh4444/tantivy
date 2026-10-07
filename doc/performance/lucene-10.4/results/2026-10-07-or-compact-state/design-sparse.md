# C: sparse, ordinal-preserving candidate contributions

## Caller first

Keep the existing private entry point and the complete routing contract:

```rust
or_maxscore(term_scorers, reader.max_doc(), threshold, callback);
```

Only the admitted three-through-32-term, f64-summing OR route changes. The caller,
collector, query API, postings codec, index format, local certificate construction,
static preference order, and region partition remain as they are. Exceptional
numeric domains still enter `BufferedUnionScorer<SumCombiner>` before any cursor
mutation.

Inside that route, replace an implicit dense candidate (every slot initialized for
every document) with an explicit sparse candidate: an ordinal-indexed leaf array
and a 32-bit membership mask. The mask says which array slots belong to the current
document. Store actual f32 leaves as before; replay only its set bits, in increasing
incoming ordinal order, into the final f64 sum.

## Grounding and rationale

The current candidate loop clears all n leaf slots, writes the matching leaves,
and—if still competitive—folds all n slots. A document that matches one of 20
clauses still clears and, when competitive, folds 20 slots. The port's window
bitmap is a useful architectural lesson: membership can be explicit, and cleanup
can visit only participating entries. Adopt that principle at candidate scope,
without adopting the port's buffered accumulation order or its pooled windows.

This is a hypothesis, not a demonstrated bottleneck. The retained profile places
89.12% self time in inlined `for_each_pruning`; the annotated zero stores identified
in the grounding are **region-bound cleanup**, followed by prefix construction.
They are not evidence that contribution clearing dominates. This design leaves
`local_max.fill(0.0)` and prefix construction untouched. It could lose if candidate
leaf counts are dense, n is small, compiler-vectorized dense folds are cheap, or
mask manipulation costs more than the eliminated stores and zero additions.

## Private types, signatures, and module map

Only `src/query/boolean_query/or_maxscore.rs` needs production changes. No public
exports or edits to `boolean_weight.rs` are needed.

The minimal representation, scoped to this function, is:

```rust
let mut contributions: Vec<Score> = vec![0.0; num_terms]; // allocate once
// Each candidate owns a fresh mask; the leaf array may retain older values.
let mut present: u32 = 0;
```

If a named helper improves readability without enlarging the interface, keep it
private in the same module:

```rust
struct CandidateLeaves {
    leaves: Vec<Score>, // indexes are original scorer ordinals
    present: u32,
}
impl CandidateLeaves {
    fn new(num_terms: usize) -> Self;
    fn begin_candidate(&mut self); // sets present = 0 only
    fn record(&mut self, ordinal: usize, leaf: Score);
    fn score(&self) -> Score; // ascending ordinal f64 replay, one f32 cast
}
```

Prefer the local representation for the first implementation: it makes the new
invariant visible beside the existing essential/optional loops and avoids a
separate stateful abstraction. Either representation must retain the n <= 32
precondition and use only valid ordinal shifts 0..31. Do not create a fully set
mask with `1u32 << n`, which is invalid for n == 32.

## Pseudocode

```text
eligibility, ordering, global/local certificates and region selection: unchanged
for each essential-driven candidate doc in the region:
    present = 0
    known = 0f64
    for each essential ordinal:
        if actual scorer doc == candidate doc:
            leaf = scorer.score()
            contributions[ordinal] = leaf
            present |= 1u32 << ordinal
            known += f64(leaf)

    competitive = true
    for optional ranks in existing reverse preference order:
        if conservative_upper(known + inclusive_prefix[rank + 1]) <= threshold:
            competitive = false
            break
        perform the existing actual seek reconciliation
        if actual scorer doc == candidate doc:
            leaf = scorer.score()
            contributions[ordinal] = leaf
            present |= 1u32 << ordinal
            known += f64(leaf)

    if competitive:
        bits = present
        exact = 0f64
        while bits != 0:
            ordinal = trailing_zeros(bits)
            exact += f64(contributions[ordinal])
            bits &= bits - 1
        score = exact as f32
        perform the existing strict score > threshold callback/update

    advance matching essential scorers exactly as today
```

A recorded zero leaf may remain in the mask: that keeps membership defined by a
match, avoids a data-dependent extra branch, and retains the exact recurrence at
matching zero leaves. Slots from older documents must never be read unless the
current mask includes them. Even an early-pruned candidate starts with an empty
mask, and the next candidate also resets it; no per-slot cleanup is necessary.

## Correctness proof

**Candidate membership.** Each scorer has a unique incoming ordinal, even when
multiple clauses repeat the same term. Each ordinal appears once in the static
preference order and belongs to exactly one essential/optional partition. A bit
is set only after reading that ordinal's actual score at the current physical
document. Therefore set bits name precisely the current candidate's matching
clauses whose leaves have been evaluated. No stale slot can contribute.

**Exact arithmetic.** For a competitive candidate, all essential clauses were
visited and every optional rank passed its probe, so every matching clause's
f32 leaf was evaluated. Removing array slots for nonmatching clauses removes
only the positive zero initialization used by the current implementation. Starting
from positive f64 zero, adding positive zero leaves the accumulator unchanged.
The admitted safe, positive-weight score domain yields nonnegative finite leaves;
zero-valued matches can stay recorded, so their actual sign is not rewritten.
Enumerating set bits with `trailing_zeros` gives increasing incoming ordinal,
identical to the current dense replay with its nonmatching zero operations
removed. No f64 addition among actual leaves is reordered. One final f32 cast is
retained, including the existing four-leaf midpoint case. This argument does not
permit summing leaves directly in essential/optional preference order.

**Pruning.** `known`, local/global maxima, inclusive prefixes, outward
`ScoreSumUpperBound`, strict cutoff, and monotonic callback threshold remain
unchanged. The sparse score is computed only where the current implementation
would replay the dense score. No new pruning rule or subtractive bound is used.

**Cursors and deletions.** Shallow region preparation, actual optional seeks,
tail reconciliation, essential advancement, ascending candidate order and
physical `max_doc` are unchanged. A membership bit is recorded only after the
existing real cursor equality check. Deleted-document filtering stays with the
existing caller/collector path. The mask has no cursor authority.

## Expected cost and compactness

Current candidate scratch payload stays `24n + 8` bytes across the existing four
arrays, plus a four-byte local mask and temporary integer state (registers when
possible). Maximum array payload remains 776 bytes at n == 32. Keep the Vec-sized
leaf array rather than replacing it with `[Score; 32]`: the fixed array would add
up to 116 bytes for a three-term query and unnecessarily change the compactness
tradeoff. No allocations per candidate or region, no pool, and no disk changes.

Eliminate n contribution zero stores for every candidate. For a fully evaluated
candidate with m matching clauses, replace n final f64 additions/loads with m,
at the cost of m bit sets and m bit-enumeration steps. Existing essential
candidate selection and advancement still scan their ordinals; this design does
not claim to remove those scans or region preparation. An early-pruned candidate
benefits only from eliminating its dense clear and pays for its recorded bits.

At n == 3 with most leaves matching, the dense implementation can be cheaper;
at n == 20 or 32 with one/few matching leaves, savings are more plausible. Start
with one uniform implementation rather than an arbitrary n cutoff. If a measured
loss occurs on dense short queries, reject or require a separately justified,
tested cost gate; do not infer that high query length guarantees sparse matches.

## Why not batch this unit

The port's single f64 score per window document accumulates leaves in execution
order. That cannot simply replace our incoming-ordinal replay. A faithful 4096-
document by 32-leaf f32 matrix would alone consume 512 KiB; even a 64-document
matrix uses 8 KiB before doc masks and state, beyond current scratch. A sparse
triplet buffer would require grouping/sorting by document and original ordinal,
capacity handling, and clearer cursor boundary contracts. Small batches may
ultimately amortize essential selection and scorer dispatch, but this sketch
rejects them for this unit: their proof and storage cost are materially larger
than an O(1) candidate mask, and our current profile does not yet prove batch
scoring is the next causal win.

## Verification and rejection plan

Retain all nine exact callback-trace tests, including tail blocks, deferred
optional-to-essential cursors, global certificates, duplicate clauses,
32-clause masks, exact midpoint scores and unsafe numeric fallback. Add one
focused exact-trace fixture with alternating disjoint and overlapping clause
matches: it must reuse leaf slots across many documents, include early-pruned
candidates, a high bit at ordinal 31, and threshold increases. This catches stale
leaf reads and mask reset failures directly. Match the independent buffered
union trace by raw score bits.

Run frozen 1227-query default and both configured BM25 exact gates, the existing
public deletion/multisegment/raw-bit tests, and required formatting/clippy checks.
A pilot may reject this attempt, but acceptance requires the full 1221-query
AB/BA schedule and same-engine controls, unchanged source/binary/index guards,
post-profile, code size and RSS accounting. Inspect broad long-OR means and every
per-query control-adjusted regression; specifically compare short/dense OR and
long/sparse cases. Report current background load. No own heavy job may overlap
latency timing.

Reject if the measured gain is noise-sized, if dense cases lose beyond controls,
if masks enlarge generated code enough to offset savings, or if compiler output
shows the old clear/fold was already negligible. Do not claim the region-cleanup
sample disappeared as evidence of this mechanism: that loop intentionally stays.
