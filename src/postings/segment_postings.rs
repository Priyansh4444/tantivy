use common::{HasLen, TinySet};

use crate::docset::DocSet;
use crate::fastfield::AliveBitSet;
use crate::positions::PositionReader;
use crate::postings::bitset_fill::or_range_into_tinysets;
use crate::postings::compression::{bitset_base_doc, dense_block_size, COMPRESSION_BLOCK_SIZE};
use crate::postings::{BlockInfo, BlockSegmentPostings, Postings};
use crate::{DocId, TERMINATED};

/// `SegmentPostings` represents the inverted list or postings associated with
/// a term in a `Segment`.
///
/// As we iterate through the `SegmentPostings`, the frequencies are optionally decoded.
/// Positions on the other hand, are optionally entirely decoded upfront.
#[derive(Clone)]
pub struct SegmentPostings {
    pub(crate) block_cursor: BlockSegmentPostings,
    cur: usize,
    position_reader: Option<PositionReader>,
    // (position offset of the block, cursor within it, sum of preceding frequencies).
    position_prefix: Option<(u64, usize, u32)>,
}

impl SegmentPostings {
    fn current_position_read_offset(&mut self) -> u64 {
        let block_offset = self.block_cursor.position_offset();
        let (start, prefix) = match self.position_prefix {
            Some((cached_block, cached_cur, cached_prefix))
                if cached_block == block_offset && cached_cur <= self.cur =>
            {
                (cached_cur, cached_prefix)
            }
            _ => (0, 0),
        };
        let prefix = prefix
            + self.block_cursor.freqs()[start..self.cur]
                .iter()
                .sum::<u32>();
        self.position_prefix = Some((block_offset, self.cur, prefix));
        block_offset + prefix as u64
    }

    /// Returns an empty segment postings object
    pub fn empty() -> Self {
        SegmentPostings {
            block_cursor: BlockSegmentPostings::empty(),
            cur: 0,
            position_reader: None,
            position_prefix: None,
        }
    }

    /// Compute the number of non-deleted documents.
    ///
    /// This method will clone and scan through the posting lists.
    /// (this is a rather expensive operation).
    pub fn doc_freq_given_deletes(&self, alive_bitset: &AliveBitSet) -> u32 {
        let mut docset = self.clone();
        let mut doc_freq = 0;
        loop {
            let doc = docset.doc();
            if doc == TERMINATED {
                return doc_freq;
            }
            if alive_bitset.is_alive(doc) {
                doc_freq += 1u32;
            }
            docset.advance();
        }
    }

    /// Returns the overall number of documents in the block postings.
    /// It does not take in account whether documents are deleted or not.
    pub fn doc_freq(&self) -> u32 {
        self.block_cursor.doc_freq()
    }

    /// Creates a segment postings object with the given documents
    /// and no frequency encoded.
    ///
    /// This method is mostly useful for unit tests.
    ///
    /// It serializes the doc ids using tantivy's codec
    /// and returns a `SegmentPostings` object that embeds a
    /// buffer with the serialized data.
    #[cfg(test)]
    pub fn create_from_docs(docs: &[u32]) -> SegmentPostings {
        use crate::directory::FileSlice;
        use crate::postings::serializer::PostingsSerializer;
        use crate::schema::IndexRecordOption;
        let mut buffer = Vec::new();
        {
            let mut postings_serializer =
                PostingsSerializer::new(0.0, IndexRecordOption::Basic, None);
            postings_serializer.new_term(docs.len() as u32, false);
            for &doc in docs {
                postings_serializer.write_doc(doc, 1u32);
            }
            postings_serializer
                .close_term(docs.len() as u32, &mut buffer)
                .expect("In memory Serialization should never fail.");
        }
        let block_segment_postings = BlockSegmentPostings::open(
            docs.len() as u32,
            FileSlice::from(buffer),
            IndexRecordOption::Basic,
            IndexRecordOption::Basic,
        )
        .unwrap();
        SegmentPostings::from_block_postings(block_segment_postings, None)
    }

    /// Helper functions to create `SegmentPostings` for tests.
    #[cfg(test)]
    pub fn create_from_docs_and_tfs(
        doc_and_tfs: &[(u32, u32)],
        fieldnorms: Option<&[u32]>,
    ) -> SegmentPostings {
        use crate::directory::FileSlice;
        use crate::fieldnorm::FieldNormReader;
        use crate::postings::serializer::PostingsSerializer;
        use crate::schema::IndexRecordOption;
        use crate::Score;
        let mut buffer: Vec<u8> = Vec::new();
        let fieldnorm_reader = fieldnorms.map(FieldNormReader::for_test);
        let average_field_norm = fieldnorms
            .map(|fieldnorms| {
                if fieldnorms.is_empty() {
                    return 0.0;
                }
                let total_num_tokens: u64 = fieldnorms
                    .iter()
                    .map(|&fieldnorm| fieldnorm as u64)
                    .sum::<u64>();
                total_num_tokens as Score / fieldnorms.len() as Score
            })
            .unwrap_or(0.0);
        let mut postings_serializer = PostingsSerializer::new(
            average_field_norm,
            IndexRecordOption::WithFreqs,
            fieldnorm_reader,
        );
        postings_serializer.new_term(doc_and_tfs.len() as u32, true);
        for &(doc, tf) in doc_and_tfs {
            postings_serializer.write_doc(doc, tf);
        }
        postings_serializer
            .close_term(doc_and_tfs.len() as u32, &mut buffer)
            .unwrap();
        let block_segment_postings = BlockSegmentPostings::open(
            doc_and_tfs.len() as u32,
            FileSlice::from(buffer),
            IndexRecordOption::WithFreqs,
            IndexRecordOption::WithFreqs,
        )
        .unwrap();
        SegmentPostings::from_block_postings(block_segment_postings, None)
    }

    /// Reads a Segment postings from an &[u8]
    ///
    /// * `len` - number of document in the posting lists.
    /// * `data` - data array. The complete data is not necessarily used.
    /// * `freq_handler` - the freq handler is in charge of decoding frequencies and/or positions
    pub(crate) fn from_block_postings(
        segment_block_postings: BlockSegmentPostings,
        position_reader: Option<PositionReader>,
    ) -> SegmentPostings {
        SegmentPostings {
            block_cursor: segment_block_postings,
            cur: 0, // cursor within the block
            position_reader,
            position_prefix: None,
        }
    }
}

impl DocSet for SegmentPostings {
    // goes to the next element.
    // next needs to be called a first time to point to the correct element.
    #[inline]
    fn advance(&mut self) -> DocId {
        debug_assert!(self.block_cursor.block_is_loaded());
        if self.cur == COMPRESSION_BLOCK_SIZE - 1 {
            self.cur = 0;
            self.block_cursor.advance();
        } else {
            self.cur += 1;
        }
        self.doc()
    }

    #[inline]
    fn seek(&mut self, target: DocId) -> DocId {
        debug_assert!(self.doc() <= target);
        if self.doc() >= target {
            return self.doc();
        }

        // As an optimization, if the block is already loaded, we can
        // cheaply check the next doc.
        self.cur = (self.cur + 1).min(COMPRESSION_BLOCK_SIZE - 1);
        if self.doc() >= target {
            return self.doc();
        }

        // Delegate block-local search to BlockSegmentPostings::seek, which returns
        // the in-block index of the first doc >= target.
        self.cur = self.block_cursor.seek(target);
        let doc = self.doc();
        debug_assert!(doc >= target);
        doc
    }

    /// Return the current document's `DocId`.
    #[inline]
    fn doc(&self) -> DocId {
        self.block_cursor.doc(self.cur)
    }

    fn size_hint(&self) -> u32 {
        self.len() as u32
    }

    fn fill_bitset_window(&mut self, min_doc: DocId, mask: &mut [TinySet]) -> DocId {
        self.fill_bitset_window_impl(min_doc, mask, false)
    }
}

impl SegmentPostings {
    pub(crate) fn fill_bitset_window_impl(
        &mut self,
        min_doc: DocId,
        mask: &mut [TinySet],
        use_lanes: bool,
    ) -> DocId {
        if mask.is_empty() {
            return self.doc();
        }
        if self.doc() < min_doc {
            self.seek(min_doc.min(TERMINATED));
        }
        let horizon = min_doc
            .saturating_add(mask.len() as u32 * 64)
            .min(TERMINATED);
        let mut alternate_mask: Option<[TinySet; 64]> = None;
        let next_doc = 'fill: loop {
            if !self.block_cursor.block_is_loaded() {
                if let Some(next) = self.try_or_unloaded_dense_block(min_doc, horizon, mask) {
                    if next == TERMINATED {
                        break TERMINATED;
                    }
                    continue;
                }
                self.block_cursor.load_block();
                self.cur = 0;
            }

            let doc = self.doc();
            if doc >= horizon {
                break doc;
            }

            match self.block_cursor.skip_reader().block_info() {
                BlockInfo::Dense { num_longs, .. } => {
                    let last = self.block_cursor.skip_reader().last_doc_in_block();
                    let base =
                        bitset_base_doc(self.block_cursor.skip_reader().last_doc_in_previous_block);
                    let to = last.saturating_add(1).min(horizon);
                    if to > doc {
                        let offset = self.block_cursor.skip_reader().byte_offset();
                        let nbytes = dense_block_size(num_longs);
                        let src = &self.block_cursor.postings_bytes()[offset..offset + nbytes];
                        or_range_into_tinysets(src, doc - base, mask, doc - min_doc, to - doc);
                    }
                    if last < horizon {
                        self.block_cursor.advance_skip_only();
                        self.cur = 0;
                        continue;
                    }
                    self.cur = self.block_cursor.seek_within_loaded_block(horizon);
                    break self.doc();
                }
                BlockInfo::BitPacked { .. } | BlockInfo::VInt { .. } => {
                    let docs = self.block_cursor.docs();
                    let len = self.block_cursor.block_len();
                    let mut i = self.cur;
                    if docs[len - 1] - docs[i] >= 256 {
                        if use_lanes && mask.len() == 64 {
                            // Consecutive postings often hit the same mask word.
                            // Alternate between independent lanes to avoid a chain
                            // of dependent memory ORs, then merge once per window.
                            let alternate =
                                alternate_mask.get_or_insert_with(|| [TinySet::EMPTY; 64]);
                            while i + 2 <= len && docs[i + 1] < horizon {
                                let left = docs[i] - min_doc;
                                let right = docs[i + 1] - min_doc;
                                mask[(left / 64) as usize].insert_mut(left % 64);
                                alternate[(right / 64) as usize].insert_mut(right % 64);
                                i += 2;
                            }
                        }
                        while i < len {
                            let d = docs[i];
                            if d >= horizon {
                                self.cur = i;
                                break 'fill d;
                            }
                            let delta = d - min_doc;
                            mask[(delta / 64) as usize].insert_mut(delta % 64);
                            i += 1;
                        }
                        self.block_cursor.advance_skip_only();
                        self.cur = 0;
                        continue;
                    }
                    let mut bucket = ((docs[i] - min_doc) / 64) as usize;
                    let mut bits = 0u64;
                    while i < len {
                        let d = docs[i];
                        if d >= horizon {
                            break;
                        }
                        let delta = d - min_doc;
                        let next_bucket = (delta / 64) as usize;
                        if next_bucket != bucket {
                            mask[bucket].union_mut(TinySet::from_bits(bits));
                            bucket = next_bucket;
                            bits = 0;
                        }
                        bits |= 1u64 << (delta % 64);
                        i += 1;
                    }
                    mask[bucket].union_mut(TinySet::from_bits(bits));
                    if i < len {
                        self.cur = i;
                        break docs[i];
                    }
                    self.block_cursor.advance_skip_only();
                    self.cur = 0;
                }
            }
        };
        if let Some(alternate) = &alternate_mask {
            for (word, &other) in mask.iter_mut().zip(alternate) {
                word.union_mut(other);
            }
        }
        next_doc
    }

    /// Membership probe for monotonically increasing COUNT candidates. The caller owns
    /// this cursor separately from the scorer, since a dense block can stay undecoded.
    pub(crate) fn contains_doc_for_count(&mut self, target: DocId) -> bool {
        if target == TERMINATED {
            return false;
        }
        self.block_cursor.seek_block(target);
        let skip = self.block_cursor.skip_reader();
        if let BlockInfo::Dense { num_longs, .. } = skip.block_info() {
            let base = bitset_base_doc(skip.last_doc_in_previous_block);
            let Some(bit) = target.checked_sub(base) else {
                return false;
            };
            if bit >= num_longs as u32 * 64 {
                return false;
            }
            let bytes = self.block_cursor.postings_bytes();
            return bytes[skip.byte_offset() + (bit / 8) as usize] & (1 << (bit % 8)) != 0;
        }
        self.block_cursor.load_block();
        let idx = self.block_cursor.seek_within_loaded_block(target);
        self.block_cursor.doc(idx) == target
    }

    /// If the current (unloaded) block is dense and lies entirely inside
    /// `[min_doc, horizon)`, OR it into `mask` and skip decode. Returns
    /// `Some(TERMINATED)` when postings are exhausted, `Some(0)` when the
    /// block was consumed, `None` when the block must be decoded.
    fn try_or_unloaded_dense_block(
        &mut self,
        min_doc: DocId,
        horizon: DocId,
        mask: &mut [TinySet],
    ) -> Option<DocId> {
        let skip = self.block_cursor.skip_reader();
        if !skip.has_remaining_docs() {
            // Leave the decoder on a TERMINATED-padded empty block so
            // `doc()` matches the `TERMINATED` we return. Union refill
            // reads `doc()` after fill_bitset_window.
            self.block_cursor.load_block();
            self.cur = 0;
            return Some(TERMINATED);
        }
        let BlockInfo::Dense { num_longs, .. } = skip.block_info() else {
            return None;
        };
        let last = skip.last_doc_in_block();
        if last >= horizon {
            return None;
        }
        let base = bitset_base_doc(skip.last_doc_in_previous_block);
        let from = min_doc.max(base);
        let to = last.saturating_add(1);
        if to > from {
            let offset = skip.byte_offset();
            let nbytes = dense_block_size(num_longs);
            let src = &self.block_cursor.postings_bytes()[offset..offset + nbytes];
            or_range_into_tinysets(src, from - base, mask, from - min_doc, to - from);
        }
        self.block_cursor.advance_skip_only();
        self.cur = 0;
        Some(0)
    }
}

impl HasLen for SegmentPostings {
    fn len(&self) -> usize {
        self.block_cursor.doc_freq() as usize
    }
}

impl Postings for SegmentPostings {
    fn position_cursor(&mut self, offset: u32) -> Option<super::postings::PositionCursor<'_>> {
        if self.position_reader.is_none() {
            return None;
        }
        let read_offset = self.current_position_read_offset();
        let remaining = self.term_freq();
        Some(super::postings::PositionCursor::new(
            self.position_reader.as_mut().unwrap(),
            read_offset,
            remaining,
            offset,
        ))
    }

    /// Returns the frequency associated with the current document.
    /// If the schema is set up so that no frequency have been encoded,
    /// this method should always return 1.
    ///
    /// # Panics
    ///
    /// Will panics if called without having called advance before.
    fn term_freq(&self) -> u32 {
        debug_assert!(
            // Here we do not use the len of `freqs()`
            // because it is actually ok to request for the freq of doc
            // even if no frequency were encoded for the field.
            //
            // In that case we hit the block just as if the frequency had been
            // decoded. The block is simply prefilled by the value 1.
            self.cur < COMPRESSION_BLOCK_SIZE,
            "Have you forgotten to call `.advance()` at least once before calling `.term_freq()`."
        );
        self.block_cursor.freq(self.cur)
    }

    fn append_positions_with_offset(&mut self, offset: u32, output: &mut Vec<u32>) {
        let term_freq = self.term_freq();
        let prev_len = output.len();
        if self.position_reader.is_some() {
            debug_assert!(
                !self.block_cursor.freqs().is_empty(),
                "No positions available"
            );
            let read_offset = self.current_position_read_offset();
            // TODO: instead of zeroing the output, we could use MaybeUninit or similar.
            output.resize(prev_len + term_freq as usize, 0u32);
            self.position_reader
                .as_mut()
                .unwrap()
                .read(read_offset, &mut output[prev_len..]);
            let mut cum = offset;
            for output_mut in output[prev_len..].iter_mut() {
                cum += *output_mut;
                *output_mut = cum;
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use common::HasLen;

    use super::SegmentPostings;
    use crate::docset::{DocSet, TERMINATED};
    use crate::fastfield::AliveBitSet;
    use crate::postings::postings::Postings;
    use crate::schema::{IndexRecordOption, Schema, Term, TEXT};
    use crate::{doc, DocId, Index};

    #[test]
    fn test_empty_segment_postings() {
        let mut postings = SegmentPostings::empty();
        assert_eq!(postings.advance(), TERMINATED);
        assert_eq!(postings.advance(), TERMINATED);
        assert_eq!(postings.len(), 0);
    }

    #[test]
    fn test_empty_postings_doc_returns_terminated() {
        let mut postings = SegmentPostings::empty();
        assert_eq!(postings.doc(), TERMINATED);
        assert_eq!(postings.advance(), TERMINATED);
    }

    #[test]
    fn test_empty_postings_doc_term_freq_returns_0() {
        let postings = SegmentPostings::empty();
        assert_eq!(postings.term_freq(), 1);
    }

    #[test]
    fn test_doc_freq() {
        let docs = SegmentPostings::create_from_docs(&[0, 2, 10]);
        assert_eq!(docs.doc_freq(), 3);
        let alive_bitset = AliveBitSet::for_test_from_deleted_docs(&[2], 12);
        assert_eq!(docs.doc_freq_given_deletes(&alive_bitset), 2);
        let all_deleted =
            AliveBitSet::for_test_from_deleted_docs(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11], 12);
        assert_eq!(docs.doc_freq_given_deletes(&all_deleted), 0);
    }

    #[test]
    fn position_prefix_tracks_repeated_reads_seeks_blocks_and_clones() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let field = schema_builder.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema_builder.build());
        let mut writer = index.writer_for_tests()?;
        let texts = ["x", "x z x", "z x z x z x", "z z x x"];
        let expected = [&[0][..], &[0, 2], &[1, 3, 5], &[2, 3]];
        for doc_id in 0..300 {
            writer.add_document(doc!(field => texts[doc_id % 4]))?;
        }
        writer.commit()?;

        let searcher = index.reader()?.searcher();
        let term = Term::from_field_text(field, "x");
        let mut postings = searcher
            .segment_reader(0)
            .inverted_index(field)?
            .read_postings(&term, IndexRecordOption::WithFreqsAndPositions)?
            .unwrap();
        let check = |postings: &mut SegmentPostings, doc_id: u32| {
            assert_eq!(postings.doc(), doc_id);
            let mut positions = Vec::new();
            postings.positions(&mut positions);
            assert_eq!(positions, expected[doc_id as usize % 4]);
            // A second read of the same cursor and an append with a nonzero offset
            // must use the same frequency prefix without changing the result.
            postings.positions(&mut positions);
            assert_eq!(positions, expected[doc_id as usize % 4]);
            let streamed: Vec<u32> = postings.position_cursor(10).unwrap().collect();
            let expected_streamed: Vec<u32> = expected[doc_id as usize % 4]
                .iter()
                .map(|pos| pos + 10)
                .collect();
            assert_eq!(streamed, expected_streamed);
            postings.append_positions_with_offset(10, &mut positions);
            assert_eq!(
                &positions[expected[doc_id as usize % 4].len()..],
                expected[doc_id as usize % 4]
                    .iter()
                    .map(|pos| pos + 10)
                    .collect::<Vec<_>>()
            );
        };

        check(&mut postings, 0);
        assert_eq!(postings.advance(), 1);
        check(&mut postings, 1);
        assert_eq!(postings.seek(2), 2);
        check(&mut postings, 2);
        let mut clone = postings.clone();
        assert_eq!(clone.seek(129), 129);
        check(&mut clone, 129);
        check(&mut postings, 2);
        for &target in &[127, 128, 129, 255, 256, 299] {
            assert_eq!(postings.seek(target), target);
            check(&mut postings, target);
            if target == 127 || target == 255 {
                let mut at_boundary = postings.clone();
                assert_eq!(at_boundary.advance(), target + 1);
                check(&mut at_boundary, target + 1);
            }
        }
        Ok(())
    }

    fn collect_windows(docs: &[DocId]) -> Vec<(DocId, Vec<DocId>, DocId)> {
        let mut postings = SegmentPostings::create_from_docs(docs);
        let mut windows = Vec::new();
        let mut min_doc = postings.doc();
        while min_doc != TERMINATED {
            let mut mask = [common::TinySet::empty(); crate::docset::BLOCK_NUM_TINYBITSETS];
            let next = postings.fill_bitset_window(min_doc, &mut mask);
            let mut hits = Vec::new();
            for (i, tiny) in mask.iter().enumerate() {
                for bit in *tiny {
                    hits.push(min_doc + (i as u32) * 64 + bit);
                }
            }
            windows.push((min_doc, hits, next));
            min_doc = next;
        }
        windows
    }

    fn expected_windows(docs: &[DocId]) -> Vec<(DocId, Vec<DocId>, DocId)> {
        let window = crate::docset::BLOCK_WINDOW;
        let mut out = Vec::new();
        if docs.is_empty() {
            return out;
        }
        let mut min_doc = docs[0];
        loop {
            let horizon = min_doc.saturating_add(window);
            let hits: Vec<DocId> = docs
                .iter()
                .copied()
                .filter(|&d| d >= min_doc && d < horizon)
                .collect();
            let next = docs
                .iter()
                .copied()
                .find(|&d| d >= horizon)
                .unwrap_or(TERMINATED);
            out.push((min_doc, hits, next));
            if next == TERMINATED {
                break;
            }
            min_doc = next;
        }
        out
    }

    #[test]
    fn fill_bitset_window_dense_gapped() {
        // 90% dense with every-10th gap: forces dense bitset blocks.
        let docs: Vec<DocId> = (0..5_000u32).filter(|i| i % 10 != 0).collect();
        assert_eq!(collect_windows(&docs), expected_windows(&docs));
    }

    #[test]
    fn fill_bitset_window_sparse_for() {
        let docs: Vec<DocId> = (0..2_000u32).map(|i| i * 17).collect();
        assert_eq!(collect_windows(&docs), expected_windows(&docs));
    }

    #[test]
    fn fill_bitset_window_two_lanes_matches_bit_walk() {
        for base in [0, TERMINATED - 40_000] {
            for gap in [3, 17, 61] {
                let mut docs: Vec<DocId> = (0..600).map(|i| base + i * gap).collect();
                if base != 0 {
                    docs.push(TERMINATED - 1);
                }
                for width in [1usize, 2, 7, 64] {
                    let mut postings = SegmentPostings::create_from_docs(&docs);
                    while postings.doc() != TERMINATED {
                        let min_doc = postings.doc().saturating_add(1);
                        let horizon = min_doc.saturating_add(width as u32 * 64).min(TERMINATED);
                        let mut mask =
                            vec![common::TinySet::from_bits(0x9249_2492_4924_9249); width];
                        let mut expected = mask.clone();
                        for &doc in docs.iter().filter(|&&doc| doc >= min_doc && doc < horizon) {
                            let delta = doc - min_doc;
                            expected[(delta / 64) as usize].insert_mut(delta % 64);
                        }
                        let next = docs
                            .iter()
                            .copied()
                            .find(|&doc| doc >= horizon)
                            .unwrap_or(TERMINATED);
                        assert_eq!(
                            postings.fill_bitset_window_impl(min_doc, &mut mask, true),
                            next
                        );
                        assert_eq!(postings.doc(), next);
                        assert_eq!(mask, expected);
                    }
                    for min_doc in [TERMINATED, TERMINATED + 1, u32::MAX] {
                        let mut mask =
                            vec![common::TinySet::from_bits(0x9249_2492_4924_9249); width];
                        let expected = mask.clone();
                        assert_eq!(
                            postings.fill_bitset_window_impl(min_doc, &mut mask, true),
                            TERMINATED
                        );
                        assert_eq!(mask, expected);
                    }
                }
            }
        }
    }

    #[test]
    fn fill_bitset_window_sparse_near_terminated_preserves_masks() {
        let docs: Vec<DocId> = (0..512u32)
            .map(|i| TERMINATED - 12_000 + i * 23)
            .chain([TERMINATED - 1])
            .collect();
        for width in [1usize, 2, 7, 64] {
            let mut postings = SegmentPostings::create_from_docs(&docs);
            while postings.doc() != TERMINATED {
                let min_doc = postings.doc().saturating_add(1);
                let horizon = min_doc.saturating_add(width as u32 * 64);
                let mut mask = vec![common::TinySet::from_bits(0x9249_2492_4924_9249); width];
                let mut expected = mask.clone();
                for &doc in docs.iter().filter(|&&doc| doc >= min_doc && doc < horizon) {
                    let delta = doc - min_doc;
                    expected[(delta / 64) as usize].insert_mut(delta % 64);
                }
                let next = docs
                    .iter()
                    .copied()
                    .find(|&doc| doc >= horizon)
                    .unwrap_or(TERMINATED);
                assert_eq!(postings.fill_bitset_window(min_doc, &mut mask), next);
                assert_eq!(postings.doc(), next);
                assert_eq!(mask, expected);
            }
            for min_doc in [TERMINATED, TERMINATED + 1, u32::MAX] {
                let mut mask = vec![common::TinySet::from_bits(0x9249_2492_4924_9249); width];
                let expected = mask.clone();
                assert_eq!(postings.fill_bitset_window(min_doc, &mut mask), TERMINATED);
                assert_eq!(mask, expected);
            }
        }
    }

    #[test]
    fn fill_bitset_window_burst() {
        let docs: Vec<DocId> = (0..4_000u32).filter(|i| (i % 192) < 128).collect();
        assert_eq!(collect_windows(&docs), expected_windows(&docs));
    }

    #[test]
    fn fill_bitset_window_leaves_terminated() {
        let docs: Vec<DocId> = (0..5_000u32).filter(|i| i % 10 != 0).collect();
        let mut postings = SegmentPostings::create_from_docs(&docs);
        let mut min_doc = postings.doc();
        while min_doc != TERMINATED {
            let mut mask = [common::TinySet::empty(); 64];
            min_doc = postings.fill_bitset_window(min_doc, &mut mask);
        }
        assert_eq!(postings.doc(), TERMINATED);
    }

    #[test]
    fn fill_bitset_window_randomized_matches_bit_walk() {
        fn next(state: &mut u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state
        }

        let mut state = 0x796a_4e31_a2d5_8b0fu64;
        for case in 0..12 {
            let limit = if case == 0 { 120 } else { 4_500 };
            let docs: Vec<DocId> = (0..limit)
                .filter(|&doc| {
                    let threshold = match case % 4 {
                        0 => 1_024,
                        1 => 920,
                        2 => 110,
                        _ if doc % 512 < 256 => 950,
                        _ => 45,
                    };
                    next(&mut state) % 1_024 < threshold
                })
                .collect();
            assert!(!docs.is_empty());

            for width in [1usize, 2, 7, 64] {
                let mut postings = SegmentPostings::create_from_docs(&docs);
                while postings.doc() != TERMINATED {
                    let min_doc = postings.doc() + (next(&mut state) % 5) as u32;
                    let horizon = min_doc + width as u32 * 64;
                    let mut mask = vec![common::TinySet::EMPTY; width];
                    for tinyset in &mut mask {
                        tinyset.insert_mut((next(&mut state) % 64) as u32);
                    }
                    let mut expected = mask.clone();
                    for &doc in docs.iter().filter(|&&doc| doc >= min_doc && doc < horizon) {
                        let delta = doc - min_doc;
                        expected[(delta / 64) as usize].insert_mut(delta % 64);
                    }
                    let expected_next = docs
                        .iter()
                        .copied()
                        .find(|&doc| doc >= horizon)
                        .unwrap_or(TERMINATED);
                    assert_eq!(
                        postings.fill_bitset_window(min_doc, &mut mask),
                        expected_next,
                        "case={case} width={width} min_doc={min_doc}"
                    );
                    assert_eq!(postings.doc(), expected_next);
                    assert_eq!(
                        mask, expected,
                        "case={case} width={width} min_doc={min_doc}"
                    );
                }
            }
        }
    }

    #[test]
    fn count_probe_matches_bit_walk_across_block_boundaries() {
        fn next(state: &mut u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state
        }

        let mut state = 0x127d_81a0_f4e8_559bu64;
        for case in 0..8 {
            let docs: Vec<DocId> = (0..9_000)
                .filter(|&doc| {
                    let chance = match case % 4 {
                        0 => 985,
                        1 => 750,
                        2 => 125,
                        _ if doc % 1_024 < 512 => 980,
                        _ => 80,
                    };
                    next(&mut state) % 1_000 < chance
                })
                .collect();
            let mut probe = SegmentPostings::create_from_docs(&docs);
            let mut saw_dense = false;
            for target in 0..9_100 {
                assert_eq!(
                    probe.contains_doc_for_count(target),
                    docs.binary_search(&target).is_ok(),
                    "case={case} target={target}"
                );
                saw_dense |= matches!(
                    probe.block_cursor.skip_reader().block_info(),
                    crate::postings::BlockInfo::Dense { .. }
                );
            }
            if case % 4 == 1 {
                assert!(saw_dense, "case={case} must test encoded dense blocks");
            }
            assert!(!probe.contains_doc_for_count(TERMINATED));
        }

        // A dense block after a large doc-id jump exercises the previous-block
        // base, and the final partial block exercises VInt and termination.
        let docs: Vec<DocId> = (0..128)
            .chain((100_000..100_200).filter(|doc| doc % 13 != 0))
            .collect();
        let mut probe = SegmentPostings::create_from_docs(&docs);
        for target in [
            0, 1, 127, 128, 99_999, 100_000, 100_127, 100_199, 100_200, TERMINATED,
        ] {
            assert_eq!(
                probe.contains_doc_for_count(target),
                docs.binary_search(&target).is_ok(),
                "target={target}"
            );
        }
    }
}
