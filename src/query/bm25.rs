use std::sync::Arc;

use crate::fieldnorm::FieldNormReader;
use crate::index::field_statistics::BlockMaxSelection;
use crate::query::Explanation;
use crate::schema::Field;
use crate::{Score, Searcher, Term};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Bm25Scoring {
    LegacyClassic,
    NativeLucene,
}

/// A coherent collection-statistics snapshot used for BM25.
///
/// `new` preserves the historical custom-provider average rounding. Native index
/// statistics use double-precision division before converting their average to f32
/// and Lucene's native IDF, reciprocal normalization, and score convention.
#[derive(Clone, Copy, Debug)]
pub struct Bm25FieldStatistics {
    doc_count: u64,
    sum_total_term_freq: u64,
    average_fieldnorm: Score,
    scoring: Bm25Scoring,
}

impl Bm25FieldStatistics {
    /// Construct custom statistics without changing historical average rounding
    /// or the classic BM25 score convention (including its k1+1 numerator).
    pub fn new(doc_count: u64, sum_total_term_freq: u64) -> Self {
        Self {
            doc_count,
            sum_total_term_freq,
            average_fieldnorm: sum_total_term_freq as Score / doc_count as Score,
            scoring: Bm25Scoring::LegacyClassic,
        }
    }
    /// Number of documents represented by this field's statistics.
    pub fn doc_count(&self) -> u64 {
        self.doc_count
    }
    /// Sum of the field's stored term frequencies.
    pub fn sum_total_term_freq(&self) -> u64 {
        self.sum_total_term_freq
    }

    fn native(doc_count: u64, sum_total_term_freq: u64) -> Self {
        Self {
            doc_count,
            sum_total_term_freq,
            average_fieldnorm: crate::index::field_statistics::native_average(
                sum_total_term_freq,
                doc_count,
            ),
            scoring: Bm25Scoring::NativeLucene,
        }
    }
}

const K1: Score = 1.2;
const B: Score = 0.75;

/// An interface to compute the statistics needed in BM25 scoring.
///
/// The standard implementation is a [Searcher] but you can also
/// create your own to adjust the statistics.
pub trait Bm25StatisticsProvider {
    /// The total number of tokens in a given field across all documents in
    /// the index.
    fn total_num_tokens(&self, field: Field) -> crate::Result<u64>;

    /// The total number of documents in the index.
    fn total_num_docs(&self) -> crate::Result<u64>;

    /// The number of documents containing the given term.
    fn doc_freq(&self, term: &Term) -> crate::Result<u64>;

    /// Return the population and token total together. The default preserves
    /// existing custom providers, including their average rounding policy.
    /// Native statistics describe physical postings, including pending deletes.
    /// A JSON field combines its indexed paths into one field population.
    fn field_statistics(&self, field: Field) -> crate::Result<Bm25FieldStatistics> {
        let tokens = self.total_num_tokens(field)?;
        Ok(Bm25FieldStatistics::new(self.total_num_docs()?, tokens))
    }
}

impl Bm25StatisticsProvider for Searcher {
    fn field_statistics(&self, field: Field) -> crate::Result<Bm25FieldStatistics> {
        let mut docs = 0u64;
        let mut tokens = 0u64;
        for segment in self.segment_readers() {
            let statistics = segment.inverted_index(field)?.field_statistics()?;
            docs += u64::from(statistics.doc_count);
            tokens += statistics.sum_total_term_freq;
        }
        Ok(Bm25FieldStatistics::native(docs, tokens))
    }

    fn total_num_tokens(&self, field: Field) -> crate::Result<u64> {
        let mut total_num_tokens = 0u64;

        for segment_reader in self.segment_readers() {
            let inverted_index = segment_reader.inverted_index(field)?;
            total_num_tokens += inverted_index.field_statistics()?.sum_total_term_freq;
        }
        Ok(total_num_tokens)
    }

    fn total_num_docs(&self) -> crate::Result<u64> {
        let mut total_num_docs = 0u64;

        for segment_reader in self.segment_readers() {
            total_num_docs += u64::from(segment_reader.max_doc());
        }
        Ok(total_num_docs)
    }

    fn doc_freq(&self, term: &Term) -> crate::Result<u64> {
        self.doc_freq(term)
    }
}

pub(crate) fn idf(doc_freq: u64, doc_count: u64) -> Score {
    assert!(doc_count >= doc_freq, "{doc_count} >= {doc_freq}");
    let x = ((doc_count - doc_freq) as Score + 0.5) / (doc_freq as Score + 0.5);
    (1.0 + x).ln()
}

fn native_idf(doc_freq: u64, doc_count: u64) -> Score {
    assert!(doc_count >= doc_freq, "{doc_count} >= {doc_freq}");
    let x = ((doc_count - doc_freq) as f64 + 0.5) / (doc_freq as f64 + 0.5);
    (1.0 + x).ln() as Score
}

fn native_idf_explanation(doc_freq: u64, doc_count: u64) -> Explanation {
    let mut explanation = Explanation::new(
        "idf, computed as log(1 + (N - n + 0.5) / (n + 0.5))",
        native_idf(doc_freq, doc_count),
    );
    explanation.add_const("n, number of docs containing this term", doc_freq as Score);
    explanation.add_const("N, number of docs with this field", doc_count as Score);
    explanation
}

fn cached_tf_component(fieldnorm: u32, average_fieldnorm: Score) -> Score {
    K1 * (1.0 - B + B * fieldnorm as Score / average_fieldnorm)
}

fn compute_tf_cache(average_fieldnorm: Score, scoring: Bm25Scoring) -> Arc<[Score; 256]> {
    let mut cache: [Score; 256] = [0.0; 256];
    for (fieldnorm_id, cache_mut) in cache.iter_mut().enumerate() {
        let fieldnorm = FieldNormReader::id_to_fieldnorm(fieldnorm_id as u8);
        let norm = cached_tf_component(fieldnorm, average_fieldnorm);
        *cache_mut = match scoring {
            Bm25Scoring::LegacyClassic => norm,
            Bm25Scoring::NativeLucene => 1.0 / norm,
        };
    }
    Arc::new(cache)
}

/// A struct used for computing BM25 scores.
#[derive(Clone)]
pub struct Bm25Weight {
    idf_explain: Option<Explanation>,
    weight: Score,
    cache: Arc<[Score; 256]>,
    average_fieldnorm: Score,
    scoring: Bm25Scoring,
}

impl Bm25Weight {
    /// Increase the weight by a multiplicative factor.
    pub fn boost_by(&self, boost: Score) -> Bm25Weight {
        if boost == 1.0f32 {
            return self.clone();
        }
        Bm25Weight {
            idf_explain: self.idf_explain.clone(),
            weight: self.weight * boost,
            cache: self.cache.clone(),
            average_fieldnorm: self.average_fieldnorm,
            scoring: self.scoring,
        }
    }

    /// Construct a [Bm25Weight] for a phrase of terms.
    ///
    /// Searcher's native field snapshot selects Lucene's raw score convention.
    /// Historical custom providers retain their classic k1+1 numerator.
    pub fn for_terms(
        statistics: &dyn Bm25StatisticsProvider,
        terms: &[Term],
    ) -> crate::Result<Bm25Weight> {
        assert!(!terms.is_empty(), "Bm25 requires at least one term");
        let field = terms[0].field();
        for term in &terms[1..] {
            assert_eq!(
                term.field(),
                field,
                "All terms must belong to the same field."
            );
        }

        let collection = statistics.field_statistics(field)?;
        let total_num_docs = collection.doc_count;
        let average_fieldnorm = collection.average_fieldnorm;

        if collection.scoring == Bm25Scoring::NativeLucene {
            let explanation = if terms.len() == 1 {
                native_idf_explanation(statistics.doc_freq(&terms[0])?, total_num_docs)
            } else {
                // Lucene rounds each individual IDF to f32, sums in f64, then rounds once.
                let mut sum = 0.0f64;
                let mut details = Vec::with_capacity(terms.len());
                for term in terms {
                    let detail = native_idf_explanation(statistics.doc_freq(term)?, total_num_docs);
                    sum += f64::from(detail.value());
                    details.push(detail);
                }
                let mut explanation = Explanation::new("idf, sum of:", sum as Score);
                for detail in details {
                    explanation.add_detail(detail);
                }
                explanation
            };
            return Ok(Bm25Weight::new_native(explanation, average_fieldnorm));
        }

        if terms.len() == 1 {
            let term_doc_freq = statistics.doc_freq(&terms[0])?;
            Ok(Bm25Weight::for_one_term(
                term_doc_freq,
                total_num_docs,
                average_fieldnorm,
            ))
        } else {
            let mut idf_sum: Score = 0.0;
            for term in terms {
                let term_doc_freq = statistics.doc_freq(term)?;
                idf_sum += idf(term_doc_freq, total_num_docs);
            }
            let idf_explain = Explanation::new("idf", idf_sum);
            Ok(Bm25Weight::new(idf_explain, average_fieldnorm))
        }
    }

    /// Construct a [Bm25Weight] for a single term.
    pub fn for_one_term(
        term_doc_freq: u64,
        total_num_docs: u64,
        avg_fieldnorm: Score,
    ) -> Bm25Weight {
        let idf = idf(term_doc_freq, total_num_docs);
        let mut idf_explain =
            Explanation::new("idf, computed as log(1 + (N - n + 0.5) / (n + 0.5))", idf);
        idf_explain.add_const(
            "n, number of docs containing this term",
            term_doc_freq as Score,
        );
        idf_explain.add_const("N, total number of docs", total_num_docs as Score);
        Bm25Weight::new(idf_explain, avg_fieldnorm)
    }
    /// Construct a [Bm25Weight] for a single term.
    /// This method does not carry the [Explanation] for the idf.
    pub fn for_one_term_without_explain(
        term_doc_freq: u64,
        total_num_docs: u64,
        avg_fieldnorm: Score,
    ) -> Bm25Weight {
        let idf = idf(term_doc_freq, total_num_docs);
        Bm25Weight::new_without_explain(idf, avg_fieldnorm)
    }

    pub(crate) fn new(idf_explain: Explanation, average_fieldnorm: Score) -> Bm25Weight {
        let weight = idf_explain.value() * (1.0 + K1);
        Bm25Weight {
            idf_explain: Some(idf_explain),
            weight,
            cache: compute_tf_cache(average_fieldnorm, Bm25Scoring::LegacyClassic),
            average_fieldnorm,
            scoring: Bm25Scoring::LegacyClassic,
        }
    }
    pub(crate) fn new_without_explain(idf: f32, average_fieldnorm: Score) -> Bm25Weight {
        let weight = idf * (1.0 + K1);
        Bm25Weight {
            idf_explain: None,
            weight,
            cache: compute_tf_cache(average_fieldnorm, Bm25Scoring::LegacyClassic),
            average_fieldnorm,
            scoring: Bm25Scoring::LegacyClassic,
        }
    }

    fn new_native(idf_explain: Explanation, average_fieldnorm: Score) -> Bm25Weight {
        Bm25Weight {
            weight: idf_explain.value(),
            idf_explain: Some(idf_explain),
            cache: compute_tf_cache(average_fieldnorm, Bm25Scoring::NativeLucene),
            average_fieldnorm,
            scoring: Bm25Scoring::NativeLucene,
        }
    }

    // Serialization selects an input independent of term IDF and query boost.
    pub(crate) fn for_native_block_bounds(average_fieldnorm: Score) -> Self {
        Self {
            weight: 1.0,
            idf_explain: None,
            cache: compute_tf_cache(average_fieldnorm, Bm25Scoring::NativeLucene),
            average_fieldnorm,
            scoring: Bm25Scoring::NativeLucene,
        }
    }

    /// Compute the BM25 score of a single document.
    #[inline]
    pub fn score(&self, fieldnorm_id: u8, term_freq: u32) -> Score {
        self.score_with_frequency(fieldnorm_id, term_freq as Score)
    }

    /// Score a fractional phrase frequency without rounding its distance weights.
    #[inline]
    pub(crate) fn score_with_frequency(&self, fieldnorm_id: u8, frequency: Score) -> Score {
        let norm = self.cache[fieldnorm_id as usize];
        match self.scoring {
            Bm25Scoring::LegacyClassic => self.weight * (frequency / (frequency + norm)),
            Bm25Scoring::NativeLucene => self.weight - self.weight / (1.0 + frequency * norm),
        }
    }

    /// Whether this term frequency could produce a score above `threshold`.
    ///
    /// Most documents can be rejected without the division in `score`. The
    /// comparison is made in f64 and inflates the weight to cover rounding in
    /// the f32 denominator, division, and final multiplication. Borderline
    /// cases use the exact scoring expression, including its tie behavior.
    #[inline]
    pub(crate) fn can_score_exceed(
        &self,
        fieldnorm_id: u8,
        term_freq: u32,
        threshold: Score,
    ) -> bool {
        if self.scoring == Bm25Scoring::NativeLucene {
            // The legacy algebraic predicate does not account for cancellation
            // in Lucene's subtraction formula. Use its actual rounded score.
            return self.score(fieldnorm_id, term_freq) > threshold;
        }
        let norm = self.cache[fieldnorm_id as usize];
        if self.weight.is_finite()
            && self.weight > 0.0
            && norm.is_finite()
            && norm > 0.0
            && threshold.is_finite()
            && threshold >= 0.0
        {
            // Use the f32-converted frequency: that is the value used by score().
            let freq = f64::from(term_freq as Score);
            let threshold = f64::from(threshold);
            let inflated_weight = f64::from(self.weight) * (1.0 + 8.0 * f64::from(Score::EPSILON));
            if freq * (inflated_weight - threshold) <= threshold * f64::from(norm) {
                return false;
            }
        }
        self.score(fieldnorm_id, term_freq) > threshold
    }

    /// Compute the maximum possible BM25 score given this weight.
    pub fn max_score(&self) -> Score {
        // Public constructors may receive invalid custom statistics. Such
        // normalization can exceed saturation, so no finite bound is safe.
        if !self.average_fieldnorm.is_finite() || self.average_fieldnorm <= 0.0 {
            return Score::INFINITY;
        }
        // With a nonnegative norm, tf / (tf + norm) is at most one.
        // A synthetic (length, frequency) pair is not a bound: quantized
        // lengths and token overlaps can make frequency exceed that length.
        // Negative boosts have nonpositive scores and therefore upper bound zero.
        self.weight.max(0.0)
    }

    #[cfg(test)]
    pub(crate) fn can_use_stored_block_max(&self, segment_average_fieldnorm: Score) -> bool {
        self.can_use_stored_block_max_with_selection(
            segment_average_fieldnorm,
            BlockMaxSelection::LegacyTfFactor,
        )
    }

    pub(crate) fn can_use_stored_block_max_with_selection(
        &self,
        segment_average_fieldnorm: Score,
        selection: BlockMaxSelection,
    ) -> bool {
        // Reject both migration directions: each comparator can have rounded
        // ties that resolve differently under the other scoring expression.
        matches!(
            (self.scoring, selection),
            (
                Bm25Scoring::LegacyClassic,
                BlockMaxSelection::LegacyTfFactor
            ) | (
                Bm25Scoring::NativeLucene,
                BlockMaxSelection::NativeSaturationInput
            )
        ) && self.weight.is_finite()
            && self.weight >= 0.0
            && self.average_fieldnorm.is_finite()
            && self.average_fieldnorm > 0.0
            && self.average_fieldnorm == segment_average_fieldnorm
    }

    #[inline]
    pub(crate) fn tf_factor(&self, fieldnorm_id: u8, term_freq: u32) -> Score {
        debug_assert_eq!(self.scoring, Bm25Scoring::LegacyClassic);
        let term_freq = term_freq as Score;
        let norm = self.cache[fieldnorm_id as usize];
        term_freq / (term_freq + norm)
    }

    pub(crate) fn supports_frequency_ceiling(&self) -> bool {
        self.scoring == Bm25Scoring::NativeLucene
            && self.weight.is_finite()
            && self.weight >= 0.0
            && self.average_fieldnorm.is_finite()
            && self.average_fieldnorm > 0.0
    }

    #[inline]
    pub(crate) fn native_saturation_input(&self, fieldnorm_id: u8, term_freq: u32) -> Score {
        debug_assert_eq!(self.scoring, Bm25Scoring::NativeLucene);
        // For fixed finite nonnegative weight, fl(w - w / fl(1 + x)) is
        // nondecreasing in x. Select x directly, before rounded score ties.
        term_freq as Score * self.cache[fieldnorm_id as usize]
    }

    /// Produce an [Explanation] of a BM25 score.
    pub fn explain(&self, fieldnorm_id: u8, term_freq: u32) -> Explanation {
        self.explain_with_frequency(fieldnorm_id, term_freq as Score)
    }

    pub(crate) fn explain_with_frequency(&self, fieldnorm_id: u8, term_freq: Score) -> Explanation {
        // The explain format is directly copied from Lucene's.
        // (So, Kudos to Lucene)
        let score = self.score_with_frequency(fieldnorm_id, term_freq);

        let norm = self.cache[fieldnorm_id as usize];
        let right_factor = match self.scoring {
            Bm25Scoring::LegacyClassic => term_freq / (term_freq + norm),
            Bm25Scoring::NativeLucene => 1.0 - 1.0 / (1.0 + term_freq * norm),
        };

        let mut tf_explanation = Explanation::new(
            "freq / (freq + k1 * (1 - b + b * dl / avgdl))",
            right_factor,
        );

        tf_explanation.add_const("freq, occurrences of term within document", term_freq);
        tf_explanation.add_const("k1, term saturation parameter", K1);
        tf_explanation.add_const("b, length normalization parameter", B);
        tf_explanation.add_const(
            "dl, length of field",
            FieldNormReader::id_to_fieldnorm(fieldnorm_id) as Score,
        );
        tf_explanation.add_const("avgdl, average length of field", self.average_fieldnorm);

        let mut explanation = Explanation::new("TermQuery, product of...", score);
        if self.scoring == Bm25Scoring::LegacyClassic {
            explanation.add_detail(Explanation::new("(K1+1)", K1 + 1.0));
        }
        if let Some(idf_explain) = &self.idf_explain {
            explanation.add_detail(idf_explain.clone());
        }
        explanation.add_detail(tf_explanation);
        explanation
    }
}

#[cfg(test)]
mod tests {
    use super::{idf, Bm25Weight};
    use crate::{assert_nearly_equals, Score};

    #[test]
    fn native_policy_keeps_old_pairs_untrusted_and_global_bound_conservative() {
        use crate::query::Explanation;
        for average in [0.5, 1.0, 100.0, 10_000.0, 1_000_000_000.0] {
            let native = Bm25Weight::new_native(Explanation::new("idf", 1.0), average);
            for boost in [0.0, 1.0, 3.25, -2.0] {
                let weight = native.boost_by(boost);
                assert!(!weight.can_use_stored_block_max(average));
                for norm in 0..=255 {
                    for frequency in [0, 1, 7, 128, 16_777_217, u32::MAX] {
                        assert!(weight.max_score() >= weight.score(norm, frequency));
                    }
                }
            }
        }
    }

    #[test]
    fn native_threshold_predicate_matches_actual_score_at_float_neighbors() {
        use crate::query::Explanation;
        for average in [0.5, 100.0, 1_000_000_000.0] {
            let native = Bm25Weight::new_native(Explanation::new("idf", 1.0), average);
            for boost in [0.0, 1.0, 3.25, -2.0] {
                let weight = native.boost_by(boost);
                for norm in 0..=255 {
                    for frequency in [0, 1, 7, 128, 16_777_217, u32::MAX] {
                        let score = weight.score(norm, frequency);
                        for threshold in [
                            score.next_down(),
                            score,
                            score.next_up(),
                            Score::NEG_INFINITY,
                            Score::INFINITY,
                            Score::NAN,
                        ] {
                            assert_eq!(
                                weight.can_score_exceed(norm, frequency, threshold),
                                score > threshold
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn field_statistics_custom_provider_keeps_legacy_average_and_scores() -> crate::Result<()> {
        use super::Bm25StatisticsProvider;
        use crate::schema::Field;
        use crate::Term;

        struct Custom {
            docs: u64,
            tokens: u64,
        }
        impl Bm25StatisticsProvider for Custom {
            fn total_num_tokens(&self, _: Field) -> crate::Result<u64> {
                Ok(self.tokens)
            }
            fn total_num_docs(&self) -> crate::Result<u64> {
                Ok(self.docs)
            }
            fn doc_freq(&self, _: &Term) -> crate::Result<u64> {
                Ok(self.docs.min(1))
            }
        }
        let term = Term::from_field_text(Field::from_field_id(0), "a");
        for (tokens, docs) in [
            (16_777_217, 3),
            (16_777_229, 7),
            (u64::MAX, 3),
            (0, 0),
            (1, 0),
            (0, 3),
        ] {
            let provider = Custom { docs, tokens };
            let actual = Bm25Weight::for_terms(&provider, std::slice::from_ref(&term))?;
            let average = tokens as Score / docs as Score;
            let expected = Bm25Weight::for_one_term(docs.min(1), docs, average);
            assert_eq!(actual.average_fieldnorm.to_bits(), average.to_bits());
            for norm in [0, 1, 255] {
                assert_eq!(
                    actual.score(norm, 3).to_bits(),
                    expected.score(norm, 3).to_bits()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn field_statistics_regression_sparse_population_and_ranking() -> crate::Result<()> {
        use crate::collector::TopDocs;
        use crate::query::{BooleanQuery, Occur, TermQuery};
        use crate::schema::{IndexRecordOption, Schema, TEXT};
        use crate::{Index, Term};

        let mut schema = Schema::builder();
        let sparse = schema.add_text_field("sparse", TEXT);
        let dense = schema.add_text_field("dense", TEXT);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        writer.add_document(doc!(sparse => "a", dense => "c"))?;
        writer.add_document(doc!(sparse => "a", dense => "c"))?;
        writer.add_document(doc!(dense => "b"))?;
        for _ in 0..7 {
            writer.add_document(doc!(dense => "c"))?;
        }
        writer.commit()?;
        let searcher = index.reader()?.searcher();
        let sparse_term = Term::from_field_text(sparse, "a");
        let weight = Bm25Weight::for_terms(&searcher, &[sparse_term.clone()])?;
        let query = BooleanQuery::new(vec![
            (
                Occur::Should,
                Box::new(TermQuery::new(sparse_term, IndexRecordOption::WithFreqs)),
            ),
            (
                Occur::Should,
                Box::new(TermQuery::new(
                    Term::from_field_text(dense, "b"),
                    IndexRecordOption::WithFreqs,
                )),
            ),
        ]);
        let top = searcher.search(&query, &TopDocs::with_limit(3).order_by_score())?;
        assert_eq!(
            top[0].1.doc_id, 2,
            "field-sensitive IDF must rank dense b above ubiquitous sparse a"
        );
        let expected = Bm25Weight::new_native(super::native_idf_explanation(2, 2), 1.0);
        assert_eq!(weight.average_fieldnorm, 1.0);
        assert_eq!(weight.score(1, 1), expected.score(1, 1));
        Ok(())
    }

    #[test]
    fn field_statistics_regression_deletion_merge_exact_tokens() -> crate::Result<()> {
        use crate::indexer::NoMergePolicy;
        use crate::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions};
        use crate::{Index, Term};

        for norms in [false, true] {
            let options = TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqs)
                    .set_fieldnorms(norms),
            );
            let mut schema = Schema::builder();
            let text = schema.add_text_field("text", options);
            let index = Index::create_in_ram(schema.build());
            let mut writer = index.writer_for_tests()?;
            writer.set_merge_policy(Box::new(NoMergePolicy));
            writer.add_document(doc!(text => "keep ".repeat(73)))?;
            writer.add_document(doc!(text => "delete"))?;
            writer.commit()?;
            writer.delete_term(Term::from_field_text(text, "delete"));
            writer.commit()?;
            let reader = index.reader()?;
            assert_eq!(
                reader
                    .searcher()
                    .segment_reader(0)
                    .inverted_index(text)?
                    .total_num_tokens(),
                74
            );
            let segments = index.searchable_segment_ids()?;
            writer.merge(&segments).wait()?;
            reader.reload()?;
            assert_eq!(
                reader
                    .searcher()
                    .segment_reader(0)
                    .inverted_index(text)?
                    .total_num_tokens(),
                73,
                "retained frequencies must remain exact with norms={norms}"
            );
        }
        Ok(())
    }

    #[test]
    fn field_statistics_regression_basic_unique_tokens_and_norm() -> crate::Result<()> {
        use crate::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions};
        use crate::Index;

        let options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::Basic),
        );
        let mut schema = Schema::builder();
        let text = schema.add_text_field("text", options);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        writer.add_document(doc!(text => "a a a b", text => "a b"))?;
        writer.commit()?;
        let searcher = index.reader()?.searcher();
        let segment = searcher.segment_reader(0);
        assert_eq!(segment.get_fieldnorms_reader(text)?.fieldnorm(0), 2);
        assert_eq!(segment.inverted_index(text)?.total_num_tokens(), 2);
        Ok(())
    }

    #[test]
    fn test_idf() {
        let score: Score = 2.0;
        assert_nearly_equals!(idf(1, 2), score.ln());
    }

    #[test]
    fn test_max_score_bounds_quantized_lengths_and_overlapping_tokens() {
        for avg_fieldnorm in [0.01, 1.0, 100.0, 10_000.0] {
            let base = Bm25Weight::for_one_term_without_explain(2, 256, avg_fieldnorm);
            for boost in [-1.0, 0.0, 0.5, 1.0, 10.0] {
                let weight = base.boost_by(boost);
                for fieldnorm_id in 0..=u8::MAX {
                    // Do not assume frequency <= decoded fieldnorm: fieldnorms
                    // round down and overlapping tokens may repeat a term.
                    for term_freq in [1, 2, 62, 63, 128, 100_000, u32::MAX] {
                        let score = weight.score(fieldnorm_id, term_freq);
                        assert!(
                            score <= weight.max_score(),
                            "avg={avg_fieldnorm} boost={boost} norm={fieldnorm_id} \
                             tf={term_freq}: {score} > {}",
                            weight.max_score()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_invalid_average_has_no_finite_score_bound() {
        for average in [0.0, -1.0, f32::NEG_INFINITY, f32::INFINITY, f32::NAN] {
            for boost in [-1.0, 0.0, 1.0] {
                let weight = Bm25Weight::for_one_term(1, 10, average).boost_by(boost);
                assert_eq!(weight.max_score(), f32::INFINITY);
                assert!(!weight.can_use_stored_block_max(average));
            }
        }
    }

    #[test]
    fn test_can_score_exceed_matches_exact_scoring() {
        for avg_fieldnorm in [1.0, 100.0, 10_000.0] {
            let base = Bm25Weight::for_one_term_without_explain(10_000, 1_000_000, avg_fieldnorm);
            for boost in [0.0, 1e-20, 1.0, 1e20, -1.0, f32::INFINITY] {
                let weight = base.boost_by(boost);
                for fieldnorm_id in 0..=u8::MAX {
                    for term_freq in [0, 1, 2, 3, 8, 127, 128, 100_000, u32::MAX] {
                        let score = weight.score(fieldnorm_id, term_freq);
                        let thresholds = [
                            -1.0,
                            0.0,
                            f32::MIN_POSITIVE,
                            f32::from_bits(score.to_bits().saturating_sub(1)),
                            score,
                            f32::from_bits(score.to_bits().saturating_add(1)),
                            f32::MAX,
                            f32::INFINITY,
                            f32::NAN,
                        ];
                        for threshold in thresholds {
                            assert_eq!(
                                weight.can_score_exceed(fieldnorm_id, term_freq, threshold),
                                score > threshold,
                                "boost={boost:?} norm={fieldnorm_id} freq={term_freq} \
                                 threshold={threshold:?} score={score:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
