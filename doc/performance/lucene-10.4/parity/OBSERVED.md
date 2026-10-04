Initial native differential run, 2026-10-03

The minimal field-population witness has four documents but only two nonempty text fields. Both engines store four tokens. Tantivy uses a BM25 population of four; Lucene reports text `docCount=2`. Counts and matching IDs agree, but scores differ after accounting for the declared 2.2 numerator convention. For `alpha`, Tantivy scores document 0 at 1.059496164 versus Lucene's 0.835574608 in the same convention. For `beta`, document 1 scores 0.693147182 versus 0.229204263.

The six-document ranking witness has three nonempty fields and twelve tokens. Tantivy's average length is 2; Lucene's is 4. For `alpha`, Tantivy ranks document 0 above document 1 (1.294378877 versus 1.241185188). Lucene ranks document 1 above document 0 (raw 0.3159688 versus 0.3081991). The population mismatch therefore changes ranking as well as score scale.

The expanded run covers 14 configurations and 318 queries. It reports 40 Tantivy/literal matching failures and the same 40 cross-engine matching failures, all involving repeated sloppy phrases. A control corpus with no missing fields and no deletes returns 234 Tantivy hits for `alpha alpha` at slop 1, while Lucene and the literal oracle return 117. Tantivy incorrectly reuses one occurrence for both repeated query terms. At slop 300, the wide-slop fixture similarly returns five Tantivy hits versus one Lucene/oracle hit. Nonrepeated wide-slop matching passes at 255, 256, 260, 300, and 511.

Nonrepeated sloppy phrase scores also differ: Tantivy counts integer phrase occurrences, while Lucene weights sloppy occurrences by distance. This can change top IDs even when counts and document sets match. All non-sloppy term, boolean, filter, boost, and exact-phrase controls with nonempty fields pass the score comparison, including deletions and one versus three segments.

The same-convention statistic/phrase diagnostic reports 226 query/configuration score mismatches and 75 canonical top-ten ordering differences across the missing-field, random, merged, and sloppy-phrase cases. These remain failures rather than an allowed baseline. Both engines' normal counts/top collectors agreed with their own exhaustive scorers in that initial 14-configuration run.

Reproduce through `run.py`; raw JSON and the complete report were saved initially under `/tmp/parity-expanded-config-oct03`, with the minimal witnesses under `/tmp/parity-minimal-expanded-oct03`. The runner always emits a fresh frozen corpus and per-engine results to the selected output directory.

The full native score gate also compares the unmodified raw scores. It fails even for the nonempty controls because Tantivy currently includes the global 2.2 numerator factor. Passing the diagnostic conversion alone never constitutes full native API parity.

The final expanded fixture set contains 16 configurations and 330 queries. It adds the three-term 256/257 carry witness and longer interleaved repetition groups. The 256 witness incorrectly matches in Tantivy and correctly does not match in Lucene; 257 matches in both. Two longer-query cases also expose Tantivy COUNT versus exhaustive-score disagreement: `alpha beta alpha beta` at slop 1 counts 5 versus 4, and `alpha alpha alpha` at slop 1 counts 6 versus 3. The final summary is recorded in `observed-summary.json`; no Lucene/literal mismatch was observed.
