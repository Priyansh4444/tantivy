# OR execution improvement grounding

Root: /home/pronsh/Coding/playground/search
Production source baseline: tantivy-pr2937 at91fb3cacd (core unchanged from c0efbc99).
Implementation worktree: tantivy-or-maxscore-oct06, branch perf/or-maxscore-oct06.
Source inspiration: lucene-rs-idf-fix-oct06 e5d1f81 (Apache2 port), primarily
src/search/bulk.rs, src/search/searcher.rs, src/codec/postings_reader.rs.

Existing balanced packet: bench/lucene-rs-balanced-oct06. Same physical text payload,
default/native BM25, same compiler/nativeopt3fatLTO1codegen, frozen1227queries,
all counts/rankedphysicaldocIDs/rawf32bits/exhaustiveoracles match. Broad top10 port
~1.70x mean-query-median advantage; OR4.2x mean, with3+termcases4.86x versus2term2.8x.
Our COUNT~5.6x geometric and warmed RSS252vs323MiB. Index626MBvs692MB has capability
differences; existing compactness should be preserved. Port TOP10 counts1000 hits,
ours no total; comparator availableAPI scope, not identicalcollectorwork.

Baseline actual perf on5 slow 3+term OR cases is retained baseline-perf.data and
baseline-perf-report.txt; all batches checked against frozen verified results.
No dropped perf samples; 10875samples. ~64.92% selfCPU in block_wand,9.62%load_block,
8.41%block_max_score,7.19%seek, additional sorting and decompression. Warmup included;
this profile establishes an OR execution target, not a universal ceiling.

Current paths: boolean_weight::for_each_pruning routes exactly2TermUnion to
two_term_or_maxscore; longer summing f64 unions to block_wand. Custom combiner/f32
accumulator fallback preserved. block_wand_single_scorer optimized separately.
TermScorer exposes doc/advance/seek/size_hint/max_score/seek_block/last_doc_in_block/
block_max_score. Shallow advance can move skip-reader ahead of loaded block: do NOT
reuse an old block's bound after arbitrary seek. Missing/untrusted/unsafe bounds
already fallback to global. Completed tails may compute loaded exact bound; supports
nondefault query-specific native envelopes and custom BM25 statistics. Scorer max
and ScoreSumUpperBound handle nonnegative proof domain. Do not silence exceptions.

Pruning callback documents must be strictlyascending, thresholdsmonotonic; current
admission strictly score>threshold. Equal-score docID tie policy retained. Outputscore
must match exhaustive SumCombiner: individualf32scores accumulatedf64, f32once. Bounds
may add conservative rounding allowances; actualdocument scores must not. Consider
ordinal-preserving contributions to avoid changed summation ordering after partition.
Boosts, duplicated terms, sparse/absent/exhausted terms, tails/deletions, nondefault
parameters/statistics, custom/non-summing combiners and highclause counts remain valid.

First accepted unit should change read-time execution only: no new postingbits/no
schema/directory/API dependencies; unchanged compact index bytes guaranteed via hashes.
Avoid copying a wholesale engine, unbounded threadlocals, or multiplecodecchanges.
Need small state/scratch layout and common path guard/fallback for hostile scorebounds.

Architecture phases Ground/Sketch/Agree(defaultproceed)/Implement/Scrap. Arena phases
Frame/Fanout/Crossjudge/Pick/Graft/Verify. Two structurallydistinct designs first;
fresh judge plus root read all packages before one implementation. Rubric:
1. Correct raw scores/count/rank and mathematicallysafe pruning, explicit edgecases.
2. Eliminates measured WAND cost plausibly using existing APIs and zero new diskbytes.
3. Small readable surface, bounded allocation/state, no unnecessarylayers.
4. Maintains other queries/customBM25/customcombiner compatibility and definedfallback.
5. Concrete baseline/treatment gate and enough timing/memory/code-size proof to accept.

Deliverable candidate design only (usage/signatures/modulemap/pseudocode/rationale),
no code edits, builds, benchmarks or enginechanges. Root will combine and delegate
one implementation after tracing. Fork-only publication authorized; no TantivyPR.
