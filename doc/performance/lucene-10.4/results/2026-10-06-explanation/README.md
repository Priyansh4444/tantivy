# Deferred BM25 explanations — October 6, 2026

Search setup now stores the IDF value and rounded display statistics, and builds
an owned explanation tree only when requested. Single-term statistics stay inline;
multi-term and explicit metadata are immutable and shared when cloned or boosted.
This removes eager tree construction and deep cloning from ordinary search setup.
Accepted source is `176410bb56a4d711b403a959fbafddf5b8d04847`, identical across
all tracked source/Cargo/config files to tested candidate `7322109659d1f163fbd24f804b2cff906bbdaa1b`.

## Measured efficiency

The unchanged standalone native release probe retains 1,000 outputs per operation
and repeats each operation three times. Setup and retention-vector allocation are
outside the counter. These are caller-thread allocation calls and requested-byte
totals, not peak/live memory, RSS or query latency.

| Operation | Before | After |
| --- | ---: | ---: |
| BM25 weight size |112 bytes |56 bytes |
| Native term scorer size |2128 bytes |2072 bytes |
| Single construction calls / requested bytes per output |2 /1360 |1 /1040 |
| Three-term native phrase construction |6 /2560 |3 /1132 |
| Tested single clone or boost |1 /160 |0 /0 |
| Tested native phrase clone or boost |4 /720 |0 /0 |

Classic phrase and no-explanation construction/clone paths remain unchanged.
On-demand explanation allocation counts are unchanged, but requested bytes rise:
single native/classic 1440→1600 (+11.1%); three-term native phrase 2000→2560
(+28%). New child vectors grow to capacity four instead of the original cloned
length. This cost is retained in the historical P021 evidence. The separately
verified [exact-size follow-up](../2026-10-06-explanation-capacity/README.md)
restores the original eager per-explain byte totals while retaining setup savings.
Explicit-constructor allocation was not measured.

## Query latency and limits

The frozen supplemental paired ruler compares the candidate with retained b50:
40-second warmup, 256 samples per query, CPU4, 20 queries and both process orders.
All owned CPU jobs were idle during timing; user background load stayed active.
Ratios below are candidate/baseline geometric means; smaller is lower latency.

| Profile/mode | Baseline first | Candidate first |
| --- | ---: | ---: |
| DEFAULT TOP_10 |0.988232 |0.979781 |
| 0.9/0.4 TOP_10, initial |1.000488 |1.015725 |
| 0.9/0.4 TOP_10, justified repeat |0.991785 |0.988773 |
| DEFAULT COUNT |0.988466 |1.005979 |
| 0.9/0.4 COUNT |1.007450 |0.999065 |
| 2.5/1 COUNT |0.997999 |0.989413 |
| 2.5/1 TOP_10 |0.985791 |0.996585 |
| 0.9/0.4 TOP_10, same b50 binary |1.011696 |1.060548 |

The initial reverse 0.9/0.4 run had sharply higher absolute latencies and mixed
per-query outliers. Initial runs remain intact; favorable repeats do not replace
them. Same-binary controls show substantial load variation, including per-query
differences up to 25.3%. Latency broadly holds within this observed variation;
there is no significant incremental latency claim. The deterministic size and
allocation reduction justifies this efficiency keep. A loaded-machine comparison
cannot establish an all-query speed win.

These are not fresh Tantivy/Lucene timings. Supplemental absolute times cannot
be transferred to the [original Lucene ruler](../2026-10-05-native-pruning-finite/README.md).
Its DEFAULT advantage, near-parity 0.9/0.4 result and slower 2.5/1 result remain
separate retained observations at b50. This change adds no pruning or index-format
policy and leaves both retained indices unchanged. Full 47-family/public-API
parity and the all-query speed target remain unfinished.

## Correctness and evidence

All 42 frozen full-tree cases match the original eager implementation in debug
and exact native release, including owned output independence, provider call/error
order, signed zero, nonfinite bits, duplicate terms and ordered native f64 phrase
accumulation. The full normal library suite passes 1,381 tests (seven ignored;
one format-writing fixture filtered). The BM25 target passes 27 tests in debug
and native release. All eight unchanged native adapters build with opt3/LTO/native
CPU and retained adapter locks. The ignored root test lock is separately retained
in test provenance; worktree creation does not copy it. Root independently passes the frozen 330-query/16-case
matrix and all three strict Wiki20 gates, including 1,913 exact f32 score comparisons
per profile and unchanged source/binary/index guards.

[manifest.json](manifest.json) hashes every member of [evidence.tar.gz](evidence.tar.gz):
original/reviewed source and patch, frozen fixtures, allocation probes/locks/raw
results, build/test logs and provenance, regression outputs, all 16 raw paired and
control runs, frozen helper and design/decision records. Root independently
recomputed all sample medians, ratios and hashes. Large baseline packages, build
targets, binaries and indices are identified by provenance rather than embedded.
Absolute paths are historical provenance. Decision `P021` records the bounded keep.
