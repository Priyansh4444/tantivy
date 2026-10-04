# Lucene 10.4.0 compatibility target

The requested target is every documented Lucene module, public API and public extension point, including features outside Tantivy's existing scope. Full parity is not established by the existing Wikipedia query suite.

Completion requires a pinned inventory with no missing or unverified entries, executable differential tests for the observable contract, matching native default scores and result ordering, lifecycle and error behavior, and equivalent public extension behavior. A Rust API equivalent does not by itself establish Java source or binary compatibility. Java compatibility needs its own facade and tests. Lucene index-format interoperability is a separate obligation, including its supported backward codecs.

Performance is workload-specific. Record CPU time/latency, indexing and merge throughput, storage, cold startup and warmed memory for comparable inputs and contracts. Maintain the earlier COUNT/TOP_10 wins as regressions, but do not infer performance of unsupported features or assume the implementation language guarantees a win. Leave the user's background workload untouched and report contention.

The previous Wikipedia measurements used explicitly matched BM25 collection statistics and score scale. They demonstrate that comparison only. The native parity harness does not override collection statistics; it reports strict raw score equality separately from the diagnostic conversion of Lucene scores by the traditional BM25 numerator 2.2. A converted-score pass cannot establish complete native parity.

Priorities for implementation follow observed failures: sparse-field BM25 population and exact frequency totals through deletion merges; repeated-term sloppy phrase occurrence reuse, traversal/frequency semantics, and wide slop; then default scoring/configuration and the wider module/API backlog. Each production correction must have an original failing reproduction, a regression and a cross-engine check before it is integrated. Index changes must preserve bound provenance and reject readers that could misinterpret newly selected bounds.

Current executable results and remaining boundaries are recorded in [the acceptance ledger](ACCEPTANCE.md), separately from the immutable source inventory.
