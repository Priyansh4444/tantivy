# Lucene BM25 IDF rounding fix

Checkout: `/home/pronsh/Coding/playground/search/lucene-rs-idf-fix-oct06`
Branch: `fix/bm25-idf-lucene`
Upstream base: `465ea8f77f4397e35de56b7f7d202158d59e5871`
Regression commit: `298fc4820c85ac4abf8244238760bbdff8931c9f`
Fix commit: `e5d1f81bff42c87886b12770f3c79648bb1963ed`

For valid statistics `docFreq = docCount = sumTotalTermFreq = 54_505`, boost 1,
frequency 1, and encoded norm 1, Java Lucene 10.5.2 produces IDF bits `3719e736`
and score bits `368be976`. Original lucene-rs produced `3719e737` and `368be977`.
The parent review's checksum-verified actual Java witness is retained in
`../lucene-rs-review-oct06/findings-receipt.json` and `Witness.java`.

The regression commit adds two independent expected-bit assertions against the
public IDF and scorer APIs. The original implementation fails both:

```text
running 2 tests
test sim::tests::score_matches_lucene_rounding ... FAILED
test sim::tests::idf_matches_lucene_rounding ... FAILED
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 20 filtered out; finished in 0.00s
```

The fix changes `ln_1p(x)` to Lucene's literal `ln(1 + x)` operation order.
Clippy's nursery `imprecise_flops` lint specifically recommends the incorrect
`ln_1p` substitution, so this function has a local exception explaining why the
Lucene operation order must be retained. No other scoring operations change.

The same regression then passes:

```text
running 2 tests
test sim::tests::idf_matches_lucene_rounding ... ok
test sim::tests::score_matches_lucene_rounding ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out; finished in 0.00s
```

Checks use the project's pinned Rust 1.99.0 (b940084d7, LLVM 23.1.1), jobs 2,
`--locked`, an isolated target directory, and no inherited Cargo profile
overrides. The checkout's native CPU config and release profile are unchanged.
There is no toolchain-version deviation. Each stage retains complete output,
command, compiler identity, and before/after hashes of every tracked file plus
Cargo.lock. `run-check.py` fails if source or lock changes during a check.

- `regression-before`: fails with status 101 at the test-first commit.
- `regression-after`: passes at the initial fix commit.
- `debug-full` and `release-full`: full workspace/all-features suites pass at
  the initial fix commit, before a formatting wrap and lint exception.
- `fmt` and `clippy-initial`: preserved failures that motivated those narrow
  mechanical adjustments.
- `fmt-final`, `clippy-final`, `debug-final`, `release-final`: final-commit
  results, independently summarized in `summary.json` when complete.

The full suite comprises 22 library unit tests, 8 lifecycle tests, 1 randomized
equivalence test, and 1 doctest. The randomized reference uses the production
IDF helper; the new Java expected-bit assertions independently cover this
rounding defect. This witness and suite do not establish universal floating
point identity across all platforms or all Lucene features.

Only `src/sim.rs` is modified upstream. No push or PR was performed by this
implementation child; the parent owns source review, Java verification, and
upstream submission. Our Tantivy checkout and the pinned port review checkout
were not modified.
