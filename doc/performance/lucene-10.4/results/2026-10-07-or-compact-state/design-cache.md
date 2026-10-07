# Candidate A: compact clause state, immutable maxima, monotonic global certificates

## Caller first

The BooleanWeight caller is unchanged:

```rust
3..=32 => or_maxscore(term_scorers, reader.max_doc(), threshold, callback),
```

The private function still consumes the incoming scorer vector and publishes only
strictly competitive `(DocId, Score)` callbacks in ascending document order.
Scorers remain physically stored in incoming order. No public interface, writer,
codec, index bytes, query parser, collector, or count path changes.

The proposed implementation replaces the separate pruning-order and local-max
vectors with one compact, sorted vector of clause records. It borrows mutable
scorers only for operations that actually select blocks, seek, score, or advance.
All ordinary position tests read the compact records. This adopts the port's
`Wrapper` separation of large cursor objects from hot position/cost metadata,
without its windows, pooled scratch, or order-dependent score accumulation.

## Grounded reason to try it

The retained profile has 89.12% self in the optimized inlined pruning route,
4.34% block bounds, and 2.23% block loading. Annotation identifies region-bound
cleanup followed by prefix construction as sampled work, but does not establish
that contribution clearing dominates. This candidate leaves per-candidate
contribution clearing unchanged.

The present route repeatedly reads `doc()` from 2,072-byte TermScorers in
candidate-minimum selection, matching essential clauses, advancement decisions,
optional seeks, region-bound cleanup, and the live cost guard. Compact records
reduce the working set for exactly those loops. Cached global maxima and costs
also remove repeated reads through each large scorer in region preparation and
priority comparison. The expected benefit is fewer scattered loads and less
region setup, rather than fewer document scores or postings decodes.

The benefit remains a hypothesis until the frozen balanced benchmark and
post-profile pass. In particular, a 3-clause query may already keep all relevant
scorer cache lines resident, so a copy/store can cost more than the removed load.

## Private types and module map

Everything stays in `src/query/boolean_query/or_maxscore.rs`.

```rust
struct ClauseState {
    ordinal: usize,       // incoming scorer index, never changes
    global_max: Score,   // fixed admitted BM25 global maximum
    cost: u32,           // fixed postings doc_freq, not remaining postings
    doc: DocId,          // exact loaded scorer.doc(), including stale shallow doc
    local_max: Score,    // current half-open region certificate
}

struct GlobalPrefix {
    certified: usize,    // raw global prefix before the live density policy
    sum: f64,            // same left-to-right additions as the old region loop
    min_cost: u32,        // minimum cost among certified clauses
}

fn or_maxscore(
    scorers: Vec<TermScorer>, max_doc: DocId, threshold: Score,
    callback: &mut dyn FnMut(DocId, Score) -> Score,
);

// Helpers own every actual cursor mutation and update its position mirror.
fn seek_clause(scorers: &mut [TermScorer], clause: &mut ClauseState,
               target: DocId) -> DocId;
fn advance_clause(scorers: &mut [TermScorer], clause: &mut ClauseState) -> DocId;

// Updates only numeric certificates plus explicit tail reconciliation.
fn select_region_bound(scorers: &mut [TermScorer], clause: &mut ClauseState,
                       lo: DocId, max_doc: DocId) -> Option<DocId>;
```

Helpers are an implementation sketch, not a requirement to split the tight loop
into many functions. Small private inlined helpers are useful to make cursor
mirroring categorical. They never expose an uncontrolled mutable scorer reference.
Do not add cached state to the public `TermScorer` type.

The vector of records is sorted once by
`global_max as f64 / cost.max(1) as f64`, then incoming ordinal. This is exactly
the existing preference order, computed from the same fixed values. Every
record's `ordinal` still addresses original-order scorer and contribution arrays.
The local prefix array is in record rank order. The final contribution array is
in incoming ordinal order.

## Source audit required for the position mirror

The current source supports this mirror without a speculative cursor property:

* `TermScorer::doc()` delegates to `SegmentPostings::doc()`, which reads the
  decoder output at the existing posting position.
* `seek_block()` changes the skip reader, bound cache, and `block_loaded`; it
  neither rewrites the decoded document array nor changes the posting position.
  A stale loaded position stays stale in both the scorer and the mirror.
* `block_max_score()` can update numeric bound state or construct a native
  envelope; its implementations do not call `load_block()` or alter the position.
  Its loaded-tail scan reads decoded arrays without modifying them.
* `score()`, `max_score()`, `last_doc_in_block()`, and `size_hint()` do not move
  the position. `size_hint()` is `SegmentPostings::len()`, the fixed doc frequency.
* The only actual position changes in this function are the explicit `seek()`
  and `advance()` calls. Both return the resulting `DocId`.

Implement each mutation as `clause.doc = scorers[clause.ordinal].seek(target)`
or the equivalent `advance()` assignment, through the two private helpers.
Use debug assertions comparing the cached doc with the actual scorer before
and after these helpers and after shallow/bound operations. Debug region-end
coherence across every record catches missed mutation sites. These assertions
are tests of the duplicated-state contract and compile out of release.

The function is the sole owner of both arrays for its duration. There is no
shared state, callback cursor access, mutable scorer escape, unsafe indexing,
or refresh policy inferred from `has_remaining_docs()`. A future cursor API
change that lets shallow selection rewrite decoded docs must fail those debug
checks and then change the mirror protocol; it must not silently publish a new
cursor invariant.

## Pseudocode

```text
Reject unsafe BM25/maximum/threshold domains before any cursor mutation,
using precisely the existing exhaustive SumCombiner fallback.

Build one record per scorer with fixed ordinal, global max, cost, actual doc.
Sort records by existing cached priority and ordinal tie break.
Allocate local prefix[n+1] and incoming contributions[n].
Initialize GlobalPrefix{certified:0, sum:0, min_cost:u32::MAX}.
lo = minimum cached doc.

while lo < max_doc:
    # Global threshold is monotonic; compute each accepted prefix addition once.
    while certified < n:
        next_sum = global.sum + f64(records[certified].global_max)
        if upper_bound.score(next_sum) > threshold: break
        global.sum = next_sum
        global.min_cost = min(global.min_cost, records[certified].cost)
        global.certified += 1
    if certified == n: return

    # Preserve exactly the present live density decision, including exhaustion.
    widened = certified
    if widened > 0:
        remaining_cost = max(cost of records[widened..] whose cached doc
                             is not TERMINATED), or 0
        if u64(global.min_cost) < 2 * u64(remaining_cost): widened = 0

    hi = max_doc
    for rank, clause in records:
        # Every branch assigns local_max; no separate zero fill required.
        clause.local_max = 0
        if rank < widened:
            clause.local_max = clause.global_max
            continue
        if clause.doc == TERMINATED: continue
        scorer.seek_block(max(lo, clause.doc))
        assert debug cached doc still equals actual scorer.doc()
        last_doc = scorer.last_doc_in_block()
        if last_doc == TERMINATED and clause.doc < lo:
            seek_clause(lo)  # helper stores returned doc
        if clause.doc == TERMINATED: continue
        hi = min(hi, min(last_doc, max_doc-1)+1)
        bound = scorer.block_max_score()
        clause.local_max = bound if finite and nonnegative else clause.global_max

    # Same cleanup and prefix order as before, now one compact sequential pass.
    prefix[0] = 0
    for rank, clause in records:
        if clause.doc >= hi: clause.local_max = 0
        prefix[rank+1] = prefix[rank] + f64(clause.local_max)
    first_essential = first rank whose inclusive prefix bound > threshold
    if none: lo=hi; continue

    seek every essential cached doc < lo through seek_clause(lo)
    loop:
        doc = minimum cached doc in records[first_essential..]
        if doc >= hi: break
        contributions.fill(0)
        known = 0f64
        for essential clause with cached doc == doc:
            leaf = scorers[ordinal].score()
            contributions[ordinal] = leaf
            known += f64(leaf)
        for optional rank in reverse:
            if upper_bound.score(known + prefix[rank+1]) <= threshold:
                mark noncompetitive; stop probing
            if clause.doc < doc: seek_clause(doc)
            if clause.doc == doc:
                leaf = scorers[ordinal].score()
                contributions[ordinal] = leaf
                known += f64(leaf)
        if competitive:
            exact_score = left-to-right f64 fold of incoming contributions,
                          converted to f32 once
            if exact_score > threshold:
                threshold = callback(doc, exact_score)
                assert debug monotonic threshold
        advance every essential cached doc == doc through advance_clause()
    debug check every cached doc equals its original scorer.doc()
    lo = hi
```

Do not retain the local essential partition past its region. Do not promote
local optional clauses mid-region in this candidate. A callback can raise the
threshold, but the global prefix is extended only at the next existing region
boundary, exactly as in current production. Keep strict comparisons and no
subtractively formed bound.

## Correctness argument

**Exact positions.** Initially the mirror equals each actual scorer's position.
Every operation that can change a position assigns its returned doc into the
corresponding record. Every other operation leaves decoded position unchanged.
Induction therefore makes all cached position tests identical to the current
actual position tests. This also preserves stale-doc behavior after shallow
selection: a stale doc below `lo` is not converted into a bound-zero proof.
Actual `seek(lo)` still reconciles that decoder before score/advance. Tails
still use the same physical `max_doc` cap and mandatory reconciliation.

**Unchanged cost policy.** Cached costs equal fixed doc frequencies. Cached
positions equal actual positions, so the live remainder set is the same.
`min(prefix costs) >= 2*max(live remainder costs)` is equivalent to the present
`all(prefix cost >= 2*remaining_cost)` predicate. Multiplication is widened to
u64 before computing the factor of two. No cost heuristic changes.

**Monotonic global certificate.** Global maxima, rank order, and upper-bound
factor are immutable. Prefix additions run in exactly the old left-to-right
f64 order. A threshold that never decreases cannot invalidate an accepted
prefix; it can only admit more clauses. Thus retaining `(certified, sum)`
returns exactly the raw prefix that the old from-zero scan would find at each
region. Distinguish this raw monotonic prefix from the selected widened prefix:
the density policy can still select zero. Never cache that selected policy
as irrevocable state. The final all-clause certificate returns only under the
same sum-bound comparison as the current implementation.

**Local arithmetic and pruning.** Region bounds and their summation order are
identical, only storage location changes. Clearing an out-of-range real doc
and prefix construction can fuse because prefix reads only the already-cleaned
current record and the prior prefix; no dependency on another uncleared bound
exists. Keep `ScoreSumUpperBound(original n)` on reordered pruning sums.
Candidate `known` retains existing bound semantics. Published score folds the
original ordinal f32 leaves through f64 in the identical order. Exact midpoint
fixtures therefore keep their current raw bits and strict tie behavior.

**Unsupported domains.** Eligibility and the exhaustive fallback stay before
all shallow or actual seeks. No signed/NaN/infinite score domain enters the
cache route. No inference about safe max-score arithmetic is newly made.

## Scratch and work accounting

On x86_64 with the sketched field layout, `ClauseState` is 24 bytes:
8-byte ordinal plus four 4-byte fields. Confirm with `size_of`, do not assume
an ABI guarantee. Its local bound occupies space that a doc-only wrapper would
otherwise pad. Array payload is:

* records: `24n`;
* local f64 prefix: `8(n+1)`;
* incoming f32 contributions: `4n`;
* total: **36n+8**, at most **1,160 bytes** for 32 terms.

There are three Vec headers (72 bytes) instead of the current four (96 bytes),
and a `GlobalPrefix` of approximately 24 bytes depending on field order/alignment.
The existing consumed scorer Vec is excluded equally in both accounts. The
new payload is 384 bytes larger at 32 terms than the current 776-byte payload,
while removing one allocation. There is no per-region allocation, pooled
resource lifetime, document window, bitmap, on-disk metadata, or public cache.

Ordinary candidate position loops now scan compact records instead of touching
large scorers. A real cursor mutation adds one mirrored 4-byte store. Cold
initialization adds one pass reading cached metadata. Global-prefix additions
fall from potentially `regions * n` to at most `n` accepted additions plus one
failed comparison per region. A previously failed next bound may optionally
be retained too, but that should be a measured graft, not required state.

The live remainder cost loop stays O(n) per region but scans compact records.
Do not add exhausted masks, precomputed suffix state, or cached doc minima in
this unit; they create more mutation paths and obscure the primary experiment.

## Verification and rejection

First run the exact existing 9 unit traces, raw-bit public gates, all 1,227
default/configured queries, and index/source/binary hashes. Add a direct fixture
where a high-cost remainder exhausts before another global prefix promotion,
to prove that the live cost choice remains equivalent. Existing sparse-tail,
shallow-deferred optional, midpoint, duplicate, reversal, 32-clause, negative,
NaN and infinity cases remain mandatory. Debug coherence assertions run through
all existing fixtures and catch a missing tail/optional/essential mutation.

A pilot should compare:
1. compact records with the existing from-zero global-prefix loop;
2. the same records plus monotonic global-prefix promotion.

This isolates the state-layout mechanism from the prefix arithmetic mechanism
if time permits. Only a complete final AB/BA schedule with same-engine controls
can accept a chosen variant. Excluded two-term OR and unrelated TERM/AND/PHRASE
are routing controls, not beneficiaries. Investigate any long-OR regression
beyond same-engine variability, especially comparable-density queries that
needed the current local-bound cost policy. Keep the current baseline worker,
compiler flags, physical index, payload, queries, background-load policy, and
no-own-heavywork timing discipline. Report post-profile/annotation changes,
code size, RSS, and exact scratch receipt.

Reject if the mirror needs mutable cursors to escape, the benefit appears only
in trimmed samples, a cost-policy difference is needed to explain a win, or
extra code/loads erase performance outside noise. A successful result is a
bounded reduction of measured long-OR execution time with unchanged exact
answers and index bytes. It does not imply complete Lucene feature parity,
all workloads becoming faster, or elimination of the remaining port advantage.

## Rubric self-assessment

1. Exact score/pruning proof: **5/5**; identical actual score fold and numeric
   certificates, with source-backed mirrored-position induction.
2. Causal reduction: **4/5**; directly removes scattered doc loads and repeated
   fixed metadata work, but existing optimized caches may limit benefit.
3. Compactness: **4/5**; +384-byte maximum array payload, one fewer allocation,
   no index bits or window scratch.
4. Interface/state clarity: **4/5**; private caller unchanged; position mirroring
   is explicit duplication and requires the centralized mutation protocol.
5. Verifiability: **5/5**; exact-trace fixtures and debug coherence checks cover
   the new contract; full frozen controls can falsify the performance claim.

Total **22/25**, with actual performance deliberately unclaimed.
