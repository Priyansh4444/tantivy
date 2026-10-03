# Field statistics architecture arena verdict

Both candidate packages were read fully. This review inspected the codec worktree and pinned Lucene 10.4.0 source, without source edits, builds, tests, or timings. Scores judge the proposed statistics unit, not full Lucene parity.

## Scores

| Criterion (0–5) | A: persisted exact statistics | B: derivation-first capability |
| --- | ---: | ---: |
| Exact per-field population/frequency lifecycle, including legacy approximate headers, DOCS-only, mapping | 5 | 5 |
| Old/new bound provenance safety in both reader directions | 5 | 5 |
| Custom provider and public API compatibility | 4 | 4 |
| Cold/warm query, merge, and storage efficiency | 4 | 2 |
| Maintainable deep ownership and minimal surface | 4 | 4 |
| Total | **22/25** | **20/25** |

**Select A as the base, graft B’s centralized capability discipline and universal reducer.** A pays a format migration and four metadata bytes per populated field to avoid mandatory scans after every process restart and preserve useful native-normalized sparse-field block bounds. Those advantages matter directly to the user's efficiency target. B is a sound no-format-migration correctness fallback, but its first-use full-postings scans and weaker pruning make it the weaker primary design.

## Grounding and decisions

1. Both designs identify the right physical statistics lifecycle. Lucene `BM25Similarity.idfExplain` uses field docCount; `avgFieldLength` divides sumTotalTermFreq by docCount. Query statistics include physically stored postings before deletion merges. The current Tantivy merge estimator demonstrably uses quantized norm sums or live/maxDoc ratios (`src/index/inverted_index_plugin.rs:505–540`), so an exact population-only change would still be incorrect. Use one coherent population/frequency pair. Legacy derivation must ignore the alive mask; retained-merge derivation must use the actual mapping.

2. A's version rejection is necessary for its chosen stored-bound representation. Current serialization chooses its pair using header tokens / norm-reader numDocs (`src/postings/serializer.rs:131–138`). Current term scoring infers the selection average from raw header tokens / maxDoc (`src/query/term_query/term_weight.rs:206–207`). An older reader cannot safely reinterpret a newly selected pair. `Footer::is_compatible` already rejects index format versions above its compiled maximum (`src/directory/footer.rs:114–124`), so version 10 is an existing mechanism with a testable invariant. B correctly avoids this issue by keeping all new pair selection legacy-normalized. Neither proposes merely adding count bytes and hoping old readers prune correctly.

3. A's side entry is feasible without modifying term-relative posting offsets: `CompositeWrite::for_field_with_idx` and `CompositeFile::open_read_with_idx` already exist (`src/directory/composite_file.rs:59`, `:164`). The `(field, 1)` entry must mean an exact paired snapshot AND a specific bound-selection interpretation. It is not simply an optional count. Absence means legacy even in a version 10 segment written through the preserved public serializer entry points.

4. Preserve original legacy selection provenance even when reduction discovers that the old token header was inaccurate. The numerator used to choose the historical pair was the old header, not the corrected reduction. Equal averages can legitimately reenable stored bounds; unequal averages must fall back. Never use live population, reduced tokens, or field population to reconstruct a legacy pair's original selection average.

5. The counter fix belongs at the term-document relation owner. `SpecializedPostingsWriter::subscribe` increments every subscription today (`src/postings/postings_writer.rs:215`), but already distinguishes a new relation at `:219–228`. `DocIdRecorder` drops repeated positions and reports `has_term_freq() == false` (`src/postings/recorder.rs:113–115`, `:148–150`). Count every occurrence when frequencies are stored, otherwise only a new relation. Then grouped-field before/after deltas reliably identify population and can support DOCS-only unique-term norms without a separate token-side population branch. JSON has separate string/non-string writers and serializes non-string terms with `DocIdRecorder` (`src/postings/json_postings_writer.rs:19–22`, `:84–106`); aggregate their real modes instead of assuming one schema-wide frequency policy.

6. Both candidate API defaults are conceptually compatible but need careful implementation. Existing custom providers can choose unusual or invalid values, and current public weight constructors accept them. Do not make the new default method reject combinations that the old provider path accepted. Apply strong consistency validation to native persisted/reduced snapshots. Existing `total_num_docs`, `total_num_tokens`, `InvertedIndexSerializer::new_field`, and `FieldSerializer::create` signatures remain. Changing native Searcher scores is the intended behavioral correction; preserving custom-provider semantics is a distinct test.

## Grafts and implementation constraints

- Put universal physical and retained reductions, pair validation, and provenance helpers in one private `field_statistics` module. Keep raw I/O and the successful lazy cell on the canonical inverted reader Arc; do not add a second independent SegmentReader field cache or a scorer-level flag API. Query and merge consume the same small paired value.
- Canonicalize concurrent reader opens before returning the Arc. The current map unlocks between checking and opening (`src/index/segment_reader.rs:229` onward), so merely placing OnceCell in each newly opened reader can still duplicate derivation under races. Open outside the lock, then return the existing-or-inserted canonical Arc under the short write lock. Never scan postings while holding the map lock.
- A no-removal merge fast path may sum exact snapshots only after proving the mapping retains every source physical document. `!has_deletes()` alone does not establish that for manual pruning. Sorted permutations preserve population/frequencies; a retained subset requires the prepass.
- Use the real effective per-term frequency semantics for reductions, including mixed JSON and short postings. Existing block decoding infers no-frequency JSON terms from skip size (`src/postings/block_segment_postings.rs:120–130`); the VInt tail separately handles absent frequency bytes (`:76–86`). Do not assume a schema WithFreqs setting proves every term has stored frequencies. Add fixtures on both sides of a compression-block boundary.
- Preserve the exact rounding algorithm that selected stored pairs. Lucene native average is `(float)(tokens / (double)docCount)`; existing legacy average is `tokens as f32 / maxDoc as f32`. Those differ for large integers. Keep a named legacy helper, and define native marker semantics using one shared native helper used by writer and reader. A later arithmetic change cannot silently reinterpret version 10 provenance. This statistics unit alone does not close Lucene's score scale, IDF arithmetic, reciprocal-cache arithmetic, phrase frequency, or overlap-norm differences.
- Count-only metadata is sufficient only because the new internal serializer guarantees the old eight-byte header is exact and marker absence protects public legacy paths. Tests must prevent any call path from emitting the marker with approximate/raw-DOCS-only totals. Do not preserve the estimator as a hidden fallback after migration.
- JSON path statistics are a separate mapping contract: Tantivy's composite field can contain multiple logical paths, while Lucene has no native JSON field equivalent. Whole-field exactness does not itself prove a Lucene per-path mapping has equivalent collection statistics. Document and test the chosen compatibility mapping instead of labelling arbitrary JSON layouts fully native-parity.

## Reject or defer

Reject unconditional historical-header trust, field-population-only metadata without provenance, norm sums for exact frequencies, per-query rescans, live-mask query statistics, and field-count-selected pairs readable by old pruning code. Defer a public `prepare_field_statistics` API until a concrete caller needs it: the benchmark may explicitly prepare through an internal driver and must report legacy cold cost. Defer in-process writer snapshot publication caches; persisted A metadata makes them unnecessary. Defer per-document exact length arrays until retained-merge overhead is measured; their storage footprint could erase the current narrow index-size advantage.

## Acceptance before claiming this unit complete

Exact independent reduction, metadata, and unmodified Lucene statistics must agree for sparse/empty/filtered/multivalue fields, Basic duplicates, disabled norms, and all supported mappings. Pending deletes leave physical statistics unchanged; deletion and manual-pruning merges rewrite them exactly. Old approximate-header, new native, new public-legacy, and mixed readers must produce exhaustive/pruned agreement under native and custom providers. A v9 reader must reject v10, while the new reader accepts actual older fixtures. Include malformed metadata, short/full-block mixed JSON, concurrent initialization, and retryable I/O failure cases. Separately measure reopened cold query, warm query, legacy preparation memory/CPU, retained-merge throughput, and storage bytes. This unit cannot be accepted merely because corrected sparse-field scores match after adapter normalization.
