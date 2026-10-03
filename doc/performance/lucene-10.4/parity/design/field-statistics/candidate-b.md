# Candidate B: derivation-first field statistics capability

## Caller usage first

```rust
// BM25 asks for one coherent collection snapshot, never two unrelated totals.
let collection = statistics_provider.field_statistics(term.field())?;
let bm25 = Bm25Weight::for_terms(statistics_provider, terms)?;

// Searcher's implementation aggregates immutable per-segment capabilities.
let local = segment_reader.field_statistics(field)?;
// local.collection() includes physically indexed deleted docs until merge.

// Merge asks the same implementation about the actual retained doc mapping.
let retained = FieldStatistics::derive_for_merge(readers, field, doc_id_mapping)?;
serializer.new_field(field, retained.sum_total_term_freq(), target_norms)?;

// Applications that must avoid first-query work may explicitly prepare fields.
searcher.prepare_field_statistics(&[title, body])?;
```

Existing callers of `Bm25StatisticsProvider` continue compiling. Its old methods remain available, and its new default method calls those methods. Only Searcher's override changes native collection semantics. Existing custom providers therefore keep their chosen population and token totals unless they explicitly adopt the new capability.

## Type and signature sketch

```rust
// Public, small value type: zero/zero denotes an empty indexed field.
// Constructor validates population and frequency consistency; no public mutable fields.
pub struct Bm25FieldStatistics {
    doc_count: u64,
    sum_total_term_freq: u64,
}

pub trait Bm25StatisticsProvider {
    fn total_num_tokens(&self, field: Field) -> Result<u64>; // unchanged
    fn total_num_docs(&self) -> Result<u64>;                 // unchanged
    fn doc_freq(&self, term: &Term) -> Result<u64>;           // unchanged

    fn field_statistics(&self, field: Field) -> Result<Bm25FieldStatistics> {
        Bm25FieldStatistics::from_provider(
            self.total_num_docs()?, self.total_num_tokens(field)?
        )
    }
}

// Private immutable capability. Query callers cannot accidentally reinterpret
// exact current collection statistics as stored-bound selection provenance.
struct FieldStatistics {
    collection: Bm25FieldStatistics,
    source: StatisticsSource,
    stored_selection: StoredSelection,
}

enum StatisticsSource {
    ExactPostings,
    ExactWriterSnapshot,
    ExactMergedPostings,
}

enum StoredSelection {
    // This is the actual serialization denominator, independent of query N.
    LegacyAllDocumentSlots { header_token_total: u64, max_doc: u32 },
    Unavailable,
}

impl SegmentReader {
    fn field_statistics(&self, field: Field) -> Result<Arc<FieldStatistics>>;
}

impl FieldStatistics {
    fn derive(reader: &SegmentReader, field: Field) -> Result<Self>;
    fn derive_for_merge(
        readers: &[SegmentReader], field: Field,
        retained: &SegmentDocIdMapping,
    ) -> Result<Bm25FieldStatistics>;
    fn collection(&self) -> &Bm25FieldStatistics;
    fn permits_stored_bound(&self, weight: &Bm25Weight) -> bool;
}

impl Searcher {
    pub fn prepare_field_statistics(&self, fields: &[Field]) -> Result<()>;
}
```

Do not publicly expose segment-bound provenance or add boolean flags to query APIs. The capability owns derivation, caching, validation, and the stored-bound permission decision. `StatisticsSource` supports debugging and lifecycle assertions; it does not let a caller select approximate statistics.

The new default provider method preserves the old protocol rather than adding a mandatory trait method or requiring provider downcasts. Empty fields receive explicit handling in `Bm25Weight::for_terms`; do not divide zero by zero or silently manufacture a field population. An indexed term with nonzero document frequency and zero field population is corruption for the native provider. Custom-provider validation must preserve its documented contract instead of silently replacing values.

## Grounding and module map

All source paths below are relative to `the Tantivy fork`.

| Module | Existing constraint | Proposed ownership |
| --- | --- | --- |
| `src/index/field_statistics.rs` (new private module) | `TermInfo` exposes doc frequency and ranges, not exact total term frequency (`src/postings/term_info.rs:10`) | Own exact posting reduction, retained-document filtering, immutable capability and provenance |
| `src/index/segment_reader.rs` | Cloned readers already share inverted-index readers through `Arc<RwLock<HashMap<...>>>` (`:32`) | Own per-field cache cells; create cells under a short lock, derive outside the map lock |
| `src/index/inverted_index_reader.rs` | Existing eight-byte header may contain an old approximate merge total; fields may be absent (`:72`, `SegmentReader::inverted_index`) | Supply dictionary and posting iteration, record option, and raw historical header; do not independently decide collection population |
| `src/core/searcher.rs` | Searcher aggregates segment readers | Aggregate each field's cached collection snapshots, optionally cache the aggregate in the immutable Searcher generation |
| `src/query/bm25.rs` | `total_num_docs()` is field-independent (`:21`); `for_terms` currently combines it with field tokens (`:109`) | Add backwards-compatible provider capability and consume its paired values for both IDF and average length |
| `src/query/term_query/term_weight.rs` | Stored-bound permission currently comes from header tokens / maxDoc (`:206`) | Ask the capability for permission; retain actual historical selection provenance |
| `src/index/inverted_index_plugin.rs` | Merge estimates tokens before serialization (`:505`, `:585`), but header and bound selection require totals before writing postings | Replace estimates with exact retained-postings preflight; reuse the exact snapshot for serialization |
| `src/postings/serializer.rs` | Header is written first and stored maxima selected with header tokens / fieldnorm reader numDocs (`:124`) | Keep existing layout and denominator for this candidate; do not claim new field-population-selected maxima |

## Exact reduction and lifecycle

The universal source of truth is the physically stored posting relation. For each field, stream all terms; mark every emitted doc ID in one temporary bitset; sum stored frequencies where the field/term records frequencies, otherwise count one per stored `(term, doc)` relation. Cardinality is field document population; the frequency sum is Lucene-style sumTotalTermFreq. Request `WithFreqs` downgraded to the actual record option, never positions. JSON mixes frequency-bearing and no-frequency terms: inspect the effective per-term posting mode, not solely the field schema. Facets, numeric fields, missing fields, and disabled norms use the same universal reduction, avoiding field-type assumptions in callers.

For ordinary string fields whose real stored norms have the existing positive-token-count invariant, a nonzero norm scan can establish document population without a posting union. It cannot establish exact token totals: quantization loses information. Use this only when population and token provenance are already independently exact, or as a memory-saving alternative while streaming frequencies. Do not use synthesized constant norms, generic JSON/facet norms, or live-doc filtering for query collection statistics.

Query derivation ignores the alive bitset. Pending deletions remain in docCount, frequencies, token totals, and IDF until the physical postings are rewritten, matching Lucene's collection-statistics lifecycle. Cache collection statistics against immutable segment content, not delete generation. A newly opened segment reader with a newer deletion mask can reuse the same physical-content snapshot. Missing fields return exact empty statistics without scanning.

Merge derivation uses the actual old-to-new mapping as its retained-document predicate. Mark mapped target doc IDs; sum only mapped frequencies. That handles deletes, manual pruning, sorted remapping, and merges with no deletes uniformly. Merge statistics do not use query statistics with a live-doc adjustment. They are a distinct reduction over retained postings. The existing serialization pass follows this preflight and writes the exact total into the existing header. Remove the quantized-norm and live/maxDoc-ratio estimator once callers migrate.

No-deletion merges may sum exact cached source snapshots. Historical headers alone are not proof of exactness: old deletion merges may already have written approximations. Cache provenance is lost on restart, so an unmarked old or new field must be verified by postings before its header can supply exact collection totals. This deliberately corrects old merged indexes without rewriting them; ordinary querying uses derived totals while stored-bound provenance continues using the original serialized header.

Fresh writers may calculate exact snapshots during their existing serialization stream: mark serialized doc IDs and add serialized frequencies, avoiding raw-token/filtered-token and DOCS-only duplicate ambiguity. Install those snapshots into readers only through a coherent publication mechanism keyed by immutable segment identity. This is an optional optimization, not a correctness dependency. It must not become an unbounded process-global cache or assume every reader was opened by the writer process.

## Bounds: preserve provenance instead of inferring it

This candidate keeps serialized block-max selection at its historical header-token-total / maxDoc average for all newly written fields as well as old fields. That avoids any new format discriminator. Native queries use exact derived tokens / exact field population. Stored pairs are eligible only when the query's finite positive average equals the actual historical selection average, and the weight passes existing finite/nonnegative guards. Otherwise use the existing conservative global term bound, with an exact loaded-block maximum when available.

Never substitute derived token totals into the historical numerator when checking eligibility. An old deletion merge may have used an approximate header to choose its pair. Never assume that an exact field population proves stored pairs were selected using that population. Mixed readers evaluate this independently per segment; there is no global old/new boolean.

This remains safe for custom providers: their query average may differ from native statistics and must be checked against actual selection provenance. It also remains safe when independent old/new averages happen to round to the same f32 value, because bound selection used that same effective f32 value. The score multiplier and BM25 parameters stay unchanged in this unit; changing those policies needs independent compatibility decisions and validation.

Tradeoff: sparse fields will frequently lose stored-bound pruning because their native query average differs from legacy maxDoc normalization. This is a performance cost, not permission to use unsafe pairs. Retaining field-population-selected bounds with constant-time provenance after reopening requires format metadata or a different universally valid bound representation; it cannot be inferred from exact derived population alone.

## Cache and query cost

Use a successful-value cache shared by SegmentReader clones, one cell per field. Concurrent misses perform one derivation; errors remain retryable rather than permanently caching transient I/O failures. Avoid holding the global field-map lock during dictionary/posting reads. Readers remain immutable, so a completed snapshot needs no invalidation and owns no document-sized bitset. Temporary memory is approximately maxDoc/8 bytes per active field derivation; prepare fields sequentially by default to avoid multiplying that allocation. Warm queries pay field lookup and a small segment aggregation, with no per-hit overhead. Aggregated statistics belong to one Searcher generation and disappear with it.

`prepare_field_statistics` is useful for managed startup or benchmark setup, but must not disguise cold cost in performance reports. First use without a trusted in-process writer snapshot can scan the field's dictionary and postings, even for a newly created index reopened in another process. That can be expensive for large vocabularies or norms-disabled fields. Reader startup may prepare explicitly configured fields; it should not eagerly scan every field by default.

The design satisfies efficient steady-state querying without changing the format. It does **not** satisfy a requirement for constant-time first queries after arbitrary process restarts. Existing on-disk data lacks both reliable exact token provenance and field population. If the rubric requires that stronger cold-query guarantee, additive metadata is essential and candidate A should win that criterion. The centralized capability still remains useful as the API/cache boundary for such metadata.

## Alternatives and rejections

* Norm sums for exact tokens: reject; decoded norms are quantized, and repeated-term/overlap semantics differ from posting frequency totals.
* Live doc count or live/maxDoc ratios: reject; field population is not live corpus population, and pending deletions remain in Lucene collection statistics.
* Union only query terms: reject; collection population includes every indexed term in the field.
* Assume the eight-byte header is exact: reject for old merged indexes and DOCS-only fields. A compatible reader cannot know whether a prior writer approximated it.
* Always decode every posting on every query: reject; immutable reader caching eliminates repeated work categorically.
* Add individual count methods to every writer, serializer, reader, and scorer: prefer one deeper capability that owns the paired statistics and their provenance.
* Persist field count only: insufficient to prove historical token exactness or bound normalization. If persistence is chosen, version/provenance and exact total-frequency semantics belong in the snapshot together.
* Select new stored maxima using exact population without a marker: reject; reopened old and new streams would be indistinguishable, permitting unsafe bound use.

## Verification units

1. One universal posting-reduction oracle covering missing/empty/analyzer-filtered fields, multivalues, DOCS-only duplicates, norms disabled, JSON mixed term modes, and facets; compare cardinality and frequency sum with unmodified Lucene collection statistics.
2. Native provider versus an unchanged custom provider: same legacy custom scores, native field-sensitive IDF and average, coherent empty-field behavior.
3. Pending deletion versus retained-postings merge: collection totals stay unchanged before merge and become exact afterward, including old approximate headers and explicit remapping.
4. Old, new, and mixed segments: exhaustive scoring and pruned TopDocs agree under native/custom averages, sparse populations, and high frequencies; every bound remains conservative.
5. Cache success, concurrent first access, retry after failed derivation, Searcher reload lifecycle, and bounded temporary memory.
6. Report cold preparation and warm-query timings separately. A design-selection rejection is appropriate if first-use cost or lost stored pruning exceeds the accepted performance budget.

## Candidate assessment and synthesis input

Strongest: exact lifecycle behavior without a format migration; historical-bound provenance remains explicit; custom providers remain source-compatible; all correctness fallbacks reduce the same physical relation.

Weakest: cold derivation can be expensive and legacy-normalized bounds remain conservative but weaker on sparse fields. Persistence is therefore an essential addition if constant-time reopened queries and native-normalized stored pruning are nonnegotiable. Recommend adopting this capability/cache ownership even if synthesis selects persisted snapshots as its fast source.

No production edits, builds, tests, or benchmark runs were performed. Only this design artifact was created.
