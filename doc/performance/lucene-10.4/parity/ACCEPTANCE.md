# Current Lucene acceptance ledger

The inventory remains an immutable source audit pinned to Tantivy `65c4e15e6`.
This ledger records subsequent executable acceptance without rewriting that
baseline or marking an entire family complete from a small fixture suite.
The requested target remains all Lucene 10.4 public APIs and feature modules.

| Contract | Current evidence | Remaining boundary |
| --- | --- | --- |
| Native field statistics | Exact physical field population and token totals; missing/empty fields, pending deletes, Basic terms and deletion merges | Broader field types and all public statistics/error contracts remain open |
| Native default BM25 | Raw scale 1, default 1.2/.75 term/boost/fractional phrase checks, frozen 330 and Wiki 20 green | Arbitrary Similarity subclasses and full scoring/API contracts remain open |
| BM25 query parameters | Validated immutable global/per-field Searcher parameters; native/classic coherent snapshots; pinned Java queries and 30 scalar digests; conservative complete-block/tail bounds | Nondefault native TOP_10 measured 33–41% slower overall on aligned Wiki20; broader profiles/workloads and NaN collector ordering remain open |
| Overlap norm policy | Native DiscountOverlaps default; persisted explicit CountAll; old Boolean append policy; actual empty-index old-reader rejection; multi-value, Basic, reopen/delete/merge and pruning fixtures | Existing norms require reindexing to change policy; arbitrary token graphs/position/error contracts remain open |
| Overlap/query configuration independence | Eight pinned native Java combinations, both policies/four profiles; exact norm/statistics/raw score/order/COUNT/explanation checks | This is a bounded matrix, not arbitrary Similarity/token-graph certification |
| Block-bound numeric cache ownership | Public weight/reader evaluation cannot read or populate fixed-owner cache; two reproduced tail underbounds fixed; native/classic debug and exact release regressions plus all three Wiki20 gates | Internal mixing hazard; no claim frozen Wiki20 previously executed it; full public API coverage remains open |
| Boolean default sums and pruning | f64 sum contract, identical-document ties, serialized bound witnesses, eight-config exhaustive gate | Arbitrary compound queries, custom combiners and full API behavior not exhaustively certified |
| Repeated sloppy phrases | Native frontier traversal, wide slop and repeated-term fixtures | Graphs, alternatives, explicit gaps/end states, full phrase-prefix/multi-phrase parity remain open |
| Supplied prefix statistics | Custom scalar/coherent providers and errors reach prefix weights; COUNT avoids statistics | This preserves existing prefix behavior; it does not establish Lucene MultiPhrase parity |
| DOCS-only scalar norms | Unique encoded terms for numeric/date/bool/bytes/IP values, aliases, signed zero and merge | Existing norms need reindexing; arbitrary field/analysis contracts remain open |
| Equivalent Wiki1M corpus | All 1,642,896 term DF/TTF; all 1M physical docID/ID/u64-sort/norm rows; all 126,703,012 postings/TF and 294,827,020 positions exactly equal | Restricted ASCII input; no exhaustive Unicode/token-graph certification |
| Native query efficiency | Aligned DEFAULT TOP_10 and all three profiles COUNT win all 20 medians in both orders; nondefault TOP_10 loses overall; aligned index 0.113% smaller; lower warmed default-JVM query RSS | Loaded-machine observations; other workloads, cold startup, constrained heaps, throughput/indexing/merge unmeasured |
| Format11 migration | Full format 9→11 payload identity; explicit old format 9 reader rejection; footer version tests | Not Lucene file interchange or a general backward-codec certification |
| All 47 feature families | Finite inventory and per-member worklist published | No family is exhaustively verified; Java facade and Lucene file compatibility absent |

The [October 5 cache ownership proof](../results/2026-10-05-pruning-cache/README.md)
records a separate safety prerequisite at e13303b3a; no speed claim.

The [October 5 profile baseline](../results/2026-10-05-native-profiles/README.md)
records complete physical postings identity, strict gates for all three profiles,
serial measurements and actual untimed pruning-route diagnosis at 0eb1af15d.
It preserves the earlier cutoff failures and loaded-machine limitations.

The [October 4 configuration report](../results/2026-10-04-configuration/README.md)
records the integrated overlap/parameter units and new DEFAULT measurements at
`872c9f55d`. The [earlier October 4 native report](../results/2026-10-04-native/README.md) contains raw
results, tolerances, commands, hashes and limitations at `7db890908`.
[Inventory extraction](inventory/README.md) and [scope](COMPATIBILITY.md) retain
the complete target. No language choice guarantees a performance result.
