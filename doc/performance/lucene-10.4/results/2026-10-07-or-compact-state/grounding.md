# Next OR improvement: grounded design task

Current production fork base: 6edfb101e7df20fb3258a283ac1070f1be709c51,
with core 412bdc245640b28f98d6e17ba91c96d2c1112b24. Isolated worktree
search/tantivy-or-refinement-oct07; output search/bench/lucene-rs-or-refinement-oct07.

The exact current native baseline is the prior candidate worker, SHA
6df0b41330dfb2a0b65cff4734b2481101aa192c56e684cefd1e113eb9e181ec.
Its retained trace (same source, profile, physical payload) is copied to baseline
files. Post-MaxScore causal profile: 89.12% self in inlined for_each_pruning,
4.34% block_max_score, 2.23% load_block. No samples lost. Annotate evidence has
2.56% at 29a8b0 inc rdx and 1.03% at 29a8d9 zero store in region-bound cleanup (followed by prefix construction); exact
instruction attribution is limited by optimized code and sampling. This is a
hypothesis signal, not a speed claim. Scoring, region preparation/ordinal loops, bounds and threshold
checks dominate remaining code. The annotated zero stores are not evidence
that candidate contribution clearing dominates; do not conflate these loops. Baseline hot instructions/complete disassembly
are retained for review.

Source: private or_maxscore on admitted 3..32 actual TermScorers, f64-sum flag.
Static pruning preference order; original scorer storage never reordered. Four
O(n) scratch arrays. Every candidate clears n contributions, visits essentials,
probes optional terms with inclusive bounds, then folds n slots in ordinal order.
One partition per half-open region; monotonic threshold may grow within region.
Cursor proofs: shallow selection can leave stale decoded blocks, actual seek
before scoring/advancing; tails reconcile at floor. Range uses physical max_doc.
Unsafe numeric domains use exhaustive SumCombiner before cursor mutation.
Existing 9 exact-trace unit tests plus public raw-bit/deletion/configured tests.

Port source: search/lucene-rs-idf-fix-oct06/src/search/bulk.rs MaxScoreBulkScorer
(roughly lines51..413). Uses window bitmaps + buffered scoring, special single
essential path, thresholds that trigger repartition; richer impact hierarchy;
pooled WindowScratch array payload80.5KiB (plus separate buffers). Adopt mechanisms only with exact incoming f64 fold,
not order-dependent port accumulation. Avoid format/index additions in this unit.

Primary current gap from latest complete comparison: candidate/fixed port ratio
of mean medians ~1.30 overall, ~2.34 long OR, ~2.60 2term OR. Two-term execution
is excluded from first next-unit scope. Prior new route doubles long OR speed
but still has loop cost. Frozen 1227 correctness/1221timing queries, 1Mdoc index,
full payload identity and defaults/configured gates already exist.

Candidate package: caller usage first, private types/signatures/module map,
pseudocode, correctness/cursor/arithmetic proof, expected overhead and rejection
cases, and rationale. No production edits/builds/benchmarks yet. At least two
structurally distinct candidates. Default proceed after root synthesis.

Rubric (1..5 each):
1 Exact original-order f32-leaf/f64-final score + conservative pruning proof.
2 Direct causal reduction of measured loop work, realistic benefit/risks.
3 Compactness: no on-disk bits, no per-region allocation, small scratch.
4 Narrow interface and readable state transitions; no new temporal coupling.
5 Verifiable controls, exact midpoint/tail/duplicate/configured coverage.

Reject any design relying on unverified f64 reorder or subtractive max bounds.
Acceptance: new exact-default/configured gates, unit/integration checks,
full AB/BA schedule + same-engine controls with stable source/binary/index hashes,
post-profile, scratch/code/RSS accounting. Pilot selects attempts, never acceptance.
No own heavyweight job overlaps latency. Preserve user/background processes.
