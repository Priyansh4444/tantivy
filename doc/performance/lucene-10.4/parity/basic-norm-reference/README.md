# Basic field norms

The writer now records the number of distinct encoded term/document relations for
every indexed, normed Basic field. Repeated numeric, boolean, byte, date and IP
values previously inflated the norm despite already being deduplicated in postings
and collection statistics. This lowered their BM25 scores relative to equivalent
Lucene DOCS term fields.

## One norm input

`InvertedIndexPluginWriter::index_document` groups values by field and snapshots
the field postings writer's `total_num_tokens()` before subscribing those values.
After the field match, its counter delta provides both Basic norm length and the
existing nonempty-field population decision. No per-document set or additional
postings traversal is needed.

`SpecializedPostingsWriter<DocIdRecorder>::subscribe` increments this counter only
on a new encoded term/document relation. The caller routes are:

| Field | Writer/norm input |
| --- | --- |
| Text Basic | DocIdRecorder; central counter delta |
| U64, I64, F64, Bool, Date, Bytes, IP | DocIdRecorder; central counter delta |
| Text WithFreqs / WithFreqsAndPositions | Frequency recorder; existing IndexingPosition token count |
| Facet, JSON | No field norms |
| Custom / unindexed fields | Skipped before subscription |

The central eligibility condition is `has_fieldnorms() && index_record_option()
== Some(Basic)`. Basic text skips its previous local norm recording, so each
eligible field records once. Missing fields retain zero norms through the existing
writer fill, while fields configured without norms retain no norm component.

Identity follows encoded postings, rather than input value equality. Distinct
dates inside one indexed second alias to one term; positive and negative floating
zero remain two encoded terms. Repeated occurrences of either encoded term add
no further Basic norm length. Frequency text keeps its occurrence-based length.

## Matching reference and red evidence

`BasicNormReference.java` ran against unmodified Lucene core and analysis-common
10.4.0 jars. Its indexed fields use binary, non-tokenized terms with
`IndexOptions.DOCS`, `omitNorms=false` and default `BM25Similarity`. These are DOCS
term fields, not numeric/date/IP point fields. Lucene's pinned `Similarity.computeNorm`
uses `FieldInvertState.getUniqueTermCount()` for DOCS. Pinned source revision:
`9983b7ce7fdd04f4d357688fb85c14277c15ea8d` (`releases/lucene/10.4.0`).

Reference artifact SHA-256 identifiers:

```text
BasicNormReference.java c3ad77de69394b875525d3e8ece1ae748e20a776d1f65fb8cb836102326482ac
lucene-core-10.4.0.jar 8f894d211a8123938ccb9ff6827d136747e0eb6b1782ada6ac9086aa911b52e2
lucene-analysis-common-10.4.0.jar 8e768c9b2a3870f1fc2655181516699e719a56b9aaf8664226a11ae7d90cb4e9
```

The documents contain `[A,A,B]`, `[A,B]`, `[A]`, and an absent field. The binary A/B
values match the Rust U64 fixture's encoded dictionary values. Other scalar
fixtures exercise their corresponding two encoded identities, including date
aliasing, with the same postings population/frequencies.

```text
population=3 tokens=5 dfA=3 dfB=2
doc=0 norm=2
doc=1 norm=2
doc=2 norm=1
doc=3 norm=0
doc=0 score=0.0561056212 bits=3d65cf02
doc=1 score=0.0561056212 bits=3d65cf02
doc=2 score=0.0725713968 bits=3d94a050
```

Before the fix, four independent Rust integration tests failed:

```text
norms: left [3, 2, 1, 0], right [2, 2, 1, 0]
doc 0 score=0.045729928: left bits 1027297102, right 1030082306
retained merged norms: left [0, 1, 3], right [0, 1, 2]
repeated signed zero norm: left 4, right 2
test result: FAILED. 0 passed; 4 failed
```

The fixed tests assert the same raw Java score bits for all seven scalar fields,
exact physical statistics before pending deletion, exact mapped statistics and
term document frequencies after merge, copied retained norms, Basic versus
frequency text controls, missing fields, disabled norms, and floating encoded
identity. No score tolerance or comparator rule changed.

Verification completed on the isolated production base `53ab299a0`: normal
library checks passed 1,355 tests (seven ignored, one fixture-generation test
filtered), and release checks passed all four new regressions plus four existing
native BM25 tests. Changed-source formatting and diff checks passed. Parent owns
the combined latest-source pruning/parity gates and performance measurements.

## Compatibility and lifecycle

This is a writer-only correction. It introduces no public API or format change.
Existing serialized norm bytes remain readable and remain unchanged when merged;
merge copies/remaps norms rather than reanalyzing input values. Reindexing is
required to correct norms already written with duplicate raw values. New documents
receive the corrected norm. Statistics and block-bound selection use the existing
physical postings and actual recorded norms; their serialization contracts do not
change.

The local verification receipts are retained under
`target/basic-norm-receipts-oct03/`, including Java source/jar SHA-256 identifiers,
the original red output, green checks and commands. This unit makes no throughput,
point-field parity, overlap-discount or scoring-parameter claim.
