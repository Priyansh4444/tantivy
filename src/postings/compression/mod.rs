use bitpacking::{BitPacker, BitPacker4x};

pub const COMPRESSION_BLOCK_SIZE: usize = BitPacker4x::BLOCK_LEN;
// in vint encoding, each byte stores 7 bits of data, so we need at most 32 / 7 = 4.57 bytes to
// store a u32 in the worst case, rounding up to 5 bytes total
const MAX_VINT_SIZE: usize = 5;
const COMPRESSED_BLOCK_MAX_SIZE: usize = COMPRESSION_BLOCK_SIZE * MAX_VINT_SIZE;

mod vint;

/// Returns the size in bytes of a compressed block, given `num_bits`.
#[inline]
pub fn compressed_block_size(num_bits: u8) -> usize {
    (num_bits as usize) * COMPRESSION_BLOCK_SIZE / 8
}

/// Byte length of a full term-frequency block. Legacy headers 0..=32
/// store only the FOR width. Larger headers pack 1..=7 exception pairs
/// in the high three bits and the low-bit width in the low five bits.
/// Header 32 stays reserved for legacy full-width u32 values.
#[inline]
pub fn compressed_freq_block_size(header: u8) -> usize {
    if header == 32 {
        compressed_block_size(32)
    } else {
        compressed_block_size(header & 31) + usize::from(header >> 5) * 2
    }
}

/// Returns the size in bytes of a dense bitset block with `num_longs` u64s.
///
/// Each bit `s` represents doc `bitset_base_doc(offset) + s`.
/// See `Lucene104PostingsWriter`, which stores dense blocks the same way
/// when the bitset is more storage-efficient than FOR deltas.
#[inline]
pub fn dense_block_size(num_longs: u8) -> usize {
    num_longs as usize * 8
}

/// Number of u64s needed to cover `doc_range` docs as a bitset.
#[inline]
pub fn num_bitset_longs(doc_range: u32) -> u32 {
    doc_range.div_ceil(64)
}

/// Base doc id for a dense bitset block: first doc id that bit 0 represents.
///
/// `offset` is the previous block's last doc (`0` for the first block, where
/// doc ids start at 0). Mirrors the `offset == 0 -> None` convention of
/// `compress_block_sorted`.
#[inline]
pub fn bitset_base_doc(offset: u32) -> u32 {
    if offset == 0 {
        0
    } else {
        offset.wrapping_add(1)
    }
}

pub struct BlockEncoder {
    bitpacker: BitPacker4x,
    pub output: [u8; COMPRESSED_BLOCK_MAX_SIZE],
}

impl Default for BlockEncoder {
    fn default() -> Self {
        BlockEncoder::new()
    }
}

impl BlockEncoder {
    pub fn new() -> BlockEncoder {
        BlockEncoder {
            bitpacker: BitPacker4x::new(),
            output: [0u8; COMPRESSED_BLOCK_MAX_SIZE],
        }
    }

    pub fn compress_block_sorted(&mut self, block: &[u32], offset: u32) -> (u8, &[u8]) {
        // if offset is zero, convert it to None. This is correct as long as we do the same when
        // decompressing. It's required in case the block starts with an actual zero.
        let offset = if offset == 0u32 { None } else { Some(offset) };

        let num_bits = self.bitpacker.num_bits_strictly_sorted(offset, block);
        let written_size =
            self.bitpacker
                .compress_strictly_sorted(offset, block, &mut self.output[..], num_bits);
        (num_bits, &self.output[..written_size])
    }

    /// Compress a single block of unsorted numbers.
    ///
    /// If `minus_one_encoded` is set, each value must be >= 1, and will be encoded in a sligly
    /// more compact format. This is useful for some values where 0 isn't a correct value, such
    /// as term frequency, but isn't correct for some usages like position lists, where 0 can
    /// appear.
    pub fn compress_block_unsorted(
        &mut self,
        block: &[u32],
        minus_one_encoded: bool,
    ) -> (u8, &[u8]) {
        debug_assert!(!minus_one_encoded || !block.contains(&0));

        let mut block_minus_one = [0; COMPRESSION_BLOCK_SIZE];
        let block = if minus_one_encoded {
            for (elem_min_one, elem) in block_minus_one.iter_mut().zip(block) {
                *elem_min_one = elem - 1;
            }
            &block_minus_one
        } else {
            block
        };

        let num_bits = self.bitpacker.num_bits(block);
        let written_size = self
            .bitpacker
            .compress(block, &mut self.output[..], num_bits);
        (num_bits, &self.output[..written_size])
    }

    /// Compress a full term-frequency block, subtracting one before packing.
    /// At most seven outliers store their index and eight high bits separately.
    /// Only choose PFOR when it saves bytes, without enlarging skip metadata.
    pub fn compress_term_freqs(&mut self, block: &[u32]) -> (u8, &[u8]) {
        debug_assert_eq!(block.len(), COMPRESSION_BLOCK_SIZE);
        debug_assert!(!block.contains(&0));
        let mut values = [0u32; COMPRESSION_BLOCK_SIZE];
        let mut widths = [0u8; 33];
        let mut width = 0u8;
        for (&frequency, value) in block.iter().zip(values.iter_mut()) {
            *value = frequency - 1;
            let value_width = (u32::BITS - value.leading_zeros()) as u8;
            widths[value_width as usize] += 1;
            width = width.max(value_width);
        }
        let mut best_width = width;
        let mut best_count = 0u8;
        let mut best_size = compressed_block_size(width);
        let mut exceptions = 0usize;
        for candidate in (width.saturating_sub(8)..width).rev() {
            exceptions += widths[(candidate + 1) as usize] as usize;
            if exceptions > 7 {
                break;
            }
            // n=1,width=0 would collide with the legacy 32-bit header.
            if exceptions == 1 && candidate == 0 {
                continue;
            }
            let size = compressed_block_size(candidate) + exceptions * 2;
            if size < best_size {
                best_size = size;
                best_width = candidate;
                best_count = exceptions as u8;
            }
        }
        if best_count == 0 {
            self.bitpacker.compress(&values, &mut self.output, width);
            return (width, &self.output[..best_size]);
        }
        let mask = (1u32 << best_width) - 1;
        let mut lows = values;
        for value in &mut lows {
            *value &= mask;
        }
        let mut cursor = self.bitpacker.compress(&lows, &mut self.output, best_width);
        for (index, &value) in values.iter().enumerate() {
            let high = value >> best_width;
            if high != 0 {
                self.output[cursor] = index as u8;
                self.output[cursor + 1] = high as u8;
                cursor += 2;
            }
        }
        debug_assert_eq!(cursor, best_size);
        (best_count << 5 | best_width, &self.output[..cursor])
    }

    /// Pack 128 values at a chosen width. Used by positions PFOR, where
    /// exceptional high bits are stored separately.
    pub fn compress_block_unsorted_at_width(&mut self, block: &[u32], width: u8) -> &[u8] {
        let written_size = self.bitpacker.compress(block, &mut self.output, width);
        &self.output[..written_size]
    }

    /// Compress a full block of sorted doc ids as a dense bitset.
    ///
    /// Bit `s` (`s = doc - base`, where `base` is [`bitset_base_doc`]) is
    /// set iff that doc is in `block`. Returns the `num_longs * 8` encoded
    /// bytes. Mirrors the unary-coding path (`spareBitSet`) in Lucene's
    /// `Lucene104PostingsWriter`.
    pub fn compress_bitset_sorted(
        &mut self,
        block: &[u32],
        offset: u32,
        num_longs: usize,
    ) -> &[u8] {
        debug_assert_eq!(block.len(), COMPRESSION_BLOCK_SIZE);
        let bytes_needed = num_longs * 8;
        debug_assert!(bytes_needed <= self.output.len());
        self.output[..bytes_needed].fill(0);
        let base = bitset_base_doc(offset);
        for &doc in block {
            let s = doc.wrapping_sub(base) as usize;
            debug_assert!(s < num_longs * 64);
            self.output[s / 8] |= 1u8 << (s % 8);
        }
        &self.output[..bytes_needed]
    }
}

#[derive(Clone)]
pub struct BlockDecoder {
    bitpacker: BitPacker4x,
    output: [u32; COMPRESSION_BLOCK_SIZE],
    pub output_len: usize,
}

impl Default for BlockDecoder {
    fn default() -> Self {
        BlockDecoder::with_val(0u32)
    }
}

impl BlockDecoder {
    pub fn with_val(val: u32) -> BlockDecoder {
        BlockDecoder {
            bitpacker: BitPacker4x::new(),
            output: [val; COMPRESSION_BLOCK_SIZE],
            output_len: 0,
        }
    }

    /// Decompress block of sorted integers.
    ///
    /// `strict_delta` depends on what encoding was used. Older version of tantivy never use strict
    /// deltas, newer versions always use them.
    pub fn uncompress_block_sorted(
        &mut self,
        compressed_data: &[u8],
        offset: u32,
        num_bits: u8,
        strict_delta: bool,
    ) -> usize {
        if strict_delta {
            let offset = std::num::NonZeroU32::new(offset).map(std::num::NonZeroU32::get);

            self.output_len = COMPRESSION_BLOCK_SIZE;
            self.bitpacker.decompress_strictly_sorted(
                offset,
                compressed_data,
                &mut self.output,
                num_bits,
            )
        } else {
            self.output_len = COMPRESSION_BLOCK_SIZE;
            self.bitpacker
                .decompress_sorted(offset, compressed_data, &mut self.output, num_bits)
        }
    }

    /// Decompress block of unsorted integers.
    ///
    /// `minus_one_encoded` depends on what encoding was used. Older version of tantivy never use
    /// that encoding. Newer version use it for some structures, but not all. See the corresponding
    /// call to `BlockEncoder::compress_block_unsorted`.
    pub fn uncompress_block_unsorted(
        &mut self,
        compressed_data: &[u8],
        num_bits: u8,
        minus_one_encoded: bool,
    ) -> usize {
        self.output_len = COMPRESSION_BLOCK_SIZE;
        let res = self
            .bitpacker
            .decompress(compressed_data, &mut self.output, num_bits);
        if minus_one_encoded {
            for val in &mut self.output {
                *val += 1;
            }
        }
        res
    }

    /// Decode legacy FOR or patched FOR term frequencies. Increment packed
    /// lows before restoring exceptions to avoid vector reads after scalar stores.
    pub fn uncompress_term_freqs(
        &mut self,
        compressed_data: &[u8],
        header: u8,
        minus_one_encoded: bool,
    ) -> usize {
        if header <= 32 {
            return self.uncompress_block_unsorted(compressed_data, header, minus_one_encoded);
        }
        let width = header & 31;
        let packed_size = self.uncompress_block_unsorted(compressed_data, width, minus_one_encoded);
        let size = compressed_freq_block_size(header);
        for pair in compressed_data[packed_size..size].chunks_exact(2) {
            // Addition preserves carries from low + 1 across the packed width.
            // The final value cannot overflow for a valid encoded frequency.
            self.output[pair[0] as usize] += (pair[1] as u32) << width;
        }
        size
    }

    /// Restore the exceptional high bits after unpacking a positions block.
    pub fn patch_unsorted_exceptions(&mut self, exceptions: &[u8], width: u8) {
        for pair in exceptions.chunks_exact(2) {
            self.output[pair[0] as usize] |= (pair[1] as u32) << width;
        }
    }

    /// Decompress a dense bitset block written by
    /// `BlockEncoder::compress_bitset_sorted`.
    ///
    /// Enumerate set bits a word at a time, so the cost is proportional
    /// to the number of docs rather than the span of the block.
    /// Returns the number of bytes consumed (`num_longs * 8`).
    pub fn uncompress_bitset_sorted(
        &mut self,
        compressed_data: &[u8],
        offset: u32,
        num_longs: usize,
    ) -> usize {
        let bytes_needed = num_longs * 8;
        debug_assert!(compressed_data.len() >= bytes_needed);
        let base = bitset_base_doc(offset);
        let mut out = 0usize;
        for (word_idx, word_bytes) in compressed_data[..bytes_needed].chunks_exact(8).enumerate() {
            let mut bits = u64::from_le_bytes(word_bytes.try_into().unwrap());
            let word_base = base.wrapping_add((word_idx * 64) as u32);
            while bits != 0 {
                self.output[out] = word_base.wrapping_add(bits.trailing_zeros());
                out += 1;
                bits &= bits - 1;
            }
        }
        debug_assert_eq!(out, COMPRESSION_BLOCK_SIZE);
        self.output_len = out;
        bytes_needed
    }

    #[inline]
    pub fn output_array(&self) -> &[u32] {
        &self.output[..self.output_len]
    }

    /// Return in-block index of first value >= `target`.
    /// Uses the padded buffer to enable branchless search.
    #[inline]
    pub(crate) fn seek_within_block(&self, target: u32) -> usize {
        crate::postings::search_block(&self.output, target)
    }

    #[inline]
    pub fn output(&self, idx: usize) -> u32 {
        self.output[idx]
    }
}

pub trait VIntEncoder {
    /// Compresses an array of `u32` integers,
    /// using [delta-encoding](https://en.wikipedia.org/wiki/Delta_encoding)
    /// and variable bytes encoding.
    ///
    /// The method takes an array of ints to compress, and returns
    /// a `&[u8]` representing the compressed data.
    ///
    /// The method also takes an offset to give the value of the
    /// hypothetical previous element in the delta-encoding.
    fn compress_vint_sorted(&mut self, input: &[u32], offset: u32) -> &[u8];

    /// Compresses an array of `u32` integers,
    /// using variable bytes encoding.
    ///
    /// The method takes an array of ints to compress, and returns
    /// a `&[u8]` representing the compressed data.
    fn compress_vint_unsorted(&mut self, input: &[u32]) -> &[u8];
}

pub trait VIntDecoder {
    /// Uncompress an array of `u32` integers,
    /// that were compressed using [delta-encoding](https://en.wikipedia.org/wiki/Delta_encoding)
    /// and variable bytes encoding.
    ///
    /// The method takes a number of int to decompress, and returns
    /// the amount of bytes that were read to decompress them.
    ///
    /// The method also takes an offset to give the value of the
    /// hypothetical previous element in the delta-encoding.
    ///
    /// For instance, if delta encoded are `1, 3, 9`, and the
    /// `offset` is 5, then the output will be:
    /// `5 + 1 = 6, 6 + 3= 9, 9 + 9 = 18`
    ///
    /// The value given in `padding` will be used to fill the remaining `128 - num_els` values.
    fn uncompress_vint_sorted(
        &mut self,
        compressed_data: &[u8],
        offset: u32,
        num_els: usize,
        padding: u32,
    ) -> usize;

    /// Uncompress an array of `u32s`, compressed using variable
    /// byte encoding.
    ///
    /// The method takes a number of int to decompress, and returns
    /// the amount of bytes that were read to decompress them.
    ///
    /// The value given in `padding` will be used to fill the remaining `128 - num_els` values.
    fn uncompress_vint_unsorted(
        &mut self,
        compressed_data: &[u8],
        num_els: usize,
        padding: u32,
    ) -> usize;

    fn uncompress_vint_unsorted_until_end(&mut self, compressed_data: &[u8]);
}

impl VIntEncoder for BlockEncoder {
    fn compress_vint_sorted(&mut self, input: &[u32], offset: u32) -> &[u8] {
        vint::compress_sorted(input, &mut self.output, offset)
    }

    fn compress_vint_unsorted(&mut self, input: &[u32]) -> &[u8] {
        vint::compress_unsorted(input, &mut self.output)
    }
}

impl VIntDecoder for BlockDecoder {
    fn uncompress_vint_sorted(
        &mut self,
        compressed_data: &[u8],
        offset: u32,
        num_els: usize,
        padding: u32,
    ) -> usize {
        self.output_len = num_els;
        self.output.iter_mut().for_each(|el| *el = padding);
        vint::uncompress_sorted(compressed_data, &mut self.output[..num_els], offset)
    }

    fn uncompress_vint_unsorted(
        &mut self,
        compressed_data: &[u8],
        num_els: usize,
        padding: u32,
    ) -> usize {
        self.output_len = num_els;
        self.output.iter_mut().for_each(|el| *el = padding);
        vint::uncompress_unsorted(compressed_data, &mut self.output[..num_els])
    }

    fn uncompress_vint_unsorted_until_end(&mut self, compressed_data: &[u8]) {
        let num_els = vint::uncompress_unsorted_until_end(compressed_data, &mut self.output);
        self.output_len = num_els;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::TERMINATED;

    #[test]
    fn test_encode_sorted_block() {
        let vals: Vec<u32> = (0u32..128u32).map(|i| i * 7).collect();
        let mut encoder = BlockEncoder::new();
        let (num_bits, compressed_data) = encoder.compress_block_sorted(&vals, 0);
        let mut decoder = BlockDecoder::default();
        {
            let consumed_num_bytes =
                decoder.uncompress_block_sorted(compressed_data, 0, num_bits, true);
            assert_eq!(consumed_num_bytes, compressed_data.len());
        }
        for i in 0..128 {
            assert_eq!(vals[i], decoder.output(i));
        }
    }

    #[test]
    fn test_encode_sorted_block_with_offset() {
        let vals: Vec<u32> = (0u32..128u32).map(|i| 11 + i * 7).collect();
        let mut encoder = BlockEncoder::default();
        let (num_bits, compressed_data) = encoder.compress_block_sorted(&vals, 10);
        let mut decoder = BlockDecoder::default();
        {
            let consumed_num_bytes =
                decoder.uncompress_block_sorted(compressed_data, 10, num_bits, true);
            assert_eq!(consumed_num_bytes, compressed_data.len());
        }
        for i in 0..128 {
            assert_eq!(vals[i], decoder.output(i));
        }
    }

    #[test]
    fn test_encode_sorted_block_with_junk() {
        let mut compressed: Vec<u8> = Vec::new();
        let n = 128;
        let vals: Vec<u32> = (0..n).map(|i| 11u32 + (i as u32) * 7u32).collect();
        let mut encoder = BlockEncoder::default();
        let (num_bits, compressed_data) = encoder.compress_block_sorted(&vals, 10);
        compressed.extend_from_slice(compressed_data);
        compressed.push(173u8);
        let mut decoder = BlockDecoder::default();
        {
            let consumed_num_bytes =
                decoder.uncompress_block_sorted(&compressed, 10, num_bits, true);
            assert_eq!(consumed_num_bytes, compressed.len() - 1);
            assert_eq!(compressed[consumed_num_bytes], 173u8);
        }
        for i in 0..n {
            assert_eq!(vals[i], decoder.output(i));
        }
    }

    #[test]
    fn test_encode_unsorted_block_with_junk() {
        for minus_one_encode in [false, true] {
            let mut compressed: Vec<u8> = Vec::new();
            let n = 128;
            let vals: Vec<u32> = (0..n).map(|i| 11u32 + (i as u32) * 7u32 % 12).collect();
            let mut encoder = BlockEncoder::default();
            let (num_bits, compressed_data) =
                encoder.compress_block_unsorted(&vals, minus_one_encode);
            compressed.extend_from_slice(compressed_data);
            compressed.push(173u8);
            let mut decoder = BlockDecoder::default();
            {
                let consumed_num_bytes =
                    decoder.uncompress_block_unsorted(&compressed, num_bits, minus_one_encode);
                assert_eq!(consumed_num_bytes + 1, compressed.len());
                assert_eq!(compressed[consumed_num_bytes], 173u8);
            }
            for i in 0..n {
                assert_eq!(vals[i], decoder.output(i));
            }
        }
    }

    #[test]
    fn test_frequency_pfor_payload_sizes_all_headers() {
        for header in 0u8..=255 {
            let expected = if header <= 32 {
                usize::from(header) * COMPRESSION_BLOCK_SIZE / 8
            } else {
                usize::from(header & 31) * COMPRESSION_BLOCK_SIZE / 8 + usize::from(header >> 5) * 2
            };
            assert_eq!(
                compressed_freq_block_size(header),
                expected,
                "header {header}"
            );
        }
    }

    #[test]
    fn test_frequency_pfor_roundtrip_widths_and_exception_counts() {
        let mut encoder = BlockEncoder::new();
        let mut decoder = BlockDecoder::default();
        for width in 0..32 {
            for exception_count in 1..=7 {
                let low = if width == 0 { 0 } else { (1u32 << width) - 1 };
                let high = (1u32 << width).saturating_add(low);
                let mut frequencies = [low + 1; COMPRESSION_BLOCK_SIZE];
                for frequency in &mut frequencies[..exception_count] {
                    *frequency = high.saturating_add(1);
                }
                let (header, bytes) = encoder.compress_term_freqs(&frequencies);
                assert_eq!(bytes.len(), compressed_freq_block_size(header));
                let mut with_junk = bytes.to_vec();
                with_junk.push(173);
                for minus_one in [false, true] {
                    let consumed = decoder.uncompress_term_freqs(&with_junk, header, minus_one);
                    assert_eq!(with_junk[consumed], 173);
                    let expected = frequencies.map(|frequency| frequency - u32::from(!minus_one));
                    assert_eq!(decoder.output_array(), &expected);
                }
                if width == 0 && exception_count == 1 {
                    assert_eq!(header, 1, "reserved legacy header 32 cannot encode PFOR");
                } else if width < 31 {
                    assert_eq!(header, (exception_count as u8) << 5 | width as u8);
                }
            }
        }
    }

    #[test]
    fn test_frequency_pfor_full_u32_and_legacy_encodings() {
        let mut encoder = BlockEncoder::new();
        let mut decoder = BlockDecoder::default();
        let mut frequencies = [1; COMPRESSION_BLOCK_SIZE];
        frequencies[127] = u32::MAX;
        let (header, bytes) = encoder.compress_term_freqs(&frequencies);
        assert!(header > 32);
        assert_eq!(
            decoder.uncompress_term_freqs(bytes, header, true),
            bytes.len()
        );
        assert_eq!(decoder.output_array(), &frequencies);
        for minus_one in [false, true] {
            for frequency in [1u32, 2, 257, u32::MAX] {
                let original = [frequency; COMPRESSION_BLOCK_SIZE];
                let (header, bytes) = encoder.compress_block_unsorted(&original, minus_one);
                assert!(header <= 32);
                assert_eq!(
                    decoder.uncompress_term_freqs(bytes, header, minus_one),
                    bytes.len()
                );
                assert_eq!(decoder.output_array(), &original);
            }
        }
        let original = [u32::MAX; COMPRESSION_BLOCK_SIZE];
        let (header, bytes) = encoder.compress_term_freqs(&original);
        assert_eq!(header, 32);
        decoder.uncompress_term_freqs(bytes, header, true);
        assert_eq!(decoder.output_array(), &original);
    }

    #[test]
    fn test_block_decoder_initialization() {
        let block = BlockDecoder::with_val(TERMINATED);
        assert_eq!(block.output(0), TERMINATED);
    }

    #[test]
    fn test_bitset_sorted_roundtrip() {
        // Dense-ish block: every other doc, range 256 -> 4 longs (32B).
        let vals: Vec<u32> = (0u32..128).map(|i| i * 2).collect();
        let mut encoder = BlockEncoder::new();
        let num_longs = 4;
        let encoded: Vec<u8> = encoder.compress_bitset_sorted(&vals, 0, num_longs).to_vec();
        assert_eq!(encoded.len(), num_longs * 8);
        let mut decoder = BlockDecoder::default();
        let consumed = decoder.uncompress_bitset_sorted(&encoded, 0, num_longs);
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoder.output_array(), &vals[..]);
    }

    #[test]
    fn test_bitset_sorted_roundtrip_with_offset() {
        // Second block: prev last = 1000, docs 1001.. with an extra gap every 7 docs.
        let vals: Vec<u32> = (0u32..128).map(|i| 1001 + i + i / 7).collect();
        let last = *vals.last().unwrap();
        let base = bitset_base_doc(1000);
        let range = last.wrapping_sub(base).wrapping_add(1);
        let num_longs = num_bitset_longs(range) as usize;
        let mut encoder = BlockEncoder::new();
        let encoded: Vec<u8> = encoder
            .compress_bitset_sorted(&vals, 1000, num_longs)
            .to_vec();
        let mut decoder = BlockDecoder::default();
        decoder.uncompress_bitset_sorted(&encoded, 1000, num_longs);
        assert_eq!(decoder.output_array(), &vals[..]);
    }

    #[test]
    fn test_encode_vint() {
        const PADDING_VALUE: u32 = 234_234_345u32;
        let expected_length = 154;
        let mut encoder = BlockEncoder::new();
        let input: Vec<u32> = (0u32..123u32).map(|i| 4 + i * 7 / 2).collect();
        for offset in &[0u32, 1u32, 2u32] {
            let encoded_data = encoder.compress_vint_sorted(&input, *offset);
            assert!(encoded_data.len() <= expected_length);
            let mut decoder = BlockDecoder::default();
            let consumed_num_bytes =
                decoder.uncompress_vint_sorted(encoded_data, *offset, input.len(), PADDING_VALUE);
            assert_eq!(consumed_num_bytes, encoded_data.len());
            assert_eq!(input, decoder.output_array());
            for i in input.len()..COMPRESSION_BLOCK_SIZE {
                assert_eq!(decoder.output(i), PADDING_VALUE);
            }
        }
    }

    #[test]
    fn test_compress_vint_unsorted_does_not_overflow() {
        let mut encoder = BlockEncoder::new();
        let input: Vec<u32> = vec![u32::MAX; COMPRESSION_BLOCK_SIZE];
        encoder.compress_vint_unsorted(&input);
    }
}

#[cfg(all(test, feature = "unstable"))]
mod bench {

    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    use test::Bencher;

    use super::*;
    use crate::TERMINATED;

    fn generate_array_with_seed(n: usize, ratio: f64, seed_val: u8) -> Vec<u32> {
        let mut seed: [u8; 32] = [0; 32];
        seed[31] = seed_val;
        let mut rng = StdRng::from_seed(seed);
        (0u32..)
            .filter(|_| rng.random_bool(ratio))
            .take(n)
            .collect()
    }

    pub fn generate_array(n: usize, ratio: f64) -> Vec<u32> {
        generate_array_with_seed(n, ratio, 4)
    }

    #[bench]
    fn bench_compress(b: &mut Bencher) {
        let mut encoder = BlockEncoder::new();
        let data = generate_array(COMPRESSION_BLOCK_SIZE, 0.1);
        b.iter(|| {
            encoder.compress_block_sorted(&data, 0u32);
        });
    }

    #[bench]
    fn bench_uncompress(b: &mut Bencher) {
        let mut encoder = BlockEncoder::new();
        let data = generate_array(COMPRESSION_BLOCK_SIZE, 0.1);
        let (num_bits, compressed) = encoder.compress_block_sorted(&data, 0u32);
        let mut decoder = BlockDecoder::default();
        b.iter(|| {
            decoder.uncompress_block_sorted(compressed, 0u32, num_bits, true);
        });
    }

    //#[test]
    // fn test_all_docs_compression_numbits() {
    // for expected_num_bits in 0u8.. {
    // let mut data = [0u32; 128];
    // if expected_num_bits > 0 {
    // data[0] = (1u64 << (expected_num_bits as usize) - 1) as u32;
    //}
    // let mut encoder = BlockEncoder::new();
    // let (num_bits, compressed) = encoder.compress_block_unsorted(&data);
    // assert_eq!(compressed.len(), compressed_block_size(num_bits));
    //}

    const NUM_INTS_BENCH_VINT: usize = 10;

    #[bench]
    fn bench_compress_vint(b: &mut Bencher) {
        let mut encoder = BlockEncoder::new();
        let data = generate_array(NUM_INTS_BENCH_VINT, 0.001);
        b.iter(|| {
            encoder.compress_vint_sorted(&data, 0u32);
        });
    }

    #[bench]
    fn bench_uncompress_vint(b: &mut Bencher) {
        let mut encoder = BlockEncoder::new();
        let data = generate_array(NUM_INTS_BENCH_VINT, 0.001);
        let compressed = encoder.compress_vint_sorted(&data, 0u32);
        let mut decoder = BlockDecoder::default();
        b.iter(|| {
            decoder.uncompress_vint_sorted(compressed, 0u32, NUM_INTS_BENCH_VINT, TERMINATED);
        });
    }
}
