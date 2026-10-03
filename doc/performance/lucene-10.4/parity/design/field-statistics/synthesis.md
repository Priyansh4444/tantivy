# Exact field statistics: synthesis decision

Selected candidate A (persisted exact snapshots) as the base, scored 22/25 by the independent judge versus derivation-first candidate B's 20/25. Parent read both complete designs and agrees. A gives constant-time reopened new-index statistics and useful sparse-field bounds; B requires cold posting scans and legacy-normalized pruning even for new indexes.

Grafts from B: a paired provider snapshot, one universal physical/retained-posting reducer, shared successful immutable cache on the canonical reader Arc, and stored-bound provenance independent of corrected query statistics. No second field cache or scorer boolean protocol. Keep the permissive default for existing custom providers and their prior rounding.

Version 10 marks newly selected native bounds and makes old readers reject the new index. Index-1 four-byte metadata guarantees an exact existing token header and field-population-selected bounds. Missing metadata means legacy, including a new file written with preserved public legacy serializers. A new reader derives legacy query statistics but checks bounds using the original serialized header/maxDoc average. Native writer and reader share one average calculation; preserve the legacy f32 cast/division separately.

Rejected: population-only changes that trust old approximate headers, unchanged version with new bound selection, norm sums or live ratios, scans on every query, and exact per-document length arrays before merge cost is measured. The last would consume about 4 MB per million documents and risk the narrow storage win. Retained merge preflight uses the actual mapping; summing source snapshots is permitted only for complete retention.

Verification before implementation: sparse BM25 ranking, repeated sloppy occurrences and large carried slop were reproduced independently against the frozen original Rust binary and unmodified Lucene. Existing composite side entries and footer version rejection were confirmed from source. Runtime acceptance of the statistics unit still requires metadata/reduction/cross-engine agreement, old/new/mixed exhaustive-pruning checks, malformed metadata and canonical concurrent-cache checks, and cold/warm/merge/storage measurements. The architecture review itself is not a passing production test.

The full parity target also includes native default score conventions, similarities/configuration, norm/overlap behavior, all documented modules, Java public extension contracts and file interoperability. This statistics unit does not claim those are complete.
