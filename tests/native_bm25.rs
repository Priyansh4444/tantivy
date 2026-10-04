//! Native values below were captured from the unmodified Lucene 10.4 AST adapter.
//! They exercise public query scoring rather than changing comparator tolerances.
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{
    Bm25StatisticsProvider, Bm25Weight, BoostQuery, PhraseQuery, Query, TermQuery,
};
use tantivy::schema::{Field, IndexRecordOption, Schema, TEXT};
use tantivy::{doc, DocAddress, Index, Score, Term};

fn index_text(documents: &[String]) -> tantivy::Result<(Index, Field)> {
    let mut schema = Schema::builder();
    let field = schema.add_text_field("text", TEXT);
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    for document in documents {
        writer.add_document(doc!(field => document.as_str()))?;
    }
    writer.commit()?;
    Ok((index, field))
}

#[test]
fn native_term_boost_and_explanations_match_lucene_raw() -> tantivy::Result<()> {
    let documents = ["alpha alpha beta", "beta", "alpha beta"].map(str::to_owned);
    let (index, field) = index_text(&documents)?;
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let term = Term::from_field_text(field, "alpha");
    let query = TermQuery::new(term.clone(), IndexRecordOption::WithFreqs);
    let weight = Bm25Weight::for_terms(&searcher, &[term])?;
    for (doc, length, frequency, expected) in [(0, 3, 2, 0.25753623f32), (2, 2, 1, 0.21363801f32)] {
        assert_eq!(
            weight.score(length, frequency).to_bits(),
            expected.to_bits()
        );
        let explanation = query.explain(&searcher, DocAddress::new(0, doc))?;
        assert_eq!(explanation.value().to_bits(), expected.to_bits());
        assert!(!explanation.to_pretty_json().contains("(K1+1)"));
    }
    let boosted = BoostQuery::new(Box::new(query), 3.25);
    assert_eq!(searcher.search(&boosted, &Count)?, 2);
    let top = searcher.search(&boosted, &TopDocs::with_limit(2).order_by_score())?;
    assert_eq!(top[0].0.to_bits(), 0.83699274f32.to_bits());
    assert_eq!(top[1].0.to_bits(), 0.69432354f32.to_bits());
    assert_eq!(
        weight.boost_by(3.25).score(3, 2).to_bits(),
        top[0].0.to_bits()
    );
    Ok(())
}

#[test]
fn native_sloppy_fractional_frequency_matches_lucene_raw() -> tantivy::Result<()> {
    let documents = ["alpha x beta", "alpha beta x"].map(str::to_owned);
    let (index, field) = index_text(&documents)?;
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let mut query = PhraseQuery::new(vec![
        Term::from_field_text(field, "alpha"),
        Term::from_field_text(field, "beta"),
    ]);
    query.set_slop(1);
    assert_eq!(searcher.search(&query, &Count)?, 2);
    let top = searcher.search(&query, &TopDocs::with_limit(2).order_by_score())?;
    assert_eq!(top[0].1.doc_id, 1);
    assert_eq!(top[0].0.to_bits(), 0.16574687f32.to_bits());
    assert_eq!(top[1].1.doc_id, 0);
    assert_eq!(top[1].0.to_bits(), 0.10724798f32.to_bits());
    for (score, address) in top {
        assert_eq!(
            query.explain(&searcher, address)?.value().to_bits(),
            score.to_bits()
        );
    }
    Ok(())
}

#[test]
fn historical_custom_provider_and_public_constructors_keep_score_bits() -> tantivy::Result<()> {
    struct Historical;
    impl Bm25StatisticsProvider for Historical {
        fn total_num_tokens(&self, _: Field) -> tantivy::Result<u64> {
            Ok(987_654_321)
        }
        fn total_num_docs(&self) -> tantivy::Result<u64> {
            Ok(123_456_789)
        }
        fn doc_freq(&self, _: &Term) -> tantivy::Result<u64> {
            Ok(987_654)
        }
    }
    let mut schema = Schema::builder();
    let field = schema.add_text_field("text", TEXT);
    let term = Term::from_field_text(field, "alpha");
    let actual = Bm25Weight::for_terms(&Historical, &[term])?;
    let average = 987_654_321u64 as Score / 123_456_789u64 as Score;
    let expected = Bm25Weight::for_one_term(987_654, 123_456_789, average);
    let without_explanation =
        Bm25Weight::for_one_term_without_explain(987_654, 123_456_789, average);
    for boost in [0.0, 1.0, 3.25, -2.0] {
        for norm in 0..=255 {
            for frequency in [1, 2, 7, u32::MAX] {
                let expected_bits = expected.boost_by(boost).score(norm, frequency).to_bits();
                assert_eq!(
                    actual.boost_by(boost).score(norm, frequency).to_bits(),
                    expected_bits
                );
                assert_eq!(
                    without_explanation
                        .boost_by(boost)
                        .score(norm, frequency)
                        .to_bits(),
                    expected_bits
                );
            }
        }
    }
    assert!(expected.explain(59, 1).to_pretty_json().contains("(K1+1)"));
    Ok(())
}

#[test]
fn native_reciprocal_score_resolves_legacy_selected_pair_tie() -> tantivy::Result<()> {
    // Physical token sum is 1600 over 16 populated fields: native average 100.
    let mut documents = vec![
        format!("alpha {}", "padding ".repeat(111)),
        format!("{}{}", "alpha ".repeat(7), "padding ".repeat(977)),
    ];
    documents.extend((0..14).map(|_| "padding ".repeat(36)));
    let (index, field) = index_text(&documents)?;
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let native = Bm25Weight::for_terms(&searcher, &[Term::from_field_text(field, "alpha")])?;
    let idf = (1.0f64 + (16.0 - 2.0 + 0.5) / (2.0 + 0.5)).ln() as f32;
    let unit_weight = native.boost_by(1.0 / idf);
    let first = unit_weight.score(59, 1);
    let last = unit_weight.score(87, 7);
    assert_eq!(first.to_bits(), 0x3eddd64c);
    assert_eq!(last.to_bits(), 0x3eddd64a);
    assert!(
        first > last,
        "the old last-on-tie selected pair cannot bound native scoring"
    );
    let legacy = Bm25Weight::for_one_term(2, 16, 100.0);
    assert_eq!(legacy.score(59, 1).to_bits(), legacy.score(87, 7).to_bits());
    Ok(())
}
