# Independent parameter-bound source review

Reviewer: `/root/count_consumer/bm25_parameters_judge`, read-only final review
of production diff `7db890908` → `ab9682faa`; main equivalent `872c9f55d`.
Verdict: **approved; no new blocking defect found**. No builds, tests, indexing
or timing ran during this review. This source review supplements the executed
tests and is not a new runtime result.

The reviewer traced Searcher/custom-provider routes, every BM25 factory, boost
cloning, TermScorer selection setup, serialized pair selection, skip ceiling
consumption, complete blocks and loaded tails, phrase/regex/prefix and Boolean
WAND callers.

For finite positive matching frequencies, classic nonnegative components give
a ratio in [0,1]; native nonnegative inverses saturate at finite weight. Validated
k1=-0 with negative-infinite inverse has a separate exact constant-weight proof.
DEFAULT's classification-free cache loop has a .25 floor with positive finite
average: overflow may produce positive-infinite classic norm or zero native
inverse, without a NaN or negative component.

Invalid average, NaN/nonproved cache or nonfinite boosted weight yields global
infinity before internal cached-result, stored-pair or tail reuse. Exact DEFAULT
bits, matching arithmetic, positive equal average and finite nonnegative weight
gate stored pairs and ceilings. Serializer factories stay DEFAULT. Nondefault
complete blocks fall back globally; safe loaded tails reduce actual scores.

Supplied providers remain authoritative, classic snapshots retain classic
arithmetic, and per-field configuration belongs to the immutable outer handle.
The reviewed scope is finite-score pruning/ordering plus literal exceptional
scalar classification. Custom-NaN TopDocs equivalence remains uncertified.
