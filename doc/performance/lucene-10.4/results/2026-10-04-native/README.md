# Native Lucene comparison — October 4, 2026

Source **7db890908f1cc8422905102da7740733fe3f22f6**, pinned Lucene **10.4.0**.
Tantivy has lower median latency on all 20 frozen Wiki1M queries for both COUNT
and TOP_10, in both measured process orders. This comparison uses each engine's
native collection statistics and raw BM25 1.2/.75 scores, without overrides or
score scaling. It also corrects the historical 55-token analyzer difference.
Full Lucene feature/API compatibility remains unfinished.

| Measurement | Tantivy process first | Lucene process first |
| --- | ---: | ---: |
| COUNT T/L geometric mean | 0.76935 (1.30× faster) | 0.78807 (1.27× faster) |
| TOP_10 T/L geometric mean | 0.47828 (2.09× faster) | 0.54941 (1.82× faster) |
| Queries with lower COUNT median | 20/20 | 20/20 |
| Queries with lower TOP_10 median | 20/20 | 20/20 |

The narrowest COUNT lead is `+new +york`: T/L 0.9413 and 0.9365.
The narrowest TOP_10 lead is `+griffith +observatory` forward (0.9303)
and `the american` reverse (0.9044). These are observations under background
load; small margins and the process-order variation remain load-sensitive.

## Equivalent corpus and native correctness

The frozen transformed JSONL contains one million articles, 1,831,702,900 bytes,
SHA-256 `2b630549676f1c58a579017b6cd949e25115fe63989f9e75cadadc5c1a1a8238`.
Its domain is lowercase ASCII words and spaces; longest word is 219 bytes.
The explicitly registered `wiki_ascii_lucene` analyzer accepts words through
255 bytes, matching the pinned Lucene analyzer for this domain. This does not
implement arbitrary Unicode StandardAnalyzer behavior. The [replay protocol](../../configured-wiki.md)
includes small boundary fixtures and the complete comparison commands.

Both indexes contain one million documents and one deletion-free segment.
Full sorted term/DF/TTF comparison passes for **1,642,896 terms**, with field
population **917,578** and token total **294,827,020**. Canonical term-table SHA:
`dc15193a1ec1d93b778bf9cb0caedd1e71129180af0a1ea0e04b0f742c6c4d20`.
All stored external IDs, u64 sort values and encoded norm bytes match for every
document; canonical map SHA:
`31e2b88a36a6ed3ebdf5b2b8df48c6a1a0fa48fe601ba5f3a465e56bf454ed3d`.
The full TSVs remain in local `search/bench/native-v11-full-logical-oct03`;
[full-logical.json](full-logical.json) records hashes and exact counts.
This full comparison does not hash every cross-engine posting position.
Small positional fixtures and independent query gates provide additional coverage.

The latest native validator passes all 20 Wiki queries: exact hit counts,
identical top-ten external-ID sets, permitted order differences confined to ties
in both models, and optimized Tantivy results matching exhaustive alive-document
scoring. The maximum relative common-top-100 score difference is
`5.71791025e-8`, below the unchanged `2e-6` tolerance.
The [raw dumps](wiki-correctness.json) retain both models and tie evidence.
TOP_10 timing replies are protocol markers, so these checks establish ranked
correctness separately from timing.

The frozen direct-AST differential also passes: **16 configurations, 330 queries,
zero structural and zero native raw-score failures**. It covers sparse/missing
fields, multiple segments, deletions, explicit merges, norm settings, repeated
sloppy phrases, boosts, nested Boolean and minimum-should-match cases.
Cases SHA remains `f632da13074c6556bcd9a2ba7605d7974f96c5bbab455100dfb711cfa30c8826`.
The obsolete 2.2 conversion diagnostic fails 301 queries as expected; it cannot
accept or reject the native scale-1 contract. All cases, both engines' dumps
and the full report are retained as `synthetic-*.gz`.
The frozen old baseline had 182 structural and 301 raw-score failures.

## Integrated corrections and verification

The ordered changes fix repeated sloppy phrase traversal, exact physical field
statistics through deletion merges, native reciprocal BM25 arithmetic, format-11
bound selection provenance, f64 default Boolean sums with conservative coupled
bounds, supplied phrase-prefix statistics, and unique-token Basic scalar norms.
Public classic single-term constructors and historical custom providers retain
their default score convention. Existing quantized norms are not reconstructed;
the Basic correction applies when documents are indexed again.

The prior integrated normal library run passed 1,355 tests (7 ignored and one
`create_format` test filtered), recorded in [normal-library.log](normal-library.log).
The final source passes all **21 release integration tests**: native BM25 (4),
Boolean sums (1), supplied prefix statistics (4), Basic norms (4), and query
pruning across eight configurations (8), in [release-integrations.log](release-integrations.log).
The benchmark-tool tests passed all 15 fixtures. This is not a claim that the
full release library suite passed: two historical `should_panic` tests depend
on debug assertions disabled in release.

Format-11 reencoding of the retained format-9 index preserves its entire
logical postings/TF/positions, fieldnorm, stored-document and u64-fast-field
payload. Six deliberately perturbed components prove the payload comparator's
sensitivity; unsupported mixed JSON/string fast fields are rejected.
These receipts are separate from the corrected corpus replay, which intentionally
adds the missing 55 tokens. A retained format-9 reader rejects format 11 before
interpreting new bounds; footer unit tests cover reader versions 4–10.
No actual format-10 binary was executed for that rejection claim.

## Latency, storage and query memory methodology

Four serial runs pin both persistent processes to CPU 4, warm for 40 seconds,
then record 256 samples per query with seed-23 randomized query order and
alternating engine order within pairs. Separate runs reverse process creation
order. The metric is median client pipe-request latency, including query parsing
and collection, with query caches disabled. Rust uses release opt-level3/LTO,
`-C target-cpu=native`, rustc 1.101.0-nightly, LLVM23.1.1. Lucene uses the native
reference adapter and default heap with ParallelGC; class/JAR/tool/binary hashes
and exact commands are retained. `do_query` SHA:
`7396f5bba982180b8551e5cf3b6ed8fd08ae3cac5cd2c172e9cedfdf15811c3a`.

All our compilation, tests and indexing completed before timing; no own workload
ran concurrently with these serial measurements. User background workloads were
left untouched. Start/end one-minute load averages were 6.86–8.74 on the recorded
host. Compressed `*.host.json.gz` retain process-name/load/CPU snapshots.
Do not compare absolute timings with historical matched runs as though corpus,
score arithmetic, physical document order and background load were held constant.

| Index | Bytes |
| --- | ---: |
| Corrected Tantivy format11 replay | 619,508,124 |
| Lucene 10.4 | 626,810,511 |

The corrected replay is **7,302,387 bytes (1.16%) smaller**. Rebuilding/merging
changes physical document order and compression layout; this difference is a
comparison of equivalent logical indexes, not an isolated codec-change saving.
The separate payload-preserving format9→11 reencode is 622,748,577 bytes,
17 bytes larger than format9 metadata. Raw per-file sizes are in [storage.json](storage.json).
The replay took 97.04 seconds and peak process-tree RSS 1,312,080 KiB; this is
preparation evidence, with other work running then. No matched Lucene indexing
run was made, so no indexing speed or memory win is claimed.

| Warmed query process | RSS, MiB | High-water RSS, MiB |
| --- | ---: | ---: |
| Tantivy | 14.90 | 14.90 |
| Lucene / default JVM heap | 346.27 | 387.64 |

Memory is a serial 20-second COUNT/TOP_10 warmup per engine, including mmap pages.
Tantivy completed 814 rounds; Lucene 298. RSS depends on heap/GC phase and warmup
work; it is not a universal peak or a controlled-heap comparison.
No disk-cold startup, constrained JVM heap, concurrent query throughput, indexing
or merge efficiency comparison was completed in this pass.

## Full compatibility scope

The [current acceptance ledger](../../parity/ACCEPTANCE.md) distinguishes these
bounded proofs from the pinned inventory of 32 modules, 3,637 public/protected
types and 26,822 declared members. All 47 feature families remain incompletely
verified. Overlap norms, configurable similarities, token graphs, spans/intervals,
vector/BKD/spatial search, joins/grouping/suggest and other modules, Java facade
contracts and Lucene file interchange still require implementation and acceptance.
The next separately verified units are overlap policy and BM25 parameter support.

## Per-query median ratios

Values below one favor Tantivy; raw samples are adjacent.

| Query | COUNT Tantivy first | COUNT Lucene first | TOP_10 Tantivy first | TOP_10 Lucene first |
| --- | ---: | ---: | ---: | ---: |
| `the` | 0.7106 | 0.7204 | 0.7876 | 0.7786 |
| `of` | 0.7007 | 0.7024 | 0.3789 | 0.4032 |
| `and` | 0.7243 | 0.8064 | 0.3641 | 0.4207 |
| `united` | 0.7650 | 0.7451 | 0.4745 | 0.6992 |
| `states` | 0.7372 | 0.8091 | 0.4078 | 0.6320 |
| `american` | 0.6912 | 0.8472 | 0.4407 | 0.5826 |
| `york` | 0.7374 | 0.8154 | 0.4229 | 0.6148 |
| `saxophone` | 0.6860 | 0.6800 | 0.6097 | 0.7514 |
| `+the +of` | 0.9228 | 0.9196 | 0.3653 | 0.3979 |
| `+united +states` | 0.7827 | 0.7805 | 0.2127 | 0.2654 |
| `+new +york` | 0.9413 | 0.9365 | 0.3176 | 0.3852 |
| `+the +american` | 0.8943 | 0.8854 | 0.4597 | 0.4785 |
| `+the +saxophone` | 0.6468 | 0.6330 | 0.6869 | 0.6965 |
| `the of` | 0.8959 | 0.9053 | 0.3594 | 0.3915 |
| `united states` | 0.7354 | 0.7376 | 0.2188 | 0.2668 |
| `the american` | 0.9048 | 0.8616 | 0.8959 | 0.9044 |
| `"united states"` | 0.6788 | 0.6819 | 0.6948 | 0.6954 |
| `"new york"` | 0.8624 | 0.8583 | 0.7069 | 0.7087 |
| `+griffith +observatory` | 0.7747 | 0.8461 | 0.9303 | 0.8184 |
| `griffith observatory` | 0.6982 | 0.6884 | 0.6549 | 0.7742 |
