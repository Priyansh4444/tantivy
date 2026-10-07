# Candidate B: local essential-clause MaxScore, one document at a time

## Caller usage first

Existing callers retain `Searcher::search(&query, &TopDocs::with_limit(10).order_by_score())`.
No query API, schema, index format, directory contract or collector API changes.
Inside `BooleanWeight::for_each_pruning`, the existing two-term path stays unchanged;
only a sum-in-f64 `SpecializedScorer::TermUnion` with 3–32 actual term scorers uses
`or_maxscore(term_scorers, reader.max_doc(), threshold, callback)`. One term, higher
clause counts, intersections, nonterm children, minimum-should-match >1 and custom
nonsumming/f32 combiners retain their existing execution paths. The small-clause
limit is an initial implementation policy, not a semantic restriction on queries.

```rust
// Existing SUPPORTS_BLOCK_WAND && SUMS_IN_F64 gate remains outside this match.
match term_scorers.len() {
    2 => two_term_or_maxscore(term_scorers, threshold, callback),
    3..=32 => or_maxscore(term_scorers, reader.max_doc(), threshold, callback),
    _ => block_wand(term_scorers, threshold, callback),
}
```

`or_maxscore` checks admission before mutating any postings. Require each weight's
existing `has_safe_score_bounds()` and a finite, strictly positive global maximum.
This deliberately sends negative and zero boosts, exceptional/custom unsafe
normalization, infinite/NaN bounds and NaN initial thresholds to an exhaustive
`BufferedUnionScorer<_, SumCombiner>` plus `for_each_pruning_scorer`. Zero boosts are
valid; rejecting them from this optimization is conservative, not a query error.
The outer combiner gate already promises the sum-in-f64 semantics. No domain error
is swallowed or a NaN silently replaced. Supported custom statistics and nondefault
BM25 use the exact existing `TermScorer` bound policy without assuming DEFAULT.

## Grounding and observed bottleneck

Read production `boolean_weight.rs` (routing/construction), `block_wand_union.rs`
(production body), `block_wand_intersection.rs` (production body), full
`score_combiner.rs`, full `union/buffered_union.rs`, `term_scorer.rs` production body,
`bm25.rs` max-score domain methods, postings loaded-bound/shallow/seek lifecycle,
`skip.rs` shallow termination, `sort_by_score.rs` collection, lucene-rs full
`search/bulk.rs`, its term scorer bound methods, baseline perf report and retained
comparison README. These are actual source constraints, rather than assumptions
from algorithm names.

The retained five-case profile has 64.92% self cycles in `block_wand`, 9.62% loading,
8.41% block bound computation and 7.19% seeks, plus explicit sorting. WAND repeatedly
sorts scorers by current doc, finds a pivot, aligns terms and restores ordering.
The port partitions essential clauses using local maximum scores and only seeks
nonessential clauses for potentially competitive candidates. Its full implementation
also uses 4096-document score windows, pooled buffers and two-level impact metadata.
This candidate adopts the essential/nonessential idea without its dense windows,
pools, codec changes or new impact bytes.

The retained port advantage is 4.2x by mean OR medians, with expensive 3+term OR
approximately 4.86x versus 2.8x for two-term OR. That does not establish this lean
variant will capture the port's full benefit; candidate acceptance needs actual
baseline/treatment timing. The profile measures WAND execution, not a proven ceiling.

## Type and signature sketch

One private module owns both the proof-bearing region state and the execution loop.
Scorers stay in their original construction order forever; only small ordinal arrays
are permuted. This matches the exhaustive buffered union's scorer order, including
stable removal of exhausted terms.

```rust
// query/boolean_query/or_maxscore.rs
pub(crate) fn or_maxscore(
    scorers: Vec<TermScorer>,
    max_doc: DocId,
    threshold: Score,
    callback: &mut dyn FnMut(DocId, Score) -> Score,
) { /* not implemented */ }

struct Clause {
    scorer: TermScorer,
    global_max: Score,
    // Last actual loaded iterator position, retained through shallow advance.
    doc: DocId,
}

struct Region {
    // Half-open [start, end), with end <= TERMINATED; never end+1 overflow.
    start: DocId,
    end: DocId,
    local_max: Box<[Score]>,         // Original ordinals; 0 for proven exhaustion.
    order: Box<[usize]>,            // Fixed cost/score preference, not doc ordering.
    optional_prefix: Box<[f64]>,    // Bound sums in order, no subtractive updates.
    first_essential: usize,         // order[..k] optional; order[k..] essential.
}

// Local helpers, not public layers or trait methods:
fn prepare_region(clauses: &mut [Clause], start: DocId, max_doc: DocId,
                  region: &mut Region, bound: &ScoreSumUpperBound) { /* pseudocode */ }
fn enlarge_optional_prefix(region: &mut Region, threshold: Score,
                           bound: &ScoreSumUpperBound) { /* pseudocode */ }
```

Actual storage should allocate all scratch once per segment query and reuse it.
`Region` owns O(n) scratch, not one allocation per region. `doc` is not a second
iterator: it explicitly records the loaded position that a shallow cursor does not
update. Contributions are a single O(n) f32 array reused per candidate. Build
`ScoreSumUpperBound::new(original_leaf_count)` once; retain that count after
exhaustion. No term's exact bound cache is duplicated, no threadlocal pool exists.

The initial fixed order sorts ordinals by `global_max / max(size_hint,1)` ascending,
with original ordinal as a deterministic secondary key. This favors treating cheap
scores from expensive/dense terms as optional. This ranking affects only efficiency;
any optional subset satisfying the cumulative-bound proof is correct. It avoids a
new cost-based sort for every region. A locally recomputed ranking is a separate
measured experiment, not part of this first unit.

## Module map and interface depth

- `boolean_query/or_maxscore.rs`: private admission, region establishment, partition,
  candidate traversal and exact scoring, plus focused algorithm tests.
- `boolean_query/mod.rs`: one module declaration/private import.
- `boolean_query/boolean_weight.rs`: the small routing change above.
- Existing `TermScorer`, BM25 bounds, postings, collector, serializer and combiners:
  used unchanged. No new per-term public getter is needed: the crate-private
  `bm25_weight()` and its `has_safe_score_bounds()` already exist.

One internal entry point hides all temporal state. The caller cannot prepare stale
bounds or construct half-initialized partitions. A separate public region API or
pass-through wrapper per cursor operation would leak shallow lifecycle knowledge
and add abstraction without hiding complexity; reject those shapes.

## Execution pseudocode

```text
admit all domains before touching cursors; otherwise exhaustive sum fallback
freeze original clause ordinals and static preference order
allocate O(n) local bounds, prefix sums and exact contribution slots
L = minimum actual clause.doc; bound = ScoreSumUpperBound(original n)
while L < max_doc and L < TERMINATED:
    for each nonexhausted clause i:
        shallow seek_block(L)
        if no remaining postings: local_max[i] = 0; cache doc=TERMINATED
        else: local_max[i] = block_max_score()  # existing provenance/tail policy
              end_i = last_doc_in_block(), capped at max_doc-1
    R = 1 + minimum relevant end_i, capped at max_doc
    # no relevant end: finished; tail TERMINATED is capped before addition
    construct optional prefix sums from these nonnegative bounds in static order
    k = longest prefix whose outward-rounded bound <= threshold
    if k == n: L = R; continue  # whole region cannot beat threshold
    synchronize actual positions of essential clauses to >=L using seek, never advance
    while min essential actual doc d < R:
        clear contribution slots; known = 0f64
        for each essential ordinal i matching d:
            slots[i] = score_i(d); known += f64(slots[i])
        for j from k-1 down to 0:  # optional in descending preference order
            if bound(known + prefix[j]) <= threshold: reject candidate; break
            i = order[j]
            if cached doc_i <= d: actual seek_i(d), updating cached doc_i
            if actual doc_i == d: slots[i] = score_i(d); known += f64(slots[i])
        if not rejected:
            exact = fold slots in ORIGINAL ordinal order using f64, cast f32 ONCE
            if exact > threshold: threshold = callback(d, exact)
        advance every essential scorer actually at d; update cached doc
        enlarge optional prefix only after advancement, using new threshold
        if k == n: break  # remaining region cannot beat current threshold
    L = R
```

If local bound safety unexpectedly fails at runtime, do not reinterpret the domain
or keep pruning with invalid metadata. Existing `TermScorer::block_max_score` should
return the admitted global bound for untrusted/missing metadata, so that is the
normal safe fallback. Assert this invariant in tests; runtime nonfinite or negative
returned bounds should be replaced by the admitted finite nonnegative global bound
before construction, never published as a stronger bound. This replacement concerns
bounds only and must not alter document scores. Corrupt index errors/panics remain
visible. An actual score outside the admitted finite/nonnegative proof domain is an
invariant failure, not something silently clamped for output.

### Shallow advance is not actual advance

`seek_block` may move the skip reader beyond the loaded posting block while leaving
`SegmentPostings.cur` and decoder output at an old document. `advance` then requires
a loaded block, and bounds from that skip block cannot certify arbitrary earlier
documents. Region bounds must therefore be read only after `seek_block(L)`, and only
used for d in [L,R). Before scoring or advancing an essential clause, synchronize it
using actual `seek(L)` if its cached doc is below L. Optional actual seeks always
use nondecreasing candidate d >= L; scoring occurs only after an exact match.

The existing `SegmentPostings::seek` has an early-return/next-old-doc shortcut. This
is safe here because if shallow seek moves the block, ALL docs in the old block
are below L. For any later actual seek d>=L, neither shortcut can falsely return a
qualifying old document; seek reaches `block_cursor.seek`, which loads the selected
block. If cached actual doc>=L, shallow seek(L) cannot have moved beyond its current
block. Never call `seek` with a target below the cached doc. A shallow-exhausted
clause can safely be marked TERMINATED only after `has_remaining_docs()==false`;
TAIL's `last_doc_in_block()==TERMINATED` alone does not mean exhausted.

After an essential `advance` crosses its bound block, the new actual doc is >=R;
no document beyond R uses the previous bound. At the next region, query each bound
again. For unloaded tails `block_max_score` may retain the global bound; do not force
a tail load merely to tighten it. Loaded tails may tighten through the existing
policy. No loop adds 1 to TERMINATED. Max_doc uses physical reader.max_doc(), not
reader.num_docs(): deleted docs do not shrink the physical address range.

## Exact-score and pruning proof

Let s_i(d) be an individual matching f32 score, or zero when absent. Admission makes
s_i finite and nonnegative. A region certificate gives s_i(d)<=u_i for every d in
[L,R). The bound is a property of that region only; no assumption about the document
immediately preceding L is needed. Zero for shallow-exhausted clauses is exact.

Let F(v) be the original-order f64 sequential sum of f32 leaves, rounded to f32 once.
The exhaustive `BufferedUnionScorer<_,SumCombiner>` computes exactly F(s(d)).
Candidate exact scores use the same leaves, ordinal order, addition precision and
one final conversion, so raw bits match, including midpoint adversaries. Do not
accumulate the final score in cost order or add the outward-rounding allowance to
it. Signed/exceptional cases run exhaustive fallback unchanged.

`ScoreSumUpperBound(n).score(x)` conservatively covers different groupings/orders
of n nonnegative contributions, with outward construction of the Higham error
allowance. Prefix sums plus `known` use at most n original leaves and n-1 additions;
there is no subtractive cancellation. For candidate checks, replace all unchecked
leaves with their u_i and all checked leaves with exact nonnegative contributions.
Monotonic addition gives a pointwise upper vector. Therefore the rounded bound of
`known + remaining_prefix` is >=F(s(d)). If that bound <=threshold, no strictly
competitive document is discarded. The existing factor covers grouping/order of
prefix and known sums; use original n, never just the number of currently essential
terms. Rebuilding remaining sums by total-minus-consumed is rejected because
subtraction can round downward and destroy conservatism.

If the total optional prefix bound <=threshold, a document matching only optional
terms cannot pass. Hence every passing document has at least one essential match;
minimum-doc merging of essential iterators visits all possible passing candidates.
Actual essential contributions are all scored, and optional contributors are checked
unless a conservative bound proves rejection. When all terms become optional, the
whole remaining region cannot pass. Thresholds are monotonic under TopDocs; therefore
within a fixed region the optional prefix may grow, never shrink. Across regions it
is rebuilt from new bounds and formerly optional iterators are synchronized to L.
Skipping their earlier documents is justified by the completed previous region.

Candidates and callbacks are in strictly increasing physical docID order. Advance
ALL essential matches after processing d, including rejected candidates. Equal
scores do not pass (`score > threshold`), exactly as existing pruning; because
lower docIDs are visited first, later equal-scoring docs cannot improve that tie.
Deletions remain collector-side filtering with unchanged threshold update policy.
Duplicate terms retain separate actual leaf ordinals and do not lose multiplicity.

## Why global-only MaxScore is not enough

BM25's global maximum is its saturated weight, independent of observed frequencies
and document normalization. Frequent terms can have loose maxima relative to their
ordinary contributions. A 20-term OR can leave almost every term essential under
global-only bounds, devolving into O(n) min scans plus all-score work and losing
WAND's useful local skipping. There is no baseline evidence that global bounds
alone partition the measured slow queries well. Therefore choose existing local
bounds first, not a speculative global-only fast path.

The downside of the minimum-all-block-end region is fragmentation: dense terms
change a 128-posting certificate frequently, causing repeated O(n) bound and prefix
work. The implementation still eliminates per-document doc sorting/pivot alignment
and can avoid decoding dense optional terms. Whether that trade wins is measured.
A wider region based only on essentials would require bounds covering multiple
optional blocks; existing API does not certify that. Falling back to global bounds
for those optional terms is safe but may destroy the useful partition. Do not use
one optional block's max across a wider region or copy the port's two-level API
without its metadata. This is a clear stopping point if experiments show local
fragmentation dominates; then prefer the independently designed bulk-window shape.

## Rejected alternatives and extension boundaries

- Dense 4096-doc window: candidate A's different shape; potentially superior throughput
  for many essential terms, but ~O(window) scratch and score clearing rather than this
  lean O(terms) shape. This candidate offers a maintainable lower-memory baseline.
- Global-only generalized MaxScore: weaker evidence given saturated BM25 bounds;
  omitted rather than pretending the ranking partition will always exist.
- Heap per document: removes sorting but adds heap traffic; linear min scan over at
  most 32 essential ordinals is smaller. High-count existing WAND remains available.
- Per-candidate full rebuild or sort of term order: defeats measured bookkeeping
  removal. Prefix expansion occurs only when the threshold enables it.
- Required nonessential clauses/intersection optimization: useful port feature, but
  increases state and correctness proof surface; defer until simple variant wins.
- New two-level impacts/frontiers: on-disk bytes and format migration outside first
  accepted unit. Learn its pruning idea without claiming its storage cost is free.
- Changing SumCombiner or score expression: breaks exact bits; no arithmetic changes.
- Permanent removal when currently optional: invalid across regions; maintain every
  nonexhausted scorer and original score ordinal.
- Switching algorithms halfway with an unsynchronized shallow iterator: unsafe;
  admission fallback is before mutation. Any future midstream fallback must synchronize
  all actual positions to the proven next unprocessed doc first.

## Validation and acceptance

1. Test-first targeted adversaries: three and 20-term OR, rare/dense mixes, missing
   terms, duplicate clauses, 127/128/129 and 255/256/257 posting lengths, sparse
   quantized norms, unloaded tails, cross-block shallow leaps, deleted docs, multiple
   segments, repeated threshold growth and equal-score lower-doc ties.
2. Exact bits/ranked IDs against independent exhaustive ordinal `SumCombiner`
   checkpoints and TopDocs for k=1,10,100 and nonzero initial thresholds, including
   f32 midpoint leaves across disparate magnitudes. Compare callbacks strictly
   increasing; bound test for every document of every covered region.
3. Valid nondefault k1/b and custom field statistics; legacy/native score expressions;
   trusted/missing/untrusted metadata. Negative/zero boosts and unsafe/NaN/infinite
   stats prove admission fallback; custom dismax/f32 combiners and minimum match>1
   prove original routing. 33/64/large clauses prove high-count path retention.
4. Full frozen 1227-query gate: ordered IDs, exact counts, all finite raw f32 bits and
   each engine's exhaustive oracle; count timing retained to detect routing accidents.
   Every timed output checksum checked. Index hashes unchanged before/after.
5. Baseline/treatment same compiler/profile/lock/CPU, serial AB/BA, seeded queries,
   TT/PP controls, all raw samples and warmup records retained. Report full301 OR,
   3+term subset, all1201 top10/count and expensive five-case perf reprofile, not just
   the best examples. Expect block_wand share to disappear for routed cases; check
   if bounds/linear merge now dominate and whether real elapsed time improves.
6. Bound scratch bytes by n; record allocations and warmed RSS on identical warmup,
   text/index hashes and binary text/code size. No claim of total memory neutrality
   from a layout test alone. Reject a broadly regressive route even if five cases win;
   document any narrower routing predicate justified by measured stable benefit.

No code was edited, built or benchmarked for this candidate. Architecture status:
Ground complete; Sketch complete; root owns cross-judge, synthesis and implementation.
