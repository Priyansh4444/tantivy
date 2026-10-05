# Ordered physical replay harness verification — October 5

Scoped unit on base `b3d22b8af469013363ee358792b38cd18efa6794`, tree-equivalent to
integrated 893051bb0. It changes offline tools and explicit fixture modes. No
core scoring, codec/footer, query-suite or correctness-ruler change is included.
No full Wikipedia rebuild or performance measurement was run by this unit.

## Observed evidence

* All 37 Python tests passed: 15 unchanged legacy, 13 configured-profile and
  nine new strict replay/comparison boundary tests. Duplicate JSON keys, bool
  sorts, duplicate/missing/foreign IDs, malformed maps, changed raw bytes,
  reused paths and a deterministic comparison-input mutation fail closed.
* The isolated jobs4 debug helper build and fresh Java compilation passed.
  One Rust real-artifact test proved that reversing the explicit merge vector
  keeps the ID-sorted ID/sort/norm inventory equal while changing physical order.
* The complete real fixture uses 19 scrambled records, physical field N=18,
  TTF=65035, unequal ordered batches 7/7/5, empty norm 0, high norm 135, u64-max
  sort and values above 2^63, plus sort ties. Original row bytes, including mixed
  CRLF/LF endings, are copied exactly through the SQLite offset permutation.
* Java and Rust physical tuples match exactly, with SHA-256
  `fb3eb9c0951bb09f690ce706fab216df7a664e1378346f3fca2b639408066c43`.
  The full canonical text postings stream also matches: four terms, 35 postings,
  65035 frequencies/positions, 18 present field documents and SHA-256
  `777c0872b138f239222da8873132f2f8eb04291dfc71a1f76db6b3504006ba13`.
* All three profiles pass the unchanged top-ten/exhaustive/raw-score gate on
  16 exact tied hits. Retained raw dumps rechecked to identical engine f32 bits:
  DEFAULT `3dde2afc`, k09-b04 `3dbb9788`, k25-b1 `3e1043cc`.
* Real substitutions detect position-only changes (`a b a` versus `a a b`),
  equal-length TF redistribution, norm and sort mutations, and reversed actual
  segment merge order. Position-only substitution preserves DF/TTF and physical
  norms/IDs/sorts; the full positions digest still rejects it.
* Changed replay bytes and newline bytes, invalid options/budgets, reused output
  paths and a deliberately early-flushed multi-segment batch reject. Failed fresh
  builds emit no completion receipt; valid retained files/receipts are preserved
  when path reuse is rejected. The ordered builder records before/after segment
  sets, runs post-merge garbage collection and checks actual final tuples plus
  unchanged map/replay receipt before success.

[`native-docorder-harness-receipt-oct05.json`](native-docorder-harness-receipt-oct05.json)
retains actual fixture commands/stdout/stderr, source/binary/class/JAR hashes,
all fixture artifact hashes and the final test report. The initial successful
fixture run preceded two observer-only additions to its executor: recording
stdin hashes and explicitly checking cross-engine f32 tie bits. Its original
executor hash is preserved alongside the final one; producer/comparator sources
and actual binary hashes are unchanged. The retained raw dumps independently
establish the exact tie-bit check. No fixture/index rebuild was needed for that
post-run observation. The subsequent integrated release pass can run the final
executor into a fresh output with full stdin-hash recording.

## Reproduction

Copy the maintained helper sources and shared modules under the standalone
adapter's src/bin as documented in configured-wiki.md, with tantivy/serde_json/
sha2 dependencies. Build debug helpers into an isolated target with jobs4, then:

```sh
cargo test --manifest-path target/native-profile-adapter-oct05/Cargo.toml \
  --target-dir target/native-profile-adapter-oct05/build --jobs 4 \
  --bin build_index -- --nocapture
python doc/performance/lucene-10.4/verify_wiki_docorder.py \
  --binaries target/native-profile-adapter-oct05/build/debug \
  --jars ../search-benchmark-game/engines/lucene-10.4.0/build/dependencies \
  --reverse-witness PATH_PRINTED_BY_RUST_TEST --output NEW_ABSENT_PATH
python -m unittest discover -s doc/performance/lucene-10.4 -p 'test_*.py'
```

The external `native_adapter_docorder.patch` remains unapplied for root review.
It copies/hashes the maintained eight binaries and both shared modules from one
binary list. Full1M physical/posting proof, all-three-profile Wiki20 gates and a
new aligned-layout performance ruler remain separate integration work.
