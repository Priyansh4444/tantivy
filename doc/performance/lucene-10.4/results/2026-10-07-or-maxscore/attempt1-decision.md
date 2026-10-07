# First attempt: redesign before acceptance

Full1227 exact count/rank/rawbits/exhaustive gate passed. Pilot373cases, all301OR
plus Wiki20/seededcontrols: longerOR mean improves ~1.75–1.8x. But query401 regresses
2.17–2.45x;7191.60–1.66x;7911.55x;6911.48x. This broad routing is not accepted yet.
All first build/verification/profile receipts, binary and source retained inattempt1;
pilot raw samples/receipt remain inpilot. Source-shape risk identified ininitial
sketch: min of dense optional block ends forces many no-candidate regions.

Next measured unit uses a global-bound certificate for the optional prefix: these
clauses' GLOBAL summed maximum is already <=threshold, so their bounds cover the
entire remaining physical range. Omit their shallow metadata/boundary fromlocal
region creation; retain local bounds for every other clause. This is a range proof,
not extending a single-block bound. No new index bits or scratch allocation.
