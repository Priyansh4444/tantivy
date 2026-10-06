# Current Tantivy versus lucene-rs, 2026-10-06

**lucene-rs is a strong search core and wins broad top-10 latency on this machine.** On the 1,201-query port-style suite, the fixed port is about **1.70× faster by the ratio of arithmetic means of per-query medians** and **4.7% faster geometrically** (approximately 4.5% lower geometric latency). Tantivy wins more individual queries, but several expensive queries dominate its arithmetic mean. Tantivy exact COUNT is about **5.61× faster geometrically** and has lower warmed RSS in this run. These results establish neither universal performance superiority nor complete Lucene feature/API parity.

This is a fresh Rust-to-Rust measurement, with the current Tantivy implementation and the corrected port. No fresh Java latency lane was run. Published ratios against Java on different machines are not used to infer which Rust engine wins.

## Complete results

Every ratio is **Tantivy / fixed port**: below 1 favors Tantivy; above 1 favors the port. AB executes Tantivy then port; BA reverses them. Geometric ratios weight each query equally in log space. The arithmetic metric divides the mean of Tantivy's per-query medians by the mean of the port's per-query medians; it is not a pooled-sample mean. Each order pools 96 samples per query/engine/operation across three rounds. No samples or queries were trimmed.

Port-style suite (all 1,201 timed queries return matches):

| Kind | Operation | Queries | Geometric T/P, AB / BA | Ratio of mean query medians, AB / BA | T / P wins in both orders |
|---|---|---:|---:|---:|---:|
| ALL | COUNT | 1201 | 0.1781 / 0.1783 | 0.1583 / 0.1591 | 1098 / 98 |
| TERM | COUNT | 300 | 0.0215 / 0.0217 | 0.0033 / 0.0033 | 300 / 0 |
| AND | COUNT | 300 | 0.6182 / 0.6195 | 0.6729 / 0.6796 | 253 / 45 |
| OR | COUNT | 301 | 0.1002 / 0.0996 | 0.0403 / 0.0401 | 298 / 3 |
| PHRASE | COUNT | 300 | 0.7561 / 0.7547 | 1.0276 / 1.0225 | 247 / 50 |
| ALL | TOP10 | 1201 | 1.0472 / 1.0465 | 1.6983 / 1.7024 | 758 / 416 |
| TERM | TOP10 | 300 | 0.6923 / 0.6877 | 0.6789 / 0.6780 | 291 / 3 |
| AND | TOP10 | 300 | 1.0393 / 1.0380 | 1.3273 / 1.3247 | 180 / 112 |
| OR | TOP10 | 301 | 2.2656 / 2.2590 | 4.2144 / 4.2022 | 38 / 260 |
| PHRASE | TOP10 | 300 | 0.7358 / 0.7419 | 0.8762 / 0.8836 | 249 / 41 |

Retained Wiki20 (all 20 timed queries return matches):

| Kind | Operation | Queries | Geometric T/P, AB / BA | Ratio of mean query medians, AB / BA | T / P wins in both orders |
|---|---|---:|---:|---:|---:|
| ALL | COUNT | 20 | 0.0232 / 0.0220 | 0.1400 / 0.1393 | 20 / 0 |
| ALL | TOP10 | 20 | 0.9058 / 0.9106 | 1.1559 / 1.1555 | 12 / 8 |

The strongest measured gap is OR TOP10: the port is about **4.2× faster by mean query medians** and **2.26× geometrically**. For `university of washington`, Tantivy/port medians are 2879.958/238.066 µs in AB and 2878.8765/256.4075 µs in BA. The port's MaxScore execution and richer impact hierarchy are concrete candidates to investigate against our WAND path. This packet contains no new profiling attribution or production optimization; that is the next performance investigation.

Main-pair RSS after identical complete warmup is **251.97 MiB Tantivy versus 322.64 MiB port**, about 22% lower for Tantivy. These are process snapshots, not whole-lifecycle peaks. TT snapshots are 234.79/234.81 MiB; PP snapshots are 317.43/317.39 MiB. Total index sizes are 626,102,393 versus 691,788,804 bytes, but auxiliary capabilities differ (Tantivy has a u64 fast sort field and compressed stored data; port has decimal stored sort text). They do not establish a whole-index or indexing-efficiency winner. Encoded text components alone are 610,742,364 bytes (`.idx/.pos/.term/.fieldnorm`) versus 623,505,487 (`.doc/.pos/.tim/.nrm`).

## Correctness and frozen inputs

All **1,227** correctness queries pass exact hit count, ordered physical document IDs and raw finite f32 score bits across engines. All **11,509** returned score bits match. Each engine's optimized output also matches its own exhaustive oracle. Six edge cases cover absent and repeated terms and overlapping repeated-term phrases; they are correctness-only, with two empty results. Proof includes duplicate/tie ordering checks and exact count validation independently of a reported top-10 lower bound. This is coverage of this frozen corpus and query set, not a proof of every Lucene feature.

The port's full decoded payload matches the retained Tantivy/Java physical payload: **1,000,000 documents, 917,578 nonempty field documents, 1,642,896 terms, 126,703,012 postings and 294,827,020 positions**. Canonical payload SHA-256 is `893a75958e6d15c997d414d5fb5fc47c633aa18c68fcd33bc0f8e13abfa8dbb8`. The Tantivy index's complete non-lock file map matches its frozen ruler; the port index is unchanged from the full audit. Verify-before = verify-after = measure-before = measure-after guards bind source, lock, compiler-built binaries, queries, controller and index identities.

Corpus replay is 1,831,702,900 bytes, SHA-256 `5a04d4c5f6e418e8d0cf035f27dda8f9781e852ef41adb133237359fca263961`. All rows, including empty fields, retain physical order. Queries combine our retained Wiki20 with the port's normalization and seeded term sampling over retained search-benchmark-game input. This reproduces the method, not the identity of the author's untracked query files. The generated suite has 300 TERM, 300 AND, 301 OR and 300 PHRASE cases. Query manifest SHA-256 is `39a54b200392befbde83469841ce6eaefe7ec1850bb5b0e25c74a0eefa709a4e`.

## Measurement contract and limits

- Tantivy source: `c0efbc99c2bcb8e6e7c0d6679a8dc9b7071744ac`; fixed port: `e5d1f81bff42c87886b12770f3c79648bb1963ed`, based on `465ea8f77f4397e35de56b7f7d202158d59e5871`.
- Both Rust 1.99.0, LLVM 23.1.1, shared locked dependency graph, opt-level 3, fat LTO, one codegen unit, `target-cpu=native`, panic abort, overflow checks off. Intel Core Ultra 7 255H. Persistent workers pinned to CPU 4; controller CPU 6. Serial requests ensure the two engines do not compete concurrently.
- Default BM25 k1=1.2, b=0.75; one segment, no deletions, identical text, positions and norms. Direct ASTs are prebuilt outside timing. Native timers enclose normal public API rewrite/weight/scorer/collector/result allocation; transport, result consumption/checksum and drop are outside timing.
- TOP10 is an **available-API comparison**: the port additionally tracks 1,000 hits; Tantivy TopDocs requests no total. Output ranks/scores are identical, but collector work is not identical. A threshold-zero port API experiment and an exact-top10-plus-count service lane were deferred. COUNT is a separate exact-output operation.
- Ten warmup iterations per query/mode/worker; 32 samples per order in each of three rounds; primary T/P and same-engine TT/PP pairs. Seeded query order, both AB/BA orders, bounded requests and global deadlines. **87,912 complete cells / 2,813,184 raw native nanosecond samples**; timing took 1340.098 seconds. Every timed result checksum is checked against the verified output.
- Known own preparation/build/test/audit sessions had finished; user processes were preserved. Unknown-owner `cc1` compiler activity appears in the initial telemetry. One-minute load ranged **1.63–11.35**. This was not a globally idle host. AB/BA and TT/PP controls quantify some observed variation; they do not eliminate every contention bias.

Broad TOP10 pooled controls are TT 0.99742/0.99658 and PP 1.00121/1.00088 (AB/BA); each primary round favors the port geometrically by 4.54–5.39%. Controls are not uniformly within 1%: Wiki20 COUNT TT BA is about 0.938. Per-query “wins beyond observed controls” in the CSV uses the maximum absolute log TT/PP spread across both orders as a deterministic heuristic, not a statistical confidence interval.

## Upstream fixes submitted

1. [PR #1: match Lucene BM25 IDF rounding](https://github.com/pc-style/lucene-rs/pull/1). Valid N=df=TTF=54,505, boost/frequency/norm=1 produces original-port IDF/score bits `3719e737`/`368be977`, versus actual Java Lucene 10.5.2 `3719e736`/`368be976`. Preserve Java's `ln(1 + x)` order instead of `ln_1p(x)`. Both expected-bit regressions fail before and pass after; full debug/release suites, fmt and clippy pass. The corrected port is what was timed here.
2. [PR #2: fail on equivalence mismatches](https://github.com/pc-style/lucene-rs/pull/2). Original comparator printed differences yet exited 0. Fixed comparator exits 1 for complete mismatches and 2 for malformed input, compares raw f32 bits, validates ordered unique hits and explicitly retains `eq`/`gte` relations from both Rust and Java writers. Eleven Python tests, Rust tests/fmt/clippy and an actual Java/Rust six-query fixture pass, including Python `-O`.

The author's historical zero-mismatch logs can be accurate. A broken exit-status gate does not itself show mismatches in those logs, and the isolated BM25 witness does not invalidate every historical corpus. Equal reported lower bounds also do not prove identical skipped blocks. Detailed retained evidence and exact patches are under `upstream-fixes/`; those child-authored READMEs describe their verification stage before root submission, while the PR links above reflect the final submitted state.

## Recompute and rerun

Run `python3 recompute.py` from this directory. It reads compressed raw timings and dumps directly, checks archive hashes, complete schedule cells, checksums, correctness and every published numerical CSV/summary value, including per-round controls. It requires only Python's standard library; the corpus/index/binaries are not needed to audit the reported numbers.

The measured `analyze.py`, `balanced.py`, `stage.py`, harness source and shared Cargo.lock are preserved byte for byte. Large samples, dumps and protocol logs use deterministic gzip (mtime 0). `packet-manifest.json` binds every published file and records original hashes for copied/compressed benchmark files. `measure/telemetry-summary.json` retains all timestamp/load averages; process inventory and the own-session inventory remain local, with the original telemetry digest retained. The original `analyze.py` requires that retained original telemetry file to reproduce its complete file-hash metadata; standalone `recompute.py` audits all numerical results without it.

A new engine run also needs the retained corpus, physical map, sorted term list, Tantivy index and original payload proof; these multi-gigabyte inputs and generated targets are excluded. The recorded preparation commands contain original absolute paths. Restore this workspace layout and check out the measured source heads (publishing this report changes the repository's documentation HEAD):

```text
search/
  tantivy-pr2937/                    # c0efbc99
  lucene-rs-idf-fix-oct06/            # e5d1f81b, available from Priyansh4444/lucene-rs
  search-benchmark-game/             # a7c75473, if regenerating queries
  bench/
    lucene-rs-balanced-oct06/        # this packet, uncompressed files restored
    native-profile-aligned-oct05/   # retained corpus/map/terms/proof
    wiki-1m-native-aligned-v11-oct05.idx
```

For a new run, copy only the Python scripts, query manifest and `harness/` into an empty `search/bench/<new-run>/` directory. Retain the supplied Cargo.lock; the recorded `resolve` stage documents its original generation and should not regenerate dependencies for a replay. Do not copy completed stage directories into the new run: stage creation deliberately refuses to overwrite receipts. Rewrite the index/audit command paths for the new location, then follow the recorded locked build/test/index/audit commands through `stage.py STAGE 3600 COMMAND...`. Run `python3 -m unittest test_balanced` before `python3 balanced.py verify` and `python3 balanced.py measure`. After successful measurement, `python3 analyze.py` produces the numerical report. Shared harness dependencies resolve relative to this layout. Missing or changed data/source/index identities fail the gate. This packet provides source and output reproducibility; it does not claim a fresh remote clone contains the excluded input data.
