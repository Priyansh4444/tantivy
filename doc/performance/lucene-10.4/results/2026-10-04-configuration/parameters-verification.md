# BM25 parameter verification

Isolated worktree: /home/pronsh/Coding/playground/search/tantivy-parity-bm25-parameters-oct04
Base: 7db890908f1cc8422905102da7740733fe3f22f6
Red commit: 6d0d5a058
Fix commit: ab9682faa

Architecture: design A direct immutable Searcher, independent B provider facade,
fresh judge, synthesized A. Complete packages copied into this receipt directory;
committed synthesis under doc/performance/lucene-10.4/parity/design/bm25-parameters.

Pinned Java source: releases/lucene/10.4.0 commit9983b7ce7fdd04f4d357688fb85c14277c15ea8d.
Unmodified core and analysis-common10.4.0 jar/source hashes are committed beside
Bm25ParametersReference.java and reference.csv. Execution script retained at
/tmp/run-bm25-parameters-java-oct04.py. lucene-reference.log has actual output.

Executed commands/results, own target, jobs4:

- `cargo test -j 4 --test bm25_parameters -- --nocapture`: red 0 passed/1 failed.
  Profile .9/.4 doc0 expected bits0x3e9c42ce, actual DEFAULT0x3e83dbca (red.log).
- Same command after implementation: 1 passed/0 failed (green-witness.log).
- `cargo test -j 4 --test bm25_parameters --test bm25_parameter_configuration --test native_bm25 -- --nocapture`: 1+4+4 passed/0 failed (configuration.log).
- `cargo test -j 4 --lib parameter_ -- --nocapture`: 5 passed/0 failed (parameters-lib.log).
- `cargo test -j 4 --lib -- --skip create_format`: 1360 passed/0 failed/7 ignored/1 filtered (lib.log).
- `cargo test -j 4 --release --lib parameter_ -- --nocapture`: final five parameter tests including adjacent-float thresholds passed (release-parameters.log).
- `cargo test -j 4 --release --test bm25_parameters --test bm25_parameter_configuration --test native_bm25 --test query_pruning_correctness`: 1+4+4+8 passed/0 failed (release-query.log).
- Changed-source rustfmt --check with skip_children=true and git diff --check: passed.

Original red production was unchanged; only a clearly marked DEFAULT-only test
configuration helper existed. Fix replaces that helper with the validated public
builder. No oracle values changed. Digests preserve all finite/signed-zero bits;
NaNs use Java Float.floatToIntBits canonicalization, not numeric tolerance.

Normal gate preceded the final additional threshold loop in the new test only;
production behavior was already identical. Release private gate executes that final
loop. Parent owns latest combined normal/parity gates; no extra broad repeat here.
Full normal skips create_format to preserve historical generated fixtures.

Scope: validated immutable query parameters, per-field Searcher settings, coherent
snapshot opt-in preserving arithmetic policy, DEFAULT-bit stored-pair/ceiling
eligibility, one-time cache bound classification, early exceptional bound fallback.
No norm policy, arbitrary Similarity hook, postings bytes, reader footer, adapter,
main branch, pushes, benchmarking or score ruler changes. Existing native_bm25.rs
formatting left untouched as parent requested. Prior7db performance is immutable,
not evidence of new parameter-unit performance. Nondefault profiles may prune less
effectively with the conservative global fallback. Invalid custom-statistics NaN
collector ordering is not claimed; direct scalar classification remains literal.
