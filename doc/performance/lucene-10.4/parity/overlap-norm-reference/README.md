# Native overlap norm reference

`OverlapNormReference.java` uses Lucene 10.4.0's unmodified IndexWriter,
BM25Similarity and IndexSearcher on tiny in-memory pretokenized fixtures.
`reference.json` records raw norm bytes, full collection totals, raw score bits,
source/jar hashes and the exact run command. Default discounting is contrasted
with explicit `new BM25Similarity(false)` and DOCS unique-term norms. Additional
fixtures cover same-term overlaps, multiple values, position lengths and gaps.

Compile with `javac -cp '<Lucene benchmark dependencies>/*' -d
/tmp/native-overlap-reference-oct04 OverlapNormReference.java` and use the
recorded Java command (adjust absolute paths if needed). Classes are generated
outside the repository. No statistics adapter, tolerance conversion or timing
is involved.

Sources are pinned to commit `9983b7ce7fdd04f4d357688fb85c14277c15ea8d`:
Similarity.java lines 95–125 and 153–162, BM25Similarity.java lines 36–108,
IndexingChain.java lines 1218–1288 and 1325–1345. The initial public Rust
regression checks the default norm/score mismatch before changing production
code and preserves the explicit legacy Boolean-schema and Basic control.

`OldReader.rs` was built against unchanged production commit
`7db890908f1cc8422905102da7740733fe3f22f6` before the fix. Copy it temporarily to
`examples/overlap_old_reader.rs` in that checkout, then build using
`CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=target/overlap-policy-oct04 cargo build
--locked --offline --example overlap_old_reader --message-format=json`. Save the
reported executable outside the target tree before rebuilding new production.

`EmptyIndexes.rs` was then built as a temporary example against the fix and run
using the same jobs/target with `cargo run --locked --offline --example
overlap_empty_indexes -- /tmp/native-overlap-empty-indexes-oct04`. It creates
four zero-segment indexes using the new production serializer and reopens each
with the new reader. Running the saved old reader on each directory opens both
legacy Boolean schemas and rejects both Discount schemas (enabled/disabled).
`old-reader-reference.json` records the exact persisted values, reader exit
codes/diagnostics, old production identity, and executable hash. Remove the
temporary example files after reproducing. No synthetic metadata substitution
or segment-footer check is used to establish the empty-index gate.

`OverlapParametersReference.java` adds a native cross-feature matrix. It builds
one tiny index for each overlap policy, reuses each reader for DEFAULT,
(.9,.4), (0,.75), and (2.5,1) query parameters, and deliberately supplies the
**opposite** query-time discountOverlaps flag. All eight cases retain their
original norm bytes and full field/term totals. Native raw score bits, tie
order, COUNT and explanation equality are checked without a statistics override.

Compile that helper together with OverlapNormReference.java; the exact javac
and Java commands, hashes of both sources and Lucene jars, pinned Lucene source
identity, and literal eight-case output are in `parameters-reference.json`.
The public `overlap_index_policy_and_native_query_parameters_match_java_independently`
test in tests/native_overlap_norms.rs consumes those bits, configures native
Searcher handles through Bm25Parameters, and checks physical frequencies and
norm bytes before and after scoring. Index-time policy is preserved even at
zero k1, where the CountAll fixture's score difference disappears and tie order
changes. The earlier default norm/score reference remains unchanged.
