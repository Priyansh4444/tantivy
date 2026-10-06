# Independent numerical results review

Read the complete analysis code, parsed every summary field and every per-query CSV row, and independently recomputed from the raw sample file without invoking the project's analyzer. Read the full schedule and timing receipt. No benchmark, build, test or engine-source change was performed.

throughput checkpoint: n/a, read-only investigation

**No numerical errors found.** There are exactly 87,912 unique complete schedule cells and 2,813,184 positive integer nanosecond samples: 3 pairs × 3 rounds × 2 orders × 1,221 queries × 2 operations × 2 sides × 32 samples. There are no missing, duplicate or extra cells, wrong engine labels or incorrect sample lengths. Every engine/query/operation/order median pools exactly 96 samples across the three rounds. All 2,442 CSV rows' medians, cross-engine ratios and TT/PP ratios match independently recomputed values. Every summary group's aggregate geometric ratio, arithmetic mean of query medians and ratio of those means agrees to floating-point precision. The raw sample SHA-256 matches the measurement receipt.

Ratios below are **Tantivy / fixed port**; a ratio below 1 favors Tantivy. Arithmetic means are means of per-query medians, not pooled sample means. Each order is summarized independently.

| Set | Operation | Order | Queries | Geometric T/P ratio | Mean T median, µs | Mean P median, µs | Ratio of mean medians |
|---|---|---|---:|---:|---:|---:|---:|
| Wiki20 | COUNT | AB | 20 | 0.02315782349 | 538.442700 | 3846.602275 | 0.1399787817 |
| Wiki20 | COUNT | BA | 20 | 0.02204680838 | 534.996725 | 3841.149800 | 0.1392803595 |
| Wiki20 | TOP10 | AB | 20 | 0.9057825390 | 255.839175 | 221.341875 | 1.1558552804 |
| Wiki20 | TOP10 | BA | 20 | 0.9105734656 | 255.595575 | 221.194950 | 1.1555217468 |
| Port-style | COUNT | AB | 1201 | 0.1781312410 | 167.657217 | 1059.005053 | 0.1583157855 |
| Port-style | COUNT | BA | 1201 | 0.1783169593 | 168.596463 | 1059.806013 | 0.1590823800 |
| Port-style | TOP10 | AB | 1201 | 1.0472066331 | 280.071478 | 164.917555 | 1.6982514592 |
| Port-style | TOP10 | BA | 1201 | 1.0465367276 | 279.861568 | 164.395656 | 1.7023659534 |

The proposed port-style claims are supported with precise wording: fixed port TOP10 is about **1.70× faster by the ratio of arithmetic means of query medians**, and about **4.7% faster by geometric speed ratio**. The latter corresponds to approximately 4.5% lower geometric latency, so “4.7% lower latency” would be a conversion error. Tantivy COUNT is about **5.61× faster geometrically**; the ratio of arithmetic means instead gives approximately 6.3×. These metrics should retain their names because they lead to materially different TOP10 impressions.

## OR loss and memory spot checks

The port-style OR TOP10 category has 301 queries. Its geometric T/P ratio is 2.265638 AB / 2.258970 BA; its ratio of mean medians is 4.214385 / 4.202198. Large individual losses are present in both orders and exceed their observed same-engine control spread. For example, `university of washington` has T/P medians of 2879.958/238.066 µs in AB and 2878.8765/256.4075 µs in BA, giving 12.0973× / 11.2277×. `the book of life` gives 11.5107× / 11.4299×. These are available-API results; the port additionally tracks 1000 hits.

Main-pair warmed `VmRSS` is 258,020 KiB / 1024 = **251.972656 MiB** for Tantivy and 330,384 KiB / 1024 = **322.640625 MiB** for the port. Thus “about 252 versus 323 MiB warmed process RSS” is accurate. These are snapshots after warmup, not whole-lifecycle peak memory measurements. The TT snapshots are 234.793/234.813 MiB and PP snapshots are 317.426/317.391 MiB, so retain the main-pair qualification rather than treating 252/323 as universal constants.

## Artifact guards and background qualification

The complete verify before/after objects are equal, verify-after equals measure-before, and the complete measure before/after objects are equal. Thus source, binary, query, controller and index identities recorded in those receipts remain equal across the gates and measurement. The port index matches its audited index-after map; audit index-before/after are equal. The Tantivy index map also exactly matches the retained frozen ruler's non-lock file hashes. Both source statuses are clean, with Tantivy `c0efbc99c2bcb8e6e7c0d6679a8dc9b7071744ac` and fixed port `e5d1f81bff42c87886b12770f3c79648bb1963ed`. Verification records all 1,227 correctness queries, zero failures and pass. Timing completed in 1340.098 seconds with a passing receipt.

The initial telemetry snapshot contains several `cc1` processes reporting substantial CPU use, and recorded one-minute load spans **1.634277–11.352051**. The idle admission receipt establishes that the enumerated known own preparation/test/audit tool sessions finished. It does **not** establish a globally idle host or identify ownership of the observed compiler processes. Do not attribute those unknown jobs to this benchmark. Report background activity as a limitation; AB/BA and TT/PP controls measure some observed variation but do not prove that the host was uncontended or eliminate every possible timing bias.
