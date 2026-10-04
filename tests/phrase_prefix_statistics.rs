//! Public statistics-provider contract for the scored multi-term prefix route.
//! These checks preserve existing prefix matching/scoring semantics; they do not
//! assert equivalence with Lucene MultiPhraseQuery.
use std::sync::atomic::{AtomicUsize, Ordering};

use tantivy::collector::{Count, TopDocs};
use tantivy::query::{
    Bm25FieldStatistics, Bm25StatisticsProvider, Bm25Weight, EnableScoring, PhrasePrefixQuery,
    PhraseQuery, Query,
};
use tantivy::schema::{Field, Schema, TEXT};
use tantivy::{doc, DocAddress, DocSet, Index, Searcher, Term, TERMINATED};

fn fixture() -> tantivy::Result<(Index, Field)> {
    let mut schema = Schema::builder();
    let text = schema.add_text_field("text", TEXT);
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    for text_value in [
        "alpha beta gamma alpha beta gamma",
        "alpha beta garden extra",
        "alpha extra beta gamma",
    ] {
        writer.add_document(doc!(text => text_value))?;
    }
    writer.commit()?;
    Ok((index, text))
}

fn terms(text: Field) -> Vec<Term> {
    ["alpha", "beta"]
        .map(|value| Term::from_field_text(text, value))
        .to_vec()
}

fn prefix(text: Field) -> PhrasePrefixQuery {
    let mut fixed = terms(text);
    fixed.push(Term::from_field_text(text, "gam"));
    // Use one suffix here, so the fixed phrase and full prefix have identical
    // phrase frequencies on the matching documents. A second prefix fixture
    // below also exercises two suffix expansions.
    PhrasePrefixQuery::new(fixed)
}

fn two_suffix_prefix(text: Field) -> PhrasePrefixQuery {
    let mut fixed = terms(text);
    fixed.push(Term::from_field_text(text, "ga"));
    PhrasePrefixQuery::new(fixed)
}

fn custom_doc_freq(term: &Term, text: Field) -> tantivy::Result<u64> {
    assert_eq!(term.field(), text);
    match term.serialized_value_bytes() {
        b"alpha" => Ok(2),
        b"beta" => Ok(7),
        other => Err(tantivy::TantivyError::InvalidArgument(format!(
            "unexpected statistics request: {other:?}"
        ))),
    }
}

struct ScalarStatistics {
    field: Field,
    scalar_calls: AtomicUsize,
    frequency_calls: AtomicUsize,
}
impl ScalarStatistics {
    fn new(field: Field) -> Self {
        Self {
            field,
            scalar_calls: AtomicUsize::new(0),
            frequency_calls: AtomicUsize::new(0),
        }
    }
}
impl Bm25StatisticsProvider for ScalarStatistics {
    fn total_num_docs(&self) -> tantivy::Result<u64> {
        self.scalar_calls.fetch_add(1, Ordering::Relaxed);
        Ok(100)
    }
    fn total_num_tokens(&self, field: Field) -> tantivy::Result<u64> {
        assert_eq!(field, self.field);
        self.scalar_calls.fetch_add(1, Ordering::Relaxed);
        Ok(600)
    }
    fn doc_freq(&self, term: &Term) -> tantivy::Result<u64> {
        self.frequency_calls.fetch_add(1, Ordering::Relaxed);
        custom_doc_freq(term, self.field)
    }
}

struct CoherentStatistics {
    field: Field,
    snapshot_calls: AtomicUsize,
    frequency_calls: AtomicUsize,
}
impl CoherentStatistics {
    fn new(field: Field) -> Self {
        Self {
            field,
            snapshot_calls: AtomicUsize::new(0),
            frequency_calls: AtomicUsize::new(0),
        }
    }
}
impl Bm25StatisticsProvider for CoherentStatistics {
    fn total_num_docs(&self) -> tantivy::Result<u64> {
        Err(tantivy::TantivyError::InvalidArgument(
            "coherent snapshot must bypass scalar population".into(),
        ))
    }
    fn total_num_tokens(&self, _: Field) -> tantivy::Result<u64> {
        Err(tantivy::TantivyError::InvalidArgument(
            "coherent snapshot must bypass scalar token total".into(),
        ))
    }
    fn doc_freq(&self, term: &Term) -> tantivy::Result<u64> {
        self.frequency_calls.fetch_add(1, Ordering::Relaxed);
        custom_doc_freq(term, self.field)
    }
    fn field_statistics(&self, field: Field) -> tantivy::Result<Bm25FieldStatistics> {
        assert_eq!(field, self.field);
        self.snapshot_calls.fetch_add(1, Ordering::Relaxed);
        Ok(Bm25FieldStatistics::new(100, 600))
    }
}

fn top(
    searcher: &Searcher,
    query: &dyn Query,
    statistics: &dyn Bm25StatisticsProvider,
) -> tantivy::Result<Vec<(f32, DocAddress)>> {
    searcher.search_with_statistics_provider(
        query,
        &TopDocs::with_limit(3).order_by_score(),
        statistics,
    )
}

#[test]
fn phrase_prefix_uses_historical_scalar_provider_for_scores() -> tantivy::Result<()> {
    let (index, text) = fixture()?;
    let searcher = index.reader()?.searcher();
    let statistics = ScalarStatistics::new(text);
    let query = two_suffix_prefix(text);
    let actual = top(&searcher, &query, &statistics)?;
    assert_eq!(statistics.scalar_calls.load(Ordering::Relaxed), 2);
    assert_eq!(statistics.frequency_calls.load(Ordering::Relaxed), 2);
    let fixed_phrase = PhraseQuery::new(terms(text));
    let expected = top(&searcher, &fixed_phrase, &statistics)?;
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 2);
    let native = top(&searcher, &query, &searcher)?;
    assert_ne!(actual[0].0.to_bits(), native[0].0.to_bits());
    Ok(())
}

#[test]
fn phrase_prefix_uses_coherent_snapshot_and_custom_term_frequencies() -> tantivy::Result<()> {
    let (index, text) = fixture()?;
    let searcher = index.reader()?.searcher();
    let statistics = CoherentStatistics::new(text);
    let query = prefix(text);
    let actual = top(&searcher, &query, &statistics)?;
    assert_eq!(statistics.snapshot_calls.load(Ordering::Relaxed), 1);
    assert_eq!(statistics.frequency_calls.load(Ordering::Relaxed), 2);
    // The ordinary phrase path is an independent public query control that
    // already honors the same coherent snapshot and per-term statistics.
    let expected = top(&searcher, &PhraseQuery::new(terms(text)), &statistics)?;
    let expected_doc_zero = expected
        .iter()
        .find(|(_, address)| address.doc_id == 0)
        .unwrap();
    assert_eq!(actual, vec![*expected_doc_zero]);
    assert!(actual[0].0 > top(&searcher, &query, &searcher)?[0].0);
    Ok(())
}

fn explanation_value(node: &serde_json::Value, description: &str) -> Option<f64> {
    if node["description"].as_str() == Some(description) {
        return node["value"].as_f64();
    }
    node["details"]
        .as_array()?
        .iter()
        .find_map(|child| explanation_value(child, description))
}

#[test]
fn phrase_prefix_explanation_uses_provider_average_and_classic_policy() -> tantivy::Result<()> {
    let (index, text) = fixture()?;
    let searcher = index.reader()?.searcher();
    let statistics = CoherentStatistics::new(text);
    let query = prefix(text);
    let weight = query.weight(EnableScoring::enabled_from_statistics_provider(
        &statistics,
        &searcher,
    ))?;
    let explanation = weight.explain(searcher.segment_reader(0), 0)?;
    let expected = Bm25Weight::for_terms(&statistics, &terms(text))?.explain(6, 2);
    assert_eq!(explanation.value().to_bits(), expected.value().to_bits());
    let json: serde_json::Value = serde_json::from_str(&explanation.to_pretty_json()).unwrap();
    assert_eq!(
        explanation_value(&json, "avgdl, average length of field"),
        Some(6.0)
    );
    assert_eq!(
        explanation_value(&json, "(K1+1)").map(|value| (value as f32).to_bits()),
        Some(2.2f32.to_bits())
    );
    Ok(())
}

#[test]
fn phrase_prefix_native_scores_and_disabled_counts_keep_current_behavior() -> tantivy::Result<()> {
    let (index, text) = fixture()?;
    let searcher = index.reader()?.searcher();
    let query = two_suffix_prefix(text);
    let native = top(&searcher, &query, &searcher)?;
    let control = top(&searcher, &PhraseQuery::new(terms(text)), &searcher)?;
    assert_eq!(native, control);
    let native_weight = Bm25Weight::for_terms(&searcher, &terms(text))?;
    for (score, address) in native {
        let (norm, frequency) = if address.doc_id == 0 { (6, 2) } else { (4, 1) };
        assert_eq!(
            score.to_bits(),
            native_weight.score(norm, frequency).to_bits()
        );
        let explanation = query.explain(&searcher, address)?;
        assert_eq!(explanation.value().to_bits(), score.to_bits());
        assert!(!explanation.to_pretty_json().contains("(K1+1)"));
    }
    // Both scalar methods of this provider return errors. COUNT must skip all
    // statistics methods, including its coherent snapshot, because scores are unused.
    let unused = CoherentStatistics::new(text);
    assert_eq!(
        searcher.search_with_statistics_provider(&query, &Count, &unused)?,
        2
    );
    assert_eq!(unused.snapshot_calls.load(Ordering::Relaxed), 0);
    assert_eq!(unused.frequency_calls.load(Ordering::Relaxed), 0);
    let disabled = query.weight(EnableScoring::disabled_from_searcher(&searcher))?;
    let mut scorer = disabled.scorer(searcher.segment_reader(0), 1.0)?;
    let mut docs = Vec::new();
    while scorer.doc() != TERMINATED {
        docs.push(scorer.doc());
        scorer.advance();
    }
    assert_eq!(docs, [0, 1]);
    Ok(())
}
