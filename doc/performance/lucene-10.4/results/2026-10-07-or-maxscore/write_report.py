import json
from pathlib import Path
p=Path(__file__).resolve().parent
s=json.loads((p/'summary.json').read_text());ps=json.loads((p/'port-summary.json').read_text());e=json.loads((p/'efficiency.json').read_text())
def load_range(folder):
 x=json.loads((p/folder/'telemetry.json').read_text());return f"{min(v['loadavg'][0] for v in x):.2f}–{max(v['loadavg'][0] for v in x):.2f}"
def group(kind,mode='top10'):return next(r for r in s['summaries'] if r['set']=='port-style' and r['mode']==mode and r['kind']==kind)
lines=['# Faster 3–32-term OR execution, 2026-10-07','',
'**Accepted win: mean top-10 latency roughly halves across 103 tested OR queries with three or more terms.** All 103 improve in both execution orders beyond their observed same-engine control variation. Across the complete 1,201-query suite, the ratio of mean query medians falls by 21–22%. Exact result bits and index contents are unchanged. This is a focused query execution improvement; it does not establish complete Lucene feature/API parity or universal superiority.','',
'The fresh candidate/fixed-port lane still favors `lucene-rs` by 1.29–1.30× overall and 2.34–2.35× on the longer OR queries by mean query medians. The earlier comparison favored it by approximately 1.70× overall and 4.85× on longer OR queries. Those historical-to-current ratios come from different runs; the primary acceptance measurement below directly pairs the new and preserved Tantivy binaries.','',
'## Direct candidate versus preserved Tantivy baseline','',
'Both engines use the same public API, input index, prebuilt ASTs and no-total TopDocs collector. Every ratio below is **candidate / baseline**; below 1 favors the candidate. Medians pool 48 samples per query/side/order across three rounds. Arithmetic ratios divide the means of those per-query medians; geometric ratios give each query equal relative weight. No rows or samples are removed.','',
'| TOP10 kind | Queries | Mean latency baseline → candidate, AB / BA (µs) | Ratio of means, AB / BA | Geometric ratio, AB / BA | Candidate wins beyond controls |',
'|---|---:|---|---|---|---:|']
for kind in ['ALL','TERM','AND','OR','OR_3PLUS','OR_2','PHRASE']:
 r=group(kind);a,b=r['AB'],r['BA'];lat=f"{a['mean_query_median_P_ns']/1000:.2f} → {a['mean_query_median_T_ns']/1000:.2f} / {b['mean_query_median_P_ns']/1000:.2f} → {b['mean_query_median_T_ns']/1000:.2f}"
 lines.append(f"| {kind} | {r['queries']} | {lat} | {a['ratio_of_mean_query_medians']:.4f} / {b['ratio_of_mean_query_medians']:.4f} | {a['geomean_T_over_P']:.4f} / {b['geomean_T_over_P']:.4f} | {r['T_wins_beyond_observed_controls']} |")
lines.extend(['',
'OR_3PLUS is 103 query ASTs with at least three terms; OR_2 is 198 two-term ASTs. Dispatch is based on the actual specialized scorer count, not merely AST length. The existing two-term route is unchanged. COUNT, TERM, AND, PHRASE and Wiki20 are negative controls. Broad COUNT arithmetic ratios are 1.0064/1.0029 and geometric ratios 1.0006/1.0006. Wiki20 top-10 arithmetic ratios are 1.0062/1.0040. These do not show a material aggregate change. Fifteen individual broad top-10 rows outside the affected 3+ OR group meet the heuristic for a baseline win; unaffected routes and noise mean this is not a proof of zero regression for every possible query.','',
'Broad top-10 same-engine controls are candidate/candidate 0.99753/0.99684 and baseline/baseline 1.00847/1.01079 geometrically. Longer OR controls are 1.01077/1.00692 and 1.00521/1.00237. Controls are not uniformly close to 1: Wiki20 COUNT baseline-control BA is 0.9476. “Beyond controls” uses the maximum absolute per-query log control spread across both orders; it is a deterministic diagnostic, not a confidence interval.','',
'## Fresh comparison with fixed lucene-rs','',
'Ratios here are **candidate Tantivy / fixed lucene-rs**, not the baseline labels used above. TOP10 returns identical ranked IDs and raw score bits, but this remains an available-API comparison: the port additionally tracks a 1,000-hit lower bound, while Tantivy TopDocs requests no total. COUNT is not retimed against the port in this unit.','',
'| Kind | Queries | Candidate / port mean latency AB (µs) | Ratio of means, AB / BA | Geometric ratio, AB / BA |',
'|---|---:|---|---|---|'])
for r in ps['summaries']:
 if r['set']!='port-style':continue
 a,b=r['AB'],r['BA'];lines.append(f"| {r['kind']} | {r['queries']} | {a['candidate_mean_query_median_ns']/1000:.2f} / {a['port_mean_query_median_ns']/1000:.2f} | {a['ratio_of_mean_query_medians_candidate_over_port']:.4f} / {b['ratio_of_mean_query_medians_candidate_over_port']:.4f} | {a['geomean_candidate_over_port']:.4f} / {b['geomean_candidate_over_port']:.4f} |")
lines.extend(['',
'The candidate is slightly faster geometrically across the whole suite (0.9737/0.9754), while the port is faster by the arithmetic metric because expensive remaining OR queries dominate the mean. Broad port/port controls are 1.00895/0.99551. These metrics answer different questions; neither should be called the universal engine winner.','',
'## What changed and why','',
'The baseline profile on five expensive OR queries spends 64.92% of sampled cycles inside `block_wand`, with a further 9.62% in block loading, 8.41% in block maximum evaluation and 7.19% in seeking. The new private `or_maxscore` enumerates candidates only from essential clauses whose local bounds can make a competitive document. It probes optional clauses only when their contribution can still affect admission. A static pruning preference avoids repeated document-order restoration. Published scores still fold the incoming f32 leaves in original ordinal order through f64 and round once to f32. Conservative bounds use the existing outward sum allowance, inclusive prefixes and no subtractive updates.','',
'Local certificates expire at physical half-open region boundaries. Shallow selection does not imply the decoded cursor moved: essential and optional seeks reconcile stale blocks before score/advance, and selected tails reconcile at the region floor. An actual doc beyond the current interval contributes zero; a stale earlier doc does not prove that. `reader.max_doc()` covers physical IDs, including deletions.','',
'A prefix whose global maxima sum below the threshold can be optional across the whole range. Its dense block ends need not repeatedly split regions around sparse essential terms. This widening is enabled only when every globally optional term has at least twice the maximum posting-count hint of the remaining live terms. Otherwise tight local certificates remain active. The density threshold chooses cost strategy; it is not a correctness assumption. Unsafe numeric domains are rejected before cursor mutation and use exhaustive canonical SumCombiner.','',
'Dispatch changes only specialized 3–32-term unions with the existing safe sum-in-f64 combiner gate. Other routes remain as before. No writer, codec, index format or public API changes. The port inspired the strategy; no port codec or richer on-disk impacts were copied.','',
'Attempt1 (pure local regions) passed correctness but introduced 2.2–2.5× losses on dense optional/sparse essential cases. Attempt2 (unconditional global widening) corrected those but introduced a 5–6× loss on `niceville high school`, where two terms have similar posting counts. Both attempts were rejected and their source/receipts retained. The final cost rule fixes those cases in the pilot and all 103 longer OR cases improve in the complete run. This records the failed attempts, not just the chosen aggregate.','',
'The post-change trace uses the same five cases, 10 warmups and three rounds of 100 iterations each. No samples were lost. It places 89.12% of sampled cycles in inlined `BooleanWeight::for_each_pruning`, 4.34% in block maximum evaluation and 2.23% in loading; the old WAND loop is absent from the reported hot list. Approximate recorded cycles fall from 49.68 billion to 24.96 billion. Trace wall times include startup/warmup and were recorded separately; they are not the acceptance latency metric. Both raw perf traces, reports, replies and checksums are retained.','',
'## Correctness and compactness','',
'- All 1,227 default-profile queries match exhaustive output, the preserved baseline and fixed port exactly: counts, ordered physical document IDs and all 11,509 returned raw f32 score bits.','- Both configured BM25 profiles, k1=0.9/b=0.4 and k1=2/b=1, pass all 1,227 against their own exhaustive oracle, each covering 11,509 raw score bits. Configured results are not claimed to match a newly configured port or Java timing lane. Existing Java-backed native fixtures also pass.','- Full library and eight relevant integration suites: 1,427 passed, 7 preexisting ignored, 0 failed. Nine new unit tests cover full/tail block boundaries, stale optional-to-essential promotion, global certificates, comparable density, exact midpoints/strict ties, duplicate leaves, disparate boosts, exhaustion, deterministic mixed corpora and unsafe numeric fallback. Public tests cover top-K 1/3/10/100, three segments, deletions and fieldnorm variants.','- Clippy and changed-file formatting pass; clippy reports preexisting test warnings outside changed files. `git diff --check` passes.','- The first full-test source guard rejected new `index_v11` files generated by the existing `compat_tests::create_format`. Tests themselves exited 0. The original receipt is preserved; `tests/guard-audit.json` proves every existing code/fixture hash stayed unchanged and only generated outputs were added. The guard now separately classifies these untracked outputs, and an explicit rerun of that fixture creation passes the guard.','- All meaningful input index file hashes stay unchanged through verification and timing. Full decoded payload identity from the frozen prior audit covers 1M physical docs, 1,642,896 terms, 126,703,012 postings and 294,827,020 positions; digest `893a75958e6d15c997d414d5fb5fc47c633aa18c68fcd33bc0f8e13abfa8dbb8`.','- Scratch array payload is 24*n+8 bytes: at most 776 bytes for 32 clauses, plus 96 bytes of four Vec headers on x86-64, allocator overhead and the existing scorer vector. Four allocations occur once per query, none per region; no pool or extra index bits. This is not an allocation-rate benchmark.',
f"- Native benchmark binary grows by {e['candidate_minus_baseline_file_bytes']:,} bytes ({100*e['candidate_minus_baseline_file_bytes']/e['binaries']['baseline']['file_bytes']:.3f}%); `.text` grows by {e['candidate_minus_baseline_text_bytes']:,} bytes ({100*e['candidate_minus_baseline_text_bytes']/e['binaries']['baseline']['sections']['.text']:.3f}%). These are harness build measurements, not every embedding application's code size.",
'- Main-pair warmed RSS is 307.76 MiB candidate / 307.68 MiB baseline (80 KiB difference), with same-engine snapshots 307.59–307.66 MiB. Fresh candidate/port top-10 snapshots are 306.82/315.16 MiB. These are process snapshots after equal warmup, not a universal incremental-memory or whole-lifecycle efficiency claim.','',
'## Measurement contract','',
'- Preserved baseline binary SHA `49aa819692bde50fe29328e96b331e9825a8ee13942402427dfa670a105e7cd1`, compiled from `c0efbc99c2bcb8e6e7c0d6679a8dc9b7071744ac`. Its non-documentation source matches the base fork HEAD `91fb3cacd5476577135014c40573dc856fb9dd37`. Candidate exact source snapshots and build hashes are retained; publishing changes documentation/HEAD but not compiled source. Fixed port is `e5d1f81bff42c87886b12770f3c79648bb1963ed`.','- Rust 1.99.0/LLVM 23.1.1, shared harness Cargo.lock, opt-level 3, fat LTO, one codegen unit, native CPU, panic abort, overflow checks off. Default worker source is byte-identical to the preserved baseline worker. Intel Core Ultra 7 255H; serial workers on CPU4, controller CPU6.','- One segment, no deletions in the 1M-doc latency corpus; default BM25 k1=1.2/b=0.75. AST construction is outside timers. Timed public API work includes weight/scorer/collector/results allocation; transport/checksum/drop are outside. Query caching is disabled. This unit benchmarks warmed search, not cold process startup or index opening.','- Main run: 1,221 queries, COUNT and TOP10, 10 warmups per query/op/worker, three seeded rounds, both AB/BA orders, 16 samples each; candidate/baseline plus candidate/candidate and baseline/baseline controls. Complete 87,912 cells / 1,406,592 samples. Every timed checksum is checked against frozen exact output.','- Fresh port run: the same 1,221 queries/TOP10, warmup/samples/rounds/orders, plus port/port controls; complete 29,304 cells / 468,864 samples. Candidate same-engine controls are in the main run. No own build/test/compression/profile work overlaps latency measurement.',
f"- User/background processes were preserved. One-minute load ranges {load_range('measure')} in the main run and {load_range('port-measure')} in the port run. This is not an idle machine; controls quantify observed variation but cannot remove every contention bias.",
'- Main scripts retain legacy `tantivy`/`port` labels: `tantivy` = candidate and `port` = baseline Tantivy. Only the separate `port-measure` lane uses `port` for actual lucene-rs. Read the schedule label contract before interpreting filenames or ratio keys.','',
'## Audit and rerun','',
'Run `python3 recompute.py` here. It verifies complete archive/original hashes, exact default/configured results, compiled source identity, test counts, schedule completeness, every timed checksum, all numerical CSV and summary values, both controls, per-round aggregates, index guards and profile replies. No binaries, corpus, native tools or third-party Python packages are needed.','',
'Raw nanosecond samples, worker protocol logs, dumps, command/source receipts, shared lock, source snapshots and design/review history are retained. Large data uses deterministic gzip (mtime 0). Raw process inventories stay local; timestamp/load summaries and original telemetry hashes are published. Target directories, binaries, generated fixture data and large input indices/corpus are excluded. The retained test Cargo.lock records the resolution used with `--locked`; it is separate from the common release harness lock.','',
'A new engine run needs the prior comparison inputs under `search/bench/lucene-rs-balanced-oct06`, the fixed port checkout, original frozen payload proof, the same Tantivy index and native binaries rebuilt with the recorded locks/profile. The measured controllers intentionally fail if original guarded source/index identities change. Restore the recorded workspace layout (scripts have original relative and absolute paths), uncompress retained data, and use a fresh output directory for new stage receipts. `stage.py` rejects overwrites. The packet itself is enough to audit published results, not to recreate excluded multi-gigabyte data from a remote clone.','',
'The next measured opportunity remains OR execution and postings/impact access. Richer impact hierarchies may help the remaining gap but would have a separate storage/format cost. This unit neither implements that nor claims indexing, opening/startup, all features or every BM25 configuration have reached Lucene parity.',''])
(p/'README.md').write_text('\n'.join(lines))
(p/'results-review.md').write_text('''# Root result review and acceptance

Root read the complete four-file production/test change and all nine unit tests.
The private route preserves incoming leaf order, uses conservative outward bounds,
reconciles shallow cursors, caps regions by physical max_doc, and falls back before
mutation for exceptional domains. The final two-times-density check changes only
cost strategy, not score or cursor proof. No blocking defect found.

Read all complete default/configured gates, test/guard audit, formatting/clippy,
final pilot, primary raw schedule and same-engine controls. Independent packet
recompute must pass before publication. All 103 three-or-more-term OR cases improve
beyond the per-query observed control spread in both orders; no affected-case loss
in the complete frozen suite. Unaffected route noise remains and is reported.

Main route exact bits agree with own exhaustive oracle and prior baseline/port.
Final candidate source hashes agree across build, full tests, verification and both
timing lanes. Original guard rejection is preserved and explained by generated
compatibility outputs; do not claim the initial source guard passed.

The performance acceptance metric is the direct same-API candidate/baseline lane:
3+ OR arithmetic ratios .51149/.50540, geometric .48187/.47524; all-query arithmetic
.78920/.78170. Fresh actual-port lane still loses arithmetic 1.29119/1.30292 overall
and 2.33664/2.34889 on longer OR. Do not imply port defeated or all Lucene APIs done.

Memory evidence is bounded scratch plus process snapshots, not an allocation/peak
memory proof. Index hashes unchanged, small measured code size growth retained.
Post-profile uses same workload, shows WAND gone and new inlined execution dominant.
Both traces include startup/warmup and are not paired latency evidence.

Accept this focused unit, publish two ordered commits (core + evidence), fast-forward
our authorized fork only. No Tantivy upstream PR in this pass.
''')
synthesis_path=p/'synthesis.md'
synthesis_base=synthesis_path.read_text().split('\n## Final measured cost choice and acceptance')[0]
synthesis_path.write_text(synthesis_base + '''
## Final measured cost choice and acceptance

Attempt2 global widening was rejected for comparable-density high/school clauses.
Global prefix widening now requires each prefix term's posting-count hint to be at
least twice the maximum hint of remaining live terms, with u64 arithmetic. Other
regions retain tight local certificates. This adds no arrays or per-region allocation.
Final pilot corrected prior regressions; full-suite default/configured raw bits pass.
Complete main run shows all 103 longer OR cases improve in both orders beyond
observed controls, about 2x by arithmetic and geometric mean. Root read every change
and post-trace. Design Verify and runtime Verify pass for this bounded unit, with
remaining port gap, contention and memory measurement limits recorded in README.
''')
print('wrote report and root review')
