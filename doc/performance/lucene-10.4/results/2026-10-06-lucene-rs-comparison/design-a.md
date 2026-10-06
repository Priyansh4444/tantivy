# Candidate A: native search timing on the retained physical Wiki corpus

Read-only architecture candidate. No source changes, builds, index writes, or timing runs were performed for this design. Inspected port commit `465ea8f77f4397e35de56b7f7d202158d59e5871` and our current checkout (requested accepted head `c0efbc99`).

throughput checkpoint: n/a, read-only investigation

Investigation todos:
1. Trace the relevant source, configuration, and runtime path. For motivation questions, inspect the change history and rationale.
2. Throughput checkpoint stays one line: `throughput checkpoint: n/a, read-only investigation`.
3. Produce an evidence-backed explanation (Overview / Key Concepts / How It Works / Where Things Live / Gotchas), or a recommendation with a tradeoffs table if the request is a decision between alternatives.
4. Apply a plain-writing pass to the reply.

Architect phases: Ground completed; Sketch completed; Agree skip (root synthesis owns selection); Implement skip (read-only assignment); Scrap pending (only if implementation exposes wrong assumptions).

## Caller first

The caller selects pinned production sources, obtains a validated corpus/index identity, and asks one controller to run correctness before timing:

```text
python balanced.py prepare --tantivy-source tantivy-pr2937 --port-source <isolated fixed port>
python balanced.py verify  --manifest run/manifest.json
python balanced.py measure --manifest run/manifest.json --cpu 4 --warmup 40 --samples 256
python balanced.py report  --manifest run/manifest.json
```

The actual commands should be rooted in absolute paths in the retained command receipts; this sketch does not require those literal CLI names. `prepare` creates an absent output directory and never writes an existing index. Each stage has exclusive outputs, source/index hashes before and after, bounded child lifetimes, and an explicit predecessor receipt. A failed correctness gate makes `measure` impossible. Keep an unfixed-port correctness run and its failures independently, then measure the corrected version after its exact gates pass. Do not erase the original evidence.

One measurement request to each native adapter loads queries and constructs engine ASTs before its timer starts. Opening the index, JSON parsing, AST construction, ID retrieval, output serialization, and exact counts used only for verification are outside top-10 timing. Each timed search starts from a fresh weight/scorer/collector using the retained opened reader. This measures a warm application API search, including weight and collector creation and result allocation. Stop the timer on return; consume results with `black_box` and release them outside the timing region, identically in both adapters. Record the raw integer nanoseconds, never rounded microseconds as the only evidence.

## Grounding and feasible corpus path

Use `/home/pronsh/Coding/playground/search/bench/native-profile-aligned-oct05/aligned.jsonl`, not the port's 469k benchmark corpus. The retained replay receipt says 1,000,000 documents, 294,827,020 tokens, 917,578 nonempty text documents, 1,831,702,900 raw bytes, SHA-256 `5a04d4c5f6e418e8d0cf035f27dda8f9781e852ef41adb133237359fca263961`. Original corpus `/home/pronsh/Coding/playground/search/bench/wiki-1m.jsonl` hashes to `2b630549676f1c58a579017b6cd949e25115fe63989f9e75cadadc5c1a1a8238`.

Reuse our `/home/pronsh/Coding/playground/search/bench/wiki-1m-native-aligned-v11-oct05.idx` after validating immutable files against the retained ruler. This avoids another Tantivy rebuild and preserves its physical doc IDs. The port builds a new index from the replay JSONL in exactly that order, with field `text`, `FieldType::TEXT` (positions/frequencies/norms), and `WhitespaceAnalyzer`.

The replay validation restricts text to `[a-z ]*` and token length <=255. Thus the port's Java-whitespace split and our SimpleTokenizer+lowercase+RemoveLongFilter(limit256) produce the same term bytes and successive positions on this domain. Port `DocumentsBuffer::invert` starts each field position at -1, the analyzer emits increment1, and its length counts positive increments. Empty documents retain norm0. Port `merge.rs` merges adjacent segments and `doc_maps` traverses readers/documents in order. These are good implementation evidence; the resulting payload must still be audited at runtime.

Use port `max_buffered_docs(25_000)` and a comparable 500MB approximate RAM buffer to avoid inheriting its benchmark's 4GB setting. Automatic adjacent merges preserve order; `force_merge(1)` then `close()` yields one segment. Index build is preparation, not indexing-performance evidence; allocation/accounting/buffering differ materially. One index writer is single-threaded here. No deletion, sorting, or remapping.

Preserve each source row's external ID as a stored-only `id` field, not an indexed keyword field: our retained ID is stored-only, so an indexed ID would add a second postings field for the port. Preserve `sort_field` as canonical decimal stored text if a physical tuple receipt is desired, because the port has no u64 docvalues/fast field. Our index does have that fast field. This is equal text search payload but unequal auxiliary indexing capability: do not claim whole-index storage or indexing efficiency parity from it. Top-10 never reads these auxiliary fields in the timed region.

## Core shapes and boundaries

```rust
// Shared benchmark module, free of engine imports.
struct QueryId(u32);
struct TermBytes(String); // strict nonempty [a-z]+, <=255 bytes
enum QuerySpec {
    Term(TermBytes),
    And(Vec<TermBytes>), // >=2, ordered, duplicates retained
    Or(Vec<TermBytes>),  // >=2, ordered, duplicates retained
    Phrase(Vec<TermBytes>), // >=2, adjacent exact positions
}
enum Operation { Top10, ExactCount }
struct QueryCase { id: QueryId, spec: QuerySpec, tags: Vec<String> }
struct Hit { physical_doc: u32, score_bits: u32 }
enum ReportedHits { Exact(u64), LowerBound(u64) }
struct SearchObservation { query: QueryId, hits: Vec<Hit>, reported: ReportedHits }
struct VerifiedCase { query: QueryId, exact_count: u64, top10: Vec<Hit> }
struct TimingSample { query: QueryId, operation: Operation, iteration: u32, elapsed_ns: u64 }

fn read_query_manifest(path: &Path) -> Result<Vec<QueryCase>, Error> { /* not implemented */ }
fn verify_observations(expected: &[VerifiedCase], actual: &[SearchObservation]) -> Result<(), Error> { /* not implemented */ }
fn summarize_pairs(first: &[TimingSample], second: &[TimingSample]) -> Result<Summary, Error> { /* not implemented */ }
```

Each executable owns its engine-specific query objects/searcher; avoid a trait abstraction that only forwards `search`. One shared module validates query semantics and defines receipt/sample wire types. Tantivy's query construction creates `TermQuery(...WithFreqs)` / `BooleanQuery` with all Must or all Should / `PhraseQuery` with positions0..n-1. Port constructs `Query::term`, `BooleanQuery::must/should`, and `PhraseQuery::term`. Field names are both `text`. Query rewrite inside normal search remains timed in both engines; no pre-created weight or scorer. Both use default BM25 k1=1.2,b=.75,scale1 and physical nonempty field N/TTF. Port has fixed BM25 constants, so k09/k25 are not cross-engine categories until a separate properly validated parameter feature exists.

## Module map

| File/module | Ownership and purpose |
|---|---|
| `shared/spec.rs` | Strict direct-AST query file, wire receipts, no engine dependencies |
| `tantivy_adapter.rs` | Pin current core via path dependency; reuse opened v11 index; build query AST; optimized TopDocs, Count, independent exhaustive scorer for gate |
| `port_adapter.rs` | Pin corrected port via path dependency; build replay index; reader stats/norm/payload proof; AST, public search/count; exhaustive reference proof |
| `balanced.py` | Serial lifecycle owner, source locks, bounded commands, CPU affinity, raw samples and before/after identity |
| `verify.py` | Exact physical/payload/score/order/count comparisons, invalid-input rejection, failure receipts |
| `queries.jsonl` | Frozen query IDs/specs/tags, hashed before build/run |
| `run/manifest.json` | Source commits+diffs, all compiler/build/profile/lock/index/corpus/query hashes and command identities |

The verifier may import/consume existing payload framing logic rather than adding another notion of equality. There is no new persistent service, engine compatibility layer, or generalized benchmark framework.

## Correctness proof before performance

1. Build fresh adapters for current `c0efbc99`/core `1ac1c323`, not the existing P021 `7322109` binary. Capture `rustc -Vv`, Cargo, both lockfiles and dependency identities, src/Cargo/config tree, build invocation and binary hashes. Use the same actual compiler, `target-cpu=native`, opt3, fat LTO, codegen-units1, overflow-checksfalse, no profiling feature, common panic strategy. Clear inherited CARGO_PROFILE_* and encoded Rust flags before explicit values; stable profile parity matters more than copying the port Cargo default blindly.
2. Validate replay bytes and every ordered physical row against `/search/bench/native-profile-aligned-oct05/lucene-physical.tsv` SHA `6f7082e3ff479c7be7fe6f819857716630fc4b6b64acc2d6005f1107127029c7`. Port `DirectoryReader` exposes segment list, document retrieval, collection_stats/term_stats; `SegmentReader` exposes norms/term_meta/postings. Assert one segment, no deletions, max_doc=num_docs=1M, N917578, TTF294827020, all norm bytes, external IDs and physical order. Check integrity.
3. Full term/postings proof is feasible without a port core API change: feed the retained sorted term list `/search/bench/native-profile-aligned-oct05/logical-comparison/tantivy_terms.tsv` (first rows `a\t784792\t6562839` etc.) through public `term_meta`. Match every df/ttf; assert `field_stats.num_terms==1,642,896`, count matches all successful distinct lookups and header sums, so no additional terms fit the header cardinality. Read every posting with positions. Port public `PostingsEnum::next_doc/freq/next_position` supports the same canonical framing as `payload_identity.rs`: length-u64+`canonical-postings-v1`, length-u64+field `text`, for each term length-u64+termbytes, df-u32, each doc-u32/TF-u32/position-count-u64/positions-u32; final term-count-u64. Expected digest `893a75958e6d15c997d414d5fb5fc47c633aa18c68fcd33bc0f8e13abfa8dbb8`, 126,703,012 postings, 294,827,020 positions. This is substantial preparation CPU but bounded and yields the strongest payload equality proof. Validate decoded df/TF/order/positions, not only hashing corrupt decoder output.
4. Frozen Wiki20 query set is already available at `/search/tantivy-pr2937/doc/performance/lucene-10.4/queries-wiki.jsonl`; translate once to strict specs with IDs and retain the translation. Use all20 (eight term, six AND, four OR, two phrase), not a hand-picked subset. Empty/absent term, repeated terms, and a missing required clause are correctness-only extra cases. Port original 1,201 queries cannot be replayed exactly unless their untracked `bench/data` query files exist/reconstructed under a frozen generator/input receipt; don't invent missing query corpus provenance.
5. For every query, exact `count` must equal; top10 physical doc IDs, order and raw f32 bits must equal and all scores finite. Dump via `.to_bits()` in Rust. Check each optimized top10 against exhaustive scoring independently, retaining all hits or a trustworthy exact top10 heap with doc tie rule. Existing Tantivy `do_query.rs` exhaustive validator uses 1e-5 tolerance: do not reuse that tolerance for this strict gate. Native oracle from proven tuples and literal scoring expression is an option for the port because its Weight/scorer internals are private; enumerate required/optional/phrase matches from public postings, use Bm25 for raw leaf score arithmetic, accumulate Boolean children in the verified Lucene/Tantivy order. For exact phrase repetitions, use a separate phrase frequency implementation with established tests, or gate against a Java reference using the shared proven physical payload.
6. Retain strict-score failures as failures, not as 'close enough' performance wins. The port's ln_1p issue must be corrected and Java scalar witness tests pass first. A passed Wiki20 alone cannot demonstrate universal parity.

## The collector mismatch that cannot be hidden

Port `IndexSearcher::search(q,10)` always uses `TopScoreDocCollector::new(10, TOTAL_HITS_THRESHOLD=1000)`. It reports a hit count that becomes a lower bound once competitive pruning is published. Our TopDocs-only operation collects top10 with no count. Our `query.count` is a separate exact operation; port public `count` also exists, but its implementation walks live matching documents with zero-boost weights. Our single-term count may be a DF fast path. Both count APIs provide the same exact result; their different algorithm is the performance property being compared.

Recommended initial measurement: unmodified public top10 APIs, label the category **top10 search, port additionally tracks 1000 hits**, and separately time exactCOUNT. It is a useful same-machine comparison of the available APIs, but cannot be advertised as identical collector work. Verify each port reported hit value against the independently exact count: EqualTo => equality, GreaterThanOrEqualTo => lowerbound<=exact. Do not demand port lowerbound equals Tantivy exact count.

For a second strictly closer top10-work experiment, add a separate small optional port method accepting total hit threshold, with existing `search` delegating threshold1000 and the benchmark requesting0. Collector already accepts arbitrary threshold; this does not require changing scoring algorithms. It is nevertheless a port source change and must be separately labeled and gated against default-threshold results/exhaustive results before timing. Avoid altering Tantivy to count1000 through Count composition: Count disables or changes pruning and is not the same threshold collector. Exact top10 + exactCOUNT composition may be added only as a separate category.

## Timing controls and reporting

- Use one CPU (existing CPU4 baseline) for each serial process, same affinity, native profile and one search thread. No cargo/indexing/oracle/capture jobs while measuring. Leave user applications untouched.
- Warm both opened mmap indexes with the same full query list and40 passes. This establishes warmed workload, not page-cold performance. Record open time separately; don't combine with query latency or promise identical mmap working sets.
- At least three paired rounds AB and BA, same query order seeded/frozen within pair. Retain every raw sample; do not trim outliers or select 'best' rounds. If copying existing256-sample ruler, reuse its statistical summary definitions and protocol fake-tests, but these adapters time inside the process so old stdout roundtrip latency cannot be compared numerically to new samples.
- Include same-engine repeat controls TT and PP under the same AB/BA sequencing, before or adjacent to measured pairs. They quantify CPU/load drift; user Steam contention must remain an explicit limit. Log loadavg, safe command names/%CPU, CPU topology/profile, per-process CPU and page faults. Don't try to eliminate noise by killing user processes.
- Compute per-query median ratio and geometric mean across all predeclared queries, counts of wins/losses, p50/p90/p99 raw distributions, and both process-order ratios. Keep top10 andCOUNT separate; don't blend into one speed score. Retain mean-of-query-medians as an additional metric if comparing their publication style, named precisely.
- Report current fork vs fixed port, and optionally original port timing separately after accuracy caveat. Our prior Lucene ratios and their published ratios are background only, never substituted into the same-machine comparison.
- RSS can be sampled after identical warmup and exposed as warmed process RSS, while peak memory/full-lifecycle allocations/indexing throughput require separate measurements. Total index bytes include unequal auxiliary field capability; don't claim storage winner from that number.

## Minimal robust upstream comparator correction

The existing `bench/scripts/compare.py` parses four columns `kind,terms,total,hits`, checks length/term identity with assertions, compares rounded f32 values with Python float equality, prints document/count diffs, and always reaches successful exit on mismatches. Fixing exit behavior does not prove their retained logs had mismatches. Their published zero-difference logs can remain accurate while the script currently fails to enforce them.

Smallest defect fix: `main(argv)->int`; explicitly validate complete kind+term identity and row count (do not use assertions, which disappear under python -O); compare every corresponding hit's doc and raw f32 bits; retain hit-vector length mismatch; print diagnostics for score-only mismatch; return1 if any mismatch,0 only if every row matches. `float32_bits` uses `struct.pack('<f', float(s))` plus `struct.unpack('<I',...)`, rejects non-finite and overflow as malformed input, preserving signed-zero identity. Malformed inputs => nonzero error distinct from ordinary mismatch if desired. Existing decimal output9 significant digits roundtrips finite f32; a future hex dump avoids interpretation ambiguity but is not needed for the basic fix.

Relation fix changes the benchmark wire format in both `bench/rust/src/bin/bench.rs` and `bench/java/Bench.java`: append a normalized `eq|gte` column, preserving the first kind/terms columns. Compare relations explicitly and don't call approximate topdocs counts exact. A strict bit/row dump match gate may require equal values+relations as an implementation regression expectation. A semantic cross-engine gate must instead independently verify exact counts and each lowerbound; two valid different lowerbounds are not an incorrect document count. State which gate is used.

Meaningful comparator tests use subprocess execution, including python -O, so exit status is actually checked: identical pass; each independently mutated query kind, query text, rowcount, hitlength, doc order, docID, one-ULP score, signedzero, count, relation fails; score-only emits diagnostic; NaN/Inf/badfloat/badinteger/badcolumn count reject. Add the Java IDF witness regression separately; comparator tests should not mirror its counter implementation.

## Tradeoffs and feasibility assessment

| Criterion (1 weak to5 strong) | A | Reason |
|---|---:|---|
| Identical text payload/semantics | 5 | Complete physical norms/docorder/postings/position digest; exact count and rawscore gates |
| Timing fairness | 4 | Same compiler/CPU/timer scope; public top10 threshold mismatch explicit, threshold0 controlled tier possible |
| Minimal adapter complexity | 3 | Two small timing adapters, but full port payload verifier and exhaustive phrase gate are real work |
| Runtime correctness proof | 5 | Validated actual decoder payload plus optimized/exhaustive/Java result gates |
| Bounded reproducibility | 4 | Existing1M corpus/index frozen; full proof/build can consume minutes and disk, stage bounds and exclusive outputs required |

Public term enumeration is private in the port (`SegmentReader::terms_iter` crate-private), but known sorted term list plus term cardinality and public lookups is sufficient for this frozen same-corpus audit. Use public code rather than exposing another core API just for verification. Full positional decode costs ~295M positions; do not claim full proof if shortening it to Wiki20 terms. A bounded first fixture/small slice can prove the adapters, but is a separately sized workload and requires rebuilding both engines with the same reduced physical order.

The largest implementation uncertainty is strict Boolean/phrase leaf accumulation and tie semantics across engines, not timing itself. Resolve with actual gates before changing scorer arithmetic. Do not relax those gates to make the benchmark run. Query public-API top10 is immediately feasible; exactly identical collector semantics requires the separate threshold0 experiment. The fixed port parameters prevent comparison on our custom-BM25 loss categories today.

## Source grounding

- Port `bench/rust/src/bin/{index,bench}.rs`, `bench/rust/src/lib.rs`: input/index/query/timer protocol.
- Port `src/search/searcher.rs`: threshold1000, public `search` and `count`, rewrite and private weight construction.
- Port `src/search/collector.rs`: threshold publication and lowerbound relation, docID tie encoding.
- Port `src/index/{buffer,merge,reader,segment}.rs`: position/norm construction, adjacent-order merge, public stats/postings/norm access.
- Port `src/document.rs`: TEXT positions/norms and stored-only field support; no u64 docvalues.
- Port `src/analysis.rs`: max255 whitespace segmentation; `src/sim.rs` fixed BM25 and numeric operations.
- Port `bench/scripts/compare.py`, `bench/java/Bench.java`: current non-gating comparator and relation omission.
- Our `doc/performance/lucene-10.4/{build_index,payload_identity,do_query}.rs`, `shared/{wiki_physical,bm25_profile}.rs`, `wiki_docorder.py`, `queries-wiki.jsonl`: existing corpus/index/profiles/proofs and timer-interface limitations.
- Retained `native-profile-aligned-oct05/{aligned.jsonl.replay.json,physical-payload-comparison.json,logical-comparison/summary.json}`: already measured identities. This design read the receipts, not the entire1.83GB replay/index anew; fresh preparation must verify them.
