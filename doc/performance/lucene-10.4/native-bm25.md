# Native BM25 scoring and bound compatibility

Tantivy's standard `Searcher` uses Lucene 10.4 BM25 defaults (`k1=1.2`, `b=.75`)
and raw score scale 1. It computes field population and token totals from
physical postings, including documents with pending deletes. DOCS-only fields
count distinct term-document relations. Merging away deleted documents updates
both statistics. Custom providers that implement only the historical required
methods, and public single-term weight constructors, retain their previous
statistics rounding, rational score formula, and `k1+1` numerator.

## Arithmetic contract

Native IDF uses f64 arithmetic and rounds to f32 once. A phrase sums individually
rounded f32 IDFs in f64, then rounds once. Average field length uses f64 division
before its f32 cast. Each fieldnorm cache entry stores the f32 reciprocal of
`k1 * ((1-b) + b * dl / avgdl)`. Native scoring preserves the evaluation order
`weight - weight / (1 + frequency * norm_inverse)`, including fractional sloppy
phrase frequencies. Boosts multiply the weight once. The native competitive
threshold test compares the actual rounded score; the old rational algebraic
shortcut remains limited to classic scoring.

The integration regression `tests/native_bm25.rs` checks raw term scores,
boosts, fractional phrase scores, explanations, and bitwise preservation of
classic/custom scores. Native expected values were produced by an unmodified
Lucene 10.4 `IndexSearcher`, rather than dividing historical rounded scores by
2.2. Library query and collector fixtures use the same native convention.

## Format 11 provenance

The postings composite entry `(field, idx=1)` has this layout:

| Payload | Statistics | Stored-pair selection |
| --- | --- | --- |
| Absent (historical/public serializer) | Exact statistics derived once from physical postings | Legacy rational TF factor, historical header average |
| Four-byte little-endian docCount (v10) | Exact header token total and field population | Legacy rational TF factor, native average rounding |
| Four-byte docCount plus byte `1` (v11) | Exact header token total and field population | Native saturation input, native average rounding |

Other payload lengths and unknown tag values fail reader validation. A format 11
footer makes readers supporting format 10 or older reject the index before
interpreting its new pairs. The current reader continues accepting historical
indexes. The public serializers still emit the legacy representation and
comparator; only the internal exact-statistics writer emits the native tag.

Native queries reject legacy-selected pairs. Classic/custom queries reject
native-selected pairs. Both policies require finite nonnegative query weight,
positive finite average, and exact equality between query and selection average.
Complete blocks with incompatible provenance or average use the global term
bound. Loaded tails, or trusted blocks whose stored pair is absent, can compute
an exact maximum from their decoded documents. The public block-score method
clears its cache and uses conservative provenance because callers may change
weights between calls.

## Why selecting the native input is safe

The native writer maximizes `x = fl(frequency * norm_inverse)`, independent of
IDF or boost. For fixed finite `w >= 0` and nonnegative finite `x`, rounded
`1 + x` is positive and nondecreasing; rounded `w / (1 + x)` is nonincreasing;
rounded `w - w / (1 + x)` is nondecreasing. Thus the selected input bounds every
score under the same native average and any supported query weight. Selecting
a rounded final score or the old rational factor does not provide this proof.

The TF byte encoding remains unchanged: values through 254 are exact, and 255
decodes to `u32::MAX`. Integer-to-f32 conversion and multiplication by the
nonnegative inverse are monotone, so this ceiling is safe for native scoring.
Classic scoring retains its previous saturated-frequency fallback because its
rounded rational expression can decrease when frequency increases.

Two fixed average-100 witnesses prove that compatibility must be symmetric.
Norm ID 59/TF 1 and norm ID 87/TF 7 tie under legacy selection but have native
score bits `0x3eddd64c` and `0x3eddd64a` at weight 1. Conversely, norm ID 52/TF 1
and norm ID 103/TF 37 order one way by native input and the other way by legacy
factor. Reusing either format under the other policy can underbound a document.

Serializer regressions reproduce both witnesses through actual stored bytes.
They cover all 256 norm IDs, frequencies 1/7/254/255/256/16,777,217/u32::MAX,
averages .5/1/100/10,000/1e9/f32::MAX, zero/subnormal/1/3.25/f32::MAX/negative
weights, equal and unequal query averages, loaded and unloaded complete blocks,
tails, and changing weights through the public API. Reader tests check the
five-byte round trip and the real native Searcher-to-TermScorer route. Native,
classic, and overridden-statistics pruning agree with exhaustive evaluation on
native, legacy, and mixed segment sets. Footer tests check old-reader rejection
and current-reader acceptance of versions 4 through 11.

## Performance acceptance

Run the native correctness gate with no conversion or score boost. Only after
correctness passes and competing builds stop, compare unchanged Wiki COUNT and
TOP workloads, both process orders, cold/warm preparation, memory, and storage.
Format 11 restores tight native full-block bounds; legacy indexes retain the
conservative fallback until rewritten. Correct arithmetic and restored pruning
are prerequisites for a measured win, rather than evidence of one by themselves.
