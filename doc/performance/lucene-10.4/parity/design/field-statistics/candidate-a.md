# Candidate A: persisted exact field statistics

## Caller usage first

```rust
// BM25 consumes one coherent pair; custom providers retain their old pair.
let collection = statistics_provider.field_statistics(term.field())?;
let weight = Bm25Weight::for_terms(statistics_provider, terms)?;

// Native Searcher sums physical per-segment snapshots, including pending deletes.
let local = segment_reader.inverted_index(field)?.field_statistics()?;

// Stored bounds use the actual serialization average, independently of query N.
let average = inverted_index.stored_selection_average_fieldnorm();
let scorer = scorer.with_segment_average_fieldnorm(average);

// Writers/merges supply an exact snapshot BEFORE header and block-pair selection.
let retained = derive_retained_field_statistics(readers, field, mapping)?;
serializer.new_field_with_statistics(field, retained, fieldnorms)?;
```

Newly persisted fields need no first-query dictionary/postings scan after reopening. Old fields reduce their physical postings once and share the immutable result. Native BM25 obtains population and token total together; callers cannot accidentally combine an exact population with an approximate historical header.

## Contract and grounding

Lucene 10.4 BM25 uses field `docCount` for both IDF and average length: documents containing at least one indexed term, including pending deletions until physical merge. `sumTotalTermFreq` sums stored frequencies; DOCS-only terms contribute one per distinct term-document relation. Grounding is the local exact `releases/lucene/10.4.0` sources `BM25Similarity` and `Lucene103BlockTreeTermsWriter`; the latter persists `docsSeen.cardinality()`. [CollectionStatistics documentation](https://lucene.apache.org/core/10_4_0/core/org/apache/lucene/search/CollectionStatistics.html) confirms the lifecycle.

Current native BM25 uses segment maxDoc. Current token headers are not proof of exactness: historical deletion merges approximate totals using quantized norms or a live/maxDoc ratio; Basic writer counters include duplicate subscriptions. The contract therefore covers exact paired statistics for new, old and mixed indexes; retained-posting statistics after all merge mappings; safe bounds; compatible custom APIs; and constant-time new-index statistics. Full BM25 floating-point/norm parity is a separate coordinated unit.

## Type and signature sketch

```rust
pub struct Bm25FieldStatistics {
    doc_count: u64,
    sum_total_term_freq: u64,
}

pub trait Bm25StatisticsProvider {
    fn total_num_tokens(&self, field: Field) -> Result<u64>; // unchanged
    fn total_num_docs(&self) -> Result<u64>;                 // unchanged
    fn doc_freq(&self, term: &Term) -> Result<u64>;           // unchanged
    fn field_statistics(&self, field: Field) -> Result<Bm25FieldStatistics> {
        Ok(Bm25FieldStatistics::new(
            self.total_num_docs()?, self.total_num_tokens(field)?
        ))
    }
}

// Private exact snapshot, created by ingestion, validated bytes, or reduction.
struct SegmentFieldStatistics {
    doc_count: u32,
    sum_total_term_freq: u64,
}

enum FieldStatisticsSource {
    PersistedNative(SegmentFieldStatistics),
    Legacy {
        serialized_token_total: u64,
        max_doc: u32,
        exact: once_cell::sync::OnceCell<SegmentFieldStatistics>,
    },
    Empty,
}

impl InvertedIndexReader {
    pub(crate) fn field_statistics(&self) -> io::Result<SegmentFieldStatistics>;
    pub(crate) fn stored_selection_average_fieldnorm(&self) -> Score;
    // Existing public total_num_tokens() remains the historical-header accessor.
}

impl InvertedIndexSerializer {
    pub(crate) fn new_field_with_statistics(
        &mut self, field: Field, statistics: SegmentFieldStatistics,
        fieldnorms: Option<FieldNormReader>,
    ) -> io::Result<FieldSerializer<'_>>;
    // Existing public new_field(field, tokens, norms) retains legacy semantics.
}

fn derive_physical_field_statistics(
    reader: &InvertedIndexReader, max_doc: u32,
) -> io::Result<SegmentFieldStatistics>;

fn derive_retained_field_statistics(
    readers: &[SegmentReader], field: Field, mapping: &SegmentDocIdMapping,
) -> Result<SegmentFieldStatistics>;
```

These signatures are sketches. The public provider pair preserves existing custom-provider semantics; do not impose new native consistency restrictions on unchanged custom implementations. Native construction validates count <= maxDoc, zero/zero for empty, no positive tokens with zero population, and tokens >= positive population. Handle empty BM25 fields explicitly rather than divide zero by zero. The Searcher override obtains paired snapshots once per segment, while the default provider method calls the old methods so existing implementations continue compiling and keep their chosen statistics.

## Persistence, versioning and bound provenance

Persist four-byte field doc count in composite postings `(field, 1)`. Preserve `(field, 0)`'s eight-byte token header and relative term offsets. A count entry guarantees BOTH exact header sumTotalTermFreq and pair selection using that total / fieldDocCount. Validate payload length and snapshot consistency at opening. This entry is a semantic provenance marker, not merely an optional counter.

**Bump INDEX_FORMAT_VERSION from 9 to 10; retain reading versions 4 through 9.** Existing footer checks make v9 readers reject v10 files. An old reader otherwise ignores additive metadata and trusts selected pairs under header/maxDoc; a pair selected under fieldDocCount can be unsafe under that old average. Additive byte compatibility does not prove pruning compatibility. The version boundary resolves new-index-read-by-old safety categorically.

A new reader still accepts unmarked fields: old files AND v10 fields written through preserved public legacy `new_field`/`FieldSerializer::create`. Absence denotes Legacy, never zero or native selection. Their exact native collection snapshot comes from reduction; stored selection average remains the ORIGINAL serialized header / maxDoc. Never substitute reduced exact tokens into a historical bound numerator: old approximate headers helped choose the original pair.

The internal exact serializer path accepts an explicit average. The public legacy path retains its historical header / norm-reader numDocs average. Norm-reader numDocs remains physical extent, also used by COUNT density selection; do not redefine it as field population. Norms-disabled fields have no useful norm-dependent stored pair and retain conservative handling.

The existing finite/nonnegative/query-average equality guard remains authoritative. Native or custom weights can trust a pair only when their effective f32 average matches its actual selection average. Mixed indexes make this decision per segment. Legacy sparse fields typically lose stored pruning safely; new fields retain native-normalized pruning. Coordinated changes to BM25 cache arithmetic must preserve pair-selection ordering or extend this compatibility contract explicitly.

## Module map and ownership

Paths are relative to `the Tantivy fork`.

| Module | Ownership/change |
| --- | --- |
| `src/query/bm25.rs` | Public paired value/default provider method; Searcher override aggregates snapshots; `for_terms` consumes paired N/tokens. |
| `src/index/inverted_index_reader.rs` | Raw header, exhaustive provenance state, successful legacy cache. Private constructor accepts maxDoc/count metadata; public empty/header APIs remain compatible. |
| `src/index/segment_reader.rs` | Open/validate `(field, 1)` and return canonical cached reader Arc. Select map entry after opening so racing callers share one cache; never derive postings under the map lock. |
| `src/index/field_statistics.rs` (new private module) | Universal exact reducer, effective per-term record modes, retained mapping, temporary bitset. Keep this complexity out of callers. |
| `src/index/inverted_index_plugin.rs` | Fresh per-field population counters; paired serialization flow; exact merge preflight replaces approximation. |
| `src/postings/postings_writer.rs` | Exact frequency semantics for token counter; thread population into `serialize_postings`. |
| `src/postings/json_postings_writer.rs` | Aggregate text/non-text counters with their effective frequency modes. |
| `src/postings/serializer.rs` | Preserve public entry points; private exact path, count metadata, explicit selection average. |
| `src/query/term_query/term_weight.rs` | Ask reader for actual stored selection average rather than infer from raw tokens/maxDoc. |
| `src/lib.rs`, footer tests | Writer version 10, oldest accepted 4, explicit v9 rejection test. |

## Exact lifecycle

Fresh ingestion already groups all values of one field per document. Snapshot its writer total before/after the group and increment population once if it grew. This covers absent/empty/filtered terms, multivalues, disabled norms, facets and JSON without a new per-token population branch.

Correct the token counter first. `SpecializedPostingsWriter<Rec>::subscribe` already knows new term-document relations from new-recorder/current_doc logic. Increment every occurrence iff `Rec::has_term_freq()`, otherwise only new relations; specialization folds the mode predicate. JSON's text/non-text writers keep their own modes. For ordinary Basic text use the grouped relation-count delta for norm recording: Lucene DOCS-only norms use uniqueTermCount. Frequency/position-bearing overlap policy remains the coordinated norm unit's responsibility. Raw duplicate counts must never receive an exact metadata marker.

Legacy exact query reduction scans all PHYSICAL field postings without alive filtering. Request frequencies without positions when actually stored; otherwise count one per relation. Mark doc IDs in a temporary bitset for cardinality. JSON requires effective per-term mode, not just field schema. This corrects both population and historical approximate totals universally, including unsupported/synthesized norms. A norm-zero scan can establish some text populations, but cannot establish exact token totals because norms are quantized; start with one universal reducer.

Pending deletion changes the alive mask, not physical collection statistics. Cache snapshots against immutable segment content, not deletion generation. Use fallible OnceCell initialization: successful values share across the canonical Arc; errors remain retryable. Scratch bitsets are released after reduction. Missing fields return exact empty snapshots without scanning.

No-removal merges sum exact source snapshots; legacy sources derive first. Deletion/manual-pruning merges prepass surviving postings with the actual mapping, mark target doc IDs and sum mapped frequencies BEFORE header/pair serialization. Normal merge then writes postings/positions with the same mapping. Sorted remapping changes IDs, not statistics. Delete the quantized-norm/live-ratio estimator after caller migration; do not silently preserve approximation on any path.

Existing segment close/commit publication publishes count metadata and header together. Failed serialization publishes no segment; reopening reconstructs provenance from committed bytes. No process-global writer cache is needed.

## Cost and alternatives

New fields add four payload bytes plus one composite-directory entry. New-index cold/warm queries pay small reads/aggregation, with no per-hit work or whole-index scan. Legacy first use scans the requested field once, with approximately maxDoc/8 temporary bytes. Concurrent field reductions can multiply scratch; report preparation separately. A retained merge preflight adds a frequency/postings pass without positions decoding.

Reject a per-document exact length array initially: about 4 MB per million docs could erase much of the measured storage win, and merge preflight is simpler. Reject quantized norm sums/live ratios, population-only fixes that trust old headers, Basic raw duplicates marked exact, and new pair selection without version rejection.

Candidate B's derivation-first shape keeps every new pair legacy-normalized and needs no format migration, but incurs cold scans even after reopening new indexes and weaker sparse-field pruning. A adopts B's coherent provider snapshot, universal exact fallback, shared immutable cache and separate historical bound provenance. A chooses persisted exact snapshots plus format 10 for cold cost and native pruning. Parent owns final synthesis.

## Verification/acceptance

1. Persisted snapshots versus independent reduction versus unmodified Lucene collection statistics: sparse, empty, filtered, multivalue, two independently sparse fields; Basic duplicates; disabled norms; JSON mixed modes; facets.
2. Pending deletion unchanged; deletion merge exact with deleted long docs; sorted/manual pruning; no-delete new/legacy/mixed merges.
3. Old fixtures with intentionally approximate headers: exact native reduction but ORIGINAL header bound average. Include v10 public-legacy serialization.
4. Round-trip existing term offsets, metadata/empty fields; reject malformed/inconsistent metadata. Assert v10 writing, v4-v9 new-reader compatibility, and v9 rejection of v10. Testing only old-index-read-by-new is insufficient.
5. Exhaustive scoring versus pruned TOP under native/custom providers, new/old/mixed segments, sparse populations/high frequencies. Every bound stays conservative. Native Lucene score/top-ID comparisons require the coordinated arithmetic/norm fixes and removal of adapter overrides.
6. Canonical concurrent cache, retryable errors, reload lifecycle. Report cold/warm time, legacy preparation CPU/memory, deletion-merge overhead and storage bytes; run established full COUNT/TOP gates after integration.

No production edits, builds, tests or benchmark runs were performed for this design package.
