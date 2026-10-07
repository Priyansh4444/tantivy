# Independent OR design cross-judge

Both complete candidate packages and the grounding were read. This is a design
judgment, not benchmark evidence or implementation acceptance. Relevant production
source checked directly: TermScorer, SumCombiner/ScoreSumUpperBound, BooleanWeight
routing, BufferedUnionScorer/refill, BM25 bounds and literal score expressions,
SegmentPostings seek/advance, BlockSegmentPostings shallow selection/load/bounds,
and SkipReader seek/remaining-doc/tail behavior. No code was edited or executed.

## Decision and scoring

Use **B, streaming local essential-clause MaxScore**, as the first implementation,
with the cursor and fallback clarifications below. It is the smaller independent
experiment and needs O(clauses) scratch instead of A's bounded leaf matrix. Keep A
as a later alternative if profiling shows linear candidate merging/scoring is the
remaining bottleneck. Do not combine both execution engines in the first patch.

Scores are 1–5; 5 is strongest against the grounding's criterion, not proof of a
measured win.

| Grounding criterion | A: window/replay | B: streaming | Reason |
|---|---:|---:|---|
| Exact bits/rank, safe pruning and edge cases | 4 | 4 | Both preserve canonical leaves and restrict numeric domains. Cursor/fallback contracts still need concrete tests; B's tail prose needs clarification. |
| Removes measured WAND work, existing APIs, zero stored bytes | 4 | 4 | Both remove candidate pivot/sort/alignment and can skip optional decoding. A batches traversal; B avoids matrix work. Neither has measured local-interval overhead yet. |
| Small readable surface and bounded resources | 3 | 5 | A adds matrix clearing, masks, two interval levels and score replay. B uses one region and one per-candidate leaf array. |
| Other query/custom scoring compatibility and fallback | 4 | 4 | Both retain caller gates, two-term and high-clause paths. A's exceptional-domain WAND fallback weakens the new exactness guarantee; B needs an explicit canonical-sum fallback contract. |
| Concrete timing/correctness/memory/code proof | 5 | 5 | Both include full frozen suite, raw bits, controls, other query classes, index hashes, RSS and binary size rather than five-case-only acceptance. |
| **Total** | **20/25** | **22/25** | B is the initial base; acceptance remains conditional. |

## Required grafts and implementation clarifications

1. Keep B's physical `reader.max_doc()` and half-open `[lo, hi)` representation.
   Pass `max_doc`, not the existing `num_docs` local in BooleanWeight: deletions
   leave physical doc IDs above the live document count. Never add one to
   TERMINATED; cap an inclusive end at `max_doc - 1` only after proving max_doc>0.

2. Graft A's **tail reconciliation**: after shallow selection, a selected tail
   with stale actual position should be physically sought to at least the region
   floor before its bound/exhaustion is consumed. An actual TERMINATED result
   permits bound zero. `TermScorer.block_cursor().has_remaining_docs()` exists;
   no new public API is required. However, this predicate counts postings in the
   selected tail, not documents after a requested target. Seeking beyond every
   real doc in a nonempty tail does not change its remaining_docs to zero. Do not
   implement B's vague “no remaining postings” as an equivalence to this predicate.
   B's max_doc cap already prevents an infinite tail loop, so reconciliation is
   a simplification/tightening rather than evidence that its capped loop is wrong.

3. Keep original scorer ordinals stable. Store exact per-leaf f32 values, and
   produce accepted scores with an original-order f64 fold and one f32 cast.
   `BufferedUnionScorer::refill` visits scorers in incoming order and stable-retains
   survivors, so this is the actual exhaustive contract. Do not use the
   essential-first running sum as the published score. Preserve the original n
   in every ScoreSumUpperBound even after exhaustion.

4. Make prefix indexing explicit in code: a rejection before probing optional
   ordinal `order[j]` must include that term and every still-unchecked optional
   bound. Use an inclusive prefix or an exclusive prefix indexed at j+1. Never
   derive the remaining bound using total-minus-consumed subtraction. Nonnegative
   leaf replacements and at most n-1 nontrivial grouped additions are covered by
   the existing outward sum allowance; an omitted current term is not.

5. Resolve unsafe admission before any cursor mutation. Prefer B's exhaustive
   canonical SumCombiner fallback for NaN thresholds, unsafe normalization,
   nonpositive weights and exceptional global bounds, rather than A's existing
   WAND fallback. The existing WAND path reorders leaves and does not establish
   this new exactness promise in exceptional domains. Caller custom non-summing
   or f32 combiners must retain the existing actual-combiner branch. Using the
   canonical fallback is valid only inside the existing two capability-flag gate.

6. No separate cached-doc wrapper is necessary if the implementation only uses
   `scorer.doc()` as the actual loaded-position observation and stores explicit
   logical exhaustion where required. Choose one representation and document
   its temporal meaning; do not maintain two independently updated positions.

7. Freeze B's preference order initially as proposed; local sorting is an
   efficiency experiment, not part of the proof. Partition only at region entry
   initially; enlarging the optional prefix after a callback is safe after
   advancing every essential match at that document, but is additional hot-loop
   logic. Either choice is implementable; avoid combining order changes with the
   first algorithm acceptance. Every region must have hi>lo and discard its local
   certificates before another region.

## Cursor proof verified against source

Shallow seek advances the skip reader and invalidates loaded/cache state, but
does not update SegmentPostings.cur or its decoder array. Scoring or advancing
that stale position is invalid. SegmentPostings.seek can return the current or
next old decoded doc without loading. This shortcut is safe for the proposed
algorithm only when a moved shallow block implies every old decoded doc is
below the requested actual seek target. That implication holds when selecting
at lo and subsequently seeking at target>=lo: the skipped full block's inclusive
last doc is below lo. Thus do not physically seek below lo, and only skip physical
reconciliation when the known actual doc>=lo and shallow selection did not move
past its block. An essential advance across its selected block moves to a doc
at or beyond that block's exclusive end, hence outside the region bounded by the
minimum block end. No new-region bound may be used for earlier documents.

A clause already physically positioned beyond the region cannot match within
it; zeroing its local bound is safe and can be grafted from A. A stale earlier
doc is not such a proof. Optional clauses must never be permanently removed
merely because their present local bound/partition is optional.

## Acceptance risks and stop conditions

The strongest remaining uncertainty is performance: the minimum of all selected
128-posting block ends may produce very short regions for dense terms. Both
designs pay O(n) metadata work there; A additionally sorts locally and clears
leaf rows, while B scans essential ordinals per candidate. Global BM25 saturation
bounds alone may be too loose to fix this fragmentation. Do not widen a region
using a single-block optional bound without a new range certificate.

Required tests before accepting either shape: ordinal midpoint raw bits; deferred
optional full-block/tail cursor reconciliation; 127/128/129 posting boundaries;
optional-to-essential changes across regions; absent/exhausted/duplicate terms;
physical max_doc with deletions; monotonic threshold changes and strict equal
ties; configured/custom BM25 and fallback domains; custom combiners/high clauses.
Then retain full 1,227-query frozen and independent-oracle checks, balanced
before/after timing, query-class controls, post-profile, warmed RSS/code size,
and unchanged index hashes. Reject or narrow a broadly regressive route even if
the five expensive profiled queries improve. No claim of closing the port's
4.2x OR gap is justified until the measurements exist.
