use crate::docset::DocSet;
use crate::positions::PositionReader;

/// Reads positions of one term in the current document, accumulating encoded deltas.
pub struct PositionCursor<'a> {
    reader: &'a mut PositionReader,
    read_offset: u64,
    remaining: u32,
    position: u32,
}

impl<'a> PositionCursor<'a> {
    pub(crate) fn new(
        reader: &'a mut PositionReader,
        read_offset: u64,
        remaining: u32,
        offset: u32,
    ) -> Self {
        Self {
            reader,
            read_offset,
            remaining,
            position: offset,
        }
    }
}

impl Iterator for PositionCursor<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        if self.remaining == 0 {
            return None;
        }
        let mut delta = [0u32; 1];
        self.reader.read(self.read_offset, &mut delta);
        self.read_offset += 1;
        self.remaining -= 1;
        self.position += delta[0];
        Some(self.position)
    }
}

/// Postings (also called inverted list)
///
/// For a given term, it is the list of doc ids of the doc
/// containing the term. Optionally, for each document,
/// it may also give access to the term frequency
/// as well as the list of term positions.
///
/// Its main implementation is `SegmentPostings`,
/// but other implementations mocking `SegmentPostings` exist,
/// for merging segments or for testing.
pub trait Postings: DocSet + 'static {
    /// The number of times the term appears in the document.
    fn term_freq(&self) -> u32;

    /// Returns a lazy cursor when this postings implementation supports it.
    fn position_cursor(&mut self, _offset: u32) -> Option<PositionCursor<'_>> {
        None
    }

    /// Returns the positions offsetted with a given value.
    /// It is not necessary to clear the `output` before calling this method.
    /// The output vector will be resized to the `term_freq`.
    fn positions_with_offset(&mut self, offset: u32, output: &mut Vec<u32>) {
        output.clear();
        self.append_positions_with_offset(offset, output);
    }

    /// Returns the positions offsetted with a given value.
    /// Data will be appended to the output.
    fn append_positions_with_offset(&mut self, offset: u32, output: &mut Vec<u32>);

    /// Returns the positions of the term in the given document.
    /// The output vector will be resized to the `term_freq`.
    fn positions(&mut self, output: &mut Vec<u32>) {
        self.positions_with_offset(0u32, output);
    }
}

impl Postings for Box<dyn Postings> {
    fn term_freq(&self) -> u32 {
        (**self).term_freq()
    }

    fn position_cursor(&mut self, offset: u32) -> Option<PositionCursor<'_>> {
        (**self).position_cursor(offset)
    }

    fn append_positions_with_offset(&mut self, offset: u32, output: &mut Vec<u32>) {
        (**self).append_positions_with_offset(offset, output);
    }
}
