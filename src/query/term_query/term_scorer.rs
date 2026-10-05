use std::sync::Arc;

use common::TinySet;

use crate::docset::DocSet;
use crate::fieldnorm::FieldNormReader;
use crate::index::field_statistics::BlockMaxSelection;
use crate::postings::{BlockSegmentPostings, FreqReadingOption, Postings, SegmentPostings};
use crate::query::bm25::{Bm25Weight, NativeInputEnvelope, NativeSelectionContext};
use crate::query::{Explanation, Scorer};
use crate::{DocId, Score};

#[derive(Clone)]
pub struct TermScorer {
    postings: SegmentPostings,
    fieldnorm_reader: FieldNormReader,
    similarity_weight: Bm25Weight,
    block_bounds: TermBlockBounds,
    use_alternate_mask: bool,
}

#[derive(Clone)]
enum TermBlockBounds {
    Global,
    StoredDefault,
    PendingNative(NativeSelectionContext),
    NativeTransform(Arc<NativeInputEnvelope>),
}

impl TermScorer {
    pub(crate) fn contains_doc_for_count(&mut self, target: DocId) -> bool {
        self.postings.contains_doc_for_count(target)
    }

    pub fn new(
        postings: SegmentPostings,
        fieldnorm_reader: FieldNormReader,
        similarity_weight: Bm25Weight,
    ) -> TermScorer {
        // Dense terms mostly copy encoded bitsets directly. Sparse-to-medium
        // terms benefit from independent mask lanes for their decoded doc IDs.
        let use_alternate_mask = postings.size_hint() <= fieldnorm_reader.num_docs() / 4;
        TermScorer {
            postings,
            fieldnorm_reader,
            similarity_weight,
            block_bounds: TermBlockBounds::Global,
            use_alternate_mask,
        }
    }

    /// Test fixtures written by the public serializer use legacy selection.
    #[cfg(test)]
    pub(crate) fn with_segment_average_fieldnorm(mut self, average_fieldnorm: Score) -> Self {
        self.block_bounds = if self
            .similarity_weight
            .can_use_stored_block_max(average_fieldnorm)
        {
            TermBlockBounds::StoredDefault
        } else {
            TermBlockBounds::Global
        };
        self
    }

    pub(crate) fn with_stored_block_max_selection(
        mut self,
        average_fieldnorm: Score,
        selection: BlockMaxSelection,
    ) -> Self {
        self.block_bounds = if self
            .similarity_weight
            .can_use_stored_block_max_with_selection(average_fieldnorm, selection)
        {
            TermBlockBounds::StoredDefault
        } else if let Some(context) = self
            .similarity_weight
            .native_selection_context(average_fieldnorm, selection)
        {
            TermBlockBounds::PendingNative(context)
        } else {
            TermBlockBounds::Global
        };
        self
    }

    pub(crate) fn seek_block(&mut self, target_doc: DocId) {
        self.postings.block_cursor.seek_block(target_doc);
    }

    #[cfg(test)]
    pub fn create_for_test(
        doc_and_tfs: &[(DocId, u32)],
        fieldnorms: &[u32],
        similarity_weight: Bm25Weight,
    ) -> TermScorer {
        assert!(!doc_and_tfs.is_empty());
        assert!(
            doc_and_tfs
                .iter()
                .map(|(doc, _tf)| *doc)
                .max()
                .unwrap_or(0u32)
                < fieldnorms.len() as u32
        );
        let segment_postings =
            SegmentPostings::create_from_docs_and_tfs(doc_and_tfs, Some(fieldnorms));
        let fieldnorm_reader = FieldNormReader::for_test(fieldnorms);
        let average_fieldnorm = fieldnorms.iter().map(|&norm| u64::from(norm)).sum::<u64>()
            as Score
            / fieldnorms.len() as Score;
        TermScorer::new(segment_postings, fieldnorm_reader, similarity_weight)
            .with_segment_average_fieldnorm(average_fieldnorm)
    }

    /// See `FreqReadingOption`.
    pub(crate) fn freq_reading_option(&self) -> FreqReadingOption {
        self.postings.block_cursor.freq_reading_option()
    }

    /// Returns a conservative upper bound on the score for the current block.
    ///
    /// Exact DEFAULT pairs require matching selection statistics. Eligible
    /// nondefault native queries conservatively enclose the stored DEFAULT input;
    /// unsupported domains retain the global bound and the existing tail policy.
    pub fn block_max_score(&mut self) -> Score {
        // DEFAULT remains the first branch, without cold envelope construction.
        if matches!(self.block_bounds, TermBlockBounds::StoredDefault) {
            return self.postings.block_cursor.block_max_score_with_stored_max(
                &self.fieldnorm_reader,
                &self.similarity_weight,
                true,
            );
        }
        if !self.similarity_weight.has_safe_score_bounds() {
            return self.similarity_weight.max_score();
        }
        // Tails or absent metadata cannot activate a transform. Shallow complete
        // block requests can: this test never decompresses postings.
        if self
            .postings
            .block_cursor
            .skip_reader()
            .selected_input_pair()
            .is_some()
        {
            if let TermBlockBounds::PendingNative(context) = self.block_bounds {
                self.block_bounds = self
                    .similarity_weight
                    .native_input_envelope(context)
                    .map(|envelope| TermBlockBounds::NativeTransform(Arc::new(envelope)))
                    .unwrap_or(TermBlockBounds::Global);
            }
            if let TermBlockBounds::NativeTransform(envelope) = &self.block_bounds {
                return self
                    .postings
                    .block_cursor
                    .block_max_score_with_native_envelope(
                        &self.fieldnorm_reader,
                        &self.similarity_weight,
                        envelope,
                    );
            }
        }
        self.postings.block_cursor.block_max_score_with_stored_max(
            &self.fieldnorm_reader,
            &self.similarity_weight,
            false,
        )
    }

    pub fn term_freq(&self) -> u32 {
        self.postings.term_freq()
    }

    pub fn fieldnorm_id(&self) -> u8 {
        self.fieldnorm_reader.fieldnorm_id(self.doc())
    }

    pub fn explain(&self) -> Explanation {
        let fieldnorm_id = self.fieldnorm_id();
        let term_freq = self.term_freq();
        self.similarity_weight.explain(fieldnorm_id, term_freq)
    }

    pub fn max_score(&self) -> Score {
        self.similarity_weight.max_score()
    }

    pub fn last_doc_in_block(&self) -> DocId {
        self.postings.block_cursor.skip_reader().last_doc_in_block()
    }

    /// Returns a mutable reference to the underlying block cursor.
    pub(crate) fn block_cursor(&mut self) -> &mut BlockSegmentPostings {
        &mut self.postings.block_cursor
    }

    /// Returns a reference to the fieldnorm reader for batch lookups.
    pub(crate) fn fieldnorm_reader(&self) -> &FieldNormReader {
        &self.fieldnorm_reader
    }

    /// Returns a reference to the BM25 weight for batch score computation.
    pub(crate) fn bm25_weight(&self) -> &Bm25Weight {
        &self.similarity_weight
    }
}

impl DocSet for TermScorer {
    #[inline]
    fn advance(&mut self) -> DocId {
        self.postings.advance()
    }

    #[inline]
    fn seek(&mut self, target: DocId) -> DocId {
        debug_assert!(target >= self.doc());
        self.postings.seek(target)
    }

    #[inline]
    fn doc(&self) -> DocId {
        self.postings.doc()
    }

    fn size_hint(&self) -> u32 {
        self.postings.size_hint()
    }

    fn fill_bitset_window(&mut self, min_doc: DocId, mask: &mut [TinySet]) -> DocId {
        self.postings
            .fill_bitset_window_impl(min_doc, mask, self.use_alternate_mask)
    }
}

impl Scorer for TermScorer {
    #[inline]
    fn score(&mut self) -> Score {
        let fieldnorm_id = self.fieldnorm_id();
        let term_freq = self.term_freq();
        self.similarity_weight.score(fieldnorm_id, term_freq)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::index::SegmentId;
    use crate::indexer::index_writer::MEMORY_BUDGET_NUM_BYTES_MIN;
    use crate::merge_policy::NoMergePolicy;
    use crate::postings::compression::COMPRESSION_BLOCK_SIZE;
    use crate::postings::SegmentPostings;
    use crate::query::term_query::TermScorer;
    use crate::query::{Bm25Weight, EnableScoring, Scorer, TermQuery};
    use crate::schema::{IndexRecordOption, Schema, TEXT};
    use crate::{
        assert_nearly_equals, DocId, DocSet, Index, IndexWriter, Score, Searcher, Term, TERMINATED,
    };

    #[test]
    fn block_bound_layout_receipt() {
        assert!(std::mem::size_of::<super::TermBlockBounds>() <= 16);
        eprintln!(
            "B1_LAYOUT TermScorer={} BlockSegmentPostings={} Bm25Weight={} FieldNormReader={} \
             TermBlockBounds={}",
            std::mem::size_of::<TermScorer>(),
            std::mem::size_of::<crate::postings::BlockSegmentPostings>(),
            std::mem::size_of::<Bm25Weight>(),
            std::mem::size_of::<crate::fieldnorm::FieldNormReader>(),
            std::mem::size_of::<super::TermBlockBounds>()
        );
    }

    #[test]
    fn test_native_bound_policy_is_lazy_and_fixed_to_actual_query() -> crate::Result<()> {
        use crate::query::Bm25Parameters;
        let mut schema = Schema::builder();
        let field = schema.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        for doc in 0..300 {
            writer.add_document(doc!(field => if doc == 0 { "alpha rare" } else { "alpha" }))?;
        }
        writer.commit()?;
        let reader = index.reader()?;
        let searcher = reader
            .searcher()
            .with_bm25_parameters(Bm25Parameters::new(0.9, 0.4)?);
        let segment = searcher.segment_reader(0);
        let term = TermQuery::new(
            Term::from_field_text(field, "alpha"),
            IndexRecordOption::WithFreqs,
        );
        let term_weight =
            term.specialized_weight(EnableScoring::enabled_from_searcher(&searcher))?;
        let scorer = term_weight.term_scorer_for_test(segment, 1.0)?.unwrap();
        assert!(matches!(
            scorer.block_bounds,
            super::TermBlockBounds::PendingNative(_)
        ));
        let mut exhaustive = scorer.clone();
        while exhaustive.doc() != TERMINATED {
            exhaustive.score();
            exhaustive.advance();
        }
        assert!(matches!(
            exhaustive.block_bounds,
            super::TermBlockBounds::PendingNative(_)
        ));
        let mut bounded = scorer.clone();
        let bound = bounded.block_max_score();
        assert!(matches!(
            bounded.block_bounds,
            super::TermBlockBounds::NativeTransform(_)
        ));
        assert!(bound >= bounded.score());
        let clone = bounded.clone();
        match (&bounded.block_bounds, &clone.block_bounds) {
            (
                super::TermBlockBounds::NativeTransform(a),
                super::TermBlockBounds::NativeTransform(b),
            ) => {
                assert!(std::sync::Arc::ptr_eq(a, b));
            }
            _ => panic!("clone must share its immutable enclosure"),
        }
        let mut untrusted = TermScorer::new(
            scorer.postings.clone(),
            scorer.fieldnorm_reader.clone(),
            scorer.similarity_weight.clone(),
        );
        assert!(matches!(
            untrusted.block_bounds,
            super::TermBlockBounds::Global
        ));
        assert_eq!(untrusted.block_max_score(), untrusted.max_score());
        let rare = TermQuery::new(
            Term::from_field_text(field, "rare"),
            IndexRecordOption::WithFreqs,
        );
        let rare_weight =
            rare.specialized_weight(EnableScoring::enabled_from_searcher(&searcher))?;
        let mut tail = rare_weight.term_scorer_for_test(segment, 1.0)?.unwrap();
        assert!(matches!(
            tail.block_bounds,
            super::TermBlockBounds::PendingNative(_)
        ));
        tail.block_max_score();
        assert!(matches!(
            tail.block_bounds,
            super::TermBlockBounds::PendingNative(_)
        ));
        let b1_searcher = reader
            .searcher()
            .with_bm25_parameters(Bm25Parameters::new(2.5, 1.0)?);
        let b1_weight =
            term.specialized_weight(EnableScoring::enabled_from_searcher(&b1_searcher))?;
        let mut b1 = b1_weight.term_scorer_for_test(segment, 1.0)?.unwrap();
        assert!(matches!(
            b1.block_bounds,
            super::TermBlockBounds::PendingNative(_)
        ));
        assert_eq!(b1.block_max_score(), b1.max_score());
        assert!(matches!(b1.block_bounds, super::TermBlockBounds::Global));
        let default_searcher = reader.searcher();
        let default_weight =
            term.specialized_weight(EnableScoring::enabled_from_searcher(&default_searcher))?;
        let default = default_weight.term_scorer_for_test(segment, 1.0)?.unwrap();
        assert!(matches!(
            default.block_bounds,
            super::TermBlockBounds::StoredDefault
        ));
        let mismatch = default.with_stored_block_max_selection(
            3.0,
            crate::index::field_statistics::BlockMaxSelection::NativeSaturationInput,
        );
        assert!(matches!(
            mismatch.block_bounds,
            super::TermBlockBounds::Global
        ));
        Ok(())
    }

    #[test]
    fn test_native_absent_metadata_does_not_activate_envelope() -> crate::Result<()> {
        use crate::query::Bm25Parameters;
        use crate::schema::{TextFieldIndexing, TextOptions};
        let mut schema = Schema::builder();
        let field = schema.add_text_field(
            "text",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_index_option(IndexRecordOption::WithFreqs)
                    .set_fieldnorms(false),
            ),
        );
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        for _ in 0..256 {
            writer.add_document(doc!(field => "alpha"))?;
        }
        writer.commit()?;
        let searcher = index
            .reader()?
            .searcher()
            .with_bm25_parameters(Bm25Parameters::new(0.9, 0.4)?);
        let term = TermQuery::new(
            Term::from_field_text(field, "alpha"),
            IndexRecordOption::WithFreqs,
        );
        let weight = term.specialized_weight(EnableScoring::enabled_from_searcher(&searcher))?;
        let mut scorer = weight
            .term_scorer_for_test(searcher.segment_reader(0), 1.0)?
            .unwrap();
        assert!(matches!(
            scorer.block_bounds,
            super::TermBlockBounds::PendingNative(_)
        ));
        assert_eq!(scorer.block_max_score(), scorer.max_score());
        assert!(matches!(
            scorer.block_bounds,
            super::TermBlockBounds::PendingNative(_)
        ));
        scorer.seek_block(128);
        assert_eq!(scorer.block_max_score(), scorer.max_score());
        assert!(matches!(
            scorer.block_bounds,
            super::TermBlockBounds::PendingNative(_)
        ));
        Ok(())
    }

    #[test]
    fn test_term_scorer_max_score() -> crate::Result<()> {
        let bm25_weight = Bm25Weight::for_one_term(3, 6, 10.0);
        let mut term_scorer = TermScorer::create_for_test(
            &[(2, 3), (3, 12), (7, 8)],
            &[0, 0, 10, 12, 0, 0, 0, 100],
            bm25_weight,
        );
        let max_scorer = term_scorer.max_score();
        crate::assert_nearly_equals!(max_scorer, 1.5249238);
        assert_eq!(term_scorer.doc(), 2);
        assert_eq!(term_scorer.term_freq(), 3);
        assert_nearly_equals!(term_scorer.block_max_score(), 1.3676447);
        assert_nearly_equals!(term_scorer.score(), 1.0892314);
        assert_eq!(term_scorer.advance(), 3);
        assert_eq!(term_scorer.doc(), 3);
        assert_eq!(term_scorer.term_freq(), 12);
        assert_nearly_equals!(term_scorer.score(), 1.3676447);
        assert_eq!(term_scorer.advance(), 7);
        assert_eq!(term_scorer.doc(), 7);
        assert_eq!(term_scorer.term_freq(), 8);
        assert_nearly_equals!(term_scorer.score(), 0.72015285);
        assert_eq!(term_scorer.advance(), TERMINATED);
        Ok(())
    }

    #[test]
    fn test_term_scorer_shallow_advance() -> crate::Result<()> {
        let bm25_weight = Bm25Weight::for_one_term(300, 1024, 10.0);
        let mut doc_and_tfs = vec![];
        for i in 0u32..300u32 {
            let doc = i * 10;
            doc_and_tfs.push((doc, 1u32 + doc % 3u32));
        }
        let fieldnorms: Vec<u32> = std::iter::repeat_n(10u32, 3_000).collect();
        let mut term_scorer = TermScorer::create_for_test(&doc_and_tfs, &fieldnorms, bm25_weight);
        assert_eq!(term_scorer.doc(), 0u32);
        term_scorer.seek_block(1289);
        assert_eq!(term_scorer.doc(), 0u32);
        term_scorer.seek(1289);
        assert_eq!(term_scorer.doc(), 1290);
        Ok(())
    }

    #[test]
    fn test_term_scorer_mask_lanes_follow_density() {
        for (num_docs, expected_alternate) in [(256, false), (1_024, true)] {
            let docs: Vec<_> = (0..128).map(|doc| (doc * 2, 1)).collect();
            let mut scorer = TermScorer::create_for_test(
                &docs,
                &vec![10; num_docs],
                Bm25Weight::for_one_term(128, num_docs as u64, 10.0),
            );
            assert_eq!(scorer.use_alternate_mask, expected_alternate);
            let mut mask = [common::TinySet::from_bits(1 << 63); 64];
            let mut expected = mask;
            for &(doc, _) in &docs {
                expected[(doc / 64) as usize].insert_mut(doc % 64);
            }
            assert_eq!(scorer.fill_bitset_window(0, &mut mask), TERMINATED);
            assert_eq!(scorer.doc(), TERMINATED);
            assert_eq!(mask, expected);
        }
    }

    #[test]
    fn test_public_scorer_uses_conservative_block_max() {
        let docs: Vec<_> = (0..256).map(|doc| (doc, 1 + doc % 8)).collect();
        let norms = vec![20; 256];
        let postings = SegmentPostings::create_from_docs_and_tfs(&docs, Some(&norms));
        let norm_reader = crate::fieldnorm::FieldNormReader::for_test(&norms);
        let weight = Bm25Weight::for_one_term(256, 1024, 1000.0);
        for boost in [-1.0, 0.0, 1.0, 3.0] {
            let mut scorer = TermScorer::new(
                postings.clone(),
                norm_reader.clone(),
                weight.boost_by(boost),
            );
            assert!(matches!(
                scorer.block_bounds,
                super::TermBlockBounds::Global
            ));
            let bound = scorer.block_max_score();
            for _ in 0..128 {
                assert!(scorer.score() <= bound);
                scorer.advance();
            }
        }
    }

    #[test]
    fn test_negative_boost_tail_block_max_includes_absent_contribution() {
        let mut scorer = TermScorer::create_for_test(
            &[(0, 1), (1, 2), (2, 3)],
            &[10; 3],
            Bm25Weight::for_one_term(3, 1024, 10.0).boost_by(-1.0),
        );
        assert_eq!(scorer.block_max_score(), 0.0);
        for _ in 0..3 {
            assert!(scorer.score() < 0.0);
            scorer.advance();
        }
    }

    #[test]
    fn test_public_block_max_does_not_reuse_another_weights_cache() {
        let norms = [10; 3];
        let postings =
            SegmentPostings::create_from_docs_and_tfs(&[(0, 1), (1, 2), (2, 3)], Some(&norms));
        let mut scorer = TermScorer::new(
            postings,
            crate::fieldnorm::FieldNormReader::for_test(&norms),
            Bm25Weight::for_one_term(3, 1024, 10.0),
        );
        let low_weight = scorer.similarity_weight.clone();
        let high_weight = low_weight.boost_by(100.0);
        let norm_reader = scorer.fieldnorm_reader.clone();
        let low_bound = scorer
            .block_cursor()
            .block_max_score(&norm_reader, &low_weight);
        let high_bound = scorer
            .block_cursor()
            .block_max_score(&norm_reader, &high_weight);
        assert!(high_bound > low_bound);
        assert!(high_bound >= high_weight.score(10, 3));
    }

    #[test]
    fn test_tail_cache_public_weight_does_not_poison_fixed_scorer() {
        let docs = [(0, 1), (1, 2), (2, 3)];
        let norms = [10; 3];
        for (profile, base) in [
            ("native", Bm25Weight::for_native_block_bounds(10.0)),
            ("classic", Bm25Weight::for_one_term(3, 1024, 10.0)),
        ] {
            let fixed_weight = base.boost_by(100.0);
            let expected = docs
                .iter()
                .map(|&(doc, tf)| {
                    fixed_weight.score(
                        crate::fieldnorm::FieldNormReader::for_test(&norms).fieldnorm_id(doc),
                        tf,
                    )
                })
                .fold(0.0f32, f32::max);
            let mut scorer = TermScorer::new(
                SegmentPostings::create_from_docs_and_tfs(&docs, Some(&norms)),
                crate::fieldnorm::FieldNormReader::for_test(&norms),
                fixed_weight,
            );
            assert_eq!(scorer.block_max_score().to_bits(), expected.to_bits());
            let reader = scorer.fieldnorm_reader.clone();
            let public_low = scorer.block_cursor().block_max_score(&reader, &base);
            let after_public = scorer.block_max_score();
            eprintln!(
                "weight {profile}: public={public_low} fixed={after_public} expected={expected}"
            );
            assert!(public_low < expected);
            let public_expected = docs
                .iter()
                .map(|&(doc, tf)| base.score(reader.fieldnorm_id(doc), tf))
                .fold(0.0f32, f32::max);
            assert_eq!(public_low.to_bits(), public_expected.to_bits(), "{profile}");
            assert_eq!(after_public.to_bits(), expected.to_bits(), "{profile}");
        }
    }

    #[test]
    fn test_tail_cache_public_reader_does_not_poison_fixed_scorer() {
        let docs = [(0, 1), (1, 2), (2, 3)];
        let fixed_reader = crate::fieldnorm::FieldNormReader::for_test(&[1; 3]);
        let public_reader = crate::fieldnorm::FieldNormReader::for_test(&[1000; 3]);
        for (profile, weight) in [
            ("native", Bm25Weight::for_native_block_bounds(10.0)),
            ("classic", Bm25Weight::for_one_term(3, 1024, 10.0)),
        ] {
            let expected = docs
                .iter()
                .map(|&(doc, tf)| weight.score(fixed_reader.fieldnorm_id(doc), tf))
                .fold(0.0f32, f32::max);
            let mut scorer = TermScorer::new(
                SegmentPostings::create_from_docs_and_tfs(&docs, Some(&[1; 3])),
                fixed_reader.clone(),
                weight.clone(),
            );
            assert_eq!(scorer.block_max_score().to_bits(), expected.to_bits());
            let public_low = scorer
                .block_cursor()
                .block_max_score(&public_reader, &weight);
            let after_public = scorer.block_max_score();
            eprintln!(
                "reader {profile}: public={public_low} fixed={after_public} expected={expected}"
            );
            assert!(public_low < expected);
            let public_expected = docs
                .iter()
                .map(|&(doc, tf)| weight.score(public_reader.fieldnorm_id(doc), tf))
                .fold(0.0f32, f32::max);
            assert_eq!(public_low.to_bits(), public_expected.to_bits(), "{profile}");
            assert_eq!(after_public.to_bits(), expected.to_bits(), "{profile}");
        }
    }

    #[test]
    fn test_public_block_max_keeps_full_global_and_tail_semantics() {
        let docs: Vec<_> = (0..129).map(|doc| (doc, 1)).collect();
        let norms = [10; 129];
        let reader = crate::fieldnorm::FieldNormReader::for_test(&norms);
        for weight in [
            Bm25Weight::for_native_block_bounds(10.0),
            Bm25Weight::for_one_term(129, 1024, 10.0),
        ] {
            let mut scorer = TermScorer::new(
                SegmentPostings::create_from_docs_and_tfs(&docs, Some(&norms)),
                reader.clone(),
                weight.clone(),
            );
            // Public complete blocks keep the global bound even when decoded.
            assert_eq!(
                scorer.block_cursor().block_max_score(&reader, &weight),
                weight.max_score()
            );
            scorer.seek_block(128);
            // A shallow-selected tail has no decoded certificate yet.
            assert_eq!(
                scorer.block_cursor().block_max_score(&reader, &weight),
                weight.max_score()
            );
            assert_eq!(scorer.seek(128), 128);
            let expected = weight.score(reader.fieldnorm_id(128), 1);
            assert_eq!(scorer.block_max_score().to_bits(), expected.to_bits());
            for boost in [3.0, -1.0, f32::INFINITY, f32::NAN] {
                let public_weight = weight.boost_by(boost);
                let public_bound = scorer
                    .block_cursor()
                    .block_max_score(&reader, &public_weight);
                let public_expected = if public_weight.has_safe_score_bounds() {
                    public_weight.score(reader.fieldnorm_id(128), 1).max(0.0)
                } else {
                    f32::INFINITY
                };
                assert_eq!(public_bound.to_bits(), public_expected.to_bits());
                assert_eq!(scorer.block_max_score().to_bits(), expected.to_bits());
            }
        }
    }

    #[test]
    fn test_missing_stored_block_max_remains_conservative() -> crate::Result<()> {
        use crate::query::Query;
        use crate::schema::{TextFieldIndexing, TextOptions};
        for (record, fieldnorms) in [
            (IndexRecordOption::Basic, true),
            (IndexRecordOption::WithFreqs, false),
        ] {
            let mut schema = Schema::builder();
            let field = schema.add_text_field(
                "text",
                TextOptions::default().set_indexing_options(
                    TextFieldIndexing::default()
                        .set_index_option(record)
                        .set_fieldnorms(fieldnorms),
                ),
            );
            let index = Index::create_in_ram(schema.build());
            let mut writer = index.writer_for_tests()?;
            for _ in 0..256 {
                writer.add_document(doc!(field => "a"))?;
            }
            writer.commit()?;
            let searcher = index.reader()?.searcher();
            let query = TermQuery::new(Term::from_field_text(field, "a"), record);
            let weight = query.weight(EnableScoring::enabled_from_searcher(&searcher))?;
            let mut scorer = weight.scorer(&searcher.segment_readers()[0], 1.0)?;
            let term_scorer = scorer.downcast_mut::<TermScorer>().unwrap();
            let bound = term_scorer.block_max_score();
            assert!(bound > 0.0);
            for _ in 0..128 {
                assert!(term_scorer.score() <= bound);
                term_scorer.advance();
            }
        }
        Ok(())
    }

    #[test]
    fn test_multisegment_block_max_matches_exhaustive_top_docs() -> crate::Result<()> {
        use crate::collector::TopDocs;
        use crate::query::Query;
        let mut schema = Schema::builder();
        let field = schema.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        writer.set_merge_policy(Box::new(NoMergePolicy));
        let text = |tf: usize, length: usize| {
            std::iter::repeat_n("a", tf)
                .chain(std::iter::repeat_n("c", length - tf))
                .collect::<Vec<_>>()
                .join(" ")
        };
        for _ in 0..128 {
            writer.add_document(doc!(field => text(2, 200)))?;
        }
        for _ in 0..127 {
            writer.add_document(doc!(field => text(1, 100)))?;
        }
        writer.add_document(doc!(field => text(10, 2000)))?;
        writer.commit()?;
        writer.add_document(doc!(field => text(0, 256000)))?;
        writer.commit()?;
        let searcher = index.reader()?.searcher();
        assert_eq!(searcher.segment_readers().len(), 2);
        let query = TermQuery::new(
            Term::from_field_text(field, "a"),
            IndexRecordOption::WithFreqs,
        );
        let weight = query.weight(EnableScoring::enabled_from_searcher(&searcher))?;
        let top = searcher.search(&query, &TopDocs::with_limit(1).order_by_score())?;
        let mut exhaustive = Vec::new();
        for (seg, reader) in searcher.segment_readers().iter().enumerate() {
            let mut scorer = weight.scorer(reader, 1.0)?;
            while scorer.doc() != TERMINATED {
                exhaustive.push((
                    scorer.score(),
                    crate::DocAddress::new(seg as u32, scorer.doc()),
                ));
                scorer.advance();
            }
        }
        exhaustive.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        assert_eq!(top, exhaustive[..1]);
        Ok(())
    }

    proptest! {
        #[test]
        fn test_term_scorer_block_max_score(term_freqs_fieldnorms in proptest::collection::vec((1u32..10u32, 0u32..100u32), 80..300)) {
        let term_doc_freq = term_freqs_fieldnorms.len();
         let doc_tfs: Vec<(u32, u32)> = term_freqs_fieldnorms.iter()
                   .cloned()
                  .enumerate()
                  .map(|(doc, (tf, _))| (doc as u32, tf))
                  .collect();

         let mut fieldnorms: Vec<u32> = vec![];
         for item in term_freqs_fieldnorms.iter().take(term_doc_freq) {
             let (tf, num_extra_terms) = item;
             fieldnorms.push(tf + num_extra_terms);
         }
         let average_fieldnorm = fieldnorms
             .iter()
             .cloned()
             .sum::<u32>() as Score / term_doc_freq as Score;
             // Average fieldnorm is over the entire index,
             // not necessarily the docs that are in the posting list.
             // For this reason we multiply by 1.1 to make a realistic value.
         let bm25_weight = Bm25Weight::for_one_term(term_doc_freq as u64,
            term_doc_freq as u64 * 10u64,
            average_fieldnorm);

         let mut term_scorer =
              TermScorer::create_for_test(&doc_tfs[..], &fieldnorms[..], bm25_weight);

         let docs: Vec<DocId> = (0..term_doc_freq).map(|doc| doc as DocId).collect();
         for block in docs.chunks(COMPRESSION_BLOCK_SIZE) {
             let block_max_score: Score = term_scorer.block_max_score();
             let mut block_max_score_computed: Score = 0.0;
             for &doc in block {
                assert_eq!(term_scorer.doc(), doc);
                block_max_score_computed = block_max_score_computed.max(term_scorer.score());
                term_scorer.advance();
             }
             assert_nearly_equals!(block_max_score_computed, block_max_score);
         }
        }
    }

    #[test]
    fn test_block_wand() {
        let mut doc_tfs: Vec<(u32, u32)> = vec![];
        for doc in 0u32..128u32 {
            doc_tfs.push((doc, 1u32));
        }
        for doc in 128u32..256u32 {
            doc_tfs.push((doc, if doc == 200 { 2u32 } else { 1u32 }));
        }
        doc_tfs.push((256, 1u32));
        doc_tfs.push((257, 3u32));
        doc_tfs.push((258, 1u32));

        let fieldnorms: Vec<u32> = std::iter::repeat_n(20u32, 300).collect();
        let bm25_weight = Bm25Weight::for_one_term(10, 129, 20.0);
        let mut docs = TermScorer::create_for_test(&doc_tfs[..], &fieldnorms[..], bm25_weight);
        assert_nearly_equals!(docs.block_max_score(), 2.5161593);
        docs.seek_block(135);
        assert_nearly_equals!(docs.block_max_score(), 3.4597192);
        docs.seek_block(256);
        // the block is not loaded yet.
        assert_eq!(docs.block_max_score(), docs.max_score());
        assert_eq!(256, docs.seek(256));
        assert_nearly_equals!(docs.block_max_score(), 3.9539647);
    }

    fn test_block_wand_aux(term_query: &TermQuery, searcher: &Searcher) -> crate::Result<()> {
        let term_weight =
            term_query.specialized_weight(EnableScoring::enabled_from_searcher(searcher))?;
        for reader in searcher.segment_readers() {
            let mut block_max_scores = vec![];
            let mut block_max_scores_b = vec![];
            let mut docs = vec![];
            {
                let mut term_scorer = term_weight.term_scorer_for_test(reader, 1.0)?.unwrap();
                while term_scorer.doc() != TERMINATED {
                    let mut score = term_scorer.score();
                    docs.push(term_scorer.doc());
                    for _ in 0..128 {
                        score = score.max(term_scorer.score());
                        if term_scorer.advance() == TERMINATED {
                            break;
                        }
                    }
                    block_max_scores.push(score);
                }
            }
            {
                let mut term_scorer = term_weight.term_scorer_for_test(reader, 1.0)?.unwrap();
                for d in docs {
                    term_scorer.seek_block(d);
                    block_max_scores_b.push(term_scorer.block_max_score());
                }
            }
            for (l, r) in block_max_scores
                .iter()
                .cloned()
                .zip(block_max_scores_b.iter().cloned())
            {
                assert_nearly_equals!(l, r);
            }
        }
        Ok(())
    }

    #[ignore]
    #[test]
    fn test_block_wand_long_test() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let mut writer: IndexWriter =
            index.writer_with_num_threads(3, 3 * MEMORY_BUDGET_NUM_BYTES_MIN)?;
        use rand::Rng;
        let mut rng = rand::rng();
        writer.set_merge_policy(Box::new(NoMergePolicy));
        for _ in 0..3_000 {
            let term_freq = rng.random_range(1..10000);
            let words: Vec<&str> = std::iter::repeat_n("bbbb", term_freq).collect();
            let text = words.join(" ");
            writer.add_document(doc!(text_field=>text))?;
        }
        writer.commit()?;
        let term_query = TermQuery::new(
            Term::from_field_text(text_field, "bbbb"),
            IndexRecordOption::WithFreqs,
        );
        let segment_ids: Vec<SegmentId>;
        let reader = index.reader()?;
        {
            let searcher = reader.searcher();
            segment_ids = searcher
                .segment_readers()
                .iter()
                .map(|segment| segment.segment_id())
                .collect();
            test_block_wand_aux(&term_query, &searcher)?;
        }
        writer.merge(&segment_ids[..]).wait().unwrap();
        {
            reader.reload()?;
            let searcher = reader.searcher();
            assert_eq!(searcher.segment_readers().len(), 1);
            test_block_wand_aux(&term_query, &searcher)?;
        }
        Ok(())
    }
}
