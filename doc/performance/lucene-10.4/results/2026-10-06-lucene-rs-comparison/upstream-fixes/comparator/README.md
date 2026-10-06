# Upstream comparator correction evidence

Worktree: `/home/pronsh/Coding/playground/search/lucene-rs-comparator-fix-oct06`; branch `fix/benchmark-equivalence-gate`. Base `465ea8f77f4397e35de56b7f7d202158d59e5871`.

Test-first `1b5f5f26110fdf2c377c40f6f6b2a166a0678d0b`; fix `cdc6d0634e05cee5e83cdd110cfdb91df0736f4a`. No port core/manifest/lock changes, no timing, push, or PR.

Original runtime repro deliberately changed docs, scores and hit count; comparator printed DIFF and returned **0**. `baseline-repro.json` preserves exact stdout/source hash/command. New tests then ran before implementation: 10 methods, 27 failed assertions; `test-first.stderr` and `test-first.json`. The tests introduce the requested five-column relation-bearing protocol as well as checking gate behavior.

After correction, the same mismatch values with explicit `eq` relations return **1**; `fixed-repro.json`. Strict equality returns0; malformed inputs return2 and legacy relationless dumps request rerunning both dump commands. Python tests cover kind/text, row/hit lengths, IDs/order, one-ULP score, signed zero, same roundedf32 decimal spellings, hit value/relation, malformed/nonfinite/overflow input and Python -O behavior. Final10 methods pass (`final-python.*`).

Pinned Rust1.99.0 b940084d7 LLVM23.1.1; isolated `/home/pronsh/Coding/playground/search/bench/lucene-rs-upstream-comparator-oct06/target`, jobs2. fmt, locked clippy workspace/alltargets/allfeatures -Dwarnings, locked workspace/allfeatures tests (**29 tests plus1 doctest**) and fixture binaries pass. Python render --check passes. All commands/stdout/stderr/exitcode/timestamps retained in stage-named files.

OpenJDK21.0.12.1 compiles changed Bench.java against retained Lucene10.5.2 jars. New Java and Rust index/dump commands ran on a fresh positions-enabled1208-doc fixture, including a frequent term requiring threshold1000 pruning, rare/absent term, AND, OR, exact phrase. Both emitted normalized eq/gte and the comparator passed all6 queries/41score bits/order/values/relations in normal and optimized Python. Decimal scientific notations differ visibly but roundtrip to equal f32. See `fixture/{corpus.txt,queries.tsv,java.tsv,rust.tsv}`, `fixture-cli.*`, `fixture-cli-optimized.*`, `final-artifacts.json`.

The comparator is a strict implementation-output equality assertion. Different lowerbounds can be semantically valid; it does not independently verify exact match counts or prove identical skipped blocks. Historical1201-query relations were never retained and that full benchmark was not rerun. README preserves historical numbers and states this limit. New Python discovery runs in CI beside render checking; generated __pycache__ files are ignored.

Exact patches are `test-first.patch`, `fix.patch`, `complete.patch`; reviewed tracked source hashes and clean status in `completion.json`. Root review/submission remains outside this delegated implementation.

## Root review follow-up: top-10 input contract

Root review found identical malformed dumps could contain duplicate document IDs. The follow-up also validates no more than10 hits and reported total>=returned hit count, applicable to both eq/gte. Tests cover all three malformed contracts under normal and optimized Python. Test-first commit `966966ea9eb6900f6ed63a6c938023a52a514019` produces6 failed assertions across11 methods (`top10-contract-test-first.*`); fix `8f338d0be097746801f739f98e2b87437a12e5f4` makes all11 methods pass (`contract-final-python.*`).

Final fmt/render and the existing real Java/Rust fixture CLI in normal/optimized Python pass again (`contract-*`). No Rust or Java source, manifests/locks or core changed relative to original fix `cdc6d0634e05cee5e83cdd110cfdb91df0736f4a`, so existing compilation/clippy/Rust runtime/Java writer fixture checks remain applicable; no heavy checks or indexing repeated. Source identity assertions are retained in completion.json. Original completion preserved as completion-first-fix.json; original full patch preserved as complete-initial.patch. Complete.patch now spans allfour commits to final head.
