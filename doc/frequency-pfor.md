# Frequency blocks in index format 9

Full term-frequency blocks contain 128 positive `u32` frequencies. The writer encodes `frequency - 1`, then chooses the smallest eligible payload. Partial blocks retain their existing VInt encoding. Document blocks, position streams, scoring statistics, and block-max scoring metadata are unchanged.

The existing one-byte frequency header in each skip entry describes the payload:

| Header | Meaning | Payload bytes |
| --- | --- | --- |
| `0..=32` | Legacy FOR width equal to the header | `16 * header` |
| `33..=255` | PFOR: exception count `header >> 5`, low-bit width `header & 31` | `16 * width + 2 * count` |

PFOR stores the 128 low parts using the existing BitPacker4x layout, followed by `count` exception pairs. Each pair contains a one-byte in-block index (`0..127`) and a one-byte high part (`1..255`), in increasing index order. There are at most seven exceptions. The writer considers low widths no more than eight bits below the block's maximum width, guaranteeing that every high part fits in one byte. PFOR is used only when its payload is strictly smaller than legacy FOR.

Header `32` remains legacy full-width FOR. It would otherwise collide with PFOR count 1, width 0, so the writer excludes that combination. Count 2 through 7, width 0 remains valid. Frequencies up to `u32::MAX` are supported: the largest encoded value is `u32::MAX - 1`.

The decoder adds one to the unpacked lows first, then restores each exception by **adding** `high << width`. Addition preserves the carry when `low + 1` reaches `1 << width`; OR would produce the wrong frequency in that case. The reconstructed result equals `low + 1 + (high << width)` and cannot overflow for valid encoded frequencies. This order also avoids reading a full SIMD lane after scalar exception stores. Historical FOR headers retain their existing increment mode, including old blocks encoded without subtracting one.

Readers that request only document IDs still read the frequency header from skip metadata. They advance over the exact payload size without decompressing frequencies. FOR document blocks advance by `document_payload_bytes + frequency_payload_bytes`; dense blocks advance by `8 * bitset_longs + frequency_payload_bytes`. Stored term-frequency sums continue to advance position offsets unchanged. Fields or terms without frequencies still have zero frequency payload bytes.

The writer advertises index format **9** in segment-file footers. The new reader retains the existing supported range beginning at format 4 and therefore reads older indexes, including format 8. A format-8 reader rejects newly written format-9 files through `IncompatibleIndex`, before interpreting their new frequency headers. Existing indexes require rewriting or merging their segments to gain PFOR storage savings; changing the library version alone does not rewrite their postings.
