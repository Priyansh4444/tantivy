# Results and acceptance

Accept the compact ClauseState change as an aggregate OR improvement, not a
universal query win. The frozen 103 three-or-more-term OR Top10 queries have
candidate/baseline ratios of mean query medians .947736 (AB) and .946919 (BA):
730.868 → 692.670 µs and 730.386 → 691.617 µs, 5.23–5.31% less elapsed time.
Their geometric mean ratios are .975999/.973639, a smaller 2.4–2.6% reduction.
All six round/order arithmetic ratios lie .9465–.9492; identical-binary arithmetic
ratios for this group lie .9939–1.0039 (candidate) and .9948–1.0038 (baseline).
Thus the group arithmetic gain exceeds the observed group self-control spread.
These are empirical controls, not confidence intervals or a universal guarantee.

66/103 queries win in both orders, 10 lose; 34 win beyond each query's maximum
observed TT/PP spread and one loses beyond it. Preserve that regression:
ID 593, `little brown jug`, 7.53% slower AB and 4.28% slower BA, with maximum
observed log control spread .03197. No corpus-tail exclusion or selective trimming.
The entire 1201-query Top10 suite falls 1.42–1.51% by arithmetic means (221.50 /
221.43 → 218.35 / 218.09 µs), but geometric ratios .9932/.9962 are close to
self-control variation. The supported causal claim is the affected OR group.

Negative scored routes have small differences: two-term OR is .52–.65% slower
by arithmetic mean, phrase .06–.35% slower, AND .64–.69% faster, TERM 0–1% faster.
These paths do not execute ClauseState. Retain those differences; code generation
and machine variability can affect untouched routes. In particular, AND COUNT
appears 14–15% faster despite not executing this engine. It is not credited to
compact metadata; an instruction-layout attribution would require a separate
causal investigation. There is no claim of exact performance neutrality on every
unaffected route. The functional contract is unchanged and all exact gates pass.

The fresh available-API fixed-port lane still favors lucene-rs by arithmetic
means: 1201 queries, candidate 218.74–219.40 µs vs port 168.08–169.13 µs,
ratio 1.293–1.305. OR3+ candidate 691.98–694.04 vs port 301.09–304.38 µs,
ratio 2.273–2.305. Across all ORs ratio 2.404–2.433. Geometric all-query ratios
.9757/.9782 favor the fork slightly; means emphasize expensive queries. This
unit does not establish overall superiority over lucene-rs or Java Lucene.
Port tracks a 1000-hit lower bound through its available API while our Top10
collector requests no totals; this is an explicit collector-work limitation.

The compiler/runtime/index/query guards pass. Native measurements use identical
prebuilt query ASTs, CPU 4 workers, CPU 6 controller, serial AB/BA requests,
10 warmups, 3 rounds and 16 samples per order. Main includes 87,912 cells and
1,406,592 samples; actual-port includes 29,304 cells and 468,864 samples. Every
run checksum is independently checked against exact dumps. Machine 1-minute load
ranges 2.65–5.10 main and 3.02–4.32 port. User processes were preserved; no own
build, test, compression, profile or preview jobs overlapped either timing lane.

1428 tests pass, seven remain ignored by the project. Ten whole-callback raw-bit
OR traces pass with coherence checks. Default and two configured BM25 profiles
each pass 1227 exact count/rank/score comparisons. Fresh configured proof is against
our independent exhaustive oracle; no fresh configured port or Java comparison.
Default output matches the retained fixed-port results and both exhaustive oracles.
Full 1M-document posting/position identity remains bound to the frozen payload.

The new binary is 2760 bytes smaller; .text is 3008 bytes smaller. Actual record
layout is 24 bytes; scratch arrays grow 776→1160 bytes at 32 terms (36n+8 vs 24n+8).
Scratch allocations fall 4→3, headers 96→72 bytes; no per-region allocations.
Warm process RSS is essentially unchanged in the primary lane (~307.5–307.6 MiB),
not proof of lower scratch memory or lower lifecycle RSS. Index files and format
are unchanged. No codec, pooled windows, public API or persistent metadata added.

Matched five-case post-profile has no lost samples and still concentrates in the
inlined BooleanWeight pruning kernel (86.35% self vs 89.12% in preserved baseline),
with bounds 5.91% and loading 2.32%. Percentages describe the changed sample mix;
they do not prove fewer instructions or isolate an exact cache-miss reduction.
Source-grounded compact-loop/cursor behavior and paired timings support this
mechanism, while more ambitious partition/window/index-impact ideas remain
separate experiments. Baseline profile binary identity is independently bound.
