# Native BM25 profile baseline — October 5, 2026

The strict comparison now passes for DEFAULT 1.2/.75,0.9/0.4 and2.5/1 on
an exactly aligned physical corpus. DEFAULT ranked search and every profile's
COUNT win all 20 queries in both process orders. Both nondefault ranked suites
lose overall:33–41% slower geometric mean. This is a baseline for further
optimization, not a completed Lucene parity or universal Rust performance claim.

Measured source 0eb1af15df2fac22b5af46b040382c2e14f5e125; production core remains
872c9f55dcc91034dcf8c15fe060dd0b1cfd3ccd. Query/validator binaries are unchanged by
the ordered-index helper unit. Lucene 10.4.0 tag peeled commit
9983b7ce7fdd04f4d357688fb85c14277c15ea8d. The frozen ruler and all actual binary,
class, source, input and output identities are retained in compressed raw receipts.

## Equivalent physical corpus and strict gates

The frozen original Lucene index is preserved. Exact original JSONL bytes are
replayed in its observed physical docID order; 40 ordered Tantivy batches are
committed and explicitly merged in ingestion order, then discarded batch files
are garbage collected. No index sort, ID remap, score/tolerance or query changes.

All 1M actual docID/stored ID/u64sort/u8norm tuples match (digest
6f7082e3ff479c7be7fe6f819857716630fc4b6b64acc2d6005f1107127029c7).
All 1,642,896 terms,126,703,012 postings/TF,294,827,020 positions match (canonical
payload digest893a75958e6d15c997d414d5fb5fc47c633aa18c68fcd33bc0f8e13abfa8dbb8).
N 917,578; TTF 294,827,020. Logical DF/TTF and ID-sorted documents also match.

The unchanged Wiki20 strict COUNT/top-ten membership/order/exhaustive/raw-score
gates pass for all profiles. All 1,913 common top100 scores per profile have
identical f32 bits; see exact-score-proof.json. The original 2.5/1 gate failed
eight exact cutoff ties on the old physical layout; every replaced score had
equal bits. That original failure is retained, and no gate was weakened.
All 48 older retained artifacts still have their original hashes.

Helper acceptance: 37Python tests; actual debug and exact integrated release tiny
fixtures cover unsigned sort extremes, mixedCRLF byte preservation, tied profiles,
full TF/positions identity and mutation/reversed-merge/early-flush rejection.
Eight malformed replay receipts reject before index/success receipt creation.
The inherited production suite/frozen330 acceptance remains in the
[October4 configuration report](../2026-10-04-configuration/README.md).
No production source changed in this new measurement unit.

## Serial loaded-machine measurements

Each suite uses CPU4, 40s warmup, 256 samples per query, seed23, 20 queries; both
process orders are serial. All own builds/tests/indexing/hashing were idle during
timing. User background workloads remained active as requested. Ratios are
Tantivy/Lucene; lower is faster. Order columns mean Tantivy-first/Lucene-first.

| Profile | COUNT ratio | COUNT wins/20 | TOP_10 ratio | TOP_10 wins/20 |
|---|---:|---:|---:|---:|
| default | 0.741415 / 0.736288 | 20 / 20 | 0.421207 / 0.476051 | 20 / 20 |
| k09-b04 | 0.742725 / 0.726081 | 20 / 20 | 1.327917 / 1.412464 | 10 / 10 |
| k25-b1 | 0.737380 / 0.733554 | 20 / 20 | 1.341649 / 1.388760 | 7 / 7 |

DEFAULT TOP_10 is 2.37×/2.10× faster; COUNT is approximately1.35×/1.36× faster.
Nondefault frequent single terms and dense conjunctions reproduce the largest
losses. Per-query medians, all samples, commands, installed-profile getter
receipts, host/process snapshots and JVM/vector configuration remain in raw files.

Aligned total bytes: Tantivy 626,102,393; Lucene 626,810,511, only 708,118bytes
(0.113%)smaller. The earlier differently ordered index's1.17% advantage is a
separate result. No isolated incremental production delta is claimed.

Warmed query RSS MiB, Tantivy/Lucene: DEFAULT 14.738/371.219;
0.9/0.4 14.691/369.625;2.5/1 14.590/1057.371. Measurements include warmed
COUNT/TOP suites and mappings; default JVM heap/GC makes these snapshots sensitive
to allocation/collection, not universal peak or constrained-heap efficiency proof.

## Actual execution diagnosis

Separate untimed atomic instrumentation retains an isolated core patch and exact
binary/source identities. Counts repeat identically twice; all 20 optimized versus
exhaustive checks pass for each installed profile. Explicit DEFAULT receipts
produce identical counts to the implicit DEFAULT path. Instrumented latencies
are never used as performance evidence.

For 'of', DEFAULT scores15,491 documents and skips5,807 blocks. Both nondefault
profiles score all 758,787 documents, decode all 5,928 complete blocks and skip none.
One bound request is loaded and5,927 are unloaded. Dense '+the +of' takes block
intersection:11,145 DEFAULT window skips versus 0 nondefault. 'the of' transitions
from two-term MaxScore to that intersection. 'the american' stays two-term
MaxScore, and sparse '+the +saxophone' takes sparse/dense intersection; neither
consults block bounds. The evidence grounds pre-decode bound improvement and
limits what a loaded-block-only optimization can accomplish.

## Scope and reproduction

Original run scripts record the executed absolute workspace paths and immutable
output guards. They are historical commands, not portable rerun wrappers: use a
fresh output/index directory and matching 0eb1af15d checkout/artifact to reproduce,
following [configured-wiki.md](../../configured-wiki.md) and the retained command
receipts. Initial instrumentation dependency/DEFAULT acknowledgement failures
are retained separately; the subsequent corrected build/run is authoritative.

sha256.json maps every compressed raw file to compressed and original hashes.
Decompress JSON with Python gzip/json to inspect without rerunning artifacts.
The frozen synthetic 330 suite, all 47 feature families/public APIs, Unicode/token
graphs, arbitrary Similarity implementations, Java facade/Lucene file interchange,
other workloads, indexing/merge, cold startup and concurrency are not newly
certified here. The [acceptance ledger](../../parity/ACCEPTANCE.md) remains bounded.
