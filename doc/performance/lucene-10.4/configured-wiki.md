# Equivalent native Wikipedia workload

The frozen transformed Wikipedia corpus contains only lowercase ASCII letters
and spaces. Its 55 occurrences of 41 words at least 40 bytes long were removed
by Tantivy's default analyzer, while Lucene retained them. The longest is 219
bytes. Consequently the historical indexes contain different token totals even
when built from the same input documents.

The configured replay uses the named `wiki_ascii_lucene` analyzer: SimpleTokenizer,
RemoveLongFilter with exclusive limit 256, then LowerCaser. Both query adapters
register this name. The builder rejects input outside `[a-z ]` and words longer
than 255 bytes. Lucene splits longer words; this configuration does not implement
that behavior or claim general StandardAnalyzer equivalence. Normal replay also
requires exactly 1,000,000 documents and 294,827,020 tokens. It creates an absent
directory and writes a separate adjacent completion receipt only after success.

## Build and verify

Use the standalone manifest/profile in the [main instructions](README.md), add
`sha2 = "0.10"` to its dependencies, and copy these additional Rust sources into
its `src/bin` directory:

- `build_index.rs`, `dump_termtotals.rs`, `dump_docmap.rs`
- `payload_identity.rs`, `payload_fixtures.rs`

Build with the same native flags and release profile:

```sh
RUSTFLAGS='-C target-cpu=native' cargo build \
  --manifest-path "$PERF_ADAPTER/Cargo.toml" --release \
  --bin do_query --bin validate_index --bin reencode_index \
  --bin build_index --bin dump_termtotals --bin dump_docmap \
  --bin payload_identity --bin payload_fixtures
PERF_BINS="$PERF_ADAPTER/target/release"
PERF_JARS="$PERF_LUCENE/build/dependencies"
python "$PERF_TOOLS/verify_wiki_analyzer.py" \
  --binaries "$PERF_BINS" --jars "$PERF_JARS" \
  --output "$PERF_RESULTS/analyzer-fixtures"
```

The real-index fixtures check 39/40/219/255-byte terms, posting positions,
long-term and phrase queries through both adapters, malformed input rejection,
zero norms, and native encoded norm 135, including Java's signed-byte promotion.
A 256-byte word is rejected. Fixture output must be absent.

Replay the original frozen JSONL into a new directory:

```sh
PERF_CORPUS="/absolute/path/to/wiki-1m.jsonl"
PERF_NATIVE_INDEX="/absolute/path/to/wiki-1m-native-equivalent-v11.idx"
"$PERF_BINS/build_index" "$PERF_NATIVE_INDEX" < "$PERF_CORPUS"
python "$PERF_TOOLS/compare_wiki_logical.py" \
  --binaries "$PERF_BINS" --tantivy-index "$PERF_NATIVE_INDEX" \
  --lucene-index "$PERF_LUCENE/idx" --jars "$PERF_JARS" \
  --output "$PERF_RESULTS/full-logical"
PERF_INDEX="$PERF_NATIVE_INDEX"
```

The offline gate compares every sorted term's decoded document frequency and
total term frequency, collection statistics, and every external-ID-sorted stored
ID/u64 sort value/encoded norm. It rejects duplicate IDs and unsupported layouts.
It does not compare every cross-engine posting position. The builder validates
the bounded input domain, and separate native query/phrase gates check search
results. Physical document order can differ between indexes; this is not a claim
of identical file layout. Keep the original indexes and timing binaries.

Before timing, also run the unchanged [native correctness gate](README.md) with
`--native-bm25`. Neither collection-statistics overrides nor score conversion
are used. Record the corpus, binary, source, toolchain and query-suite hashes.
Serialize latency and memory runs after builds finish, leaving background user
processes untouched.

## Re-encoding compatibility fixtures

For an existing deletion-free, single-segment index, re-encode to a different
absent directory with `reencode_index`. Then run:

```sh
python "$PERF_TOOLS/verify_payload_sensitivity.py" \
  --binaries "$PERF_BINS" --output "$PERF_RESULTS/payload-sensitivity"
python "$PERF_TOOLS/compare_payload.py" \
  --binary "$PERF_BINS/payload_identity" \
  --old "$PERF_OLD_INDEX" --new "$PERF_REENCODED" \
  --output "$PERF_RESULTS/payload-identity"
```

The sensitivity gate detects changes to postings, frequencies, positions,
fieldnorms, stored documents and u64 fast fields. The full walk requires exact
logical payload identity and reports serialized statistics separately. It fails
closed for unsupported segmentation, deletions, mixed JSON or fast-field types.
This identity check applies to re-encoding, not to the configured corpus replay
whose admitted token sequence intentionally corrects the earlier 55-token loss.
