# Native finite-profile pruning — October 5, 2026

Kept production candidate **b50e2aef3abd77a43fbad56dbaecf2f6c0540a50** improves
0.9/0.4 ranked search by **29.5%** against accepted
**e13303b3aaa8c15815ab5ea951a3449111438e81** in both process orders on the frozen
aligned Wiki1M/Wiki20 workload. It has costs: DEFAULT ranked search is about
**2.2–2.4% slower** against that baseline, and some small bound-using queries
regress about 6–12%. The correctness gates pass. This is a bounded improvement,
not completion of the all-query speed target or all Lucene public APIs.

The unchanged external Lucene ruler still finds configured-profile ranked
losses. 0.9/0.4 is near parity overall, while 2.5/1 remains 25.8–36.9% slower.
No language choice guarantees a speed result, and no older-layout numbers are
transferred to these artifacts.

## Change and proof

The native format11 writer stores a real pair maximizing the rounded DEFAULT
TF-times-inverse input. The candidate conservatively encloses the input under
an eligible nondefault native query cache, so later complete blocks can be
rejected before decoding postings. It retains actual reader selection-average
provenance, selected-only TF255 expansion to u32MAX, immutable fixed-owner
policy, outward binary32/binary64 rounding, and the literal native score formula.
DEFAULT stays the first branch. Scorers that receive no bound requests do not
build an envelope. Unsupported
or unsafe domains, including b=1's infinite inverse, retain global bounds and
the existing loaded/unloaded tail policy. Scores and index bytes do not change.
The public different-weight/reader cache ownership fix remains separate.

Integrated verification passed 1,376 normal library tests with 7 ignored and one
fixture generator filtered; release BM25/term/cursor/serializer groups passed.
The unchanged frozen 330-query/16-configuration suite has no structural or raw
score failures under its existing gate. All three strict Wiki20
COUNT/top-ten ID-set/tie/exhaustive gates pass, with exactly matching binary32
bits for all 1,913 common top100 scores per profile. A separately compiled
native opt3/LTO probe of the final production source matches 384 exact M/R/C/bound
vectors and covers 245,760 literal scores; 16 b=1 cases reject conservatively.
Independent IEEE32/IEEE64 rational quantization and codec/owner fixtures support
the arithmetic and actual Dense/FOR/unloaded-block certificate.

The generic build layout receipt grew TermScorer 2088→2104 bytes. The exact
native candidate receipt is 2128 bytes; no same-target native baseline layout was
measured. Do not compare these different build configurations. Activated finite
bounds retain an immutable 256-entry old cache and enclosure; the cold work and
allocation count matter, especially for small terms.

## Isolated latency change

Immutable uninstrumented native binaries use the same target-cpu=native,
opt3, LTO and disabled overflow-check profile. Supplemental paired runs use
CPU 4, 40-second warmup, 256 samples/query, seed 23 and both process orders. All own
build/index/hash/test jobs were idle; user background processes remained active.
This paired ruler adds equal protocol checks to both labels, so its absolute
latencies must not be transferred to the original Tantivy/Lucene ruler.

Values below are candidate/accepted-baseline geometric means across 20 queries;
less than 1 is faster. Every raw sample and per-query median is retained.

| Profile/mode | Baseline first | Candidate first |
| --- | ---: | ---: |
| 0.9/0.4 TOP_10 |0.705322 |0.704735 |
| DEFAULT TOP_10 |1.023981 |1.021922 |
| DEFAULT COUNT |1.002468 |1.003349 |
| 0.9/0.4 COUNT |1.007410 |1.007903 |
| 2.5/1 TOP_10 |1.006684 |0.987728 |
| 2.5/1 COUNT |1.001374 |1.007758 |

The paired helper passed 57 fake protocol/lifecycle fixtures and six real
same-binary controls before it was frozen. Their aggregate ratios ranged
0.988661–1.004665; individual query variance was larger. B1's DEFAULT cost is
reported explicitly, not dismissed because counters are unchanged.

## Unchanged Lucene ruler

Lucene 10.4.0 is pinned at 9983b7ce7fdd04f4d357688fb85c14277c15ea8d. The configured
Java classes, query suite, aligned index pair and original timing/memory helpers
are unchanged. Only the Tantivy binary and output paths replace those in the
[aligned baseline](../2026-10-05-native-profiles/README.md) commands. Actual
profile getters acknowledge exact parameter bits and native N 917578 / TTF 294827020.
Native scoring uses scale 1 and physical collection statistics, without matching
or converting scores to a surrogate convention.

| Profile/mode | T/L, Tantivy first | T/L, Lucene first | Lower T medians (first/second) |
| --- | ---: | ---: | ---: |
| DEFAULT COUNT |0.749409 |0.741112 |20/20 |
| DEFAULT TOP_10 |0.480675 |0.490635 |18/19 |
| 0.9/0.4 COUNT |0.747572 |0.751179 |20/20 |
| 0.9/0.4 TOP_10 |1.000555 |1.034113 |10/10 |
| 2.5/1 COUNT |0.761373 |0.732503 |20/20 |
| 2.5/1 TOP_10 |1.368604 |1.258441 |5/7 |

DEFAULT misses were `the american` in both initial orders, and `+the +saxophone`
in the first. Four justified control repeats used the same unchanged ruler:
accepted e133 won 20/19 medians, candidate b50 won 20/20. The accepted binary's
remaining miss was also `the american`. These near-parity queries vary under
current user load; all initial failures and repeats are retained. The repeats
are not substituted into the table or used to claim a stable all-query win.
The paired DEFAULT regression remains documented.

The 0.9/0.4 losses remain the seven common single terms and three dense Boolean
queries in both orders. Large improvements versus Tantivy's previous global
bounds do not establish a Lucene win on those queries. The 2.5/1 original
execution remains global; differences versus old external timings are not an
isolated candidate speedup.

## Execution and efficiency

Separate untimed atomic instrumentation compares against the retained 0eb
counter reference. For 0.9/0.4 `of`, full decodes fell 5928→1283, actual term scores
758787→164227 and 4645 complete blocks were skipped. `the` fell 6442→1384 decodes
and 824698→177274 scores. DEFAULT and 2.5/1 retain identical original 19 counters;
COUNT builds no transform. All 240 rows, repeats, counter accounting, exact
counts and exhaustive checks pass. The instrumented binary is separately
identified and is never timed or used for RSS claims.

Both indices retain identical bytes and hashes: Tantivy 626,102,393 versus
Lucene 626,810,511, a 708,118-byte (0.113%) storage difference. Complete physical
rows, all 1,642,896 terms, 126,703,012 postings/TF and 294,827,020 positions retain the
previous exact identity proof. All 48 older retained artifact hashes are unchanged.

| Warmed query RSS MiB | Tantivy | Lucene |
| --- | ---: | ---: |
| DEFAULT |18.871 |564.492 |
| 0.9/0.4 |18.762 |260.195 |
| 2.5/1 |18.598 |1062.328 |

These are warmed process snapshots including mapped pages under default JVM heap
and GC behavior. They are not isolated incremental RSS deltas, peak/indexing
memory, constrained-heap measurements, concurrent throughput or universal limits.

## Evidence and next unit

[manifest.json](manifest.json) hashes every member of
[evidence.tar.gz](evidence.tar.gz): exact final source patch, native build
provenance, all gate outputs, independent compiled probe/oracle sources, vector
inputs, raw sampled measurements, protocols/counters, interrupted run artifacts,
completed controller source and the read-only next-design arena. Executables and
large indices stay identified by hash and are not embedded. Historical absolute
paths are provenance, not promises of portable command paths.

An earlier tool session interrupted a four-result sequence; its partial command
receipt and raw outputs remain separately labeled and do not drive acceptance.
The full fresh 12 paired runs, 15 Lucene/memory runs and four DEFAULT controls
completed with a durable controller. Controller source is now preserved in the
evidence rather than relying only on temporary files surviving a session restart.

The next design review defers production b=1 policies until actual strict
`M<H` eligibility and threshold-effective unloaded reach are observed. All
three proposed same-format designs share this information limit; tighter
ratios cannot repair a failed exclusion. An isolated observational probe is
being prepared. No B2 bound has been implemented or accepted. It also cannot
repair the remaining finite-profile losses or all unrelated feature families.
All 47 public-API/feature families remain incompletely verified; Java facade
and Lucene file interchange are absent.
