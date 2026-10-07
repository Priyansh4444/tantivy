# Faster 3–32-term OR execution, 2026-10-07

**Accepted win: mean top-10 latency roughly halves across 103 tested OR queries with three or more terms.** All 103 improve in both execution orders beyond their observed same-engine control variation. Across the complete 1,201-query suite, the ratio of mean query medians falls by 21–22%. Exact result bits and index contents are unchanged. This is a focused query execution improvement; it does not establish complete Lucene feature/API parity or universal superiority.

The fresh candidate/fixed-port lane still favors `lucene-rs` by 1.29–1.30× overall and 2.34–2.35× on the longer OR queries by mean query medians. The earlier comparison favored it by approximately 1.70× overall and 4.85× on longer OR queries. Those historical-to-current ratios come from different runs; the primary acceptance measurement below directly pairs the new and preserved Tantivy binaries.

## Direct candidate versus preserved Tantivy baseline

Both engines use the same public API, input index, prebuilt ASTs and no-total TopDocs collector. Every ratio below is **candidate / baseline**; below 1 favors the candidate. Medians pool 48 samples per query/side/order across three rounds. Arithmetic ratios divide the means of those per-query medians; geometric ratios give each query equal relative weight. No rows or samples are removed.

| TOP10 kind | Queries | Mean latency baseline → candidate, AB / BA (µs) | Ratio of means, AB / BA | Geometric ratio, AB / BA | Candidate wins beyond controls |
|---|---:|---|---|---|---:|
| ALL | 1201 | 284.81 → 224.77 / 286.59 → 224.03 | 0.7892 / 0.7817 | 0.9373 / 0.9415 | 126 |
| TERM | 300 | 15.22 → 15.18 / 15.11 → 15.35 | 0.9971 / 1.0159 | 0.9944 / 1.0259 | 4 |
| AND | 300 | 170.46 → 171.15 / 169.91 → 168.16 | 1.0041 / 0.9897 | 1.0015 / 0.9894 | 7 |
| OR | 301 | 623.87 → 382.35 / 633.28 → 384.10 | 0.6129 / 0.6065 | 0.7760 / 0.7763 | 111 |
| OR_3PLUS | 103 | 1441.93 → 737.53 / 1469.54 → 742.71 | 0.5115 / 0.5054 | 0.4819 / 0.4752 | 103 |
| OR_2 | 198 | 198.32 → 197.58 / 198.26 → 197.55 | 0.9963 / 0.9964 | 0.9943 / 1.0021 | 8 |
| PHRASE | 300 | 328.56 → 329.89 / 326.89 → 327.96 | 1.0040 / 1.0033 | 0.9995 / 0.9978 | 4 |

OR_3PLUS is 103 query ASTs with at least three terms; OR_2 is 198 two-term ASTs. Dispatch is based on the actual specialized scorer count, not merely AST length. The existing two-term route is unchanged. COUNT, TERM, AND, PHRASE and Wiki20 are negative controls. Broad COUNT arithmetic ratios are 1.0064/1.0029 and geometric ratios 1.0006/1.0006. Wiki20 top-10 arithmetic ratios are 1.0062/1.0040. These do not show a material aggregate change. Fifteen individual broad top-10 rows outside the affected 3+ OR group meet the heuristic for a baseline win; unaffected routes and noise mean this is not a proof of zero regression for every possible query.

Broad top-10 same-engine controls are candidate/candidate 0.99753/0.99684 and baseline/baseline 1.00847/1.01079 geometrically. Longer OR controls are 1.01077/1.00692 and 1.00521/1.00237. Controls are not uniformly close to 1: Wiki20 COUNT baseline-control BA is 0.9476. “Beyond controls” uses the maximum absolute per-query log control spread across both orders; it is a deterministic diagnostic, not a confidence interval.

## Fresh comparison with fixed lucene-rs

Ratios here are **candidate Tantivy / fixed lucene-rs**, not the baseline labels used above. TOP10 returns identical ranked IDs and raw score bits, but this remains an available-API comparison: the port additionally tracks a 1,000-hit lower bound, while Tantivy TopDocs requests no total. COUNT is not retimed against the port in this unit.

| Kind | Queries | Candidate / port mean latency AB (µs) | Ratio of means, AB / BA | Geometric ratio, AB / BA |
|---|---:|---|---|---|
| ALL | 1201 | 226.78 / 175.63 | 1.2912 / 1.3029 | 0.9737 / 0.9754 |
| TERM | 300 | 15.07 / 22.27 | 0.6769 / 0.6738 | 0.6921 / 0.6860 |
| AND | 300 | 172.22 / 132.26 | 1.3022 / 1.3124 | 1.0300 / 1.0385 |
| OR | 301 | 383.46 / 158.85 | 2.4139 / 2.4389 | 1.6575 / 1.6818 |
| OR_3PLUS | 103 | 743.07 / 318.01 | 2.3366 / 2.3489 | 1.5932 / 1.6099 |
| OR_2 | 198 | 196.40 / 76.06 | 2.5820 / 2.6366 | 1.6920 / 1.7204 |
| PHRASE | 300 | 335.83 / 389.21 | 0.8629 / 0.8705 | 0.7594 / 0.7541 |

The candidate is slightly faster geometrically across the whole suite (0.9737/0.9754), while the port is faster by the arithmetic metric because expensive remaining OR queries dominate the mean. Broad port/port controls are 1.00895/0.99551. These metrics answer different questions; neither should be called the universal engine winner.

## What changed and why

The baseline profile on five expensive OR queries spends 64.92% of sampled cycles inside `block_wand`, with a further 9.62% in block loading, 8.41% in block maximum evaluation and 7.19% in seeking. The new private `or_maxscore` enumerates candidates only from essential clauses whose local bounds can make a competitive document. It probes optional clauses only when their contribution can still affect admission. A static pruning preference avoids repeated document-order restoration. Published scores still fold the incoming f32 leaves in original ordinal order through f64 and round once to f32. Conservative bounds use the existing outward sum allowance, inclusive prefixes and no subtractive updates.

Local certificates expire at physical half-open region boundaries. Shallow selection does not imply the decoded cursor moved: essential and optional seeks reconcile stale blocks before score/advance, and selected tails reconcile at the region floor. An actual doc beyond the current interval contributes zero; a stale earlier doc does not prove that. `reader.max_doc()` covers physical IDs, including deletions.

A prefix whose global maxima sum below the threshold can be optional across the whole range. Its dense block ends need not repeatedly split regions around sparse essential terms. This widening is enabled only when every globally optional term has at least twice the maximum posting-count hint of the remaining live terms. Otherwise tight local certificates remain active. The density threshold chooses cost strategy; it is not a correctness assumption. Unsafe numeric domains are rejected before cursor mutation and use exhaustive canonical SumCombiner.

Dispatch changes only specialized 3–32-term unions with the existing safe sum-in-f64 combiner gate. Other routes remain as before. No writer, codec, index format or public API changes. The port inspired the strategy; no port codec or richer on-disk impacts were copied.

Attempt1 (pure local regions) passed correctness but introduced 2.2–2.5× losses on dense optional/sparse essential cases. Attempt2 (unconditional global widening) corrected those but introduced a 5–6× loss on `niceville high school`, where two terms have similar posting counts. Both attempts were rejected and their source/receipts retained. The final cost rule fixes those cases in the pilot and all 103 longer OR cases improve in the complete run. This records the failed attempts, not just the chosen aggregate.

The post-change trace uses the same five cases, 10 warmups and three rounds of 100 iterations each. No samples were lost. It places 89.12% of sampled cycles in inlined `BooleanWeight::for_each_pruning`, 4.34% in block maximum evaluation and 2.23% in loading; the old WAND loop is absent from the reported hot list. Approximate recorded cycles fall from 49.68 billion to 24.96 billion. Trace wall times include startup/warmup and were recorded separately; they are not the acceptance latency metric. Both raw perf traces, reports, replies and checksums are retained.

## Correctness and compactness

- All 1,227 default-profile queries match exhaustive output, the preserved baseline and fixed port exactly: counts, ordered physical document IDs and all 11,509 returned raw f32 score bits.
- Both configured BM25 profiles, k1=0.9/b=0.4 and k1=2/b=1, pass all 1,227 against their own exhaustive oracle, each covering 11,509 raw score bits. Configured results are not claimed to match a newly configured port or Java timing lane. Existing Java-backed native fixtures also pass.
- Full library and eight relevant integration suites: 1,427 passed, 7 preexisting ignored, 0 failed. Nine new unit tests cover full/tail block boundaries, stale optional-to-essential promotion, global certificates, comparable density, exact midpoints/strict ties, duplicate leaves, disparate boosts, exhaustion, deterministic mixed corpora and unsafe numeric fallback. Public tests cover top-K 1/3/10/100, three segments, deletions and fieldnorm variants.
- Clippy and changed-file formatting pass; clippy reports preexisting test warnings outside changed files. `git diff --check` passes.
- The first full-test source guard rejected new `index_v11` files generated by the existing `compat_tests::create_format`. Tests themselves exited 0. The original receipt is preserved; `tests/guard-audit.json` proves every existing code/fixture hash stayed unchanged and only generated outputs were added. The guard now separately classifies these untracked outputs, and an explicit rerun of that fixture creation passes the guard.
- All meaningful input index file hashes stay unchanged through verification and timing. Full decoded payload identity from the frozen prior audit covers 1M physical docs, 1,642,896 terms, 126,703,012 postings and 294,827,020 positions; digest `893a75958e6d15c997d414d5fb5fc47c633aa18c68fcd33bc0f8e13abfa8dbb8`.
- Scratch array payload is 24*n+8 bytes: at most 776 bytes for 32 clauses, plus 96 bytes of four Vec headers on x86-64, allocator overhead and the existing scorer vector. Four allocations occur once per query, none per region; no pool or extra index bits. This is not an allocation-rate benchmark.
- Native benchmark binary grows by 22,528 bytes (0.475%); `.text` grows by 17,984 bytes (0.698%). These are harness build measurements, not every embedding application's code size.
- Main-pair warmed RSS is 307.76 MiB candidate / 307.68 MiB baseline (80 KiB difference), with same-engine snapshots 307.59–307.66 MiB. Fresh candidate/port top-10 snapshots are 306.82/315.16 MiB. These are process snapshots after equal warmup, not a universal incremental-memory or whole-lifecycle efficiency claim.

## Measurement contract

- Preserved baseline binary SHA `49aa819692bde50fe29328e96b331e9825a8ee13942402427dfa670a105e7cd1`, compiled from `c0efbc99c2bcb8e6e7c0d6679a8dc9b7071744ac`. Its non-documentation source matches the base fork HEAD `91fb3cacd5476577135014c40573dc856fb9dd37`. Candidate exact source snapshots and build hashes are retained; publishing changes documentation/HEAD but not compiled source. Fixed port is `e5d1f81bff42c87886b12770f3c79648bb1963ed`.
- Rust 1.99.0/LLVM 23.1.1, shared harness Cargo.lock, opt-level 3, fat LTO, one codegen unit, native CPU, panic abort, overflow checks off. Default worker source is byte-identical to the preserved baseline worker. Intel Core Ultra 7 255H; serial workers on CPU4, controller CPU6.
- One segment, no deletions in the 1M-doc latency corpus; default BM25 k1=1.2/b=0.75. AST construction is outside timers. Timed public API work includes weight/scorer/collector/results allocation; transport/checksum/drop are outside. Query caching is disabled. This unit benchmarks warmed search, not cold process startup or index opening.
- Main run: 1,221 queries, COUNT and TOP10, 10 warmups per query/op/worker, three seeded rounds, both AB/BA orders, 16 samples each; candidate/baseline plus candidate/candidate and baseline/baseline controls. Complete 87,912 cells / 1,406,592 samples. Every timed checksum is checked against frozen exact output.
- Fresh port run: the same 1,221 queries/TOP10, warmup/samples/rounds/orders, plus port/port controls; complete 29,304 cells / 468,864 samples. Candidate same-engine controls are in the main run. No own build/test/compression/profile work overlaps latency measurement.
- User/background processes were preserved. One-minute load ranges 4.95–11.93 in the main run and 9.63–15.41 in the port run. This is not an idle machine; controls quantify observed variation but cannot remove every contention bias.
- Main scripts retain legacy `tantivy`/`port` labels: `tantivy` = candidate and `port` = baseline Tantivy. Only the separate `port-measure` lane uses `port` for actual lucene-rs. Read the schedule label contract before interpreting filenames or ratio keys.

## Audit and rerun

Run `python3 recompute.py` here. It verifies complete archive/original hashes, exact default/configured results, compiled source identity, test counts, schedule completeness, every timed checksum, all numerical CSV and summary values, both controls, per-round aggregates, index guards and profile replies. No binaries, corpus, native tools or third-party Python packages are needed.

Raw nanosecond samples, worker protocol logs, dumps, command/source receipts, shared lock, source snapshots and design/review history are retained. Large data uses deterministic gzip (mtime 0). Raw process inventories stay local; timestamp/load summaries and original telemetry hashes are published. Target directories, binaries, generated fixture data and large input indices/corpus are excluded. The retained test Cargo.lock records the resolution used with `--locked`; it is separate from the common release harness lock.

A new engine run needs the prior comparison inputs under `search/bench/lucene-rs-balanced-oct06`, the fixed port checkout, original frozen payload proof, the same Tantivy index and native binaries rebuilt with the recorded locks/profile. The measured controllers intentionally fail if original guarded source/index identities change. Restore the recorded workspace layout (scripts have original relative and absolute paths), uncompress retained data, and use a fresh output directory for new stage receipts. `stage.py` rejects overwrites. The packet itself is enough to audit published results, not to recreate excluded multi-gigabyte data from a remote clone.

The next measured opportunity remains OR execution and postings/impact access. Richer impact hierarchies may help the remaining gap but would have a separate storage/format cost. This unit neither implements that nor claims indexing, opening/startup, all features or every BM25 configuration have reached Lucene parity.
