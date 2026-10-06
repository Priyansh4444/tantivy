# Cross-judge verdict

Read both `design-a.md` and `design-b.md` end to end. This evaluates proposed designs, not completed correctness or performance evidence. No engine changes or heavy jobs were performed.

throughput checkpoint: n/a, read-only investigation

**Use A as the base.** It measures the intended warm native search API with the same scope in both engines and supplies the more concrete route to full 1M text-payload identity. Borrow B's persistent worker scheduling and explicit execution bounds. Transport and query AST construction must remain outside the primary native timer.

| Criterion (1 weak, 5 strong) | A | B | Judgment |
|---|---:|---:|---|
| Payload/semantics identity | 5 | 3 | A specifies the replay build, complete ordered physical/norm checks, full public postings/positions traversal, digest framing and expected counters. B asks for full proofs but first tries an impossible Java-index opening path and makes the closer top-k policy depend on another core API. |
| Timing fairness | 4 | 3 | A times both native public searches after AST construction, including rewrite, weights/scorers, collectors and returned result allocation. Its threshold1000 versus no-totals difference is explicit. B symmetrically measures IPC/parsing/AST/encoding too, but that is a different caller-latency workload and can obscure fast native search differences; symmetry alone does not make it the right primary metric. |
| Minimal adapter complexity | 3 | 3 | A's full verifier and independent Boolean/phrase oracle require substantial care. B keeps its wire boundary small, but couples both a matched top-k lane and its exhaustive port oracle to a new threshold API and adds timed transport machinery. Neither is a trivial adapter. |
| Runtime correctness proof | 4 | 3 | A provides a concrete full-payload proof and demands strict cross-engine scores/counts/order plus optimized versus exhaustive checks. Its exact phrase/Boolean oracle remains implementation work. B demands similar gates, but its stock-index route cannot work and its port oracle is contingent on the deferred API. Neither has executed these gates. |
| Bounded reproducibility | 4 | 5 | Both freeze provenance and refuse overwrites. B adds request, phase and total deadlines with an explicit incomplete-result state; A names bounded lifetimes without assigning operational limits. |

## Why A is the base

A's “Caller first” and “Core shapes and boundaries” define the useful primary timing scope: retain opened readers and prebuilt engine ASTs, then create a fresh weight/scorer/collector through each engine's real public API for every search. Stop the timer when that API returns; consume and release results outside it. This preserves the API's actual work while excluding controller transport and adapter serialization. Integer nanosecond samples are retained. B's “Protocol and measured work” is coherent as an additional warmed caller-latency experiment, but should not replace this measurement.

A's “Correctness proof before performance,” step 3, is also the most actionable payload audit. Known sorted terms plus cardinality checks allow public port lookups without exposing a core enumeration API. Audit every decoded posting and position, validate ordering/frequencies/counters, and reproduce the full canonical digest over all 1M documents. Retained Tantivy/Java receipts establish expected values; they do not certify the freshly built port index or current adapters. A fixture can validate the implementation first, but cannot stand in for the full audit.

A's “The collector mismatch that cannot be hidden” correctly allows the initial **available-API lane**: Tantivy top10 with no totals versus port top10 additionally tracking 1000 hits. This is an explicitly qualified useful comparison, with separate exact COUNT timing. It must not be described as identical collector work. Threshold0 can be a separately named, separately gated feature experiment later; it need not block this primary lane. Port lower-bound totals must be checked against an independently exact count using their relation, not equated to the exact count.

## Two grafts from B

1. **Persistent workers and alternating requests, with untimed transport.** Keep both readers and query ASTs resident. Have the controller request one query/operation at a time in frozen AB/BA or ABBA/BAAB blocks, and return the worker's native elapsed nanoseconds. Pin both workers to the same chosen logical CPU and keep the controller off that core. This obtains B's order balancing and same-binary controls without including pipe latency, parsing or serialization in the primary samples. Record both starting orders and run TT and PP controls under the same schedule.
2. **Explicit finite limits and incomplete receipts.** Adopt B's bounded requests/phases/total run concept, freeze the exact sample budget before execution, and fail without timed retries or auto-growing loops. Set separate realistic bounds for the full build/payload proof and timing; a timing budget must not quietly shrink the complete correctness audit. Keep raw samples, all failures and before/after identity receipts in an exclusive output directory.

## Reject or defer

- Remove B's “first try opening the existing Lucene index” branch. The pinned port's format cannot open the Java index. Build a fresh port index from the aligned replay in the same physical order and prove its full payload. Also remove the later-authorization detour: index preparation is necessary to the already requested comparison and must use a new isolated output path.
- Do not make threshold0 a prerequisite for the available-API lane, or use the parent clock as the primary engine metric. The closer threshold lane and an end-to-end service lane are optional distinct experiments with their own labels and gates.

For the narrow upstream work, retain the smallest strict legacy comparator correction described in both designs: explicit row/query/hit validation, recovered raw f32-bit equality, score-only diagnostics, and a nonzero exit on mismatch, including execution under `python -O`. Add the independent IDF arithmetic witness/fix separately. A relation-aware wire-format extension belongs in its own change only if that specific relation contract is being asserted. None of these fixes substitutes for the complete current-binary corpus and result gates.

The resulting design is A's replay/payload proof and native timing scope, B's persistent request schedule and concrete limits, a clearly labeled threshold1000 public-API lane, and independently reviewable upstream fixes. Strict correctness remains a prerequisite for every reported timing category.
