# Review of compact clause metadata

Root reviewed the complete production diff and fixture, rather than accepting a
child summary. Admission and canonical fallback still precede cursor mutation.
Cached global maximum and doc frequency are immutable in TermScorer. The stable
original ordinal tie break preserves preference order. Every actual seek/advance
in this engine goes through two private ClauseState methods assigning the returned
document ID. Shallow selection and bound/score reads leave the mirror unchanged;
debug checks certify coherence at those boundaries. The previously selected block
end is preserved after tail reconciliation. No mutable cursor escapes the engine.

The fused local-bound cleanup/prefix loop preserves the original addition order,
and every local_max branch initializes the field for that region. Published
contributions still replay the original incoming ordinal order in f64 and round
once; the candidate clear remains dense and unchanged. Admission, deletions,
physical bounds, strict threshold/tie behavior, segment merge and COUNT routes
retain their existing behavior. No disk format or public API changes.

The new oracle fixture forces an actual exhausted high-cost remainder to change
the live-density decision, proves both policy branches were reached, and compares
the whole callback stream including exact score bits with BufferedUnionScorer.
The observer is thread-local and cfg(test); release has no observer or counters.
The final observer tuple has a private cfg(test) RegionChoice alias to remove the
new Clippy type-complexity warning. All final gates bind the resulting source.

Acceptance is conditional on paired native timings and observed self controls.
One fewer allocation is not a claim of lower RSS: array payload grows by 384 bytes
at 32 terms and is verified from the actual host type layout. Remaining port gap
must be reported from the fresh actual-port lane, not old headline ratios.
