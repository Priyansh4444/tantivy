# Verified COUNT and storage improvements — October 3, 2026

Production source: `dcfcb2d8a9f8716f26e26fe77dc623af90f2135c`. On the frozen Wikipedia 1M / 20-query suite,
Tantivy leads Lucene 10.4 on every COUNT and TOP_10 query in both measured process
orders. The new index is smaller, and the warmed Rust query process uses less
resident memory with the default JVM configuration. Small individual margins
remain sensitive to the background workload; this is a workload-specific result.

## Final comparison

| Surface | Forward | Reverse |
| --- | ---: | ---: |
| COUNT Tantivy/Lucene geometric mean | 0.7492 (1.33× faster) | 0.7699 (1.30× faster) |
| TOP_10 Tantivy/Lucene geometric mean | 0.4953 (2.02× faster) | 0.4988 (2.00× faster) |
| Queries with lower COUNT median | 20 / 20 | 20 / 20 |
| Queries with lower TOP_10 median | 20 / 20 | 20 / 20 |

`+new +york` COUNT, previously order-sensitive, measures
502.9 vs 575.5 µs forward
(ratio 0.8739), and
536.3 vs 548.3 µs reverse
(ratio 0.9781). The reverse margin is still narrow.

Both persistent processes share CPU 4. Each run uses 40 seconds of warmup and
256 samples per query, randomized query order with fixed seed 23, alternating
engines, and median pipe-request latency including parsing and collection.
Separate runs reverse process creation order. Query cache is disabled.
BM25 k1=1.2, b=0.75, population 1,000,000, token total 294,826,965 and equivalent
2.2 score scale are matched as described in [the methodology](../../README.md).

Background processes were left untouched as requested. The raw provenance
records load averages and artifact hashes. These are loaded-machine observations,
not quiet-machine guarantees. Do not compare absolute timings or geometric means
with the earlier report as though background load were held constant.

## Accepted changes

1. `85334cdb1`: frequency PFOR stores up to seven high-byte exceptions in the
   existing skip-header byte. It saves storage; no CPU gain is claimed for the
   codec alone. Decoder increment-before-patch and the rare-header size branch
   retain simpler implementations at measured performance parity.
2. `c915b4b8e`: medium-density terms alternate inserts into two independent
   masks, using one shared fill routine and a cached density selector. In the
   isolated both-order comparison, `+new +york` improves 5.4–7.2% and
   `+united +states` improves 7.9–10.2% against the frozen Rust binary.
3. `dcfcb2d8a`: four inserts share one horizon check across the same two masks.
   Against the preceding combined build, `+new +york` improves 3.5–3.7% and
   `+united +states` improves 5.2–6.4%. Dense `+the +of` is 0.6–2.0% slower in
   that paired comparison, while remaining ahead of Lucene in the final suite.

A fresh direct final-versus-frozen-Rust comparison measures `+new +york`
candidate/baseline 0.8857 forward and 0.9142 reverse
(11.4% / 8.6% lower latency).
Each paired run uses 20 seconds of warmup and 512 samples for five counting
cases. Reverse JSON labels swap the binaries; the ratios here are normalized.

## Storage and query-process memory

| Index | Bytes |
| --- | ---: |
| Frozen Tantivy format 8 | 641,173,024 |
| New Tantivy format 9 | 622,748,560 |
| Lucene 10.4 | 626,810,511 |

The new index saves 18,424,464 bytes (2.87%)
against the frozen Rust index and is 4,061,951 bytes
(0.65%) smaller than Lucene. It was created by
merging the frozen source index, without retokenizing or removing documents:
one segment and 1,000,000 documents. Full-frequency payloads shrink from
59,318,368 to 40,910,098 bytes. Position, fieldnorm, fast-field and stored-document
payload hashes match the source exactly; format-version footers differ.

| Warmed process | RSS, MiB | Process high-water RSS, MiB |
| --- | ---: | ---: |
| Tantivy | 14.86 | 14.86 |
| Lucene / default JVM heap | 616.67 | 617.38 |

Memory is a Linux process snapshot after running COUNT and TOP_10 over the same
suite for 20 seconds per engine, serially. Mapped index pages are included.
This measures query processes with the default JVM heap; indexing throughput,
indexing memory and deliberately constrained JVM heaps are not measured.
RSS also depends on GC phase and how many rounds fit the warmup: an earlier
snapshot of the preceding combined build measured 15.07 MiB for Tantivy and
364.81 MiB for Lucene. That raw snapshot is retained rather than treating the
latest process-memory ratio as a fixed property of either engine.

The new reader passes the full correctness suite on both the format-8 and
format-9 indexes. The frozen format-8 reader rejects new segment files through
`IncompatibleIndex` before interpreting their frequency headers. See the
[format documentation](../../../../frequency-pfor.md).

## Correctness

The final source passes 1,327 normal library tests (8 ignored), all eight release
query-regression configurations, and the comparator's six fixtures. The query
gate covers 3,328 top-k, 1,664 COUNT and 832 literal-match comparisons across
multiple segments, deletions, missing fields, fieldnorm settings, score ties,
boosts, phrases and phrase prefixes. Logs are adjacent to this report.

Fresh cross-engine gates pass all 20 queries on both index versions: exact
counts, identical top-ten external-ID sets, and optimized results matching
exhaustive alive-document scoring. Rank inversions are accepted only within
ties in both scoring models. Maximum relative shared-top-100 score difference
is 2.51778687e-07, below the 2e-6 tolerance. TOP_10 timing replies contain only
the protocol marker; correctness is established by the independent validator.

## Search trail and limits

Batch-of-four same-bucket insertion, unconditional scratch copying, duplicate
const fill routines, and four quarter-window lanes were rejected or refined.
Forced block-load inlining was also rejected in the preceding report. The
accepted route keeps one fill implementation and two borrowed masks. More
unrolling would buy diminishing branch savings and add register/code costs.
The next substantial target is packed-doc decoding, which accounted for about
15% of COUNT cycles in the captured profile; changing its representation needs
a fresh speed/size experiment rather than a language-based assumption.

The working decision log remains outside the fork at
`search/bench/oct03-decision.tsv`. Raw paired samples for the rejected and
accepted attempts are retained here. The full-suite lead, smaller index and
green gates complete this pass; they do not prove that Rust wins every Lucene
workload, corpus, query type or hardware configuration.

## Per-query median ratios

Values below 1 favor Tantivy. Raw samples and artifact hashes are adjacent.

| Query | COUNT forward | COUNT reverse | TOP_10 forward | TOP_10 reverse |
| --- | ---: | ---: | ---: | ---: |
| `the` | 0.7001 | 0.7040 | 0.6981 | 0.7404 |
| `of` | 0.7789 | 0.7619 | 0.3879 | 0.3908 |
| `and` | 0.7276 | 0.7749 | 0.3922 | 0.3969 |
| `united` | 0.7289 | 0.7201 | 0.6854 | 0.7213 |
| `states` | 0.7687 | 0.6635 | 0.6328 | 0.6110 |
| `american` | 0.7658 | 0.7419 | 0.5628 | 0.5628 |
| `york` | 0.7243 | 0.6589 | 0.6108 | 0.6488 |
| `saxophone` | 0.6723 | 0.5864 | 0.8110 | 0.7703 |
| `+the +of` | 0.8410 | 0.8794 | 0.2946 | 0.2893 |
| `+united +states` | 0.7477 | 0.7867 | 0.2241 | 0.2096 |
| `+new +york` | 0.8739 | 0.9781 | 0.3091 | 0.3123 |
| `+the +american` | 0.8015 | 0.8516 | 0.3931 | 0.4036 |
| `+the +saxophone` | 0.6063 | 0.8779 | 0.6646 | 0.6985 |
| `the of` | 0.8193 | 0.8165 | 0.2879 | 0.2837 |
| `united states` | 0.7073 | 0.7300 | 0.2285 | 0.2249 |
| `the american` | 0.7628 | 0.8086 | 0.5260 | 0.5092 |
| `"united states"` | 0.6963 | 0.6950 | 0.7852 | 0.7717 |
| `"new york"` | 0.8859 | 0.8828 | 0.7225 | 0.7275 |
| `+griffith +observatory` | 0.8319 | 0.8798 | 0.8532 | 0.9854 |
| `griffith observatory` | 0.6187 | 0.7144 | 0.6897 | 0.6645 |
