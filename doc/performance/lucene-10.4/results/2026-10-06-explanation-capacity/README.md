# Sized BM25 explanation children — October 6, 2026

Known-size IDF child vectors now use their exact length. A crate-private helper
adopts the owned vector, retaining absent details for an empty vector. Single
materialization uses its two ordered leaves; native sums collect the ordered
record iterator. Public APIs, scalar values, rounding, descriptions, provider
calls/errors, explicit-tree cloning, scoring, bounds and index format are unchanged.

Accepted source is `1ac1c323a4e1ef872dfc6a917e962d54f18fb0ea`, identical across
all tracked source/Cargo/config files to tested `cefcd1edec930d3983fcea39d3b266cbf77c0fe9`.
The two-file change follows the separately accepted [deferred-explanation unit](../2026-10-06-explanation/README.md).

## Allocation result

The unchanged standalone probe retains 1,000 outputs per operation and repeats
each operation three times. Setup and retention-vector allocations are outside
the caller-thread counter. Per-output requested bytes and allocation calls:

| Owned explanation | P021 bytes | P022 bytes | Calls, unchanged |
| --- | ---: | ---: | ---: |
| Native/classic single |1600 |1440 |4 |
| Three-term native phrase |2560 |2000 |7 |
| Classic phrase/no explanation |1280 |1280 |3 |

Single explanations use 10% fewer requested bytes; native three-term explanations
use 21.875% fewer versus P021. Both return exactly to the original eager
implementation's per-explain byte totals. All first 18 construction/clone/boost
operations exactly match P021: weights remain 56 bytes, single construction has
one allocation, native phrase construction three, and tested clones/boosts zero.
These are requested-byte totals, not peak/live memory, RSS or latency measurements.
There is no new query-speed or fresh Lucene comparison claim.

## Verification and evidence

All 27 BM25 tests pass in debug and exact native opt3/LTO/overflow-off release,
including all 42 unchanged frozen whole-tree/scalar cases. The original allocation
probe is byte-identical; all 139 dependency identities/checksums/edges match the
retained lock, with only the probe-root name changed. Root independently verified
all 23 operation rows, full source and runner/log/compiler hashes, and before/after
guards. All five explanation rows exactly match the original eager allocation
baseline; all call counts remain unchanged.

The first `--locked` test attempt failed before compilation because a new worktree
does not inherit the ignored root test Cargo.lock. Its failed log/receipt remain.
Preparation now always copies and verifies the retained P021 test lock before a
locked test phase; no dependencies are resolved. The main ignored lock stays
unchanged. Explicit profile settings remove inherited profile overrides and
encoded Rust flags. Subsequent successful runs have separate IDs and artifacts.

Validation is targeted to this renderer-only allocation change. P021's broader
1,381 library tests, frozen330/all three Wiki20 gates and 16 paired/control runs
remain separate historical evidence at P021. They are not attributed to this
new source, and no broader query adapter/timing rerun is claimed. Full Lucene
public-API parity and an all-query speed win remain unfinished.

[manifest.json](manifest.json) hashes every member of [evidence.tar.gz](evidence.tar.gz):
actual baseline/candidate/integrated source and diff, unchanged tests/golden,
adopted lock preparation, all successful and failed test logs/provenance,
unchanged allocation probe/lock/raw outputs, independently reviewed runner and
root audits, and the keep/integration receipt. Build caches and binaries are
identified by provenance rather than embedded. Absolute paths are historical.
Decision `P022` records this narrow efficiency keep.
