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

Also copy `shared/wiki_physical.rs` to `src/bin/shared/wiki_physical.rs` for the
physical inventory and ordered replay modes. Keep `shared/bm25_profile.rs` there
for the configured query helpers, as described in the main instructions.

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

## Replay the frozen Lucene physical order

Exact score ties use physical docID in both engines. To require identical cutoff
IDs under ties, export Lucene's actual docID order and replay original corpus rows
in that order into a separate Tantivy index. This is an opt-in offline path;
ordinary replay and inventory arguments retain their output. Preserve the
original corpus and both retained indexes.

Compile `DumpWikiLogical.java` into a fresh class directory and export its
`documents-physical` mode. It requires one deletion-free segment and emits
`docID`, stored ID, unsigned sort value and encoded norm byte. The new mode
checks exactly one stored ID and a complete unique physical mapping.

```sh
PERF_ORDER_CLASSES="$PERF_RESULTS/physical-classes"
mkdir "$PERF_ORDER_CLASSES"
javac -cp "$PERF_JARS/*" -d "$PERF_ORDER_CLASSES" "$PERF_TOOLS/DumpWikiLogical.java"
java -cp "$PERF_ORDER_CLASSES:$PERF_JARS/*" DumpWikiLogical \
  "$PERF_LUCENE/idx" documents-physical > "$PERF_RESULTS/lucene-physical.tsv"
PERF_ORDERED_CORPUS="$PERF_RESULTS/wiki-lucene-order.jsonl"
python "$PERF_TOOLS/wiki_docorder.py" replay \
  --corpus "$PERF_CORPUS" --physical-map "$PERF_RESULTS/lucene-physical.tsv" \
  --output "$PERF_ORDERED_CORPUS"
PERF_ALIGNED_INDEX="/absolute/path/to/wiki-1m-native-aligned-v11.idx"
"$PERF_BINS/build_index" "$PERF_ALIGNED_INDEX" \
  --ordered-batches 25000 --physical-map "$PERF_RESULTS/lucene-physical.tsv" \
  --replay-receipt "$PERF_ORDERED_CORPUS.replay.json" < "$PERF_ORDERED_CORPUS"
```

The Python boundary validates the fixed original raw SHA-256, 1M unique IDs,
ASCII token domain, physical field N=917578 and TTF=294827020. It rejects duplicate
JSON keys, bool sorts, malformed maps, gaps and changed sort/norm values. SQLite
records byte offsets, lengths and per-line hashes; u64 sorts use decimal TEXT,
preserving values above 2^63. Replay copies original bytes without JSON
reserialization, including each row's newline bytes. A complete ID bijection
and equal per-ID raw-line manifests are required. Output, locator and receipt
paths must be absent. The sidecars are exactly `OUTPUT.replay.json` and
`OUTPUT.locator.sqlite`; failed fresh runs emit no completion receipt. Reusing
an existing path fails without changing its retained files or receipt. The
`--fixture-docs` and `--fixture-corpus-sha256` options are paired and mark fixture
mode explicitly.

Ordered indexing uses one worker and `NoMergePolicy`. Each nonempty blocking
batch commit must create exactly one new deletion-free segment of the batch's
size and retain all prior segments. The helper records those actual IDs/ranges
in ingestion order, then merges that explicit vector. Metadata order sorts
segments by size and cannot serve as the ordering authority. An early multiple
flush fails before merging/completion; choose a smaller batch in a new output.
`--memory-bytes` is optional and ordered-only; default is 500000000. The
per-thread arena must be below u32::MAX minus 1000000, so a 5GB arena is invalid.
The three ordered flags are required together and validated before creation.
After merge, discarded batch files are garbage collected. Before emitting
`INDEX.build.json`, the builder checks every final physical ID/sort/norm tuple,
actual field count, raw input hash and unchanged map/replay receipt.

Prove complete physical and text posting identity independently:

```sh
"$PERF_BINS/dump_docmap" "$PERF_ALIGNED_INDEX" --physical > "$PERF_RESULTS/tantivy-physical.tsv"
"$PERF_BINS/payload_identity" "$PERF_ALIGNED_INDEX" > "$PERF_RESULTS/tantivy-payload.json"
java -cp "$PERF_ORDER_CLASSES:$PERF_JARS/*" DumpWikiLogical \
  "$PERF_LUCENE/idx" postings-digest > "$PERF_RESULTS/lucene-postings.json"
python "$PERF_TOOLS/wiki_docorder.py" compare \
  --tantivy-physical "$PERF_RESULTS/tantivy-physical.tsv" \
  --lucene-physical "$PERF_RESULTS/lucene-physical.tsv" \
  --tantivy-payload "$PERF_RESULTS/tantivy-payload.json" \
  --lucene-postings "$PERF_RESULTS/lucene-postings.json" \
  --output "$PERF_RESULTS/physical-payload-proof.json"
```

Payload inputs are paired. The Java digest matches the unchanged Rust
`canonical-postings-v1` framing: raw term BytesRef slice, little-endian lengths,
DF/docID/TF, position counts/all positions and final term count. It compares every
posting and position plus actual text N/TTF headers, with an O(N) presence vector
and per-document positions in Rust. Comparison inputs must keep the same hashes
before and after verification. Also rerun the existing full logical comparison,
then the unchanged Wiki20 gates for DEFAULT, `k09-b04` and `k25-b1` before
measurements. Reordering affects locality and storage; establish fresh baselines
for the aligned pair and preserve reports for earlier layouts.

[`native_adapter_docorder.patch`](native_adapter_docorder.patch) updates the
external builder after the earlier profile-support patch. One authoritative
binary list drives copy/build/publish; every maintained source and both shared
modules are hashed and guarded. Apply it when preparing a new committed artifact.

The real fixture script `verify_wiki_docorder.py --binaries PATH --jars PATH
--reverse-witness PATH --output ABSENT_PATH` checks scrambled exact-byte replay,
unequal batches, empty/high norms, unsigned sorts, more than ten exact ties in
all three profiles and position/TF/norm/sort/byte/flush sensitivities. Produce its
reverse witness with `cargo test --bin build_index -- --nocapture` in the
standalone adapter. The test prints the directory containing its two actual
indexes and proves reversed explicit merge order can preserve ID-sorted
inventory while failing physical identity. Fixture evidence is recorded in
[`native-docorder-harness-receipt-oct05.md`](native-docorder-harness-receipt-oct05.md).

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
