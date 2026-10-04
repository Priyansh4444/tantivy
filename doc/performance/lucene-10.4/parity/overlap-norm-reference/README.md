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
