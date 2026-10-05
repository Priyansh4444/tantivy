# Native overlap × BM25 parameter acceptance

Commit `e1cb97e4579e03865ca6864aaf7d14c4f081eac5`, independent tree
tantivy-parity-overlap-parameters-oct04 / branch parity/overlap-parameters-oct04,
based on combined main `872c9f55d`. Clean worktree. Four changed files are only
the Java helper, its raw reference JSON, reference README and the existing public
overlap integration test. No production changes, pushes, timings or generated
compatibility fixtures. All build/test processes exited; worker CPU idle.

The pinned Lucene 10.4 helper builds one tiny index per index-time policy, then
reuses each reader for DEFAULT, (.9,.4), (0,.75), and (2.5,1) query profiles.
Every query uses the opposite discountOverlaps flag from indexing. All eight
cases preserve norms [2,2] or [3,2], populated count 2, full token total 5,
alpha DF/TTF 2, COUNT 2. Raw native score bits and tie order match Rust exactly,
including zero-k1 removing the CountAll score difference and changing order to
doc0/doc1. Both Java and public Rust check explanations; Rust also reads actual
posting frequencies and checks physical norm bytes/statistics after queries.

Source/reference provenance is committed in
doc/performance/lucene-10.4/parity/overlap-norm-reference/parameters-reference.json:

- Lucene source pin: `9983b7ce7fdd04f4d357688fb85c14277c15ea8d`.
- New Java source SHA256:
  `eb7e27adf2d70d8d9a4d8603305092d119ab122262323e6d2134efced6d9f025`.
- Raw reference JSON SHA256:
  `0d680665eebd509b109f6b5893e5a179325a24da4edab21811fbfc5020b0c934`.
- Supporting token-stream Java source hash, Lucene jar hashes, exact javac/Java
  commands and native Version 10.4.0 are embedded in that JSON. No statistics
  override, score conversion or tolerance is used.

Verification used jobs4 and isolated target/overlap-parameters-oct04 with
`--locked --offline`:

- `cargo test --test native_overlap_norms`, debug and release: **12 passed**
  each, 0 failed/ignored/filtered. Includes the eight-case new test and all prior
  overlap tests. Logs /tmp/overlap-parameters-{debug,release}-oct04.log.
- `cargo test --lib query::bm25::parameter_tests`, debug and release:
  **5 passed** each, 0 failed/ignored, **1363 filtered**. This is a focused module
  check, not a full-library claim. Logs
  /tmp/overlap-parameters-module-{debug,release}-oct04.log.
- Existing public targets bm25_parameters and bm25_parameter_configuration,
  debug and release: **1+4 passed** each. Logs
  /tmp/overlap-parameters-public-{debug,release}-oct04.log.
- Full `cargo fmt --all -- --check` and `git diff --check`: pass.

Total scoped evidence is 22 passing tests per profile. Parent owns combined
full-library/frozen correctness and serial performance gates.
