# Root synthesis: compact clause metadata, one mechanism first

Root read all three packages end to end and audited doc(), shallow seek_block(),
block bounds, score() and actual seek()/advance() source behavior. Read fresh judge.
Root rubric scores: A[5,4,4,4,5]=22; B[5,3,5,3,5]=21; C[5,3,5,4,4]=21.
Fresh judge scores all22, selects A for strongest causal link. Agreement on base.
No dropout. Arena Frame/Fan out/Cross-judge/Pick/Graft complete. Architect Ground/
Sketch/Agree complete; default proceed. Runtime Verify remains conditional.

Choose ONLY the compact ClauseState vector replacing order and local_max vectors.
Cache fixed original ordinal, global maximum, doc frequency and exact current doc;
keep local bound in that compact record. Sort records by the same cached priority
and original ordinal tie break. Original scorer and contribution arrays remain in
incoming order. Preserve global-prefix from-zero arithmetic and existing live cost
choice exactly in this first measurement; no GlobalPrefix state, no new bound gates.

Real cursor mutations go through private record methods that assign returned doc.
Use source-backed debug coherence checks before/after mutations, after shallow
selection/bound evaluation and region boundaries. Numeric scoring itself does not
move the cursor. Shallow selections leave the decoded doc stale in both objects;
actual seek still reconciles. No mutable scorer escape or public state/API change.
Region cleanup and prefix construction can form one sequential record pass; each
prefix depends only on the cleaned current local bound and previous prefix.

Graft B's verification focus: a demoted optional/previously skipped clause later
becomes essential, with exact traces across tails. Existing tests already force it.
Graft C's guardrail: do not mistake region-bound cleanup for contribution clearing;
keep dense candidate replay unchanged in this unit. No implementation mechanism
from either losing design is combined before a causal measured win.

Reject for this unit: monotonic global certificate cache (sound but separate mechanism),
sparse bitmap replay (hypothesis less directly visible), threshold repartition and
single-driver duplicate kernels (larger control-flow/proof surface), buffered window
scoring and richer index impacts (larger memory/format costs).

Expected scratch36n+8 bytes at64-bit, max1160, one fewer allocation (three vsfour);
no per-region allocation or on-disk change. Measure code/RSS, do not promise memory
win. Require all exact gates and full balanced AB/BA+controls before accepting.
Root owns heavy build/tests/timing jobs and source review. If compact records regress
or friction repeats, reject/redesign from ground instead of stacking cost gates.
