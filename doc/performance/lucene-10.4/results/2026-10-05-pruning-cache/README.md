# Block-bound numeric cache ownership — October 5

Public cursor evaluations now neither read nor populate the fixed TermScorer's
numeric cache. Previously, a public lower-weight or different-reader tail
evaluation could overwrite that cache; a later fixed-owner request then returned
an underbound. The new public path preserves the old complete-block global and
loaded/unloaded-tail literal results, while cache writes remain private.

Worker commit 7fcfe0ef488d3b338ff66e29febe01280de1b0f9; integrated release source
e13303b3aaa8c15815ab5ea951a3449111438e81. Exactly two source/test files change.
No scoring expression, normalization, selected-pair policy, query loop, codec or
index bytes change. The shared literal reducer retains actual decoder length,
NaN-to-infinity mapping, nonnegative contribution clamp and safety-first fallback.

Two real serialized-tail tests fail before the fix. Native weight mixing returns
0.71428573 instead of 71.42857; reader mixing returns 0.032658458 instead of 0.88495576.
The sequence uses crate-internal TermScorer cursor access followed by its public
cursor API. This is a reproduced internal ownership hazard, not a claim that the
frozen Wiki20 production path currently follows that sequence.

After the fix, 38 focused debug tests pass with one existing ignored long test.
Tests prime the owner cache, require an exact lower public result (proving it
ignores that cache), then require the unchanged high owner result (proving it
cannot poison it), for native/classic weights and readers. Additional coverage
checks full-global/unloaded-tail/loaded-tail semantics, public higher/negative/
nonfinite weights and preserved private results. Independent full source/log/
hash review approves; root personally reviewed the entire two-file diff.

Root built immutable uninstrumented native adapters from the exact integrated
commit and ran the 13 TermScorer regressions in release mode. All three unchanged
strict Wiki20 COUNT/top-ten/exhaustive/raw-score gates pass. Each profile has
1,913 common top100 scores with zero f32-bit mismatches. Original aligned indices,
classes and older measurement binaries remain retained.

Raw commands, expected failing and successful outputs, source/test-binary hashes,
independent review, actual release provenance and strict gate results are
compressed under raw, with original/compressed hashes in sha256.json.
run-release-original.py records executed absolute paths/output guards.
Compiler/test elapsed times are verification metadata, not search measurements.

This correctness prerequisite makes no speed claim. Query binaries differ from
the preserved baseline; the subsequent transform must still demonstrate its own
performance beyond noise and preserve DEFAULT/COUNT floors. The
[aligned profile baseline](../2026-10-05-native-profiles/README.md) remains the
measured comparison. Nondefault ranked losses and all 47-feature/public-API scope
remain unfinished.
