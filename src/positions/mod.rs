//! Tantivy can (if instructed to do so in the schema) store the term positions in a given field.
//!
//! This position is expressed as token ordinal. For instance,
//! In "The beauty and the beast", the term "the" appears in position 0 and position 3.
//! This information is useful to run phrase queries.
//!
//! The [position](crate::index::SegmentComponent::Positions) file contains all of the
//! bitpacked positions delta, for all terms of a given field, one term after the other.
//!
//! Each term is encoded independently.
//! Like for posting lists, tantivy relies on simd bitpacking to encode the positions delta in
//! blocks of 128 deltas. Because we rarely have a multiple of 128, the final block encodes
//! the remaining values with variable int encoding.
//!
//! Each new-format term starts with `VInt(u64::MAX)` followed by the number of full
//! blocks, a width byte per block, an exception-count byte per block, the blocks,
//! and finally the variable-int tail. A full block stores 128 low-bit values at
//! its width, followed by up to seven `(index, high byte)` exception pairs.
//! The preceding format starts directly with the number of blocks and has no
//! exception counts; the reader accepts both formats.
//!
//! Contrary to postings list, the reader does not have access on the number of positions that is
//! encoded, and instead stops decoding the last block when its byte slice has been entirely read.
//!
//! Widths and exception counts are encoded separately so a reader can skip
//! blocks without decoding their values.
mod reader;
mod serializer;

use bitpacking::{BitPacker, BitPacker4x};

pub use self::reader::PositionReader;
pub use self::serializer::PositionSerializer;

const COMPRESSION_BLOCK_SIZE: usize = BitPacker4x::BLOCK_LEN;

#[cfg(test)]
pub(crate) mod tests {

    use common::{BinarySerializable, VInt};
    use proptest::prelude::*;
    use proptest::sample::select;

    use super::PositionSerializer;
    use crate::directory::OwnedBytes;
    use crate::positions::reader::PositionReader;
    use crate::postings::compression::BlockEncoder;

    fn create_positions_data(vals: &[u32]) -> crate::Result<OwnedBytes> {
        let mut positions_buffer = vec![];
        let mut serializer = PositionSerializer::new(&mut positions_buffer);
        serializer.write_positions_delta(vals);
        serializer.close_term()?;
        serializer.close()?;
        Ok(OwnedBytes::new(positions_buffer))
    }

    fn gen_delta_positions() -> BoxedStrategy<Vec<u32>> {
        select(&[0, 1, 70, 127, 128, 129, 200, 255, 256, 257, 270][..])
            .prop_flat_map(|num_delta_positions| {
                proptest::collection::vec(
                    select(&[1u32, 2u32, 4u32, 8u32, 16u32][..]),
                    num_delta_positions,
                )
            })
            .boxed()
    }

    proptest! {
        #[test]
        fn test_position_delta(delta_positions in gen_delta_positions()) {
            let delta_positions_data = create_positions_data(&delta_positions).unwrap();
            let mut position_reader = PositionReader::open(delta_positions_data).unwrap();
            let mut minibuf = [0u32; 1];
            for (offset, &delta_position) in delta_positions.iter().enumerate() {
                position_reader.read(offset as u64, &mut minibuf[..]);
                assert_eq!(delta_position, minibuf[0]);
            }
        }
    }

    #[test]
    fn test_position_read() -> crate::Result<()> {
        let position_deltas: Vec<u32> = (0..1000).collect();
        let positions_data = create_positions_data(&position_deltas[..])?;
        assert_eq!(positions_data.len(), 1224);
        let mut position_reader = PositionReader::open(positions_data)?;
        for &n in &[1, 10, 127, 128, 130, 312] {
            let mut v = vec![0u32; n];
            position_reader.read(0, &mut v[..]);
            for i in 0..n {
                assert_eq!(position_deltas[i], i as u32);
            }
        }
        Ok(())
    }

    #[test]
    fn test_empty_position() -> crate::Result<()> {
        let mut positions_buffer = vec![];
        let mut serializer = PositionSerializer::new(&mut positions_buffer);
        serializer.close_term()?;
        serializer.close()?;
        let position_delta = OwnedBytes::new(positions_buffer);
        assert!(PositionReader::open(position_delta).is_ok());
        Ok(())
    }

    #[test]
    fn test_multiple_write_positions() -> crate::Result<()> {
        let mut positions_buffer = vec![];
        let mut serializer = PositionSerializer::new(&mut positions_buffer);
        serializer.write_positions_delta(&[1u32, 12u32]);
        serializer.write_positions_delta(&[4u32, 17u32]);
        serializer.write_positions_delta(&[443u32]);
        serializer.close_term()?;
        serializer.close()?;
        let position_delta = OwnedBytes::new(positions_buffer);
        let mut output_delta_pos_buffer = [0u32; 5];
        let mut position_reader = PositionReader::open(position_delta)?;
        position_reader.read(0, &mut output_delta_pos_buffer[..]);
        assert_eq!(
            &output_delta_pos_buffer[..],
            &[1u32, 12u32, 4u32, 17u32, 443u32]
        );
        Ok(())
    }

    #[test]
    fn test_position_read_with_offset() -> crate::Result<()> {
        let position_deltas: Vec<u32> = (0..1000).collect();
        let positions_data = create_positions_data(&position_deltas[..])?;
        assert_eq!(positions_data.len(), 1224);
        let mut position_reader = PositionReader::open(positions_data)?;
        for &offset in &[1u64, 10u64, 127u64, 128u64, 130u64, 312u64] {
            for &len in &[1, 10, 130, 500] {
                let mut v = vec![0u32; len];
                position_reader.read(offset, &mut v[..]);
                for i in 0..len {
                    assert_eq!(v[i], i as u32 + offset as u32);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn test_position_read_after_skip() -> crate::Result<()> {
        let position_deltas: Vec<u32> = (0..1_000).collect();
        let positions_data = create_positions_data(&position_deltas[..])?;
        assert_eq!(positions_data.len(), 1224);

        let mut position_reader = PositionReader::open(positions_data)?;
        let mut buf = [0u32; 7];
        let mut c = 0;

        let mut offset = 0;
        for _ in 0..100 {
            position_reader.read(offset, &mut buf);
            position_reader.read(offset, &mut buf);
            offset += 7;
            for &el in &buf {
                assert_eq!(c, el);
                c += 1;
            }
        }
        Ok(())
    }

    #[test]
    fn test_position_reread_anchor_different_than_block() -> crate::Result<()> {
        let positions_delta: Vec<u32> = (0..2_000_000).collect();
        let positions_data = create_positions_data(&positions_delta[..])?;
        assert_eq!(positions_data.len(), 5003499);
        let mut position_reader = PositionReader::open(positions_data)?;
        let mut buf = [0u32; 256];
        position_reader.read(128, &mut buf);
        for i in 0..256 {
            assert_eq!(buf[i], (128 + i) as u32);
        }
        position_reader.read(128, &mut buf);
        for i in 0..256 {
            assert_eq!(buf[i], (128 + i) as u32);
        }
        Ok(())
    }

    #[test]
    fn test_position_requesting_passed_block() -> crate::Result<()> {
        let positions_delta: Vec<u32> = (0..512).collect();
        let positions_data = create_positions_data(&positions_delta[..])?;
        assert_eq!(positions_data.len(), 533);
        let mut buf = [0u32; 1];
        let mut position_reader = PositionReader::open(positions_data)?;
        position_reader.read(230, &mut buf);
        assert_eq!(buf[0], 230);
        position_reader.read(9, &mut buf);
        assert_eq!(buf[0], 9);
        Ok(())
    }

    #[test]
    fn test_position() -> crate::Result<()> {
        const CONST_VAL: u32 = 9u32;
        let positions_delta: Vec<u32> = std::iter::repeat_n(CONST_VAL, 2_000_000).collect();
        let positions_data = create_positions_data(&positions_delta[..])?;
        assert_eq!(positions_data.len(), 1_015_627);
        let mut position_reader = PositionReader::open(positions_data)?;
        let mut buf = [0u32; 1];
        position_reader.read(0, &mut buf);
        assert_eq!(buf[0], CONST_VAL);
        Ok(())
    }

    #[test]
    fn test_position_advance() -> crate::Result<()> {
        let positions_delta: Vec<u32> = (0..2_000_000).collect();
        let positions_data = create_positions_data(&positions_delta[..])?;
        assert_eq!(positions_data.len(), 5_003_499);
        for &offset in &[
            10,
            128 * 1024,
            128 * 1024 - 1,
            128 * 1024 + 7,
            128 * 10 * 1024 + 10,
        ] {
            let mut position_reader = PositionReader::open(positions_data.clone())?;
            let mut buf = [0u32; 1];
            position_reader.read(offset, &mut buf);
            assert_eq!(buf[0], offset as u32);
        }
        Ok(())
    }

    #[test]
    fn test_patched_blocks_random_access_and_clone() -> crate::Result<()> {
        let mut values = Vec::new();
        let mut seed = 0x9876_5432u32;
        for block in 0..32usize {
            let width = 3 + block % 22;
            for i in 0..128usize {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let mut value = seed & ((1u32 << width) - 1);
                if i == 7 || i == 63 || i == 121 {
                    value |= (1 + block as u32 % 8) << width;
                }
                values.push(value);
            }
        }
        values.extend([0u32; 128]);
        values.extend([u32::MAX; 128]);
        values.extend((0..37).map(|n| n as u32 * 17));

        let data = create_positions_data(&values)?;
        let mut header = data.as_slice();
        assert_eq!(VInt::deserialize(&mut header)?.0, u64::MAX);
        let blocks = VInt::deserialize(&mut header)?.0 as usize;
        assert_eq!(blocks, 34);
        let counts = &header[blocks..blocks * 2];
        assert!(counts.iter().any(|&count| count > 0));
        assert!(counts.iter().all(|&count| count <= 7));

        let mut reader = PositionReader::open(data)?;
        let mut clone = reader.clone();
        for &(offset, len) in &[
            (0, 128),
            (120, 24),
            (3072, 300),
            (128, 8),
            (4000, 200),
            (255, 257),
            (4, 3),
            (values.len() - 37, 37),
        ] {
            let mut output = vec![0; len];
            reader.read(offset as u64, &mut output);
            assert_eq!(output, values[offset..offset + len]);
            clone.read(offset as u64, &mut output);
            assert_eq!(output, values[offset..offset + len]);
        }
        Ok(())
    }

    #[test]
    fn test_legacy_position_term_remains_readable() -> crate::Result<()> {
        let values: Vec<u32> = (0..300).map(|n| (n * 23) % 1024).collect();
        let mut old = Vec::new();
        VInt(2).serialize(&mut old)?;
        let mut encoder = BlockEncoder::new();
        let mut payload = Vec::new();
        for block in values[..256].chunks_exact(128) {
            let (width, bytes) = encoder.compress_block_unsorted(block, false);
            old.push(width);
            payload.extend_from_slice(bytes);
        }
        old.extend_from_slice(&payload);
        for &value in &values[256..] {
            VInt(value as u64).serialize(&mut old)?;
        }
        let mut reader = PositionReader::open(OwnedBytes::new(old))?;
        for &(offset, len) in &[(0, 128), (123, 140), (280, 20), (5, 9), (253, 47)] {
            let mut output = vec![0; len];
            reader.read(offset as u64, &mut output);
            assert_eq!(output, values[offset..offset + len]);
        }
        Ok(())
    }
}
