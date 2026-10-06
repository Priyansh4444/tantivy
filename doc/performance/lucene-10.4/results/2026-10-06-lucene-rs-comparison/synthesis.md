# Chosen benchmark contract

Root read A, B and the fresh judge end to end. Scores (payload, timing, simplicity,
runtime proof, bounds): A 5/4/3/5/4, B 4/3/4/4/5. Choose A, agreeing with judge.
Graft persistent workers from B, but use internal native timers and untimed transport.
Graft explicit timeouts and exclusive stage outputs. Reject the Java-index-open lane
(the port's format is incompatible), and defer the threshold-zero API experiment.
The primary top10 lane compares available APIs: port additionally tracks 1000 hits.
COUNT is a separate exact-output lane. Default BM25 only.

Use current Tantivy c0efbc99 and fixed port e5d1f81. Frozen Wiki20 and a generated
1201-query suite using the port's normalization/sampling over our retained SBG query
input. This does not claim identity with their untracked benchmark query files.

One standalone Cargo package, common compiler 1.99, opt3/fatLTO/codegen1/native,
overflow checks off, panic abort, no inherited profile overrides. Separate binaries
link their own engine. Prebuilt direct ASTs outside timers; rewrite/weight/scorer/
collector/result allocation inside API timer; result black_box/drop outside.

Shared JSONL QueryCase: id sequential usize, kind TERM/AND/OR/PHRASE, ordered
terms (duplicates retained), tags. ASCII [a-z0-9], length 1..255; TERM one token,
others >=2. Persistent worker args INDEX QUERIES. Startup READY JSON receipt.
Request JSON {op:"dump",id} returns {id,count,hits:[[doc,raw_bits]],reported:null
or {value,relation:"eq"|"gte"},oracle:{count,hits}}. Every response newline+flush.
Request {op:"run",id,mode:"count"|"top10",iterations} returns {id,mode,ns:[u64],
checksum:u64}; 1..256 iterations. Invalid commands terminate nonzero.

Root owns Cargo.toml, shared lib.rs, tantivy_worker.rs and controller. Port agent
owns port_worker.rs, port_prepare.rs only. Port prepare modes index CORPUS OUTPUT
MAP, audit INDEX MAP TERMS: new isolated replay index, stored-only id and decimal
sort, preserve empty docs and row order, field text TEXT positions/norms, batch25K
RAM500MB, force_merge1. Audit all physical tuples and canonical full postings/
positions digest, stats/checksums and invariants. No engine core modifications.

Correctness is mandatory: full fresh-port payload proof, exact count/rank/raw bits
and independently exhaustive scorer oracle per engine. Finite scores, distinct IDs,
descending score/ascending doc ties, all queries covered. No tolerances. A failing
query/category remains a failure; retain it and withhold that lane's speed claim.
Original port defect reproduction stays separate from fixed-port results.

Timing: serial CPU4 workers, controller off core4; AB/BA paired rounds, all frozen
queries, warmup10/full corpus, samples32 per order initially, same-binary TT/PP
controls. No own compilation/index/audit jobs during timing. User processes intact.
Native API nanoseconds, RSS after equal warmup, safe load telemetry. Each request
120sec maximum for exhaustive proof, timing request10sec; build/index/audit stage
60min bounds, timing45min total. Exclusive receipts, no silent retries or trimming.
No whole-index storage/indexing-efficiency claim: auxiliary fast-field capability
differs. Prove source/lock/index/query identities before/after.

Upstream fixes stay independent. Relation-aware comparator extends both producers
by a small explicit five-column format: prevents interpreting a lower bound as an
exact count; historical numbers retained with relation limitation. Root deliberately
chooses that contract over the judge's suggested legacy-only correction.
