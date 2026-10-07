# Compact OR clause metadata — 2026-10-07

This packet records an additional measured improvement on top of the fork's
streaming MaxScore OR implementation. It learns from lucene-rs's compact wrapper
metadata, without copying its buffered-window scorer or changing our index format.

**Affected OR Top10 group: 731 → 692 µs, 5.2–5.3% less time** by the mean of
per-query medians over 103 frozen three-or-more-term OR queries, in both AB/BA
orders. Geometric reduction is 2.4–2.6%. One query regresses beyond its observed
self-control variation; all rows are retained. lucene-rs still leads this group
by ~2.3× and the whole broad suite by ~1.3× on the fresh available-API comparison.
See [results-review.md](results-review.md) for exact values, controls and limits.

The implementation puts ordinal, current doc, fixed cost and score bounds in
24-byte ClauseState records. Repeated metadata checks avoid reaching through
~2 KiB posting cursor objects. Real cursor mutations own mirror updates; shallow
selection retains stale decoded docs until actual reconciliation. Original scorer
order and exact incoming-order f64 score replay remain unchanged. The local
bound-cleanup/prefix loop is fused without changing bound addition order.

1428 tests pass (7 ignored). Default and two BM25 configurations each pass 1227
exact count/rank/raw-score comparisons; every timed checksum is also verified.
Ten independent whole-callback oracle fixtures cover dynamic thresholds, tails,
optional promotion, exhausted live-cost decisions and numeric fallback. The binary
shrinks 2760 bytes. Scratch allocations fall 4→3, with array payload rising 776→1160
bytes at 32 terms; RSS is effectively unchanged. Disk files remain byte-identical.

The main lane compares the candidate with preserved baseline **Tantivy** from
fork 6edfb101/core 412bdc245; legacy `port`/P labels in `measure/` mean that baseline.
The separate `port-measure/` lane uses actual fixed lucene-rs e5d1f81. Main compares
identical native APIs and prebuilt ASTs. Port's available Top10 API additionally
tracks a 1000-hit lower bound; our collector requests no totals. This is warm
query execution, not startup, index opening, indexing, or full Lucene API parity.
Configured proof does not imply a fresh configured Java/port performance win.

All frozen queries and every sample are retained, with deterministic gzip for
large files. Main schedule: 1221 queries ×2 modes ×3 pairs ×3 rounds ×2 orders ×2
sides ×16 samples. Actual-port: 1221 ×2 pairs ×3 rounds ×2 orders ×2 sides ×16.
Workers CPU 4, controller CPU 6, 10 warmups. User/background processes were preserved;
main 1-minute load 2.65–5.10, port 3.02–4.32. Guards bind sources, binaries, indexes,
query corpus, compiler, lock and harness. No own heavy work overlaps latency.

Run the independent audit from this directory:

```sh
python3 recompute.py
```

It needs only Python's standard library and the retained packet. It validates the
complete manifest, archive and original-content digests, exact output and score
bits, test receipts, compiled source, index/binary guards, entire schedules, every
checksum, CSV row and published aggregate. It does not invoke either search engine.
`verify-tracked-packet.py` additionally checks Git's tracked inventory and audits
a clean `git archive` to catch ignored lockfiles or other missing publication data.

[architecture.md](architecture.md) contains the source-grounded Mermaid diagram;
[search-architecture.html](search-architecture.html) provides the two interactive
views with local-only controls. Highlighted execution mechanisms come from port
learnings; exact arithmetic, conservative bounds and compact Tantivy index layout
remain our contracts. Design alternatives and root synthesis are retained for
review; sketches are not claims of accepted runtime behavior.
