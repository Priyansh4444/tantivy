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
