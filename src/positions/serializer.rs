use std::io::{self, Write};

use common::{BinarySerializable, CountingWriter, VInt};

use crate::positions::COMPRESSION_BLOCK_SIZE;
use crate::postings::compression::{BlockEncoder, VIntEncoder};

/// The PositionSerializer is in charge of serializing all of the positions
/// of all of the terms of a given field.
///
/// It is valid to call write_position_delta more than once per term.
pub struct PositionSerializer<W: io::Write> {
    block_encoder: BlockEncoder,
    positions_wrt: CountingWriter<W>,
    positions_buffer: Vec<u8>,
    legacy_positions_buffer: Vec<u8>,
    block: Vec<u32>,
    bit_widths: Vec<u8>,
    legacy_bit_widths: Vec<u8>,
    exception_counts: Vec<u8>,
}

impl<W: io::Write> PositionSerializer<W> {
    /// Creates a new PositionSerializer writing into the given positions_wrt.
    pub fn new(positions_wrt: W) -> PositionSerializer<W> {
        PositionSerializer {
            block_encoder: BlockEncoder::new(),
            positions_wrt: CountingWriter::wrap(positions_wrt),
            positions_buffer: Vec::with_capacity(128_000),
            legacy_positions_buffer: Vec::with_capacity(128_000),
            block: Vec::with_capacity(128),
            bit_widths: Vec::new(),
            legacy_bit_widths: Vec::new(),
            exception_counts: Vec::new(),
        }
    }

    /// Returns the number of bytes written in the positions write object
    /// at this point.
    /// When called before writing the positions of a term, this value is used as
    /// start offset.
    /// When called after writing the positions of a term, this value is used as
    /// end offset.
    pub fn written_bytes(&self) -> u64 {
        self.positions_wrt.written_bytes()
    }

    fn remaining_block_len(&self) -> usize {
        COMPRESSION_BLOCK_SIZE - self.block.len()
    }

    /// Writes all of the given positions delta.
    pub fn write_positions_delta(&mut self, mut positions_delta: &[u32]) {
        while !positions_delta.is_empty() {
            let remaining_block_len = self.remaining_block_len();
            let num_to_write = remaining_block_len.min(positions_delta.len());
            self.block.extend(&positions_delta[..num_to_write]);
            positions_delta = &positions_delta[num_to_write..];
            if self.remaining_block_len() == 0 {
                self.flush_block();
            }
        }
    }

    fn flush_block(&mut self) {
        // encode the positions in the block
        if self.block.is_empty() {
            return;
        }
        if self.block.len() == COMPRESSION_BLOCK_SIZE {
            let (width, original) = self
                .block_encoder
                .compress_block_unsorted(&self.block, false);
            self.legacy_bit_widths.push(width);
            self.legacy_positions_buffer.extend_from_slice(original);
            let mut best_width = width;
            let mut best_count = 0u8;
            let mut best_size = width as usize * 16;
            let mut widths = [0u8; 33];
            for &value in &self.block {
                widths[(u32::BITS - value.leading_zeros()) as usize] += 1;
            }
            let mut exceptions = 0usize;
            for candidate in (width.saturating_sub(8)..width).rev() {
                exceptions += widths[(candidate + 1) as usize] as usize;
                if exceptions <= 7 {
                    let size = candidate as usize * 16 + exceptions * 2;
                    if size < best_size {
                        best_size = size;
                        best_width = candidate;
                        best_count = exceptions as u8;
                    }
                }
            }
            self.bit_widths.push(best_width);
            self.exception_counts.push(best_count);
            if best_count == 0 {
                self.positions_buffer.extend_from_slice(original);
            } else {
                let mask = (1u32 << best_width) - 1;
                let mut lows = [0u32; COMPRESSION_BLOCK_SIZE];
                for (&value, low) in self.block.iter().zip(lows.iter_mut()) {
                    *low = value & mask;
                }
                let encoded = self
                    .block_encoder
                    .compress_block_unsorted_at_width(&lows, best_width);
                self.positions_buffer.extend_from_slice(encoded);
                for (i, &value) in self.block.iter().enumerate() {
                    let high = value >> best_width;
                    if high != 0 {
                        self.positions_buffer.push(i as u8);
                        self.positions_buffer.push(high as u8);
                    }
                }
            }
        } else {
            debug_assert!(self.block.len() < COMPRESSION_BLOCK_SIZE);
            let block_vint_encoded = self.block_encoder.compress_vint_unsorted(&self.block[..]);
            self.positions_buffer.extend_from_slice(block_vint_encoded);
            self.legacy_positions_buffer
                .extend_from_slice(block_vint_encoded);
        }
        self.block.clear();
    }

    /// Close the positions for the current term.
    pub fn close_term(&mut self) -> io::Result<()> {
        self.flush_block();
        let marker_bytes = 10usize;
        let new_payload_bytes =
            self.positions_buffer.len() + self.exception_counts.len() + marker_bytes;
        let use_pfor = new_payload_bytes < self.legacy_positions_buffer.len();
        if use_pfor {
            VInt(u64::MAX).serialize(&mut self.positions_wrt)?;
        }
        VInt(self.bit_widths.len() as u64).serialize(&mut self.positions_wrt)?;
        if use_pfor {
            self.positions_wrt.write_all(&self.bit_widths[..])?;
            self.positions_wrt.write_all(&self.exception_counts[..])?;
            self.positions_wrt.write_all(&self.positions_buffer)?;
        } else {
            self.positions_wrt.write_all(&self.legacy_bit_widths)?;
            self.positions_wrt
                .write_all(&self.legacy_positions_buffer)?;
        }
        self.bit_widths.clear();
        self.legacy_bit_widths.clear();
        self.exception_counts.clear();
        self.positions_buffer.clear();
        self.legacy_positions_buffer.clear();
        Ok(())
    }

    /// Close the positions for this term and flushes the data.
    pub fn close(mut self) -> io::Result<()> {
        self.positions_wrt.flush()
    }
}
