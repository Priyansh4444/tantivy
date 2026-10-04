# Query-time BM25 parameters

```rust
let parameters = Bm25Parameters::new(0.9, 0.4)?;
let searcher = reader.searcher().with_bm25_parameters(parameters)
    .with_field_bm25_parameters(title, Bm25Parameters::new(2.5, 1.0)?)?;
let hits = searcher.search(&query, &collector)?;
```

Parameters are validated against Lucene 10.4's constructor domain: finite k1>=0
and non-NaN b in [0,1]. Getters preserve float bits, including negative zero.
Parameters belong to the immutable Searcher handle. Builders preserve explicit
field overrides; old clones keep their settings; fresh reader handles start at
DEFAULT. Field validation checks the current schema's ordinal, not its origin.
Reader generation and shared readers remain unchanged. No norm bytes are rewritten.

The coherent field-statistics snapshot carries parameters together with population,
token total, average rounding and native/classic arithmetic policy. An explicitly
supplied statistics provider remains authoritative over Searcher settings.
`Bm25FieldStatistics::new` and historical public single-term weight factories stay
classic/DEFAULT with their original bits. `snapshot.with_parameters(parameters)`
opts in to different parameters while retaining that snapshot's arithmetic policy.
Explanations report actual k1/b. Existing term, phrase, regex phrase and phrase-prefix
provider routes consume the same snapshot.

## Literal arithmetic and bound provenance

Native cache/scoring remain literal f32 expressions from pinned Lucene:

```text
inverse = 1 / (k1 * ((1 - b) + b * decoded_norm / average))
score = weight - weight / (1 + frequency * inverse)
```

No zero-k1 rewrite, f64 cache promotion or reciprocal clamp is used. Native IDF
and phrase IDF retain their existing double/float boundaries. Explicit classic
parameters retain the historical ratio and `(1+k1)` numerator convention.

Accepted extreme values can create infinite intermediates. For finite positive
matching frequency, a nonnegative inverse (including positive infinity) yields
the global bound max(weight,0) with finite weight. Negative zero k1 can produce
negative infinity inverses: frequency>0 gives denominator negative infinity,
division signed zero, and score exactly weight. This is a separate constant-score
proof. Positive subnormal k1 may underflow normalization to zero; MAX k1 may
overflow it to infinity. Those scalar outcomes match Java.

Native nonempty physical statistics have average>=1 and decoded norms are finite,
so the inner component remains finite/nonnegative. Invalid custom or overridden
tiny averages can produce NaN through zero times infinity. Frequency zero times
an infinite inverse also produces NaN. Scalar outputs retain these classifications;
this unit does not promise rankable NaNs or NaN TopDocs parity.

Cache construction determines bound safety once. DEFAULT with a positive finite
average has inner component at least .25 and needs no per-entry classification;
the const false cache path preserves its prior arithmetic. Other profiles classify
all entries. Unsafe cache or nonfinite weight yields global infinity **before**
cached pairs or loaded-tail reduction. The residual maximum reducer maps unexpected
NaNs to infinity rather than silently discarding them with f32::max.

Existing serialized pairs certify exact DEFAULT float bits, plus their existing
native/classic policy and selection-average provenance. Every nondefault profile
rejects them, even at an equal average. Frequency-ceiling support stays restricted
to safe native DEFAULT weights. Writers and merges select DEFAULT; no postings
bytes or footer version change. Nondefault queries may prune less efficiently:
complete blocks use a global bound while safe loaded tails can use actual maxima.

## Executed reference and regression

`Bm25ParametersReference.java` ran with unmodified Lucene core/analysis-common
10.4.0 jars. Source/jar hashes are in `sha256.txt`; source is pinned to
`9983b7ce7fdd04f4d357688fb85c14277c15ea8d` (`releases/lucene/10.4.0`).

The real IndexSearcher fixture contains `alpha alpha beta`, `beta`, and
`alpha beta`, using norms-enabled text fields and default index-time similarity.
Query-time similarity varies independently. Before implementation, the executed
Rust witness on untouched production failed at profile (.9,.4), document 0:

```text
actual DEFAULT: 0.25753623, bits 0x3e83dbca (1048828874)
expected Lucene (.9,.4): bits 0x3e9c42ce (1050428110)
test result: FAILED. 0 passed; 1 failed
```

The red test's explicit DEFAULT-only configuration helper was replaced by the
validated public builder. Fixture inputs and every expected score remain unchanged.
Constructor validation ran in Java before the Rust API existed; Rust validation
tests entered with implementation because absent APIs cannot execute a runtime red.

`reference.csv` contains accepted/rejected parameter values and getter bits,
twenty real query scores, thirty scalar digests over all 256 norm IDs, eight
integer/fractional/zero frequencies and six boosts, plus separate subclass-only
zero/tiny-average cases. Digests use Java Float.floatToIntBits's canonical NaN;
finite values, infinities and signed zeros retain exact bits. The tiny-average
subclass cases are not claimed to be native physical index states.

Further tests exercise cloned/per-field handles, global override preservation,
invalid fields, supplied legacy provider authority, explicit classic snapshots,
boosts, exact/sloppy phrases and prefix explanations. Actual serialized 401-document
postings include complete blocks, tails and TF300 ceiling metadata; custom profiles
reject DEFAULT pairs, all bounds cover actual finite scores, and optimized term/
Boolean results agree with exhaustive scoring at adjacent-float thresholds and
several TopDocs limits. Exceptional caches/nonfinite weights force infinity even
after an earlier internal bound cache or on a loaded tail. Existing DEFAULT custom
provider score-bit tests remain unchanged.

Verification receipts and architecture candidates are retained in
`target/bm25-parameters-receipts-oct04/`. The synthesized ownership/bound design is
in `../design/bm25-parameters/synthesis.md`. This is parameter support, not arbitrary
Similarity subclasses, new overlap behavior, or a performance claim. Root's prior
7db benchmark artifact remains independent of this new code until remeasured.

Completed checks on the isolated base: full normal library tests passed 1,360
tests (seven ignored, fixture-generating create_format filtered); release passed
all five private parameter tests and seventeen public configuration/native/pruning
tests. The final adjacent-threshold extension ran in the release parameter gate;
the full normal gate had already passed the same production implementation.
Changed-source formatting and diff checks passed. Historical native_bm25.rs and
all score-comparison rulers remain unchanged. Parent owns combined latest-source
overlap/parameter orthogonality fixtures, strict corpus gates and new timing.
