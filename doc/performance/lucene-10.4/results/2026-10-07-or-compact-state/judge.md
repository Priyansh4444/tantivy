# Independent design judge

I read the grounding and all three designs end to end, then checked the current
`or_maxscore` implementation, `TermScorer` shallow selection/bound methods,
`SegmentPostings` position methods, and the port's wrapper/partition/direct-driver
implementation. These scores rank experiments; they do not establish a speedup.

| Criterion | A: compact clause state | B: adaptive partition/direct driver | C: sparse contributions |
| --- | ---: | ---: | ---: |
| Exact arithmetic and conservative pruning | 5 | 5 | 5 |
| Causal reduction of measured work | 4 | 3 | 2 |
| Compactness | 4 | 5 | 5 |
| Narrow interface/readable state | 4 | 4 | 5 |
| Verifiable controls and edge coverage | 5 | 5 | 5 |
| Total | **22** | **22** | **22** |

## Recommendation

Use **A's compact clause metadata as the next narrow measured base**, with the
existing from-zero global-prefix scan. The equal totals conceal different risks:
A has the strongest connection to the presently observed region-loop work;
B has a plausible candidate-count benefit but needs evidence of useful threshold
crossings; C targets a clear source-level operation whose dominance the retained
trace specifically does not establish. This selection is conditional on exact
mirroring and the full measurement gates, not on the root's stated preference.

For this first variant, replace `order` and ordinal-indexed `local_max` with the
sorted `ClauseState` records, cache fixed global maxima/costs, mirror actual docs,
and fuse out-of-range cleanup with prefix construction. Keep contribution clearing,
original-order score replay, per-region partitioning, density policy, region
boundaries, and strict cutoff semantics unchanged. These edits form one coherent
state-layout experiment. They do not require the new persistent `GlobalPrefix`.

**Gate monotonic global-prefix caching separately.** Its proof is sound under
the callback contract, but it introduces another temporal state and independently
removes repeated arithmetic. Do not hide its effect inside the first metadata
comparison. After a successful metadata pilot, retain that exact source/binary
as the intermediate baseline and pilot the prefix-cache graft against it. If
time permits only one accepted variant, prefer a complete narrow measurement to
an inseparable collection of plausible optimizations. Rejection of the first
variant is useful evidence; do not add B or C to rescue it without a new attempt.

## Why A is admissible

The source supports the proposed doc mirror: actual `seek` and `advance` return
the new doc, while `seek_block` selects block metadata without replacing the
decoded position. The inspected `block_max_score` route may construct a numeric
envelope but does not advance postings. `size_hint` is fixed postings length,
not remaining length. A record therefore can contain both the current decoded
doc and immutable query metadata without changing cost decisions.

Crucially, mirroring must preserve a stale decoded doc after shallow selection.
It must not eagerly substitute a skip-reader boundary or infer exhaustion from
remaining-doc metadata. The existing tail reconciliation is an actual seek;
record the returned doc there as at all other mutation sites. Preserve the
baseline sequence concerning the previously selected `last_doc` as well, rather
than silently combining this layout experiment with a different boundary policy.

The original scorer vector remains in incoming ordinal order. Pruning ranks index
records, each record retains its original scorer/leaf ordinal, and published
scores retain the exact incoming-order f32-leaf/f64 fold. Bounds retain their
existing addition order and `ScoreSumUpperBound(original n)`. Fusing cleanup with
prefix construction is safe because each prefix addition uses only that record's
already-cleaned bound and the preceding prefix value.

The density heuristic must still inspect *live* remainder docs each region.
Caching costs is safe; caching the selected widening policy or a permanently
live suffix is not equivalent. A running minimum of raw global-prefix costs is
valid only in the separately measured prefix-cache variant. In the first variant,
prefer the current explicit density predicate over additional derived state.

## Required proof/test grafts

1. Make cursor mutation own the mirror update categorically: private helpers or
   record methods for actual seek/advance, with no mutable scorer escape. Add
   debug coherence checks around shallow/bound selection and region transitions,
   and retain the existing stale-tail/deferred-optional tests. Debug checks should
   be absent from release measurement, rather than becoming production loads.
2. Add the A fixture where a high-cost remainder exhausts and the live density
   choice changes. Compare full callbacks/raw score bits to the independent
   buffered union; verify this fixture exercises the relevant branch rather than
   relying solely on final Top-K equality. A test-only diagnostic can establish
   branch coverage without adding counters to the measured binary.
3. Measure `size_of::<ClauseState>()` and array payload explicitly. The proposed
   24-byte record gives 1,160 bytes at 32 terms, versus 776 bytes presently: this
   is a small **increase**, not a memory reduction. One fewer allocation does not
   imply lower RSS. Record code size, warm RSS, allocator/header accounting, and
   unchanged index hashes honestly.
4. Preserve every existing midpoint, original-order permutation, duplicate,
   32-term, boundary/tail, deletion/multisegment, unsafe-numeric, and configured
   gate. The exact original score fold is mandatory even if an alternative
   execution-order sum happens to pass the frozen corpus.
5. For the later prefix-cache graft, test that raw certification stays monotone
   while selected density widening can still revert to zero as the remainder
   changes. Assert no local partition or block certificate persists into a new
   region. Compare the prefix boundary/raw arithmetic with the from-zero variant
   at equality and adjacent f32 thresholds.

## Assessment of the alternatives

**B is a worthwhile later independent experiment.** Its conservative prefix gate
and shrinking essential suffix have a valid candidate-completeness proof. Advance
all matching scorers in the old essential set before demotion; reconcile newly
essential scorers at the next region. Using `>=` for the next threshold gate is
correct for this fork's `bound <= threshold` optional admission. Keeping score
replay intact distinguishes the design from copying port accumulation order.
However, the retained samples do not quantify threshold promotions or the fraction
of single-essential candidates, and split kernels can grow instruction footprint.
Do not graft its new execution states onto A now. Its test-only candidate/promotion
diagnostic is the appropriate way to decide whether to try it next.

**C has the smallest semantic change but weakest current causal evidence.** Its
membership mask prevents stale-leaf reads, and increasing set-bit traversal
preserves the order of actual additions. Removing positive-zero additions leaves
the nonnegative admitted-domain fold unchanged; actual zero matching leaves may
remain recorded. The 32-term shift edge and early-pruned/reset paths need direct
tests. Dense 3-term cases can plausibly lose to bit bookkeeping, and the annotated
zero stores belong to another loop. Keep this as its own later variant; do not
attribute any disappearing region-cleanup sample to sparse contribution replay.

## Acceptance and stopping rule

Keep the specified exact default/configured gates and the full frozen AB/BA
schedule with same-engine controls, stable source/binary/index hashes, all raw
samples, and no overlapping own heavy jobs. Inspect each long-OR regression,
not only the aggregate; unaffected two-term/COUNT/other query routes remain
controls. The profile must explain the final mechanism rather than stand in for
latency evidence. A few-percent pilot movement under background load is not an
accepted win. Reject layout growth without a reproducible benefit, and publish
the retained rejected attempt as such rather than claiming all three designs
were incorporated or that the remaining port/Lucene gap is closed.
