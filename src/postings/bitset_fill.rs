//! Bulk-OR a posting bitset into a window of [`TinySet`]s.
//!
//! Mirrors Lucene's `FixedBitSet.orRange` used by
//! `Lucene104PostingsReader.BlockPostingsEnum.intoBitSet` for UNARY blocks.

use common::TinySet;

/// OR `len` bits of `src` starting at bit `src_from` into `dest` starting at
/// bit `dest_from`.
///
/// `src` is a little-endian bitset (`bit s` lives at `src[s/8] & (1 << s%8)`),
/// matching [`super::compression::BlockEncoder::compress_bitset_sorted`].
/// `dest[i]` covers destination bits `[i*64, (i+1)*64)`.
pub(crate) fn or_range_into_tinysets(
    src: &[u8],
    src_from: u32,
    dest: &mut [TinySet],
    dest_from: u32,
    len: u32,
) {
    if len == 0 {
        return;
    }
    debug_assert!((dest_from as usize) + (len as usize) <= dest.len() * 64);

    let mut remaining = len;
    let mut src_bit = src_from;
    let mut dest_idx = (dest_from / 64) as usize;
    let dest_off = dest_from % 64;

    if dest_off != 0 {
        let take = remaining.min(64 - dest_off);
        let bits = load_unaligned_bits(src, src_bit, take);
        dest[dest_idx].union_mut(TinySet::from_bits(bits << dest_off));
        src_bit += take;
        remaining -= take;
        dest_idx += 1;
    }

    while remaining >= 64 {
        let bits = load_unaligned_bits(src, src_bit, 64);
        dest[dest_idx].union_mut(TinySet::from_bits(bits));
        src_bit += 64;
        remaining -= 64;
        dest_idx += 1;
    }

    if remaining > 0 {
        let bits = load_unaligned_bits(src, src_bit, remaining);
        dest[dest_idx].union_mut(TinySet::from_bits(bits));
    }
}

/// Load the low `nbits` bits of `src` starting at bit `bit` (`nbits <= 64`).
fn load_unaligned_bits(src: &[u8], bit: u32, nbits: u32) -> u64 {
    debug_assert!(nbits <= 64);
    if nbits == 0 {
        return 0;
    }
    let byte_idx = (bit / 8) as usize;
    let bit_off = bit % 8;
    if let Some(bytes) = src.get(byte_idx..byte_idx + 8) {
        let word = u64::from_le_bytes(bytes.try_into().unwrap());
        let mut bits = word >> bit_off;
        if bit_off + nbits > 64 {
            let next_byte = src.get(byte_idx + 8).copied().unwrap_or(0);
            bits |= u64::from(next_byte) << (64 - bit_off);
        }
        return if nbits == 64 {
            bits
        } else {
            bits & ((1u64 << nbits) - 1)
        };
    }
    // `bit_off + nbits` can be 71, so the scratch word is u128.
    let bytes_needed = ((bit_off + nbits + 7) / 8) as usize;
    let mut v = 0u128;
    for i in 0..bytes_needed {
        let b = src.get(byte_idx + i).copied().unwrap_or(0);
        v |= u128::from(b) << (8 * i);
    }
    v >>= bit_off;
    let bits = v as u64;
    if nbits < 64 {
        bits & ((1u64 << nbits) - 1)
    } else {
        bits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bit_in_bytes(bytes: &[u8], bit: u32) -> bool {
        let b = bytes.get((bit / 8) as usize).copied().unwrap_or(0);
        (b & (1 << (bit % 8))) != 0
    }

    #[test]
    fn or_range_matches_bit_walk() {
        // 5 longs = 320 bits, with a mix of set bits.
        let mut src = vec![0u8; 40];
        for bit in [0u32, 1, 7, 8, 63, 64, 65, 127, 200, 255, 256, 319] {
            src[(bit / 8) as usize] |= 1 << (bit % 8);
        }
        for src_from in [0u32, 1, 7, 8, 63, 64, 200] {
            for dest_from in [0u32, 1, 5, 63, 64, 100] {
                for len in [0u32, 1, 7, 8, 64, 65, 128, 200] {
                    let dest_bits = dest_from + len;
                    if dest_bits > 16 * 64 {
                        continue;
                    }
                    if src_from + len > 320 {
                        continue;
                    }
                    let mut dest = [TinySet::empty(); 16];
                    or_range_into_tinysets(&src, src_from, &mut dest, dest_from, len);
                    for i in 0..len {
                        let expected = bit_in_bytes(&src, src_from + i);
                        let db = dest_from + i;
                        let got = dest[(db / 64) as usize].contains(db % 64);
                        assert_eq!(
                            got, expected,
                            "src_from={src_from} dest_from={dest_from} len={len} i={i}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn or_range_randomized_matches_bit_walk() {
        fn next(state: &mut u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state
        }

        let mut state = 0x8a5c_4f2d_19b7_63e1u64;
        for _ in 0..10_000 {
            let src_len = (next(&mut state) % 81) as usize;
            let mut src = vec![0u8; src_len];
            for byte in &mut src {
                *byte = next(&mut state) as u8;
            }
            let src_from = (next(&mut state) % (src_len as u64 * 8 + 1)) as u32;
            let dest_from = (next(&mut state) % 321) as u32;
            let available = (src_len as u32 * 8 - src_from).min(320 - dest_from);
            let len = (next(&mut state) % (u64::from(available) + 1)) as u32;
            let mut actual = [TinySet::EMPTY; 5];
            for bit in 0..320u32 {
                if next(&mut state) & 7 == 0 {
                    actual[(bit / 64) as usize].insert_mut(bit % 64);
                }
            }
            let mut expected = actual;
            for offset in 0..len {
                if bit_in_bytes(&src, src_from + offset) {
                    let bit = dest_from + offset;
                    expected[(bit / 64) as usize].insert_mut(bit % 64);
                }
            }
            or_range_into_tinysets(&src, src_from, &mut actual, dest_from, len);
            for bit in 0..320u32 {
                assert_eq!(
                    actual[(bit / 64) as usize].contains(bit % 64),
                    expected[(bit / 64) as usize].contains(bit % 64),
                    "src_len={src_len} src_from={src_from} dest_from={dest_from} len={len} \
                     bit={bit}"
                );
            }
        }
    }

    #[test]
    fn load_unaligned_bits_matches_bit_walk() {
        let mut state = 0x2e8b_3b9f_64a5_17cdu64;
        for src_len in 0..=17usize {
            let mut src = vec![0u8; src_len];
            for byte in &mut src {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state as u8;
            }
            for bit in 0..=(src_len as u32 * 8 + 8) {
                for nbits in 0..=64u32 {
                    let expected = (0..nbits).fold(0u64, |word, offset| {
                        word | (u64::from(bit_in_bytes(&src, bit + offset)) << offset)
                    });
                    assert_eq!(
                        load_unaligned_bits(&src, bit, nbits),
                        expected,
                        "src_len={src_len} bit={bit} nbits={nbits}"
                    );
                }
            }
        }
    }
}
