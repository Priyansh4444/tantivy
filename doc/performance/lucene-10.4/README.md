# Wikipedia 1M comparison with Lucene 10.4

The [October 4 configuration checkpoint](results/2026-10-04-configuration/README.md)
adds native overlap norm policy and immutable per-field BM25 parameters. It retains
the frozen correctness gate and DEFAULT latency leads after integration.

The [October 4 native comparison](results/2026-10-04-native/README.md) matches
native collection statistics and raw scores on the corrected equivalent corpus.
It records both-order query wins, complete term/ID/sort/norm comparison, and
the remaining full-API scope.

The [October 3 follow-up](results/2026-10-03-followup/README.md) adds measured
COUNT improvements and smaller frequency blocks to the
[corrected baseline](results/2026-10-03/README.md). Both reports include matching
collection statistics, cross-engine correctness checks, both process orders,
and raw samples.

These tools compare COUNT and TOP_10 for the fixed 20 queries in
[`queries-wiki.jsonl`](queries-wiki.jsonl). Use the same first one million Wikipedia
documents in both indexes, one segment per index, and no query cache. Existing
indexes are required; these tools do not download the corpus or build indexes.
The Tantivy index must have the frozen schema's `text` field and stored string
`id` field. Its single `.idx` file contains the text field's eight-byte token total in
composite entry `(text, idx=0)`; field metadata can precede that entry.

For an equivalent native corpus, use the
[configured ASCII replay and full logical comparison](configured-wiki.md).
It corrects the historical 55-token analyzer difference and preserves the old
indexes as compatibility fixtures.

The historical reports above use a **matched scoring comparison**, with BM25 `k1=1.2`, `b=0.75`, the same
document population and average field length, and the same score scale. It is
different from the search-benchmark-game Lucene adapter's BM25 `0.9/0.4` and from
Lucene's ordinary treatment of documents without indexed text. Consequently,
earlier results with different statistics or parameters cannot establish this
comparison's speed ratio.

## Prerequisites

- The maintained Tantivy fork at the commit being measured.
- The search-benchmark-game `engines/lucene-10.4.0` directory, including
  `src/main/java/DoQuery.java`, its Gradle build, and the Wikipedia 1M `idx/`.
- The matching Tantivy Wikipedia 1M index, using this fork's format.
- Rust, Python 3.11 or later, a JDK supporting Lucene 10.4 and `jdk.incubator.vector`
  (Java 21 or later), and Linux `taskset` when CPU pinning is enabled.

Set these paths from the Tantivy fork root:

```bash
PERF_FORK="$PWD"
PERF_TOOLS="$PERF_FORK/doc/performance/lucene-10.4"
PERF_LUCENE="/absolute/path/to/search-benchmark-game/engines/lucene-10.4.0"
PERF_INDEX="/absolute/path/to/wiki-1m-position-pfor-current.idx"
PERF_ADAPTER="$PERF_FORK/target/wiki-perf-adapter"
PERF_CLASSES="$PERF_FORK/target/wiki-perf-lucene-classes"
PERF_RESULTS="$PERF_FORK/target/wiki-perf-results"
mkdir -p "$PERF_ADAPTER/src/bin" "$PERF_RESULTS"
```

Build the existing game engine's Java classes and dependencies:

```bash
gradle -p "$PERF_LUCENE" classes copyDependencies
python "$PERF_TOOLS/prepare_native_lucene.py" \
  --lucene-dir "$PERF_LUCENE" --output-dir "$PERF_CLASSES"
```

`prepare_native_lucene.py` generates `DoQueryNative.java` in the specified
output directory and compiles it with the native result dumper. The expected game
source substitutions are checked before compilation. Current binaries use
native scoring, so pass `--native-bm25` explicitly on each comparison below.

## Build the Rust protocol adapter and validator

The standalone source files keep the adapter outside the library's production
code. `do_query.rs` implements the timed stdin/stdout protocol; `validate_index.rs`
compares optimized top ten and COUNT with exhaustive alive-document scoring,
and emits stored external IDs and scores for cross-engine checks.

```bash
cp "$PERF_TOOLS/do_query.rs" "$PERF_ADAPTER/src/bin/do_query.rs"
cp "$PERF_TOOLS/validate_index.rs" "$PERF_ADAPTER/src/bin/validate_index.rs"
cp "$PERF_TOOLS/reencode_index.rs" "$PERF_ADAPTER/src/bin/reencode_index.rs"
cat > "$PERF_ADAPTER/Cargo.toml" <<EOF
[package]
name = "wiki-perf-adapter"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
tantivy = { path = "$PERF_FORK" }
serde_json = "1.0"

[profile.release]
lto = true
opt-level = 3
overflow-checks = false
EOF
RUSTFLAGS='-C target-cpu=native' cargo build \
  --manifest-path "$PERF_ADAPTER/Cargo.toml" --release \
  --bin do_query --bin validate_index --bin reencode_index
```

This fork writes format 11 metadata and retains the
[format-9 frequency block encoding](../../frequency-pfor.md). It still reads
supported older indexes. To measure storage using the frozen
existing index, rewrite its segments without retokenizing into a **new, absent**
output directory:

```bash
PERF_REENCODED="/absolute/path/to/wiki-1m-native-v11.idx"
"$PERF_ADAPTER/target/release/reencode_index" "$PERF_INDEX" "$PERF_REENCODED"
PERF_INDEX="$PERF_REENCODED"
```

Readers limited to format 10 or older reject newly written format-11 segment
files. Keep the source index if older deployments still need it.

Record the Rust toolchain, fork commit, JVM and CPU used for the run. The result
JSON also records hashes of the binary, query suite, Java classes, dependency
jars, and tool sources. These hashes identify artifacts; the `--commit` label
must refer to the source actually used to build the Rust binary.

## Check correctness before timing

```bash
python "$PERF_TOOLS/compare_wiki_correctness.py" \
  --tantivy-validator "$PERF_ADAPTER/target/release/validate_index" \
  --tantivy-index "$PERF_INDEX" --lucene-dir "$PERF_LUCENE" \
  --lucene-classes "$PERF_CLASSES" --native-bm25 --commit "$(git rev-parse HEAD)" \
  --output "$PERF_RESULTS/correctness.json"
```

The gate checks exact COUNT, internal Tantivy ranking correctness, unique IDs,
finite scores, complete result dumps, and identical cross-engine top-ten ID
sets. Scores for shared top-100 IDs must agree within `2e-6` relative tolerance.
Order differences are accepted only when both score models tie the inverted
documents within that tolerance. A separate cutoff check requires scores in
both dumps and ties in both models; the frozen suite additionally requires the
exact top-ten set. Checks remain enabled under Python `-O`.

The small fixtures test the gate itself without starting either engine:

```bash
python -m unittest discover -s "$PERF_TOOLS" -p test_compare_wiki_correctness.py
```

The library's `tests/query_pruning_correctness.rs` supplies broader regression
coverage: multiple segments, missing and empty fields, deletions, fieldnorms
disabled, score ties, negative and mixed boosts, exact phrases, and phrase prefixes.
Passing the fixed Wikipedia suite alone does not establish correctness for all
query types and index shapes.

## Measure COUNT and TOP_10

Run the following with `PERF_COMMAND=COUNT`, then with `PERF_COMMAND=TOP_10`:

```bash
PERF_COMMAND=COUNT
python "$PERF_TOOLS/suite_lucene_interleaved.py" \
  --tantivy-binary "$PERF_ADAPTER/target/release/do_query" \
  --tantivy-index "$PERF_INDEX" --lucene-dir "$PERF_LUCENE" \
  --lucene-classes "$PERF_CLASSES" --native-bm25 --cpu-core 4 \
  --command "$PERF_COMMAND" --warmup-seconds 40 --iterations 128 \
  --output "$PERF_RESULTS/$PERF_COMMAND-tantivy-first.json"
python "$PERF_TOOLS/suite_lucene_interleaved.py" \
  --tantivy-binary "$PERF_ADAPTER/target/release/do_query" \
  --tantivy-index "$PERF_INDEX" --lucene-dir "$PERF_LUCENE" \
  --lucene-classes "$PERF_CLASSES" --native-bm25 --cpu-core 4 \
  --command "$PERF_COMMAND" --warmup-seconds 40 --iterations 128 --lucene-first \
  --output "$PERF_RESULTS/$PERF_COMMAND-lucene-first.json"
```

Both persistent processes share the selected CPU, default core 4. Change
`--cpu-core` to an allowed CPU on your machine, or use `--cpu-core none` to disable
pinning. Avoid overlapping timed runs with builds or other CPU-heavy work.

The timer measures a complete request through the pipes, including query parsing
and collection. Warmup cycles across all queries for 40 seconds. Each of 128
iterations shuffles the query suite with fixed seed 23 and alternates engine
order for each pair. Results include every sample, per-query medians and the
geometric mean of Tantivy/Lucene latency ratios. TOP_10's protocol reply is `1`;
the separate correctness gate provides ranking verification.

## Snapshot warmed query-process memory

Run this separately from latency measurements and builds:

```bash
python "$PERF_TOOLS/compare_process_memory.py" \
  --tantivy-binary "$PERF_ADAPTER/target/release/do_query" \
  --tantivy-index "$PERF_INDEX" --lucene-dir "$PERF_LUCENE" \
  --lucene-classes "$PERF_CLASSES" --native-bm25 --cpu-core 4 --warmup-seconds 20 \
  --output "$PERF_RESULTS/process-memory.json"
```

The snapshot includes mapped index pages and records RSS, resident anonymous and
file pages, and process high-water RSS. It uses the default JVM heap and disabled
query cache, as in the latency comparison. These are warmed query-process
measurements, not indexing throughput or a universal peak-memory claim.

## Why the statistics and boost are matched

The historical Tantivy baseline computed BM25 population from all indexed
document slots. Lucene normally uses documents containing indexed terms in the field. In the frozen corpus this
is one million documents versus 917,578. `MatchedStatisticsSearcher` supplies
Lucene with the all-document population and Tantivy's persisted token total.
The frozen totals are 294,826,965 Tantivy tokens and 294,827,020 Lucene tokens;
the override uses the former for the shared average length. Actual postings,
term frequencies and norms stay in each engine's index.

Lucene's BM25 omits the historical Tantivy baseline's constant `k1+1` score multiplier. A positive 2.2
boost on TOP_* requests matches that scale. Lucene forwards this factor to the
underlying weight; it adds no per-document scoring wrapper. COUNT remains an
unwrapped query because scores are unused. `DumpLuceneResults` uses the boost
when obtaining rankings.

The override preserves Lucene's pruning bounds: Lucene stores competitive
frequency/norm pairs and evaluates them with the current query's similarity.
See the official Lucene 10.4 sources for
[BM25 scoring](https://github.com/apache/lucene/blob/releases/lucene/10.4.0/lucene/core/src/java/org/apache/lucene/search/similarities/BM25Similarity.java),
[boost propagation](https://github.com/apache/lucene/blob/releases/lucene/10.4.0/lucene/core/src/java/org/apache/lucene/search/BoostQuery.java),
and [score-bound evaluation](https://github.com/apache/lucene/blob/releases/lucene/10.4.0/lucene/core/src/java/org/apache/lucene/search/MaxScoreCache.java).


Current Tantivy `Searcher` scoring uses exact physical field populations and
Lucene's native IDF, reciprocal normalization, and raw score convention. Public
`Bm25Weight::for_one_term` constructors and custom providers using the default
field-statistics method retain the historical arithmetic and scale. Native
comparisons therefore use score scale 1; historical matched overrides describe
the earlier baseline. See [native scoring and bound compatibility](native-bm25.md)
for the format 11 migration and its verification.

Native Lucene BM25 comparisons use `prepare_native_lucene.py --lucene-dir PATH
--output-dir /tmp/lucene-native-classes` followed by `--native-bm25
--lucene-classes /tmp/lucene-native-classes` on the timing, correctness, or memory
tool. This path uses Lucene's unmodified `IndexSearcher` collection statistics,
BM25 defaults k1=1.2 and b=.75, score scale 1, and a disabled query cache. The
protocol COUNT query is unwrapped. It does not read Tantivy's token header or
instantiate `MatchedStatisticsSearcher`. `DumpNativeLuceneResults` writes the
actual text maxDoc, docCount, and sumTotalTermFreq to stderr.

`--native-bm25` and `--matched-bm25` are mutually exclusive. Correctness and memory
retain their historical matched default; the timing suite retains its original
game-adapter default (k1=.9/b=.4). Explicit `--matched-bm25` preserves the earlier
statistics overrides and score scale 2.2. The matched path now locates the text
field's idx=0 entry through the actual composite directory and outer footer,
including when version 10 or 11 metadata precedes the postings. Its header is a stored
value, not independent proof of exact legacy merged-index statistics.

For diagnosis before Rust's native score change, native mode alone accepts
`--lucene-score-scale 2.2`. This explicitly scales TOP scores while retaining
Lucene's native collection statistics; reports record the scale and must not be
presented as default native score parity. Default native runs use scale 1.
Native provenance hashes `DoQueryNative` and `DumpNativeLuceneResults` classes;
matched provenance hashes the matched classes. Existing warmup durations,
iteration counts, alternating order, CPU selection, and COUNT/TOP markers remain
unchanged. Verify with `python -m unittest discover -s doc/performance/lucene-10.4
-p 'test_*.py'`; these fixture gates launch no search engines.
