# Candidate B: persistent protocol with one parent clock

Read-only architecture, 2026-10-06. Targets: OUR Tantivy `c0efbc99c2bcb8e6e7c0d6679a8dc9b7071744ac` and lucene-rs `465ea8f77f4397e35de56b7f7d202158d59e5871`. Both source checkouts were clean when inspected. No builds, index scans, or measurements were run.

throughput checkpoint: n/a, read-only investigation

## Caller first

The future controller takes two executable paths, existing index paths, a frozen query manifest, and a bounded schedule. It opens one persistent child per engine, validates their startup receipts, proves correctness, then sends the same requests sequentially to both children. Both workers are pinned to the same allowed CPU; the parent uses another CPU. The parent measures from request write/flush through receipt of the complete response. Workers parse the request into their engine's direct AST on every request.

```python
pair = Pair.open(tantivy_spec, lucene_rs_spec, cpu=chosen_cpu)
pair.require_payload_identity(expected_physical_receipt)
pair.require_results(query_manifest, exact_count=True, score_bits=True, order=True)
controls = Pair.open(tantivy_spec, tantivy_spec, cpu=chosen_cpu)
controls.measure(schedule)                  # same binary, two persistent processes
result = pair.measure(schedule)            # ABBA and BAAB blocks, fixed warmups
result.write_new_directory(output_path)    # refuses an existing output directory
```

This is a proposed caller, not executable code. It produces per-query COUNT and TOP10 distributions, both starting orders, same-binary controls, and provenance. There is no combined “engine wins” result if any semantic gate fails. COUNT can proceed independently when the top-k policy or score-bit gate remains unresolved.

## Protocol and measured work

Use one versioned newline JSON protocol for both adapters. Example requests are `{"v":1,"seq":7,"op":"COUNT","query":{"and":[{"term":"alpha"},{"term":"beta"}]}}` and the same request with `op:"TOP10"`. Query variants are TERM, ordered AND, ordered OR, and exact contiguous PHRASE; the initial corpus permits lowercase ASCII terms of 1–255 bytes. Reject empty clauses, unsupported variants, trailing data, invalid sequence numbers, and oversized lines. Preserve duplicates and clause order; do not deduplicate or reorder terms in the shared boundary parser.

Startup emits a READY receipt, outside timing: engine/source/binary/lock/profile identities, actual field name, one-segment/live-doc statistics, index identity, BM25 bits, query-cache policy, and collector policy. Each request has exactly one explicitly flushed response:

```rust
enum Operation { Count, Top10 }
enum QueryAst {
    Term(AsciiTerm), And(Vec<AsciiTerm>), Or(Vec<AsciiTerm>),
    Phrase(Vec<AsciiTerm>),
}
struct Request { seq: Sequence, op: Operation, query: QueryAst }
struct Hit { doc: PhysicalDocId, score_bits: F32Bits }
enum ResultBody { Count { exact: u64 }, Top10 { hits: Vec<Hit> } }
struct Response { seq: Sequence, result: ResultBody }

fn serve(index: OpenIndex, input: impl BufRead, output: impl Write)
    -> Result<(), ProtocolError>; // not implemented
fn execute(engine: &Engine, request: Request) -> Result<ResultBody, SearchError>;
// not implemented: convert AST, invoke public engine API, own result, encode, flush
```

Physical IDs are canonical decimal u32; score bits are eight lowercase hexadecimal digits. TOP10 has no claimed total count. An ERROR response terminates that run; no timed retry or silently dropped sample. EOF cleanly closes the child. READY is not a per-request profile command.

Use the same boundary parser/encoder source in both adapter packages. The measured span includes identical wire parsing, direct AST construction, engine rewrite/weight/scorer/collector work, result ownership, encoding, and IPC. Initialization, physical-map validation, source hashing, and query-file loading stay outside it. Engine AST construction remains inside it for both engines; no prebuilt queries on one side. Retain library rewriting on both sides. Do not use Tantivy's text QueryParser against the port's direct AST, and do not mix the existing port's internal `Instant` with a parent clock.

## Collector policy: the blocking mismatch

The port's public `IndexSearcher::search(query, n)` always creates `TopScoreDocCollector::new(n, 1000)`. Competitive-score publication is delayed until `total_hits > threshold`; `TopDocs` exposes an exact-or-lower-bound relation. Tantivy `TopDocs::with_limit(10)` does not request totals. Omitting the port's count from output does not remove its work. The existing port has no public threshold argument, and weight creation/segment scoring are private. An adapter cannot honestly solve this by constructing a public collector alone.

Recommended matched TOP10 policy requires one separately reviewed port API: `search_with_total_hits_threshold(query, n, threshold)`, factoring the existing implementation and preserving `search` as the threshold-1000 wrapper. Use threshold zero for the matched top-k-only lane, retain the actual port counter/relation in an untimed diagnostic dump, and prove ties/order/exhaustive top10 before timing. Threshold zero is an approximation of “no total requested,” not a proof of identical pruning trajectories; the different native top-k implementations are the algorithms being measured. No scoring or skip rules are copied into the adapter.

If that core API change is out of scope, publish exact public COUNT comparisons and label public `search(...,10)` versus Tantivy TopDocs as **different collector contracts**, without a matched TOP10 speed claim. An additional equal-output service lane may compare exact `TOP10+COUNT`: Tantivy's tuple collector versus port `search` plus `count`. That measures the actual APIs for the same returned service and legitimately includes the port's second pass; it is not the top-k-only lane. Raising a Tantivy count threshold or merely ignoring the port's relation would not establish the same contract.

DEFAULT is the only initial profile: k1 bits `3f99999a`, b bits `3f400000`, scale `3f800000`, physical field statistics. The port hardcodes 1.2/.75; do not report the retained k09/k25 profiles as supported. Its IDF uses f64 `ln_1p(x)` while native Tantivy uses f64 `(1+x).ln()`. This is a concrete possible score-bit gate failure. Diagnose actual failures; do not loosen equality or rescale scores to get a timing result.

## Corpus and runtime correctness proof

Accessible retained inputs under `/home/pronsh/Coding/playground/search`:

| Input | Path |
|---|---|
| Original 1M corpus | `bench/wiki-1m.jsonl` |
| Physical-order replay | `bench/native-profile-aligned-oct05/aligned.jsonl` |
| Frozen physical tuples | `bench/native-profile-aligned-oct05/lucene-physical.tsv` and `tantivy-physical.tsv` |
| Reusable Tantivy index | `bench/wiki-1m-native-aligned-v11-oct05.idx` |
| Existing native Lucene index | `search-benchmark-game/engines/lucene-10.4.0/idx` |
| Evidence | `bench/native-profile-aligned-oct05/aligned.jsonl.replay.json`, `physical-payload-comparison.json`, `frozen-ruler.json` |

The receipts certify 1,000,000 physical docs, 917,578 nonempty field docs, 294,827,020 tokens, one segment, and equal complete docID/ID/sort/norm and TF/docID/position payloads. Replay raw SHA256 is `5a04d4c5f6e418e8d0cf035f27dda8f9781e852ef41adb133237359fca263961`; physical tuple SHA256 is `6f7082e3ff479c7be7fe6f819857716630fc4b6b64acc2d6005f1107127029c7`. Current Tantivy opens the retained index with its current binary; the old receipt's binary is not the current baseline.

First try opening the existing Lucene index with the port and prove codec/field/statistics/positions compatibility. The port advertises Lucene104 encodings, which is encouraging source evidence, not a successful-open claim. If incompatible, create a new port index only in a later authorized stage: replay every aligned JSON row in order, including empty text, with positions and the same field/token/norm semantics, then verify complete physical and postings receipts. Existing port preparation drops empty normalized documents and uses a different 468,867-document corpus, so it cannot be reused unchanged. Our replay text is already constrained to `[a-z ]` with tokens at most 255 bytes; no further article extraction or normalization is needed. A different physical field name can be mapped explicitly, never a different term population.

Before timings: verify live/max docs, no deletions, one segment, physical-ID map, all norm bytes, document-frequency/token/position digest and counters. Then require exact COUNT and exact ordered `(physical_doc_id, f32_bits)` TOP10 for all 20 frozen queries plus the selected port query manifest. Compare optimized top10 to an exhaustive scored oracle inside each engine: the proposed port threshold API with `u64::MAX` prevents competitive-score publication; Tantivy enumerates live scored documents and sorts them. The port oracle therefore also depends on the approved API; the stock threshold1000 search is not an exhaustive oracle. Require the full returned length (including <10 matches), unique IDs, finite scores, descending score and ascending physical-ID ties. Fail closed on missing/reordered queries, differing clauses, counts, hits, score bits, or order. An exact-score failure blocks the matched score lane rather than becoming a tolerance-based success. Old aligned receipts ground corpus reuse; they do not prove current port runtime correctness.

## Bounded measurement and artifacts

Start with 20 frozen queries × COUNT/TOP10, 10 untimed warmups per cell, 25 measured repetitions per cell in four fixed ABBA/BAAB blocks. Gate the complete selected manifest once, including the upstream query suite if adopted; do not silently sample its correctness while claiming all queries. Freeze manifest bytes and hashes before running. Cap each request at 10 seconds, each phase at 15 minutes, and total timing/control execution at 45 minutes; exceeding a cap produces an incomplete result. No auto-growing iteration loop or index rebuild is part of timing.

Keep both processes resident with indices open, but only one active search at a time. Pin both workers to the same available logical CPU, record its core/SMT topology, and keep the parent off that core. Warm both indices before the first timed block. Run same-binary controls for both engines, with the identical schedule and both starting orders. A shared-protocol NOP may characterize the IPC floor; report it without subtracting it from tiny query times. Retain raw per-request samples, block/order medians and paired uncertainty; report observed control spread and do not claim differences below it. Initial failures do not become a speed claim by pooling categories.

Record compiler, explicit release opt3/LTO/overflow/native-CPU flags, unset encoded flags, source/HEAD/lock/manifest/binary SHA256, index identity, affinity, startup receipts, request/response logs and hashes, timeout status, and controller source SHA. Refuse overwrite. Read the same provenance before/after; no own build, full hash, index proof scan, or other benchmark runs during timing. Leave user processes intact and record observed load. This is an end-to-end warmed-query metric, not isolated scoring cycles or peak-RSS proof.

## Minimal upstream comparator correction

Two whole shapes are viable; keep this upstream fix separate from the new balanced protocol.

| Shape | Change | Proven contract / limit |
|---|---|---|
| A: strict old four-column dump | Change only Python comparator. Parse both dumps, validate KIND+terms+row count, equal hit lengths, unique valid IDs, finite f32, then compare every ordered ID and recovered raw f32 bit pattern and total value. Use explicit exceptions and nonzero exit on any mismatch. | Smallest reviewable fix, compatible with existing nine-digit scientific f32 output. Proves equality of serialized totals; relation is absent, so cannot prove exact-count semantics or relation equality. |
| B: versioned relation-aware dump | Change both Rust/Java dump producers and comparator. Emit total value plus `eq`/`gte`, and preferably score bits. Reject unknown versions/relations. Require complete payload equality. | Stronger Lucene-versus-port contract, more moving parts; `gte` values are lower bounds, not COUNT. Still needs separate exact COUNT if that is the asserted service. |

Recommend A immediately, documenting its relation limit. The present comparator ignores KIND on the second side, compares Python float values instead of raw bits, truncates score comparison with `zip`, does not flag score-only differences, and exits successfully on mismatches. Fix those categorically in one parse-and-compare boundary. Add small synthetic fixtures for mismatched KIND, query/row/hit lengths, signed zero, score-only difference, invalid/duplicate IDs, NaN/infinity, totals, and a valid round-trip. Ordinary strict equality should include `+0/-0` bits. Do not infer pruning parity from equal lower-bound totals. B is appropriate when the upstream claim is specifically total-hit relation parity; it should not be forced into the Tantivy top-k-only response.

## Module map and judgment

Three deep boundaries suffice: a shared protocol module owns validated wire types and encoding; each engine adapter owns index opening/profile receipt/AST conversion/public search; one parent controller owns gates, scheduling, clock, affinity, persistence and artifacts. A standalone strict comparator owns upstream legacy dump validation. No generic benchmark plugin framework, per-operation wrapper classes, or copied scorer implementation is needed.

| Criterion | Grade | Reason |
|---|---|---|
| Payload/semantics equality | Conditional strong | Full physical/postings + exact count/bits/order; matched top-k requires approved threshold API or is withheld. |
| Timing fairness | Strong | Same persistent protocol, parent clock, balanced direct AST parsing and payload, fixed orders and controls. IPC floor is visible. |
| Minimal adapter complexity | Moderate | Two small adapters and shared wire boundary; threshold API is an explicit extra dependency. Legacy comparator A is very small. |
| Runtime correctness proof | Strong design, unexecuted | Full identity proof, exact results and exhaustive oracle before timing; no transferred old binary proof. |
| Bounded reproducibility | Strong | Frozen inputs, finite schedule/deadlines, exclusive outputs and before/after provenance. |

Synthesis recommendation: take persistent-protocol B when warmed caller latency is the desired metric; retain native search timers only as a separately named diagnostic. Choose strict legacy comparator A for the immediate upstream correction, then relation-aware B only if the upstream assertion needs it. Resolve the threshold policy before authorizing any matched TOP10 timing.

## Source grounding

- Port `bench/rust/src/bin/bench.rs` and `bench/rust/src/lib.rs`: preloaded direct AST, search-only clock, existing dump serialization. `bench/rust/src/bin/index.rs` / `bench/scripts/prep.py`: whitespace/positions options and empty-document dropping.
- Port `src/search/searcher.rs:19,163,197`, `src/search/collector.rs:54,118`: fixed threshold1000, private scoring path, public exact count, threshold-gated competitive scores. `src/sim.rs:63–87`: fixed parameters and IDF arithmetic.
- Port `bench/scripts/compare.py`: legacy comparator defects; `bench/scripts/bench-all.sh`: fixed Rust-first ordering.
- Tantivy `doc/performance/lucene-10.4/do_query.rs:166–246`: per-line QueryParser, scalar TOP10 acknowledgment and tuple count collector. `shared/bm25_profile.rs`: exact profile receipt. `shared/wiki_physical.rs`: physical tuple verifier.
- Tantivy `src/query/bm25.rs:154`: native IDF arithmetic. `doc/performance/lucene-10.4/wiki_docorder.py:90` and `build_index.rs:270–307`: exact replay schema, constrained text, every-row indexing. Retained evidence paths above were read as receipts, not rerun.
