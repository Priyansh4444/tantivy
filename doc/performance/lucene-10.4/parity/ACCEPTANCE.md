# Current Lucene acceptance ledger

The inventory remains an immutable source audit pinned to Tantivy `65c4e15e6`.
This ledger records subsequent executable acceptance without rewriting that
baseline or marking an entire family complete from a small fixture suite.
The requested target remains all Lucene 10.4 public APIs and feature modules.

| Contract | Current evidence | Remaining boundary |
| --- | --- | --- |
| Native field statistics | Exact physical field population and token totals; missing/empty fields, pending deletes, Basic terms and deletion merges | Broader field types and all public statistics/error contracts remain open |
| Native default BM25 | Raw scale 1, default 1.2/.75 term/boost/fractional phrase checks, frozen 330 and Wiki 20 green | Configurable parameters, overlap norms and arbitrary Similarity hooks remain open |
| Boolean default sums and pruning | f64 sum contract, identical-document ties, serialized bound witnesses, eight-config exhaustive gate | Arbitrary compound queries, custom combiners and full API behavior not exhaustively certified |
| Repeated sloppy phrases | Native frontier traversal, wide slop and repeated-term fixtures | Graphs, alternatives, explicit gaps/end states, full phrase-prefix/multi-phrase parity remain open |
| Supplied prefix statistics | Custom scalar/coherent providers and errors reach prefix weights; COUNT avoids statistics | This preserves existing prefix behavior; it does not establish Lucene MultiPhrase parity |
| DOCS-only scalar norms | Unique encoded terms for numeric/date/bool/bytes/IP values, aliases, signed zero and merge | Existing norms need reindexing; arbitrary field/analysis contracts remain open |
| Equivalent Wiki1M corpus | All 1,642,896 term DF/TTF and all 1M ID/u64-sort/norm rows exactly equal | Restricted ASCII input; no exhaustive Unicode/token-graph or cross-engine position hashing |
| Native query efficiency | All 20 COUNT/TOP_10 medians lower in both orders; smaller index and lower default-JVM query RSS | Loaded-machine observations; other workloads, cold startup, constrained heaps, throughput/indexing/merge unmeasured |
| Format11 migration | Full format 9→11 payload identity; explicit old format 9 reader rejection; footer version tests | Not Lucene file interchange or a general backward-codec certification |
| All 47 feature families | Finite inventory and per-member worklist published | No family is exhaustively verified; Java facade and Lucene file compatibility absent |

The [October 4 native report](../results/2026-10-04-native/README.md) contains raw
results, tolerances, commands, hashes and limitations at `7db890908`.
[Inventory extraction](inventory/README.md) and [scope](COMPATIBILITY.md) retain
the complete target. No language choice guarantees a performance result.
