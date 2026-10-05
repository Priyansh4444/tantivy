# Overlap policy and BM25 parameters — October 4, 2026

Production source **872c9f55dcc91034dcf8c15fe060dd0b1cfd3ccd**, pinned Lucene **10.4.0**.
This checkpoint adds persisted overlap norm policy and immutable global/per-field
BM25 query parameters, then repeats the frozen native correctness and latency gates.
All measurements below use native DEFAULT 1.2/.75 scores and collection statistics,
with no score scaling or statistics overrides. These results do not measure
nondefault parameter performance or establish complete Lucene feature/API parity.

| Measurement | Tantivy process first | Lucene process first |
| --- | ---: | ---: |
| COUNT T/L geometric mean | 0.76134 (1.31×) | 0.75915 (1.32×) |
| TOP_10 T/L geometric mean | 0.54836 (1.82×) | 0.55893 (1.79×) |
| Lower COUNT medians | 20/20 | 20/20 |
| Lower TOP_10 medians | 20/20 | 20/20 |

The narrowest measured margins are:

- count-tantivy-first: `+the +of`, T/L 0.9107.
- count-lucene-first: `+new +york`, T/L 0.9390.
- top_10-tantivy-first: `+griffith +observatory`, T/L 0.9421.
- top_10-lucene-first: `the american`, T/L 0.9483.

Background load remained active. Small margins and process-order variation are
load-sensitive; this is a comparison with Lucene on the recorded workload, not
an isolated speedup claim over the previous Tantivy checkpoint.

## Behavior and compatibility

New text indexing defaults to `FieldNormPolicy::DiscountOverlaps`; overlapping
tokens still contribute to frequencies and full collection token totals. Basic
fields retain unique encoded-term norms. Explicit `CountAllTokens` persists the
historical Boolean schema shape. Old Boolean/missing norm schemas deserialize to
CountAll and preserve their append behavior. Discount persists a typed object
in the existing schema slot; an actual pre-fix format-11 reader rejects it even
in a zero-segment index. Existing norm bytes are not reconstructed: changing
index-time norm policy requires reindexing. The postings footer remains 11.

`Bm25Parameters` validates Lucene's finite nonnegative k1 and b in [0,1], retaining
signed-zero bits. Immutable Searcher builders support global and per-field
settings without changing shared readers or old clones; fresh reader handles use
DEFAULT. Coherent snapshots carry parameters and their native/classic arithmetic
policy. Supplied statistics providers remain authoritative. DEFAULT public classic
weight factories preserve their historical convention. Nondefault profiles reject
stored DEFAULT pairs and use conservative bounds, which may reduce pruning.
Literal exceptional scalar outcomes match Java; NaN collector ordering is not certified.

The [overlap reference](../../parity/overlap-norm-reference/README.md) includes an
eight-case matrix: two index-time overlap policies crossed with four query profiles,
with the query-time Java discount flag deliberately opposite the indexing flag.
Both engines preserve physical norm bytes, full token totals and frequencies;
raw score bits, ranked document order, COUNT and explanations match exactly.
At k1=0, the CountAll fixture becomes a tie and changes ranked order as Lucene does.
The [parameter reference](../../parity/bm25-parameters-reference/README.md) retains
the runtime red, constructor domain/getter checks, actual IndexSearcher queries
and 30 scalar digests over all 256 norm IDs, fractional/zero frequencies and boosts.

## Verification

The combined production source passes **1,360 normal library tests** (seven
ignored, fixture-generating create_format filtered), plus **37 release integrations**.
The final added cross-feature test passes in debug and release on the isolated
combined base; the final overlap suite also passes **12/12 on main in release**.
Together the main release selection and final overlap target cover 38 distinct
integration tests. Five private parameter tests also pass in debug/release on the
cross-feature tree. Receipts and logs are adjacent. Full-source formatting and
diff checks pass. A separate source review traces cached-pair, complete-block,
loaded-tail, signed-zero, provider and boost bound paths.

The unchanged direct-AST gate remains **16 configurations, 330 queries, zero
structural and zero raw-score failures**. Frozen case SHA is
`f632da13074c6556bcd9a2ba7605d7974f96c5bbab455100dfb711cfa30c8826`;
cases and both raw engine dumps are retained as synthetic-*.gz. The obsolete
2.2 conversion diagnostic still fails 301 queries and is not an acceptance ruler.
All **20 Wiki queries** pass exact hit counts, top-ten ID sets, order agreement
outside permitted score ties, and optimized-versus-exhaustive checks. Maximum
relative common-top-100 raw-score difference is `5.71791025e-08` against the
unchanged 2e-6 tolerance. Timing TOP_10 replies are protocol markers; correctness
is established separately by [wiki-correctness.json](wiki-correctness.json).

## Corpus, timing and efficiency

This uses the same corrected Wiki1M replay as the [preceding native report](../2026-10-04-native/README.md).
Its lowercase ASCII corpus has one million documents, SHA
`2b630549676f1c58a579017b6cd949e25115fe63989f9e75cadadc5c1a1a8238`.
All 1,642,896 DF/TTF rows and all one million stored ID/u64-sort/norm rows matched
exactly in that report; the index bytes and production posting/norm decoder are
unchanged by these units. The retained full-logical proof is inherited, not a
new full dictionary/position walk. The corpus contains no overlap tokens, so the
new norm default does not alter it. Arbitrary Unicode analysis/token graphs and
exhaustive cross-engine positional hashing remain outside this proof.

Four serial runs pin persistent processes to CPU4, warm 40 seconds, then record
256 samples/query with seed23 shuffled queries and alternating engine order.
Both process-creation orders are measured, query caches disabled. Median client
pipe-request latency includes parsing and collection. Rust is release opt-level3,
LTO, target-cpu=native, rustc1.101 nightly/LLVM23.1.1; Lucene uses the unchanged
native adapter/default JVM heap/ParallelGC. Exact commands, raw samples and host
captures are retained. Compiled do_query SHA: `08def66818616d8d83449a9af1582e4d45beea3ff80b8842b60fafb6ca7e6802`.
The native binaries are built at the production SHA above; subsequent changes
before the report are acceptance tests/reference documents only.

No own build, test or indexing job ran concurrently with serial timing.
Preparation completed before it; the final main-only regression rerun followed it.
User background workloads were untouched. Start/end one-minute load averages
across latency runs were 3.37–4.74.

Equivalent logical index sizes remain **619,508,124 bytes Tantivy**
versus **626,810,511 bytes Lucene**, a **1.17%**
reduction. This is a corpus/index-layout comparison, not an isolated codec saving.
See [storage.json](storage.json) for per-file sizes.

| Warmed query process | RSS, MiB | High-water RSS, MiB | 20-second rounds |
| --- | ---: | ---: | ---: |
| tantivy | 14.78 | 14.78 | 810 |
| lucene | 366.56 | 372.81 | 317 |

[Memory receipts](memory.json) use a separate serial 20-second COUNT/TOP_10
warmup per engine. RSS includes mmap pages and depends on the default JVM
heap/GC phase; no constrained-heap, universal peak, startup, indexing, merge or
concurrent-throughput efficiency claim follows.

## Remaining scope

The [acceptance ledger](../../parity/ACCEPTANCE.md) keeps bounded results separate
from the immutable 47-family/all-public-API inventory. Arbitrary Similarity hooks,
token graphs, spans/intervals, vector/BKD/spatial search, joins/grouping/suggest,
other module contracts, Java facade and Lucene file interchange remain unfinished.
No feature family is exhaustively certified.

## Per-query median ratios

Values below one favor Tantivy; raw samples are adjacent.

| Query | COUNT Tantivy first | COUNT Lucene first | TOP_10 Tantivy first | TOP_10 Lucene first |
| --- | ---: | ---: | ---: | ---: |
| `the` | 0.7537 | 0.7245 | 0.7323 | 0.7953 |
| `of` | 0.6567 | 0.6705 | 0.3777 | 0.4083 |
| `and` | 0.8045 | 0.7358 | 0.3752 | 0.4073 |
| `united` | 0.7596 | 0.7721 | 0.7020 | 0.6741 |
| `states` | 0.8339 | 0.8055 | 0.6238 | 0.6660 |
| `american` | 0.7720 | 0.7638 | 0.5766 | 0.5784 |
| `york` | 0.7120 | 0.7819 | 0.6500 | 0.6587 |
| `saxophone` | 0.7240 | 0.8479 | 0.9043 | 0.8233 |
| `+the +of` | 0.9107 | 0.8963 | 0.3841 | 0.3974 |
| `+united +states` | 0.7256 | 0.7490 | 0.2428 | 0.2702 |
| `+new +york` | 0.8890 | 0.9390 | 0.3841 | 0.3855 |
| `+the +american` | 0.8533 | 0.8933 | 0.5014 | 0.5076 |
| `+the +saxophone` | 0.6366 | 0.5650 | 0.7405 | 0.6834 |
| `the of` | 0.8896 | 0.8762 | 0.3682 | 0.3912 |
| `united states` | 0.6824 | 0.7278 | 0.2481 | 0.2713 |
| `the american` | 0.8408 | 0.8375 | 0.9067 | 0.9483 |
| `"united states"` | 0.6820 | 0.6789 | 0.7034 | 0.6994 |
| `"new york"` | 0.8598 | 0.8622 | 0.6916 | 0.6990 |
| `+griffith +observatory` | 0.7669 | 0.5801 | 0.9421 | 0.9010 |
| `griffith observatory` | 0.5822 | 0.6223 | 0.7769 | 0.7469 |
