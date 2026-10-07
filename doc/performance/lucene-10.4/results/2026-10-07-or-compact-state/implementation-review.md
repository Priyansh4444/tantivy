# Compact clause metadata implementation review

Implemented only `src/query/boolean_query/or_maxscore.rs` in the isolated worktree.
No other production source file, public interface, caller route, query feature,
index format, codec, or scorer representation changed. No build, test, profile,
benchmark or commit ran in this delegated implementation task. Root owns runtime
verification and acceptance.

## Chosen change

Replace the pruning-order ordinal Vec and separate ordinal-indexed local-max Vec
with one sorted private `Vec<ClauseState>`. Each record caches its incoming
ordinal, immutable global maximum, immutable postings doc frequency, exact decoded
current DocId, and current-region local maximum. The original scorer and leaf
contribution arrays retain incoming order. Sorting uses the same f64 maximum /
max(cost,1) priority and incoming ordinal tie break.

The from-zero global-prefix scan still runs at every region. Its additions remain
left-to-right in preference order with the same `ScoreSumUpperBound(original n)`
comparison. The density policy still explicitly tests every certified prefix cost
against twice the maximum cost among *live decoded* remainder records, using u64.
There is no persistent GlobalPrefix, sparse contribution bitmap, changed admission,
repartition, direct-driver kernel, subtractive bound, per-region allocation, or
new temporal policy. Contribution clearing and exact incoming-order f64 fold are
unchanged.

Region preparation still selects shallow blocks in preference order and preserves
the previously selected `last_doc` even when a tail requires actual reconciliation.
Every local bound is initialized to zero before its branch assigns a certificate.
Out-of-range cleanup and local prefix construction now share one sequential record
pass, which does not change the order of any bound addition.

## Mutation-site proof

There are exactly two direct actual cursor mutations in production:

* `ClauseState::seek`: `self.doc = scorers[self.ordinal].seek(target)`.
* `ClauseState::advance`: `self.doc = scorers[self.ordinal].advance()`.

The seek method is used at all three original call sites: selected-tail floor
reconciliation, essential floor reconciliation, and optional candidate probing.
The advance method is used for every matching essential clause after a candidate.
No mutable scorer reference escapes either method and no actual cursor mutation
occurs elsewhere in the new route. Initial records read each actual `doc()` once.

Shallow `seek_block` and `block_max_score` calls are retained at their original
semantic sites. Debug checks compare mirrored and actual decoded docs before and
after shallow selection, after numeric bound evaluation, before and after every
actual seek/advance, before and after score evaluation, and at region start/end
(including the no-essential skip). Shallow operations intentionally do not update
the mirror; a stale decoded document remains stale in both states. These checks
will execute across all existing exact-trace fixtures under root's debug tests,
and compile out of release measurement. They categorically catch a future API
change or missed mutation update rather than relying on manually refreshing a
cached doc in scattered loops.

A loaded doc at/after `hi` still zeroes its local certificate. A stale doc below
`lo` does not. Actual essential/optional seek still reconciles it before scoring
or advancement. The cap continues using physical `max_doc`, and the prior
selected-last-doc boundary rule is unchanged.

## Independent fixture and diagnostic

Added `exhausted_high_cost_remainder_changes_live_density_choice`. It uses three
independent posting lists: 300 weak spaced postings, 200 stronger early postings,
and two rare postings (0 and3500). Under the fixed .04 threshold only the first
global maximum is certified. Initially the 300-cost optional prefix fails the
2*200 live-remainder policy. Actual tail reconciliation later exhausts the
200-cost term, and the same prefix then passes against the remaining 2-cost term.

The fixture compares the entire callback DocId/raw-score-bit sequence with the
independent canonical BufferedUnionScorer/f64 oracle via the existing `check`
helper, checks literal accepted docs [0,3500], and verifies that observed raw
prefix count remains one while selected widening changes from zero at cost200
to one at cost2 in later regions. It repeats the oracle comparison with Top1.

Minimal `#[cfg(test)]` instrumentation records `(lo, raw prefix, selected prefix,
remaining cost)` only when this test enables an optional thread-local collector.
The collector is owned/taken by the fixture and isolated from parallel tests by
thread-local storage. No engine signature, wrapper, runtime argument or public
API was added. The observer adds test-only scaffolding at one region point; it
and its branch/counters are entirely absent from release. Debug coherence remains
the general cursor contract guard across all existing tests.

The meaningful oracle fixture also prints actual host
`size_of::<ClauseState>()` and the maximum32-term array payload. This is an output
receipt, not an assertion of a chosen ABI or a vacuous implementation-mirror test.
Root should run this fixture with `--nocapture` to retain actual layout evidence.
Expected x86_64 layout is24 bytes and payload1160 bytes (36n+8), up384 from the
prior maximum776, while reducing four scratch allocations to three. Three Vec
headers are72 bytes on x86_64 (prior96); consumed scorer Vec excluded equally.
Actual size/RSS/code-size evidence remains pending and no memory win is claimed.

## Static verification and remaining gates

Ran stable Rust1.99 rustfmt on the edited file and `git diff --check`; both return0.
Rustfmt emitted only existing repository nightly-setting notices. Inspected full
diff and enumerated every seek/advance site with rg. Worktree status shows only
this intended module modified.

Runtime compilation, all ten exact traces, configured/default1227 gates,
integration suite, final balanced timing/control schedule, profile, index hashes,
layout print, code size and RSS are pending root-owned stages. Performance remains
a hypothesis. No implementation friction required broadening the chosen scope.
