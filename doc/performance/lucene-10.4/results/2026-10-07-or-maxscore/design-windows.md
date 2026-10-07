# Candidate A: local-window MaxScore with canonical score replay

This is a design sketch, not an implementation or a speedup claim. Scope is the
read-time execution of ordinary summing, term-only OR with 3–32 clauses. No
posting-format, schema, writer, public-query, collector or directory change is
needed. Preserve the current one/two-term implementations and all other query
paths. The caller owns the existing score callback; the new function hides all
window, partition, scratch and posting-cursor lifecycle rules.

## Caller usage first

```rust
// Existing BooleanWeight::for_each_pruning, TermUnion arm only.
if TScoreCombiner::SUPPORTS_BLOCK_WAND && TScoreCombiner::SUMS_IN_F64 {
    match term_scorers.len() {
        2 => two_term_or_maxscore(term_scorers, threshold, callback),
        3..=32 => window_maxscore(term_scorers, threshold, callback),
        _ => block_wand(term_scorers, threshold, callback),
    }
} else {
    // Existing BufferedUnionScorer with the actual custom combiner.
    // No change to that arm.
}
```

`window_maxscore` performs its own numeric eligibility check before mutating any
scorer. Unsupported inputs return through the existing `block_wand` path. The
caller does not learn whether a particular window used stored, transformed or
global bounds. COUNT and ordinary exhaustive union stay on their current paths.
Do not route two-term OR here merely to simplify dispatch: the present two-term
algorithm needs a separate performance reason before changing it.

## Types and signature

```rust
const INNER_WINDOW: usize = 256;
const MAX_WINDOW_TERMS: usize = 32;

pub(crate) fn window_maxscore(
    scorers: Vec<TermScorer>,
    threshold: Score,
    callback: &mut dyn FnMut(DocId, Score) -> Score,
) {
    // not implemented
}

// All arrays are in original incoming scorer ordinal, never doc/score order.
struct ClauseState {
    global_bound: Score,
    window_bound: Score,
    cost: u32,             // fixed size_hint, clamped to >=1 for sorting
    essential: bool,
}

struct WindowScratch {
    candidates: [u64; INNER_WINDOW / 64],
    // Exactly n * INNER_WINDOW f32s, one row per original scorer ordinal.
    // Only essential rows are populated. It is reused for this query only.
    contributions: Box<[Score]>,
}
```

The incoming `Vec<TermScorer>` is the owning state. Keep it in canonical order
throughout; exhausted scorers stay as harmless terminated entries. Use a small
`Vec<ClauseState>` and an ordinal vector for bound/cost sorting. No wrapper with
pass-through `Deref`, separate reusable engine object, thread-local pool or new
postings API. Private functions for constructing an outer window, partitioning
and replaying an inner window are acceptable if they isolate meaningful rules;
do not turn each scoring step into a public capability.

Maximum matrix storage is 32,768 bytes, candidate mask 32 bytes, plus O(32) clause
state and ordinals. With fewer clauses the matrix is smaller. The 32-clause cap
is a resource policy, not a changed Boolean-query limit; larger unions retain
the existing algorithm. Allocate the matrix once per eligible query invocation,
on the heap, and zero the essential rows before reuse. No per-window allocation.

## Module map

1. New `query/boolean_query/window_maxscore.rs`: private implementation and its
   focused cursor/score/partition tests; one `pub(crate)` entry function.
2. `query/boolean_query/mod.rs`: declare module and expose that function locally.
3. `boolean_weight.rs`: the one dispatch change above.
4. Reuse `score_combiner::ScoreSumUpperBound` unchanged. Reuse `TermScorer` and
   its crate-private BM25/block-cursor accessors unchanged.

No new information crosses the TermScorer boundary. Its existing block-bound
policy continues to decide what is numerically trustworthy.

## Grounding in the current sources

Read production execution and relevant tests in `block_wand_union.rs`,
`boolean_weight.rs`, `term_query/term_scorer.rs`, `score_combiner.rs`,
`union/buffered_union.rs`, and the seek/bound/load/advance paths in
`postings/block_segment_postings.rs` and `postings/segment_postings.rs`.
Read the port's full `search/bulk.rs` and `search/term_scorer.rs`, term-only OR
dispatch in `search/searcher.rs`, and shallow advance, batch postings and impact
paths in `codec/postings_reader.rs`. Also read the balanced report and the
retained baseline perf report.

The current WAND implementation keeps scorers ordered by their current document,
computes pivots, shallow bounds and alignment, then restores document order after
candidate advancement. Five slow 3+ term cases spend about 64.92% self CPU in
`block_wand`, 9.62% loading blocks, 8.41% obtaining bounds and 7.19% seeking.
Those measurements establish an execution target, not a guaranteed gain.

The port partitions clauses by local upper-bound/cost, obtains candidates from
essential clauses only, accumulates a fixed inner window, then probes optional
clauses for candidates that can still compete. Its 4,096-document pooled scratch,
collector-specific interface, heap, adaptive windows, required-clause inference
and richer impact hierarchy should not all be copied into the first Tantivy
unit. This design adopts local partitioning and batching while preserving our
score contract with existing metadata.

## Algorithm and cursor lifecycle

Separate an **outer bound interval** from the 256-document **inner scratch
interval**. Capping the outer interval at 256 would repeatedly revisit sparse
blocks spanning millions of doc IDs. Outer intervals end at existing skip-block
boundaries; inner intervals start at an actual essential document and skip empty
gaps within that bound interval.

Eligibility, before any cursor mutation:

- 3–32 incoming term scorers and the caller's summing-f64 combiner gate.
- Every scorer has BM25 `has_safe_score_bounds()` and finite, strictly positive
  global `max_score()`. Strict positivity intentionally keeps negative and zero
  boosted terms on the existing implementation; zero global bounds do not prove
  that a scorer is nonnegative because negative weights also have bound zero.
- The initial threshold is ordered (not NaN). Infinities retain normal comparison
  behavior: +infinity returns no competitive results; -infinity makes every
  live clause essential. No clamping exceptional inputs into an ordinary domain.

For a new outer interval starting at `lo`, inspect every nonterminated clause:

1. If its current loaded document is below `lo`, shallow-select at `lo`.
   Otherwise shallow-select at its current document. Targets are monotonically
   increasing per scorer.
2. If the selected block is a tail (`last_doc_in_block == TERMINATED`), perform
   `seek(max(lo, doc))` before asking for its bound. This reconciles any stale
   loaded document and establishes tail exhaustion. A terminated result has
   bound zero. Without this step, an old optional document plus an unbounded
   tail interval could falsely keep the outer loop alive after the real end.
3. For each live selected block, record its bound and its inclusive end. Form
   `hi` as the minimum exclusive end among live selected blocks, with sentinel-
   aware addition. `hi` is strictly greater than `lo`. No doc equals TERMINATED.
4. A clause whose known current document is at or beyond the final `hi` cannot
   match this interval and has local bound zero. A stale current document below
   `lo` does not justify zero: it retains the selected-block bound.
5. A local bound must be finite and >=0 for this eligible domain. An unexpected
   negative/NaN/nonfinite local bound is replaced with that clause's certified
   finite global bound; do not use `f32::max` to hide a NaN. This degrades pruning,
   preserving the proof. No fallback may restart already consumed documents.

After step 1, an old `doc()` can still refer to an earlier decoded block. It is
safe to inspect it as a numeric lower bound only. Before calling `score` or
`advance` on that scorer, reconcile using `seek(target >= lo)`; a shallow reader
is not a positioned posting iterator. Never query a new `block_max_score` and
reuse a previous interval's cached end alongside it.

Partition once per outer interval using the interval's starting threshold:

- Stable sort *ordinals* by local bound divided by clamped cost, with ordinal as
  deterministic tie breaker. This prioritizes making low-value expensive clauses
  optional; no score arithmetic is done in this order.
- Greedily extend an optional prefix while the existing `ScoreSumUpperBound`
  applied to the prefix sum is <= starting threshold. Remaining clauses are
  essential. Use the original number of leaves for every upper-bound allowance.
- If every clause is optional, no document in the interval can compete: advance
  the logical outer floor directly to `hi`, without loading skipped full blocks.
- Keep this partition fixed until the outer interval ends. Increasing thresholds
  can only strengthen the optional-only rejection proof. Repartitioning on each
  callback is unnecessary for the first unit and adds hot-loop work.

For a surviving interval, seek each essential scorer with stale `doc < lo` to
`lo`. Its real new document may already be outside the interval. Let `inner_lo`
be the minimum real essential document below `hi`; no such document finishes
the outer interval. Let `inner_hi = min(hi, inner_lo.saturating_add(256))`.

Iterate essential scorers in original ordinal. For every actual posting in this
inner interval, set its candidate bit and store the scorer's exact f32 `score()`
in its ordinal row at `doc-inner_lo`; advance normally. The bit is independent
of the numeric value, so a zero-scoring match remains a candidate when appropriate.
Each essential scorer ends at or beyond `inner_hi`, possibly on a later loaded
block. The stored outer bound remains a certificate for the old interval; do
not recompute it from that new cursor state mid-window.

Flush candidate bits in increasing doc ID. For a candidate, iterate all original
scorer ordinals. An essential contribution comes from its stored row. An optional
contribution comes from `seek(candidate)` only if the scorer's current document
is <= candidate; if it equals the candidate, use the exact `score()`. Accumulate
individual f32 leaves in f64 in original ordinal and cast to f32 exactly once.
Call the callback only when this exact score is strictly above the current
threshold. Record the returned monotonic threshold and continue ascending.

Before optional probes, or between ordinals, a safe early rejection may use the
canonical actual f64 prefix plus a precomputed f64 suffix of unvisited local
bounds. Apply `ScoreSumUpperBound::new(original_n)` to that nonnegative grouped
sum. If the rounded upper bound is <= current threshold, discard this candidate
without visiting the suffix. Actual document scores never use that allowance.
For the first implementation this inner rejection is optional: omitting it is
correct and is easier to trace, but may lose important savings on long unions.

After flushing, find the next real essential doc below the same `hi`, thereby
jumping empty doc-ID gaps. When none remains, set outer `lo = hi`, discard all
old local certificates and partition state, and repeat. The tail reconciliation
establishes final exhaustion even if optional scorers were never physically
advanced through noncompetitive complete blocks.

## Pseudocode

```text
if unsupported numeric domain or term count:
    current block_wand(scorers, threshold, callback); return
allocate bounded scratch once; keep scorer order unchanged
lo = minimum current document
while lo < TERMINATED:
    (hi, local_bounds) = select_outer_interval_and_reconcile_tails(lo)
    if no live scorer: return
    optional, essential = partition(local_bounds, threshold)
    if essential is empty: lo = hi; continue
    seek essential scorers to lo if their loaded document is behind
    while (inner_lo = minimum essential doc) < hi:
        inner_hi = min(hi, inner_lo + 256)
        clear candidate bits and essential contribution rows
        for essential ordinal in ORIGINAL ORDER:
            while scorer.doc < inner_hi:
                mask[scorer.doc-inner_lo] = true
                row[ordinal][scorer.doc-inner_lo] = scorer.score()
                scorer.advance()
        for doc in ascending mask:
            prefix = f64(0)
            for ordinal in ORIGINAL ORDER:
                if upper_bound(prefix + suffix_bounds[ordinal]) <= threshold:
                    reject doc; break
                leaf = stored essential leaf OR lazy optional exact leaf OR 0
                prefix += f64(leaf)
            if full score assembled and f32(prefix) > threshold:
                threshold = callback(doc, f32(prefix))
    lo = hi
```

## Correctness argument

For each term and each document inside `[lo, hi)`, its contribution is between
zero and that term's recorded local bound in the eligible domain. Missing terms
contribute zero. Trusted default metadata, native transformed metadata and
global fallback are all supplied by the existing TermScorer policy, including
custom statistics and nondefault BM25 parameters; this algorithm does not
reinterpret a stored saturation pair itself.

The optional set has an outward-rounded summed upper bound <= the interval's
initial threshold. Thus a document matching only optional terms cannot have a
rounded score strictly greater than that threshold, or any later monotonic
threshold. Every competitive document must match at least one essential term,
and essential enumeration marks every such document inside the interval.

For every unpruned candidate, the replay contains precisely the same f32 term
scores and same ordinal order as BufferedUnionScorer's `refill`, which visits
scorers in incoming order and retains their relative order as terms exhaust.
Storing leaves does not change their bits. It also avoids the port-style grouped
essential/optional accumulation order. Canonical f64 summation followed by one
f32 cast preserves raw output score bits, including f32 rounding midpoints.

The partial-prefix rejection uses a conservative upper bound on a sum of
nonnegative original leaves. Prefix/suffix grouping does not reduce the leaf
count used for the existing Higham error allowance. Monotone f32 conversion
then makes `upper <= threshold` safe for the current strict `score > threshold`
admission contract. Preserve the established local ascending doc-ID callback
order: later equal-scoring doc IDs cannot displace earlier equal-scoring hits
under the existing collector tie policy. Cross-segment policy stays untouched.

Duplicate clauses remain separate incoming scorers with separate ordinals and
weights. No deduplication or boost coalescing is introduced. Absent and exhausted
terms contribute zero without changing original leaf count. Deleted documents
retain the existing callback/collector filtering contract; local maxima may
include deletions, which weakens pruning without rejecting a live winner.

Invalid custom statistics, negative/zero weights, exceptional global bounds,
NaN threshold and >32 clauses never enter this proof domain: dispatch returns
to current behavior before mutation. This design does not claim to repair every
pre-existing exceptional-domain behavior of WAND. Custom non-summing or f32
combiners continue through the actual generic combiner, without a new fallback
that accidentally changes their arithmetic.

## Rationale and rejected alternatives

- **Single f64 score per scratch document:** approximately 2 KiB and attractive,
  but adding optional terms after essential terms reorders the canonical sum.
  Existing midpoint regressions demonstrate a one-bit difference is possible
  even when all inputs are positive. A last-bit change can alter threshold ties.
  The bounded f32 matrix pays at most 32 KiB to preserve the stronger contract.
- **Unbounded matrix or port's whole pooled scorer:** needless memory growth or
  persistent per-thread retention and a larger implementation surface. Cap only
  this optimization's domain, retaining the public ability to execute long ORs.
- **256-ID outer intervals:** pathological repeated metadata work for sparse
  block spans. Use block-bounded outer intervals and actual-doc-based inner
  intervals instead. Do not conflate physical posting blocks with doc-ID spans.
- **Global-bound-only MaxScore:** smaller and avoids shallow lifecycle entirely,
  but may leave very many common clauses essential when global saturation is
  loose. It is a useful structurally different contender, not the window design.
- **Port's outer minimum-size adaptation:** needs bounds valid across several
  posting blocks. Tantivy's current block_max_score is a single selected-block
  certificate, so extending past the minimum block end with that bound is
  unsafe. A genuine range-bound API or new hierarchy is a later independent unit.
- **Infer required clauses / add essential heaps / SIMD batch scoring now:**
  possibly valuable, but unnecessary to prove the first candidate elimination
  and window batching result. Add only after profiling this simpler unit.
- **Change index metadata first:** existing postings already support local safe
  bounds. This execution experiment can preserve index bytes exactly and tells
  us whether richer metadata is worth its storage cost later.

Risk: many tiny outer intervals may cause too much repartition work, and row
clearing plus ordinal replay may outweigh removed WAND bookkeeping. The profile
supports trying this design, not accepting it without measurement. If a simpler
global MaxScore design wins while preserving bits, prefer its smaller surface.

## Validation and acceptance plan

1. Test first: synthetic term unions 3–32 clauses compare complete callback
   traces and raw f32 bits with an exhaustive canonical union under identical
   monotonic threshold callbacks. Include thresholds just below/equal/above
   scores, all-essential/all-optional and repeated threshold changes, and the
   existing [1, 2^-24, 2^-53, 2^-53] midpoint ordering example using feasible
   weighted leaves. Check strictly increasing callback doc IDs and no duplicate
   callbacks. Use exact bits, not tolerance-based `nearly_equals`.
2. Cursor adversaries: shallow-selected full blocks ahead of loaded docs;
   optional never probed in several outer intervals then essential later; tail
   lengths 1/127/128/129; dense and sparse codecs; very large gaps; a scorer's
   first document beyond another's block boundary; block-end and sentinel-adjacent
   doc IDs. Confirm no score/advance call sees an unloaded shallow cursor.
3. Numeric/routing matrix: default and configured BM25, legacy/custom statistics,
   varying IDF/boost magnitudes, missing norms/metadata, duplicated/absent terms,
   negative/zero/nonfinite boosts and unsafe statistics, 1/2/33+ terms, custom
   f32 and non-summing combiners, minimum_should_match and excluded terms. Assert
   unsupported cases retain existing dispatch; test multisegment/deletions and
   tie order through TopDocs as well as scorer-level traces.
4. Frozen 1,227-query gate: baseline, treatment, fixed port and exhaustive oracles
   match exact count, ordered physical IDs and raw score bits. Keep all 11,509
   returned bits, complete query set and existing payload/index hash guards.
   Hash every original non-lock index file before/after; zero new stored bytes
   is a categorical result of this read-time-only patch, then verified by hashes.
5. Performance: same compiler/dependencies/native profile, baseline/treatment
   binary hashes; serial pinned workers, both orders and same-engine controls.
   Measure all 301 ORs and specifically 3+ terms, complete 1,201-query suite,
   Wiki20, COUNT, single/two-term negative controls. Report both geometric ratio
   and ratio of mean query medians, tails and individual regressions; no selecting
   only the five profiled cases or declaring universal improvement.
6. Retain samples/checksums/source/index/query identities and load observations.
   Track maximum scratch allocation, warmed RSS and release binary/code size.
   A measured speedup must exceed observed controls without meaningful broad-
   suite regressions or index-size change. If rows/partition cost lose, do not
   graft codec changes onto a failed shape: reject or redesign first.

## Synthesis decision

Pending independent cross-judge and root review. This candidate prioritizes
local pruning strength and batch traversal with a bounded explicit leaf matrix;
it should be compared with a structurally simpler streaming/global-bound design
before choosing an implementation.
