# Native overlap policy unit: verified receipt

Isolated tree `tantivy-parity-overlap-policy-oct04`, branch
`parity/overlap-policy-oct04`, based on main production
`7db890908f1cc8422905102da7740733fe3f22f6`. No main writes, pushes or timings.
Initial source review occurred during the parent's serial latency window;
builds/tests/reference indexing began only after explicit CPU release.

Commits, in order:

- Red: `808e7e005894ef0c7aa9e45eb774368b8fd52604` — public native norm/raw-score
  witness, pinned Java reference.
- Fix: `23f011dc3ffbeff38d74ddf7d92343493a812823` — persisted typed norm policy,
  legacy writer safety, boundary/lifecycle/pruning coverage and documentation.

Four production source files change: text_options, schema export,
postings_writer overlap counter, and the non-Basic Str norm write in
inverted_index_plugin. index_meta/field_entry/schema changes are only exact JSON
snapshot expectations for freshly constructed defaults. Existing legacy Boolean
deserialization fixtures remain Boolean. Footer format stays 11; no scorer,
provider, statistics, bound-selection/profile or norm-byte decoder change.

New construction defaults to DiscountOverlaps. Legacy Boolean/missing norm
schemas deserialize CountAllTokens and retain their append policy. Explicit
CountAll serializes the old Boolean shape; Discount serializes the existing
fieldnorms slot as an enabled/policy object. Disabled norm settings preserve
latent policy. Unknown/missing tagged-object fields and unknown policies reject.
Basic remains unique encoded term/document counting. Frequencies and populated
counts/full token totals remain unchanged.

## Runtime red and green

Pinned native Lucene 10.4 commit:
`9983b7ce7fdd04f4d357688fb85c14277c15ea8d`. The tiny Java helper uses an
unmodified native IndexSearcher and the no-argument BM25Similarity for default
cases, plus explicit BM25Similarity(false) control. Source/jar hashes, exact
command and raw scores/norm bytes are committed in
`doc/performance/lucene-10.4/parity/overlap-norm-reference/reference.json`.

Original Rust red, before production changes:

```text
physical norm doc 0
  left: 3
 right: 2
raw score bits
  left: 1033692019
 right: 1035524425
test result: FAILED. 1 passed; 2 failed; 0 ignored
```

The same initial three tests passed after the fix. Expanded final coverage is
11/11 in both debug and release. It covers same-term overlaps/frequency, Basic
unique counting, multi-value boundaries, position_length, start gaps, ngrams,
empty first/intermediate/final values, missing/empty documents, disabled norms,
historical provider formula/explanations/COUNT, legacy append, reopen, sorted
physical norm bytes, deletion/merge, and 257-doc pruning versus exhaustive
scores across four boosts. Nonoverlap norm bytes, statistics and pinned native
raw score bits are invariant across policies.

Actual pre-fix format-11 reader binary was saved before changing production.
The new serializer created four genuinely empty indexes. That old Index::open
accepts both CountAll Boolean schemas (exit 0) and rejects both Discount object
schemas, including disabled (exit 2), with no segment/footer available. Source,
build/run commands, binary/source hashes, exact persisted values and diagnostics
are committed as OldReader.rs, EmptyIndexes.rs, README.md and
old-reader-reference.json beside the Java reference.

## Verification scope

All Rust commands use `CARGO_BUILD_JOBS=4` and isolated
`CARGO_TARGET_DIR=target/overlap-policy-oct04`, `--locked --offline`.

- `cargo test --lib`: **1356 passed, 0 failed, 7 ignored, 0 filtered**. This full
  normal library run included create_format. Its own generated untracked v11
  directory was verified and removed after the run; tracked older fixtures are
  unchanged. Log `/tmp/native-overlap-full-lib-final-oct04.log`.
- Public debug and release selection: native_overlap_norms 11, native_bm25 4,
  native_basic_norms 4, phrase_prefix_statistics 4, query_pruning_correctness 8:
  **31 passed per profile**. Logs `/tmp/native-overlap-public-debug-oct04.log`
  and `/tmp/native-overlap-public-release-oct04.log`.
- After removing one unused integration-test binding, only the changed overlap
  target was rerun in debug/release: **11 passed per profile**, and all-targets
  check rerun. Final logs `/tmp/native-overlap-final-debug-oct04.log`,
  `/tmp/native-overlap-final-release-oct04.log`,
  `/tmp/native-overlap-final-check-oct04.log`. Production/library source did not
  change after its full normal pass.
- `cargo check --all-targets`, changed-Rust-file rustfmt check and
  `git diff --check`: pass. Full `cargo fmt --all -- --check` identifies only
  unchanged pre-existing formatting in tests/native_bm25.rs; parent owns that
  cleanup after integration. No new warnings remain from this unit.

No feature-wide or speed claim follows from this unit. The parent's frozen
native correctness gate and integrated performance rerun remain separate.
Unsupported/decreasing/overflowing token positions, explicit frequency
attributes and immense-term behavior are separate contracts, not altered here.
