# BM25 parameter synthesis

Base: candidate A (`bm25-parameters-design-a-oct04.md`), with provider-compatibility
and exceptional-float cautions from B (`bm25-parameters-design-b-oct04.md`). Fresh
judge (`bm25-parameters-design-judge-oct04.md`) agrees. Both were read in full.
Only one external candidate runner fit the available team slots; A was authored
by the unit owner, B independently by the reviewer, then a fresh judge evaluated
both complete packages. No production edit preceded the architecture synthesis.

Rubric: Java parameter/arithmetic exactness; old provider/API bit compatibility;
bound provenance/exceptional-float safety; immutable cloning/perfield ownership;
maintainability. A and B agree on arithmetic and bounds. A wins caller compatibility
and maintenance by retaining actual Searcher, existing Query::explain callers,
and EnableScoring. Reject B's facade/new public forwarding API and Deref hazards.

Implementation contract remains A, with these explicit grafts/clarifications:

- Untrusted cache or nonfinite weight returns global infinity before using any
  block-bound cache, stored-pair access or loaded-tail reduction. This categorically
  prevents f32::max from hiding NaN. Positive-frequency scalar NaNs remain literal;
  no finite ordering/TopDocs contract is claimed for invalid custom statistics.
- Native -0 k1's negative-infinite inverse has its separate constant-w proof for
  finite positive frequency; it is not called nonnegative. Exact zero-frequency
  outcomes are checked directly against Java. Cache construction stores capability
  once; scalar scoring keeps the pinned expression without parameter branches.
- DEFAULT bit profile is required for stored pairs and frequency ceiling. Existing
  writers/readers/mergers keep their format/provenance unchanged; document tag1 as
  DEFAULT. No configurable stored profile or format bytes.
- Config is outside shared SearcherInner; supplied providers remain authoritative.
  Classic snapshot opt-in does not become native; public historical factories and
  default custom snapshots keep previous average, IDF and evaluation boundaries.
- Bound proof applies to finite matching scores and finite positive frequencies.
  Valid native nonempty stats imply avg>=1, making extreme accepted parameters
  finite/non-NaN on matching terms (infinite intermediates are still allowed).
  Arbitrary subclass-only tiny/zero averages stay a separate direct-score fixture.

Pinned Java validation/raw/public-term fixture ran before implementation. Runtime
red on untouched production uses an explicitly DEFAULT-only helper, comparing the
captured nondefault profile (.9,.4), not changing the expected scores. Validation
API tests enter with the fix because the API is absent on the red production base.
After focused green, run full normal and release pruning checks in own target jobs4.
No performance claims carry forward from the immutable root7db metrics.
