# Sloppy phrase parity correction

Plain `PhraseQuery` with positive slop uses a dedicated positional frontier.
Exact slop zero retains the existing position cursor, intersection, and pruning
paths. The frontier orders clauses by normalized position, query offset, and
original query ordinal; the ordinal survives document-intersection cost sorting.
Distances use signed 64-bit arithmetic and compare against the full `u32` slop.

Term identity from `PhraseWeight` supplies repeat groups. Repeated clauses at
separate offsets consume distinct actual token positions. Same-offset duplicate
clauses retain Lucene's sharing rule. Collision repair advances the lesser
normalized clause and repairs queued keys. The successor captured before an
advance remains unchanged until the next pop, including across collision repair.

The matcher emits windows from Lucene's frontier traversal, rather than all
Cartesian assignments. Each emitted distance contributes `1 / (1 + distance)`
to an ordered `f32` frequency. Scoring and explanations consume that fractional
frequency. BM25 statistics and the existing numerator convention remain unchanged.

Reference: [Lucene 10.4 SloppyPhraseMatcher](https://github.com/apache/lucene/blob/releases/lucene/10.4.0/lucene/core/src/java/org/apache/lucene/search/SloppyPhraseMatcher.java)
and [PhraseQueue](https://github.com/apache/lucene/blob/releases/lucene/10.4.0/lucene/core/src/java/org/apache/lucene/search/PhraseQueue.java).

## Evidence

The red regression commit `fb0291c57` reproduces occurrence reuse, three-term
slop-256 carry, and integer-versus-fractional scoring. New tests add independent
bounded two/three-term assignment existence oracles, repeated scorer danger
transitions, same-offset sharing, and fractional explanations. Frozen native
Lucene explanations anchor high-repeat frequency: 97 alpha tokens with
`alpha alpha alpha` slop 1 give frequency 95; eight alpha-beta pairs with
`alpha beta alpha beta` slop 4 give frequency 9.

The frozen synthetic differential corpus has 16 configurations and 330 queries;
SHA256 `f632da13074c6556bcd9a2ba7605d7974f96c5bbab455100dfb711cfa30c8826`.
After this correction, literal matching, cross-engine match sets/counts, and each
engine's normal collectors versus exhaustive scoring all agree. The remaining
48 report entries are ranking differences; the 198 converted-score failures
occur exclusively in configurations with missing/empty fields. The 301 raw-score
failures remain. Thus this is phrase-semantic parity evidence, not full native
score parity. All four deterministic nonmissing controls, wide-slop witnesses,
and interleaved-repeat fixtures pass the declared same-convention diagnostic.

The runner lives in the parity-gate worktree under
`doc/performance/lucene-10.4/parity/run.py`. Run a copied standalone Rust manifest
with its Tantivy dependency pointed at this worktree and a separate Cargo target.
The recorded artifacts are under `search/bench/phrase-parity-results-oct03`.

## Scope limits

The public generic `PhraseScorer` constructor receives postings without term
identity. It preserves its signature and cannot resolve repeated identity by
comparing position arrays. Regex phrase alternatives also lack term-set collision
groups; their repetition semantics are not claimed fixed. Their nonrepeated
frontier scoring and explanations follow the new frequency convention.
Token graphs, overlapping synonyms, explicit multi-term alternative groups, and
Lucene Matches position/offset enumeration are outside this correction's proof.

No speed claim is made for this correctness change. Exact-path regressions cover
phrase prefixes, pruning, global statistics, and danger seeking. Performance
comparison belongs to the integrated candidate after native statistics changes.
