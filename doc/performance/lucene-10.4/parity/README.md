This standalone harness indexes identical explicit token sequences in Tantivy and Lucene 10.4.0, then constructs queries directly from a shared JSON AST. It uses native BM25 statistics. It does not use `MatchedStatisticsSearcher`, either query parser, or either text analyzer.

Run from the repository root (Java 21 or newer is required):

```sh
python doc/performance/lucene-10.4/parity/run.py --minimal --output /tmp/parity-minimal
python doc/performance/lucene-10.4/parity/run.py --output /tmp/parity-expanded
```

The default jar location is the sibling `search-benchmark-game/engines/lucene-10.4.0/build/dependencies` directory. Override it with `--jars`. The standalone Rust manifest is `rust/Cargo.toml`; its default target directory is `/tmp/tantivy-parity-build-oct03`. Neither the repository manifest nor production code is modified. To replay a frozen corpus, pass `--cases /path/to/cases.jsonl`.

Each output directory contains frozen cases, both engines' raw results and stderr, and `report.json`. Exit status 1 means a differential check failed; native mismatches are retained as failures. `--skip-build` reuses binaries and the Java classes in the selected output directory, so only use it when those artifacts match the current sources.

Each case supplies unique integer IDs and a `tokens` array per document. `null` or an omitted array means missing text; `[]` means an empty field. Positions are contiguous, starting at zero, without overlapping tokens. A `segment` label change commits the current segment; automatic merging is disabled. `deleted: true` deletes the ID after all input is committed. `merge: true` explicitly rewrites the remaining segments into one. `fieldnorms: false` disables norms in both engines.

Query AST shapes:

| Type | Members |
| --- | --- |
| `term` | `term`: literal token |
| `phrase` | `terms`: two or more literal tokens; `slop`: nonnegative integer, default 0 |
| `boost` | `query`: child AST; `boost`: positive finite number |
| `bool` | `clauses`: `{occur, query}` entries; optional `minimum_should_match` |

Boolean occurrences are `must`, `should`, `must_not`, and `filter`. A filter contributes no score. The Tantivy adapter represents it as a required `ConstScoreQuery` with score zero. Boolean defaults follow Lucene: without a required clause, at least one optional clause must match; required clauses permit zero optional matches unless an explicit minimum says otherwise.

The adapters collect top ten hits normally and independently walk a fully scored, unpruned query weight. The Python oracle checks live matching IDs directly against literal tokens. For sloppy phrases, the independent literal oracle enumerates positional assignments, rejects reuse of the same occurrence by repeated terms, and compares the full normalized position range with slop. It checks existence rather than summing assignment scores, because Lucene deliberately scores only windows encountered by its positional frontier traversal. The runner checks exact counts, exhaustive IDs, scores, and whether a top-ten collector omitted a better hit. Across engines it compares scores and the top ten after ordering equal scores by external ID; actual collectors may choose different documents at a tied boundary because their internal document IDs differ.

The overall `passed` gate requires native raw-score parity as well as count and ranking parity. A passing converted diagnostic cannot make the overall verdict pass. Raw native scores are preserved. Tantivy includes `(k1 + 1)` in the BM25 numerator; Lucene omits that global factor. The report separately records raw-score mismatches and a statistic/phrase diagnostic comparing Tantivy to `2.2 * Lucene_raw`, using `k1=1.2` and `b=0.75`. This declared scoring-convention conversion changes neither collection statistics nor ranking. Score tolerance is `4e-6 * max(1, abs(score_a), abs(score_b))`.

The fixtures include a minimal missing/empty-field statistics witness and a separate ranking flip, deterministic term-frequency outliers, one and three segments, deletes, explicit merges with and without norms, seed 7/19/31 corpora, boolean nesting and minimum matches, exact repeated phrases, and slop around the 255/256 boundary, a three-term carried-slop failure at 256, and longer interleaved repetition groups. See `OBSERVED.md` for the initial failures.

Unsupported scope: arbitrary parser syntax, token graphs or overlaps, multivalued fields and position gaps, phrase alternatives, prefixes/wildcards/fuzzy queries, negative or zero boosts, custom similarity parameters, and arbitrary analyzers. These are not normalized into the supported AST. Native sloppy phrase behavior is exercised even where a mismatch is already known; the harness does not silently exclude it from its verdict.
