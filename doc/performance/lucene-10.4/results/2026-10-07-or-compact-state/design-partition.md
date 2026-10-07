# Candidate B: threshold-triggered repartition with a single-driver loop

## Caller usage first

The BooleanWeight caller remains exactly the current private route:

```rust
or_maxscore(term_scorers, reader.max_doc(), threshold, callback);
```

Eligibility, exhaustive fallback, the 3..=32-term admission, original scorer storage, physical document range, and existing local/global bound construction remain unchanged. The caller acquires no new options or lifecycle obligations. The improvement belongs entirely to `or_maxscore.rs`.

Inside each certified half-open region, execution changes from one immutable essential slice to an execution plan that shrinks only when the collector threshold crosses the next conservative prefix bound. A plan with exactly one essential scorer uses that scorer directly as the document driver. It does not find a minimum, scan essential matches, or scan essential advances for each candidate.

This adopts two concrete mechanisms from the port's `MaxScoreBulkScorer`: its threshold-triggered repartition and its single-essential execution path. It deliberately keeps Tantivy's scalar, original-ordinal score replay rather than copying the port's window accumulation order.

## Existing constraint and causal hypothesis

Current code computes `first_essential` once after constructing a region's prefix bounds. A callback can raise `threshold` many times within that region, but dense scorers remain essential until `hi`; they continue producing candidates that the higher threshold could already rule out. Every candidate also runs separate essential minimum, score-match, and advance-match loops, even when the essential slice has length one.

The retained baseline attributes 89.12% self time to the inlined execution routine. It does not establish that these particular loops dominate. The annotated bound-cleanup stores do not prove a candidate-contribution bottleneck. This proposal directly removes demonstrably redundant candidate enumeration after threshold growth and removes essential iterator scaffolding in the single-driver state; the size of either benefit is unmeasured.

The previous global-certificate refinement can produce wider regions with sparse essential drivers. That makes within-region threshold promotion plausible, but many top-10 thresholds settle early, and small block regions may offer few promotions. No speedup percentage is promised.

## Private types and signatures

No new public type, module, dependency, or scorer method is needed. A small private plan is sufficient:

```rust
// Sketch, not an implementation.
struct RegionPartition {
    first_essential: usize,
    next_threshold: Score,
}

fn partition_at_threshold(
    prefix: &[f64],
    upper_bound: &ScoreSumUpperBound,
    threshold: Score,
    previous_first: usize,
) -> RegionPartition {
    // not implemented
}
```

`prefix` is the already-existing immutable region prefix array. `previous_first` is zero at region entry and the previous partition's first rank after a threshold promotion. Because the threshold is monotone, the helper only moves this rank forward. It returns `first_essential == num_terms` when the entire remaining region is uncompetitive. Otherwise:

```text
next_threshold = upper_bound.score(prefix[first_essential + 1])
```

This is the exact gate for admitting the next essential term into the optional prefix. No score-bound subtraction is introduced. The helper needs no scorers, document positions, mutable bounds, or callback access, so partition arithmetic remains separate from cursor mutation.

There is no separately stored driver enum that could disagree with the partition. Dispatch derives the execution state from `num_terms - first_essential`:

- zero: skip the rest of this certified region;
- one: direct scalar driver `order[num_terms - 1]`;
- two or more: existing essential merge loop.

The production function can keep these states in a labeled region-execution loop. Do not extract many pass-through helpers or put the whole query behind a new object merely to express this dispatch. The difficult score/cursor invariants remain together in the existing private module.

## Module map

```text
boolean_weight.rs
  existing 3..32-term route, unchanged
    -> or_maxscore.rs
         eligibility and canonical fallback, unchanged
         query scratch and static ordinal preference, unchanged
         region bound certificate, unchanged
         partition_at_threshold (new pure helper)
         region execution
           merge driver (existing mechanics)
           direct single driver (new specialized mechanics)
         original-ordinal f64 score replay, unchanged
```

No writer, codec, index-reader API, index format, query AST, collector, or cache changes are required.

## Execution pseudocode

```text
validate eligible domains before touching cursors, else exhaustive fallback
allocate existing order/local_max/prefix/contributions arrays once
build each current region [lo, hi) using the unchanged cursor/bound rules
construct immutable prefix over original n terms in pruning preference order
partition = partition_at_threshold(prefix, threshold, previous_first = 0)
reconcile every initially essential scorer at lo with actual seek

region_execution:
    if partition.first == n:
        leave region

    if n - partition.first == 1:
        driver = order[n - 1]
        while driver.doc < hi:
            doc = driver.doc
            clear the existing contribution array
            leaf = driver.score
            contributions[driver] = leaf
            known = f64(leaf)
            probe optional ranks in reverse with existing inclusive upper bounds
            if candidate remains competitive:
                replay all contribution slots in ORIGINAL ordinal order
                if rounded score > threshold:
                    threshold = callback(doc, rounded score)
                    threshold_changed = true
            driver.advance()
            if threshold_changed and threshold >= partition.next_threshold:
                partition = partition_at_threshold(prefix, threshold, partition.first)
                continue region_execution
        leave region

    else:
        while minimum actual doc over current essential suffix < hi:
            doc = that minimum
            perform existing clear, matching-essential score visits,
              optional probes, exact original-ordinal replay and callback
            advance ALL matching scorers in the OLD essential suffix
            if callback occurred and threshold >= partition.next_threshold:
                partition = partition_at_threshold(prefix, threshold, partition.first)
                continue region_execution
        leave region

lo = hi  // discard all local certificates; build the next region anew
```

The multiple-essential path should enter the dispatch again only on a promotion, not branch on the number of essentials for every candidate. The single path similarly runs directly until its driver leaves the region or the partition becomes fully pruned. In a region already in the single state, its only possible promotion is to zero essentials.

The merge and direct loops share the same optional-probing and score-replay semantics. If the compiler-friendly implementation duplicates a small candidate kernel, exact tests must cover both paths; do not replace the exact replay with reordered addition to avoid duplication. A local closure is acceptable only if it compiles cleanly without distributing mutable cursor ownership or introducing a large callback argument surface.

The threshold gate is tested only after a successful callback. A rejected candidate cannot alter the threshold. `threshold_changed` can be a control-flow fact rather than a stored per-document boolean. The old essential set advances before changing the partition; this keeps processed-document cursor state easy to audit.

## Correctness proof obligations

### Prefix admission and candidate completeness

For every document inside `[lo, hi)`, each clause's contribution is nonnegative and bounded by its immutable `local_max`. The existing `ScoreSumUpperBound::new(original_num_terms)` supplies the conservative allowance for differently ordered sums. Thus the rounded exact original-order score of a document matching only `order[..k]` is at most `upper_bound.score(prefix[k])`.

When that bound is `<= threshold`, such a document cannot satisfy the actual strict `score > threshold` acceptance rule. Promoting that prefix to optional therefore removes only impossible winners from candidate enumeration. The surviving candidate set is the union of the remaining essential suffix. Repartition reuses the same proof with a larger threshold and the same region certificate.

The threshold crossing must use `>= next_threshold`, because optional admission is `bound <= threshold`. Copying the port's comparison convention would be wrong here. It must use the outward `ScoreSumUpperBound` result, not a bare cast of `prefix[k]` or a `next_up` added to the collector threshold. Exact raw-f32 equality around a midpoint remains observable.

### No rewind or missing future winner

All callbacks are in increasing physical DocId order. After processing a candidate, all matching scorers in the old essential suffix advance before any partition transition. Newly demoted optional scorers may already point beyond that document, which is a valid forward position for future optional probes. Essential membership can only shrink within a region; no newly essential stale decoder needs reconciliation during a promotion.

At the next region, all current proofs are discarded and membership can grow again. The existing actual `seek(lo)` for each newly essential scorer must remain. Globally optional clauses still obey the original density decision and global certificate; this proposal does not infer wider region ends after a promotion.

The single driver has already been reconciled at `lo` by the region-entry logic or was a member of the old reconciled essential suffix. It is scored only at its actual loaded `doc`, then advanced with the actual scorer method. No shallow position is scored or advanced. Tail reconciliation, physical `max_doc`, deleted-document callback behavior, and duplicate clause ownership remain the current implementation's rules.

### Exact published score

The contribution array is still cleared for every candidate. Each matching leaf is written to its incoming ordinal. The final score still folds all slots from ordinal zero through ordinal n-1 in f64, then converts once to f32. The direct driver only replaces how the essential leaf is found; it does not change the fold, the optional probe bounds, or leaf scoring. Duplicate terms remain separate leaves and scorers.

`known` may remain in pruning preference order only under the existing upper-bound proof. Do not publish it directly. Do not reduce `ScoreSumUpperBound`'s leaf count after terms become optional or essential count shrinks.

### Exceptional thresholds

The initial NaN and unsafe weight fallback remains before cursor mutation. `+infinity` prunes every region when the prefix bound is finite or infinite, consistent with strict acceptance. An infinite next gate is crossed only by an infinite threshold. Initial negative infinity still makes all finite-bounded terms essential. The contract continues to require monotone callback thresholds, including debug assertions already present.

## Compactness and runtime overhead

All four existing arrays remain unchanged: `24n + 8` bytes of array payload on x86_64, at most 776 bytes at n=32, plus existing Vec headers and allocator overhead. The plan is two scalar fields, expected 16 bytes with usize/f32 padding on x86_64; a compiler can keep it in registers. No new per-query heap allocation, per-region allocation, pool, window bitmap, term-by-document matrix, or index bits are added.

At region entry, the partition helper performs the same prefix scan the current code already performs. During a region it advances the rank monotonically, so there are at most n successful term promotions. A crossing costs the work needed to advance those ranks, plus dispatch; it does not rebuild all region bounds or re-sort clauses. There is one extra bound-gate comparison per accepted callback, usually far fewer than the number of candidates. When a region's threshold never crosses its next prefix bound, repartition provides no enumeration benefit and adds that comparison.

Single-driver execution removes three one-element essential iterator loops per candidate: finding a minimum, testing matching essential clauses, and testing which essential clause to advance. It replaces them with one cached ordinal, direct score, and direct advance. This is likely a modest constant-factor gain by itself. Optional probes, contribution clearing, original-order replay, and actual decoding remain. Code duplication can increase instruction-cache pressure; disassembly and `.text` size must be recorded.

## Rejection cases and measurement plan

Reject this candidate if full-query AB/BA controls show a reproducible regression, especially queries whose threshold settles early or whose region ends already occur every 128 postings. Also reject if the two specialized paths make cursor/score semantics materially harder to audit or the emitted code growth outweighs measured savings.

Correctness additions should deliberately force:

1. A threshold crossing one and several prefix gates inside one wide certified region, with many remaining documents in a newly demoted dense essential clause. Compare full callback traces, not just final Top-K.
2. Multi-essential -> single-essential -> fully pruned transitions, including raw bound equality and immediate neighboring f32 thresholds.
3. The existing exact four-leaf f64 midpoint case executed through both merge and single-driver paths, original ordinal permutations, duplicates, and disparate boosts.
4. Demoted clauses that retain future positions, then become essential in the next region, across 127/128/129 and 255/256/257 posting boundaries and stale decoded tails.
5. A direct driver with gaps, exhausted optional clauses, deleted physical documents, and all configured BM25/unsafe fallback cases.

Run the existing default and configured 1,227-query exact gates unchanged. Pilot timings select an attempt; acceptance requires the full 1,221-query AB/BA native schedule with same-engine controls, stable source/binary/index hashes, post-profile, and memory/code/index accounting. Unchanged two-term OR and COUNT routes remain negative controls.

For mechanism evidence, use a separate diagnostic build or deterministic unit fixtures to count enumerated candidates, promotions, and single-driver candidates. Do not add these counters to the accepted production timing binary. No measurement should overlap our own build/profile jobs, and user background processes remain running.

## Rationale and synthesis input

This is structurally distinct from changing scratch representation or replacing ordinal score replay: it changes the temporal candidate-driving plan while preserving the full exact contribution vector. Its primary opportunity is eliminating later candidates after a threshold promotion, rather than shaving stores from every existing candidate.

It has a narrow interface, no new shared state, and clear transitions: certified region -> monotone partition -> merge/single/pruned -> next region. Bounds never outlive their certified region. The design borrows the port's execution mechanism with materially smaller scratch and preserves Tantivy's stricter arithmetic contract.

Suggested rubric self-assessment, before measurement: arithmetic/cursor proof 5; causal opportunity 3 (remaining benefit is workload-dependent and unmeasured); compactness 5; interface/readability 4 (avoid excessive kernel duplication); verifiability 5. Total 22/25. This is a hypothesis ranking, not an acceptance verdict.
