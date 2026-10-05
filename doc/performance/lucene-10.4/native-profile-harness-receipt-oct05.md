# Native configured-profile harness verification — October 5

This receipt covers the maintained benchmark helpers and caller boundaries on
base `f596e25a98084a841ccbe9120db976a8de96542e`. It introduces no library scoring,
index, codec or pruning change. The two presets are `k09-b04` and `k25-b1`.
The design selected one shared mode/command path and exact-bit child transport;
no independent engine override or custom decimal CLI is exposed.

## Verification

* All 28 Python fixtures passed: the unchanged 15 legacy fixtures and 13 new
  configured boundary, preparation, pipe deadline and process cleanup fixtures.
  The preparation mock now creates compiled outputs; manifests continue to
  require actual class files. A delayed duplicate receipt cannot pass memory
  warmup as a nonempty query response. EOF-ignoring workers are reaped, all
  pipes close, and repeated cleanup succeeds.
* The isolated jobs4 debug Rust adapter build succeeded. Its source copy contains
  the shared module under `src/bin/shared`. No shared native target was modified.
  The original interrupted `rust-build.log` records an inner attribute ordering
  error; the successful `rust-finish-build.json` records the corrected final
  sources. Existing macro-use/dead-code warnings remain; neither log is a
  performance measurement or the immutable release artifact for the parent run.
* Java prepared successfully in the previously absent
  `target/native-profile-receipts-oct05/java-finish-classes`, with all three
  source/class hashes in `native-build.json`. The actual preparation path selects
  strict configured generation; the legacy pure rewrite fixture remains unchanged.
* 44 actual invalid argument cases across `do_query`, `validate_index`,
  `DoQueryNative` and `DumpNativeLuceneResults` rejected before opening an absent
  index. Cases include partial/extra argument pairs, malformed/uppercase hex,
  nonfinite and out-of-domain parameters, and Java configured scale violations.
* On the preserved Wiki index, `+griffith +observatory` passed unchanged COUNT,
  ranking/exhaustive and cross-engine raw-score checks for DEFAULT and both
  profiles. Both nondefault raw dumps differ from DEFAULT. Persistent protocol
  and batch observations agree: physical field N=917578, TTF=294827020, scale
  bits `3f800000`; installed parameter bits match each requested preset.
* The retained DEFAULT Rust and Java helpers rejected configured requests within
  the five-second receipt deadline (0.0152s and 0.6159s in this run). These are
  rejection checks, not latency benchmark results.

Raw commands, results, hashes and observed dumps are preserved in
[`native-profile-harness-receipt-oct05.json`](native-profile-harness-receipt-oct05.json).
Its build records identify the isolated checkout and classes, not future main
release artifacts. The external builder patch remains unapplied for root review.

## Reproduction commands

Run from the isolated checkout. The Rust source and shared module must first be
copied according to the README. The target manifest points at this checkout and
has `tantivy` plus `serde_json` dependencies; it is a debug correctness build.

```bash
cargo build --manifest-path target/native-profile-adapter-oct05/Cargo.toml \
  --target-dir target/native-profile-adapter-oct05/build --jobs 4 \
  --bin do_query --bin validate_index
python -m unittest discover -s doc/performance/lucene-10.4 -p 'test_*.py'
python doc/performance/lucene-10.4/prepare_native_lucene.py \
  --lucene-dir ../search-benchmark-game/engines/lucene-10.4.0 \
  --output-dir target/native-profile-receipts-oct05/java-finish-classes --fresh-output
```

Choose a new absent Java directory when repeating preparation. Runtime commands
and every invalid case are recorded in the JSON receipt. The maintained Wiki20
correctness tool, frozen DEFAULT gates, serialized immutable release build and
configured measurements belong to the subsequent integration pass. No full
Lucene API or configured performance claim follows from this harness unit.
