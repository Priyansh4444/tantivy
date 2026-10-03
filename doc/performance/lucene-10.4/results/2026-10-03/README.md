# Corrected fork versus Lucene 10.4 — October 3, 2026

Production code: `e1a1ebcf6`; reproducible comparison tools: `27ea716cd`.
The correctness fixes preserve a clear aggregate query-speed advantage. This is
not a complete win on every surface: `+new +york` COUNT is sensitive to process
order, and the Tantivy index remains larger. No new speed optimization was accepted
in this pass.

## Results

Both processes were pinned to CPU 4. Each run used 40 seconds of warmup and 256
samples per query, randomized query order, alternating engines, and median pipe
request latency (including parsing). Separate runs reverse process creation order.
The workload is the frozen Wikipedia 1M corpus, one segment, 20 queries, cache off.

| Surface | Tantivy/Lucene, forward | Tantivy/Lucene, reverse | Interpretation |
| --- | ---: | ---: | --- |
| COUNT geometric mean | 0.752 | 0.762 | 1.31–1.33× aggregate speedup |
| TOP_10 geometric mean | 0.377 | 0.384 | 2.61–2.65× aggregate speedup |
| Index bytes | 641,173,024 / 626,810,511 | unchanged | Tantivy +14,362,513 bytes (2.29%) |

TOP_10 is faster on all 20 queries in both orders. COUNT is faster on 19 queries
in both orders. `+new +york` measures 241.4 vs 230.9 µs (1.045×) forward and
241.9 vs 244.2 µs (0.991×) reverse. These observations do not establish a stable
win or loss for that query. Low-microsecond single-term requests include material
protocol overhead.

Background processes were left untouched as requested. Load average was about
1.0 at the start of the full comparison and rose during the run; see
`lucene-provenance-oct03.json`. This differs from the previous Steam shader-heavy
run. Background load, scheduling, and frequency changes limit small-delta claims.

## Correctness and comparability

The fresh cross-engine gate passed all 20 queries: exact COUNT and top-ten
external-ID sets match; Tantivy optimized ranking matches exhaustive alive-doc
scoring. Rank-order differences occur only within score ties. The maximum relative
score difference among shared top-100 IDs is 2.51778687e-07 (0.00002518%).

BM25 uses k1=1.2, b=0.75, a shared all-document population of 1,000,000,
294,826,965 tokens, and equivalent 2.2 score scale. Lucene ordinarily excludes
empty fields from its BM25 population; this comparison overrides its collection
statistics to perform equivalent ranked work. The prior unmatched timings are
superseded. The tools and rationale are in [the parent README](../../README.md).

The same production revision previously passed 1,317 library tests (8 ignored),
the eight-configuration query gate (3,328 top-k, 1,664 COUNT, and 832 literal
match comparisons), and termination-boundary regressions. Those library tests
were not rerun for this documentation-only update. The comparator’s six fixture
checks were rerun and passed during this pass.

## Call-stack experiment

COUNT executes Weight::count → BooleanWeight.scorer → Intersection count →
SegmentPostings::fill_bitset_window → block loading. Its two leading term scorers
already use concrete dispatch. Lucene 10.4 likewise chooses 4096-document dense
conjunction windows at density 1/32 and consumes packed docs or encoded bitsets.

Adding `#[inline(always)]` to load_block removed the two direct block-load calls
from bitset-fill assembly. The function grew from 1,617 to 3,987 bytes; this is
a plausible instruction-footprint cost, not proof of the measured regression.
`+new +york` became 1.2% slower forward and 3.1% slower in reverse (ratios are
normalized to candidate/baseline; reverse JSON labels are swapped). Paired COUNT
results matched. The one-line mutation was reverted and never entered the fork.

Repeated BlockInfo classification is a possible future profiling target, but
restructuring cursor control flow without evidence that it dominates is not
justified by this near-tie. The next substantial category is packed-postings
consumption/representation, with correctness and index size measured together.

## Per-query median ratios

Values below 1 favor Tantivy. Raw samples, artifact hashes, and exact medians are
in the adjacent JSON files.

| Query | COUNT forward | COUNT reverse | TOP_10 forward | TOP_10 reverse |
| --- | ---: | ---: | ---: | ---: |
| `the` | 0.741 | 0.675 | 0.667 | 0.663 |
| `of` | 0.666 | 0.716 | 0.244 | 0.246 |
| `and` | 0.758 | 0.767 | 0.233 | 0.246 |
| `united` | 0.730 | 0.768 | 0.438 | 0.448 |
| `states` | 0.691 | 0.771 | 0.387 | 0.409 |
| `american` | 0.682 | 0.723 | 0.295 | 0.311 |
| `york` | 0.677 | 0.742 | 0.371 | 0.395 |
| `saxophone` | 0.703 | 0.719 | 0.746 | 0.749 |
| `+the +of` | 0.951 | 0.927 | 0.273 | 0.279 |
| `+united +states` | 0.796 | 0.804 | 0.129 | 0.118 |
| `+new +york` | 1.045 | 0.991 | 0.200 | 0.204 |
| `+the +american` | 0.902 | 0.869 | 0.340 | 0.380 |
| `+the +saxophone` | 0.519 | 0.586 | 0.621 | 0.649 |
| `the of` | 0.975 | 0.893 | 0.273 | 0.270 |
| `united states` | 0.792 | 0.767 | 0.127 | 0.125 |
| `the american` | 0.871 | 0.855 | 0.514 | 0.531 |
| `"united states"` | 0.681 | 0.672 | 0.744 | 0.704 |
| `"new york"` | 0.877 | 0.870 | 0.681 | 0.690 |
| `+griffith +observatory` | 0.765 | 0.814 | 0.846 | 0.863 |
| `griffith observatory` | 0.482 | 0.487 | 0.571 | 0.566 |
