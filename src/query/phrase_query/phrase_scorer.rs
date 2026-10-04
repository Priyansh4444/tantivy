use std::cmp::Ordering;

use super::sloppy_phrase_matcher::SloppyPhraseMatcher;
use crate::docset::{DocSet, SeekDangerResult, TERMINATED};
use crate::fieldnorm::FieldNormReader;
use crate::postings::{PositionCursor, Postings, SegmentPostings};
use crate::query::bm25::Bm25Weight;
use crate::query::{Intersection, Scorer};
use crate::{DocId, Score};

struct PostingsWithOffset<TPostings> {
    offset: u32,
    query_ordinal: usize,
    postings: TPostings,
}

impl<TPostings: Postings> PostingsWithOffset<TPostings> {
    pub fn new(
        segment_postings: TPostings,
        offset: u32,
        query_ordinal: usize,
    ) -> PostingsWithOffset<TPostings> {
        PostingsWithOffset {
            offset,
            query_ordinal,
            postings: segment_postings,
        }
    }

    pub fn positions(&mut self, output: &mut Vec<u32>) {
        self.postings.positions_with_offset(self.offset, output)
    }

    fn position_cursor(&mut self) -> Option<PositionCursor<'_>> {
        self.postings.position_cursor(self.offset)
    }

    fn term_freq(&self) -> u32 {
        self.postings.term_freq()
    }
}

impl<TPostings: Postings> DocSet for PostingsWithOffset<TPostings> {
    fn advance(&mut self) -> DocId {
        self.postings.advance()
    }

    fn seek(&mut self, target: DocId) -> DocId {
        self.postings.seek(target)
    }

    fn doc(&self) -> DocId {
        self.postings.doc()
    }

    fn size_hint(&self) -> u32 {
        self.postings.size_hint()
    }
}

pub struct PhraseScorer<TPostings: Postings> {
    intersection_docset: Intersection<PostingsWithOffset<TPostings>, PostingsWithOffset<TPostings>>,
    num_terms: usize,
    left_positions: Vec<u32>,
    right_positions: Vec<u32>,
    phrase_count: u32,
    fieldnorm_reader: FieldNormReader,
    similarity_weight_opt: Option<Bm25Weight>,
    slop: u32,
    sloppy_matcher: Option<Box<SloppyPhraseMatcher>>,
    phrase_frequency: f32,
    keep_positions_for_prefix: bool,
}

/// Returns true if and only if the two sorted arrays contain a common element
fn intersection_exists(left: &[u32], right: &[u32]) -> bool {
    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() && right_index < right.len() {
        let left_val = left[left_index];
        let right_val = right[right_index];
        match left_val.cmp(&right_val) {
            Ordering::Less => {
                left_index += 1;
            }
            Ordering::Equal => {
                return true;
            }
            Ordering::Greater => {
                right_index += 1;
            }
        }
    }
    false
}

pub(crate) fn intersection_count(left: &[u32], right: &[u32]) -> usize {
    let mut left_index = 0;
    let mut right_index = 0;
    let mut count = 0;
    while left_index < left.len() && right_index < right.len() {
        let left_val = left[left_index];
        let right_val = right[right_index];
        match left_val.cmp(&right_val) {
            Ordering::Less => {
                left_index += 1;
            }
            Ordering::Equal => {
                count += 1;
                left_index += 1;
                right_index += 1;
            }
            Ordering::Greater => {
                right_index += 1;
            }
        }
    }
    count
}

/// Intersect twos sorted arrays `left` and `right` and outputs the
/// resulting array in left.
///
/// Returns the length of the intersection
#[inline]
fn intersection(left: &mut Vec<u32>, right: &[u32]) {
    let mut left_index = 0;
    let mut right_index = 0;
    let mut count = 0;
    let left_len = left.len();
    let right_len = right.len();
    while left_index < left_len && right_index < right_len {
        let left_val = left[left_index];
        let right_val = right[right_index];
        match left_val.cmp(&right_val) {
            Ordering::Less => {
                left_index += 1;
            }
            Ordering::Equal => {
                left[count] = left_val;
                count += 1;
                left_index += 1;
                right_index += 1;
            }
            Ordering::Greater => {
                right_index += 1;
            }
        }
    }
    left.truncate(count);
}

impl<TPostings: Postings> PhraseScorer<TPostings> {
    // If similarity_weight is None, then scoring is disabled.
    /// Generic postings do not carry term identity. Repeated-term collision
    /// handling is supplied internally by `PhraseWeight` for plain phrases.
    pub fn new(
        term_postings: Vec<(usize, TPostings)>,
        similarity_weight_opt: Option<Bm25Weight>,
        fieldnorm_reader: FieldNormReader,
        slop: u32,
    ) -> PhraseScorer<TPostings> {
        Self::new_with_offset(
            term_postings,
            similarity_weight_opt,
            fieldnorm_reader,
            slop,
            0,
            false,
        )
    }

    pub(crate) fn new_with_offset(
        term_postings_with_offset: Vec<(usize, TPostings)>,
        similarity_weight_opt: Option<Bm25Weight>,
        fieldnorm_reader: FieldNormReader,
        slop: u32,
        offset: usize,
        keep_positions_for_prefix: bool,
    ) -> PhraseScorer<TPostings> {
        let (seek_result, mut scorer) = Self::new_danger(
            term_postings_with_offset,
            similarity_weight_opt,
            fieldnorm_reader,
            slop,
            offset,
            keep_positions_for_prefix,
            0,
        );
        if let SeekDangerResult::SeekLowerBound(target) = seek_result {
            if target < TERMINATED {
                scorer.seek(target);
            }
        }
        scorer
    }

    /// Creates a phrase scorer near `target` without scanning forward to the next phrase match.
    ///
    /// On a miss, the scorer follows the danger-state contract: it must only receive further
    /// `seek_danger` calls until one returns [`SeekDangerResult::Found`].
    pub(crate) fn new_danger(
        term_postings_with_offset: Vec<(usize, TPostings)>,
        similarity_weight_opt: Option<Bm25Weight>,
        fieldnorm_reader: FieldNormReader,
        slop: u32,
        offset: usize,
        keep_positions_for_prefix: bool,
        target: DocId,
    ) -> (SeekDangerResult, PhraseScorer<TPostings>) {
        Self::new_danger_with_repeat_groups(
            term_postings_with_offset,
            similarity_weight_opt,
            fieldnorm_reader,
            slop,
            offset,
            keep_positions_for_prefix,
            target,
            Vec::new(),
        )
    }

    pub(crate) fn new_danger_with_repeat_groups(
        mut term_postings_with_offset: Vec<(usize, TPostings)>,
        similarity_weight_opt: Option<Bm25Weight>,
        fieldnorm_reader: FieldNormReader,
        slop: u32,
        offset: usize,
        keep_positions_for_prefix: bool,
        target: DocId,
        repeat_groups: Vec<Vec<usize>>,
    ) -> (SeekDangerResult, PhraseScorer<TPostings>) {
        let sloppy_matcher = (slop > 0).then(|| {
            Box::new(SloppyPhraseMatcher::new(
                term_postings_with_offset
                    .iter()
                    .map(|(offset, _)| *offset as u32)
                    .collect(),
                repeat_groups,
                slop,
            ))
        });
        for (_, postings) in &mut term_postings_with_offset {
            if postings.doc() < target {
                // We do not optimize for that seek.
                // This would require a constructor Intersection::new that
                // accepts postings in the danger zone and we prefer to avoid that.
                postings.seek(target);
            }
        }
        let num_docs = fieldnorm_reader.num_docs();
        let max_offset = term_postings_with_offset
            .iter()
            .map(|&(offset, _)| offset)
            .max()
            .unwrap_or(0)
            + offset;
        let num_docsets = term_postings_with_offset.len();
        let postings_with_offsets = term_postings_with_offset
            .into_iter()
            .enumerate()
            .map(|(query_ordinal, (offset, postings))| {
                PostingsWithOffset::new(postings, (max_offset - offset) as u32, query_ordinal)
            })
            .collect::<Vec<_>>();
        let intersection_docset = Intersection::new(postings_with_offsets, num_docs);
        let mut scorer = PhraseScorer {
            intersection_docset,
            num_terms: num_docsets,
            left_positions: Vec::with_capacity(100),
            right_positions: Vec::with_capacity(100),
            phrase_count: 0u32,
            similarity_weight_opt,
            fieldnorm_reader,
            slop,
            sloppy_matcher,
            phrase_frequency: 0.0,
            keep_positions_for_prefix,
        };
        let doc = scorer.doc();
        debug_assert!(doc >= target);
        let seek_result = if doc >= TERMINATED {
            SeekDangerResult::SeekLowerBound(TERMINATED)
        } else if doc > target {
            SeekDangerResult::SeekLowerBound(doc)
        } else if scorer.phrase_match() {
            SeekDangerResult::Found
        } else {
            SeekDangerResult::SeekLowerBound(target + 1)
        };
        (seek_result, scorer)
    }

    pub fn phrase_count(&self) -> u32 {
        self.phrase_count
    }

    pub(crate) fn get_intersection(&mut self) -> &[u32] {
        intersection(&mut self.left_positions, &self.right_positions);
        &self.left_positions
    }

    pub(crate) fn phrase_frequency(&self) -> f32 {
        self.phrase_frequency
    }

    fn phrase_match(&mut self) -> bool {
        if let Some(matcher) = self.sloppy_matcher.as_mut() {
            for ordinal in 0..self.num_terms {
                let clause = self.intersection_docset.docset_mut_specialized(ordinal);
                clause
                    .postings
                    .positions(matcher.positions_mut(clause.query_ordinal));
            }
            self.phrase_count = 0;
            self.phrase_frequency = 0.0;
            if !matcher.reset_document() {
                return false;
            }
            if self.similarity_weight_opt.is_some() {
                let (count, frequency) = matcher.frequency();
                self.phrase_count = count;
                self.phrase_frequency = frequency;
                return count > 0;
            }
            return matcher.next_match_distance().is_some();
        }
        if self.similarity_weight_opt.is_some() {
            let count = self.compute_phrase_count();
            self.phrase_count = count;
            self.phrase_frequency = count as f32;
            count > 0u32
        } else {
            self.phrase_exists()
        }
    }

    fn phrase_exists(&mut self) -> bool {
        if self.slop == 0 && self.num_terms == 2 && !self.keep_positions_for_prefix {
            let (left, right) = self.intersection_docset.first_two_mut();
            if let (Some(mut left), Some(mut right)) =
                (left.position_cursor(), right.position_cursor())
            {
                let mut left_pos = left.next();
                let mut right_pos = right.next();
                while let (Some(l), Some(r)) = (left_pos, right_pos) {
                    match l.cmp(&r) {
                        Ordering::Less => left_pos = left.next(),
                        Ordering::Equal => return true,
                        Ordering::Greater => right_pos = right.next(),
                    }
                }
                return false;
            }
        }
        self.compute_phrase_match();
        intersection_exists(&self.left_positions, &self.right_positions)
    }

    fn compute_phrase_count(&mut self) -> u32 {
        self.compute_phrase_match();
        intersection_count(&self.left_positions, &self.right_positions) as u32
    }

    fn compute_phrase_match(&mut self) {
        self.intersection_docset
            .docset_mut_specialized(0)
            .positions(&mut self.left_positions);
        for ordinal in 1..self.num_terms - 1 {
            self.intersection_docset
                .docset_mut_specialized(ordinal)
                .positions(&mut self.right_positions);
            intersection(&mut self.left_positions, &self.right_positions);
            if self.left_positions.is_empty() {
                return;
            }
        }
        self.intersection_docset
            .docset_mut_specialized(self.num_terms - 1)
            .positions(&mut self.right_positions);
    }
}

impl PhraseScorer<SegmentPostings> {
    /// The phrase frequency is at most the frequency of any constituent term.
    /// Compute a block bound using the query's BM25 weight, as the stored block
    /// impact was chosen using segment-local statistics at indexing time.
    fn first_term_block_can_compete(&mut self, threshold: Score) -> (DocId, bool) {
        let cursor = &self
            .intersection_docset
            .docset_mut_specialized(0)
            .postings
            .block_cursor;
        let end = cursor.skip_reader().last_doc_in_block();
        let weight = self.similarity_weight_opt.as_ref().unwrap();
        let can_compete = cursor
            .docs()
            .iter()
            .zip(cursor.freqs())
            .any(|(&doc, &freq)| {
                weight.can_score_exceed(self.fieldnorm_reader.fieldnorm_id(doc), freq, threshold)
            });
        (end, can_compete)
    }

    /// Collect exact phrase matches while pruning uncompetitive blocks and
    /// avoiding position reads for uncompetitive documents.
    pub(crate) fn for_each_pruning_exact(
        &mut self,
        mut threshold: Score,
        callback: &mut dyn FnMut(DocId, Score) -> Score,
    ) {
        debug_assert_eq!(self.slop, 0);
        debug_assert!(self.similarity_weight_opt.is_some());

        // The constructor has already checked the first phrase match.
        let mut doc = self.doc();
        let mut block_end = 0;
        let mut block_can_compete = true;
        while doc != TERMINATED {
            let score = self.score();
            if score > threshold {
                threshold = callback(doc, score);
            }

            loop {
                // Scores are nonnegative. Until the collector has a positive
                // threshold, a block cannot be pruned, so avoid scanning it.
                if threshold > 0.0 && doc > block_end {
                    (block_end, block_can_compete) = self.first_term_block_can_compete(threshold);
                }
                doc = if threshold > 0.0 && block_end < TERMINATED && !block_can_compete {
                    self.intersection_docset.seek(block_end + 1)
                } else {
                    self.intersection_docset.advance()
                };
                if doc == TERMINATED {
                    return;
                }

                let mut max_phrase_freq = u32::MAX;
                for i in 0..self.num_terms {
                    let freq = self
                        .intersection_docset
                        .docset_mut_specialized(i)
                        .term_freq();
                    max_phrase_freq = max_phrase_freq.min(freq);
                }
                let fieldnorm_id = self.fieldnorm_reader.fieldnorm_id(doc);
                if !self
                    .similarity_weight_opt
                    .as_ref()
                    .unwrap()
                    .can_score_exceed(fieldnorm_id, max_phrase_freq, threshold)
                {
                    continue;
                }
                if self.phrase_match() {
                    break;
                }
            }
        }
    }
}

impl<TPostings: Postings> DocSet for PhraseScorer<TPostings> {
    fn advance(&mut self) -> DocId {
        loop {
            let doc = self.intersection_docset.advance();
            if doc == TERMINATED || self.phrase_match() {
                return doc;
            }
        }
    }

    fn seek(&mut self, target: DocId) -> DocId {
        debug_assert!(target >= self.doc());
        let doc = self.intersection_docset.seek(target);
        if doc == TERMINATED || self.phrase_match() {
            return doc;
        }
        self.advance()
    }

    fn seek_danger(&mut self, target: DocId) -> SeekDangerResult {
        debug_assert!(
            target >= self.doc(),
            "target ({}) should be greater than or equal to doc ({})",
            target,
            self.doc()
        );
        let seek_res = self.intersection_docset.seek_danger(target);
        if seek_res != SeekDangerResult::Found {
            return seek_res;
        }
        // The intersection matched. Now let's see if we match the phrase.
        if self.phrase_match() {
            SeekDangerResult::Found
        } else {
            SeekDangerResult::SeekLowerBound(target + 1)
        }
    }

    fn doc(&self) -> DocId {
        self.intersection_docset.doc()
    }

    fn size_hint(&self) -> u32 {
        // We adjust the intersection estimate, since actual phrase hits are much lower than where
        // the all appear.
        // The estimate should depend on average field length, e.g. if the field is really short
        // a phrase hit is more likely
        self.intersection_docset.size_hint() / (10 * self.num_terms as u32)
    }

    /// Returns a best-effort hint of the
    /// cost to drive the docset.
    fn cost(&self) -> u64 {
        // While determing a potential hit is cheap for phrases, evaluating an actual hit is
        // expensive since it requires to load positions for a doc and check if they are next to
        // each other.
        // So the cost estimation would be the number of times we need to check if a doc is a hit *
        // 10 * self.num_terms.
        self.intersection_docset.size_hint() as u64 * 10 * self.num_terms as u64
    }
}

impl<TPostings: Postings> Scorer for PhraseScorer<TPostings> {
    #[inline]
    fn score(&mut self) -> Score {
        let doc = self.doc();
        let fieldnorm_id = self.fieldnorm_reader.fieldnorm_id(doc);
        if let Some(similarity_weight) = self.similarity_weight_opt.as_ref() {
            if self.sloppy_matcher.is_some() {
                similarity_weight.score_with_frequency(fieldnorm_id, self.phrase_frequency)
            } else {
                similarity_weight.score(fieldnorm_id, self.phrase_count)
            }
        } else {
            1.0f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_intersection_sym(left: &[u32], right: &[u32], expected: &[u32]) {
        test_intersection_aux(left, right, expected, 0);
        test_intersection_aux(right, left, expected, 0);
    }

    fn test_intersection_aux(left: &[u32], right: &[u32], expected: &[u32], _slop: u32) {
        let mut left_vec = Vec::from(left);
        assert_eq!(intersection_count(&left_vec, right), expected.len());
        intersection(&mut left_vec, right);
        assert_eq!(&left_vec, expected);
    }

    #[test]
    fn test_intersection() {
        test_intersection_sym(&[1], &[1], &[1]);
        test_intersection_sym(&[1], &[2], &[]);
        test_intersection_sym(&[], &[2], &[]);
        test_intersection_sym(&[5, 7], &[1, 5, 10, 12], &[5]);
        test_intersection_sym(&[1, 5, 6, 9, 10, 12], &[6, 8, 9, 12], &[6, 9, 12]);
    }
}

#[cfg(all(test, feature = "unstable"))]
mod bench {

    use test::Bencher;

    use super::*;

    #[bench]
    fn bench_intersection_short(b: &mut Bencher) {
        let mut left = Vec::new();
        b.iter(|| {
            left.clear();
            left.extend_from_slice(&[1, 5, 10, 12]);
            let right = [5, 7];
            intersection(&mut left, &right);
        });
    }

    #[bench]
    fn bench_intersection_medium(b: &mut Bencher) {
        let mut left = Vec::new();
        let left_data: Vec<u32> = (0..100).collect();
        b.iter(|| {
            left.clear();
            left.extend_from_slice(&left_data);
            let right = [5, 7, 55, 200];
            intersection(&mut left, &right);
        });
    }

    #[bench]
    fn bench_intersection_count_short(b: &mut Bencher) {
        b.iter(|| {
            let left = [1, 5, 10, 12];
            let right = [5, 7];
            intersection_count(&left, &right);
        });
    }
}
