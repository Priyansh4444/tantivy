# Root synthesis: streaming local MaxScore first

Root read both design packages end to end and checked the score and shallow cursor
sources, then read the independent judge. Root scores (criterion order in grounding):
A = [4,4,3,4,5] (20/25); B = [4,4,5,4,5] (22/25).
Agreement with fresh judge: select B, not a blend of two execution engines.

The public API remains unchanged. One private `or_maxscore(scorers, max_doc,
threshold, callback)` entry point drives admitted 3–32 term sum-in-f64 unions.
One/two-term, high-clause, generic combiner, COUNT and other Boolean routes remain
as before. Unsafe numeric admission uses exhaustive SumCombiner before mutation.
No codec/writer/serialized/index-format change. Scratch O(clauses) once per query.

Grafts from A: physically reconcile a selected tail at the region floor; zero a
local bound when an actual doc is beyond the final region; explicit stale loaded
cursor proof. `has_remaining_docs` alone is not proof of target exhaustion.
Use B's physical max_doc cap, stable original scorer array, static preference
ordinals, non-subtractive inclusive optional-prefix bounds and exact original-order
f64 leaf fold. Rebuild partition at region entry; permit enlargement only after
all current essential matches advance, if the implementation stays readable.

Rejected for first unit: 256-ID contribution matrix, two levels of windows, pooled
scratch, local cost sorting every region, richer on-disk impact hierarchy, inferred
required terms. These add separate cost/proof surfaces before the smaller shape has
been measured. Keep A as a measured alternative if streaming does not win.

Architecture Ground/Sketch complete, Agree default proceed. Arena Frame/Fanout/
Cross-judge/Pick/Graft complete; design Verify is conditional on actual implementation
checks, not a speed claim. No dropout (an interrupted first judge had no output and
was replaced by a fresh judge). Main risk is short-region overhead. Acceptance:
exact bits/ordered results/full frozen suite, safe cursor/domain tests, broad AB/BA
performance with same-engine controls, post-profile, RSS/binary size/index hashes.
Do not ship a broadly regressive route. Root owns every build and benchmark job.

## Measured implementation refinement

Attempt1 passed exact1227, but the pilot's dense-optional/sparse-essential queries
regressed badly despite a longerOR aggregate win. Root retained that source/binary/
trace and rejected acceptance of broad route. The refined region preparation first
certifies a static prefix using GLOBAL maxima <=threshold. Those leaves are optional
for the whole range, so their block ends need not fragment local intervals. Allother
terms retain local certificates. This uses the global range proof explicitly omitted
from single-block widening; it is not a global-only replacement of local MaxScore.
No new allocation or index bits. Child implementation and root readback include
two extra exact-trace tests covering sparse gaps/tails and threshold-enabled global
certificates. Final acceptance still requires a new build/verify/pilot/fullmeasurement.

## Final measured cost choice and acceptance

Attempt2 global widening was rejected for comparable-density high/school clauses.
Global prefix widening now requires each prefix term's posting-count hint to be at
least twice the maximum hint of remaining live terms, with u64 arithmetic. Other
regions retain tight local certificates. This adds no arrays or per-region allocation.
Final pilot corrected prior regressions; full-suite default/configured raw bits pass.
Complete main run shows all 103 longer OR cases improve in both orders beyond
observed controls, about 2x by arithmetic and geometric mean. Root read every change
and post-trace. Design Verify and runtime Verify pass for this bounded unit, with
remaining port gap, contention and memory measurement limits recorded in README.
