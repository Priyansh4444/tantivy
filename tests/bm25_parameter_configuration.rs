//! Configuration ownership and provider authority through public search APIs.
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{
    Bm25FieldStatistics, Bm25Parameters, Bm25StatisticsProvider, BooleanQuery, BoostQuery,
    EnableScoring, Occur, PhrasePrefixQuery, PhraseQuery, Query, TermQuery,
};
use tantivy::schema::{Field, IndexRecordOption, Schema, TEXT};
use tantivy::{doc, DocAddress, Index, Searcher, Term};

fn parameters(k1: f32, b: f32) -> Bm25Parameters {
    Bm25Parameters::new(k1, b).unwrap()
}

fn fixture() -> tantivy::Result<(Index, Field, Field)> {
    let mut schema = Schema::builder();
    let title = schema.add_text_field("title", TEXT);
    let body = schema.add_text_field("body", TEXT);
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    // Same physical corpus as the pinned Java parameter-reference term fixture.
    for value in ["alpha alpha beta", "beta", "alpha beta"] {
        writer.add_document(doc!(title => value, body => value))?;
    }
    writer.commit()?;
    Ok((index, title, body))
}

fn term(field: Field) -> TermQuery {
    TermQuery::new(
        Term::from_field_text(field, "alpha"),
        IndexRecordOption::WithFreqs,
    )
}

fn top(searcher: &Searcher, query: &dyn Query) -> tantivy::Result<Vec<(f32, DocAddress)>> {
    searcher.search(query, &TopDocs::with_limit(3).order_by_score())
}

fn score(searcher: &Searcher, query: &dyn Query, doc: u32) -> tantivy::Result<u32> {
    Ok(query
        .explain(searcher, DocAddress::new(0, doc))?
        .value()
        .to_bits())
}

fn detail_value(node: &serde_json::Value, description: &str) -> Option<f64> {
    if node["description"].as_str() == Some(description) {
        return node["value"].as_f64();
    }
    node["details"]
        .as_array()?
        .iter()
        .find_map(|child| detail_value(child, description))
}

#[test]
fn per_field_parameters_survive_cloning_and_global_reconfiguration() -> tantivy::Result<()> {
    let (index, title, body) = fixture()?;
    let reader = index.reader()?;
    let base = reader.searcher();
    let configured = base
        .clone()
        .with_bm25_parameters(parameters(2.5, 1.0))
        .with_field_bm25_parameters(title, parameters(0.9, 0.4))?;
    let preserved = configured.clone();
    let changed = configured.with_bm25_parameters(parameters(0.0, 0.75));

    // Literal bits come from Lucene 10.4 reference.csv; each field uses its
    // own profile, and changing the default must retain the title override.
    assert_eq!(score(&preserved, &term(title), 0)?, 0x3e9c42ce);
    assert_eq!(score(&preserved, &term(body), 0)?, 0x3e27672c);
    assert_eq!(score(&changed, &term(title), 0)?, 0x3e9c42ce);
    assert_eq!(score(&changed, &term(body), 0)?, 0x3ef0a451);
    assert_eq!(score(&base, &term(title), 0)?, 0x3e83dbca);
    assert_eq!(score(&reader.searcher(), &term(title), 0)?, 0x3e83dbca);

    let compound = BooleanQuery::new(vec![
        (Occur::Must, Box::new(term(title))),
        (Occur::Must, Box::new(term(body))),
    ]);
    let expected = f32::from_bits(0x3e9c42ce) + f32::from_bits(0x3e27672c);
    let hits = top(&preserved, &compound)?;
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].1, DocAddress::new(0, 0));
    assert_eq!(hits[0].0.to_bits(), expected.to_bits());
    assert_eq!(score(&preserved, &compound, 0)?, expected.to_bits());
    assert_eq!(preserved.search(&compound, &Count)?, 2);

    let actual = changed.field_statistics(title)?.parameters();
    assert_eq!(
        (actual.k1().to_bits(), actual.b().to_bits()),
        (0.9f32.to_bits(), 0.4f32.to_bits())
    );
    let fresh = reader.searcher().field_statistics(body)?.parameters();
    assert_eq!(
        (fresh.k1().to_bits(), fresh.b().to_bits()),
        (
            Bm25Parameters::DEFAULT.k1().to_bits(),
            Bm25Parameters::DEFAULT.b().to_bits()
        )
    );
    assert!(base
        .with_field_bm25_parameters(Field::from_field_id(u32::MAX), Bm25Parameters::DEFAULT)
        .is_err());
    Ok(())
}

struct Historical;
impl Bm25StatisticsProvider for Historical {
    fn total_num_tokens(&self, _: Field) -> tantivy::Result<u64> {
        Ok(16)
    }
    fn total_num_docs(&self) -> tantivy::Result<u64> {
        Ok(8)
    }
    fn doc_freq(&self, _: &Term) -> tantivy::Result<u64> {
        Ok(2)
    }
}

#[test]
fn supplied_historical_provider_is_authoritative_over_searcher_parameters() -> tantivy::Result<()> {
    let (index, title, _) = fixture()?;
    let base = index.reader()?.searcher();
    let configured = base
        .clone()
        .with_bm25_parameters(parameters(0.0, 0.75))
        .with_field_bm25_parameters(title, parameters(2.5, 1.0))?;
    let query = term(title);
    let collector = TopDocs::with_limit(3).order_by_score();
    let baseline = base.search_with_statistics_provider(&query, &collector, &Historical)?;
    let actual = configured.search_with_statistics_provider(&query, &collector, &Historical)?;
    assert_eq!(actual, baseline);
    assert_ne!(
        actual[0].0.to_bits(),
        top(&configured, &query)?[0].0.to_bits()
    );

    let weight = query.weight(EnableScoring::enabled_from_statistics_provider(
        &Historical,
        &configured,
    ))?;
    let explanation = weight.explain(configured.segment_reader(0), 0)?;
    assert_eq!(explanation.value().to_bits(), actual[0].0.to_bits());
    let json = serde_json::from_str(&explanation.to_pretty_json()).unwrap();
    assert_eq!(
        detail_value(&json, "k1, term saturation parameter").map(|value| (value as f32).to_bits()),
        Some(1.2f32.to_bits())
    );
    assert_eq!(
        detail_value(&json, "b, length normalization parameter"),
        Some(0.75)
    );
    assert!(explanation.to_pretty_json().contains("(K1+1)"));
    Ok(())
}

struct ExplicitClassic;
impl Bm25StatisticsProvider for ExplicitClassic {
    fn total_num_tokens(&self, _: Field) -> tantivy::Result<u64> {
        Ok(16)
    }
    fn total_num_docs(&self) -> tantivy::Result<u64> {
        Ok(8)
    }
    fn doc_freq(&self, _: &Term) -> tantivy::Result<u64> {
        Ok(2)
    }
    fn field_statistics(&self, _: Field) -> tantivy::Result<Bm25FieldStatistics> {
        Ok(Bm25FieldStatistics::new(8, 16).with_parameters(parameters(2.0, 0.0)))
    }
}

#[test]
fn explicit_custom_snapshot_keeps_classic_ratio_and_its_own_parameters() -> tantivy::Result<()> {
    let (index, title, _) = fixture()?;
    let searcher = index
        .reader()?
        .searcher()
        .with_bm25_parameters(parameters(0.0, 0.75));
    let query = term(title);
    let hits = searcher.search_with_statistics_provider(
        &query,
        &TopDocs::with_limit(3).order_by_score(),
        &ExplicitClassic,
    )?;
    assert_eq!(
        hits.iter()
            .map(|(_, address)| address.doc_id)
            .collect::<Vec<_>>(),
        [0, 2]
    );
    // With b=0 and k1=2, classic TF2 contributes 3*2/(2+2)=1.5;
    // TF1 contributes 3*1/(1+2)=1. Native scores omit that numerator.
    let idf = 3.6f64.ln();
    assert!((f64::from(hits[0].0) - idf * 1.5).abs() < 1e-6);
    assert!((f64::from(hits[1].0) - idf).abs() < 1e-6);
    let weight = query.weight(EnableScoring::enabled_from_statistics_provider(
        &ExplicitClassic,
        &searcher,
    ))?;
    let explanation = weight.explain(searcher.segment_reader(0), 0)?;
    assert_eq!(explanation.value().to_bits(), hits[0].0.to_bits());
    let json = serde_json::from_str(&explanation.to_pretty_json()).unwrap();
    assert_eq!(
        detail_value(&json, "k1, term saturation parameter"),
        Some(2.0)
    );
    assert_eq!(
        detail_value(&json, "b, length normalization parameter"),
        Some(0.0)
    );
    assert_eq!(detail_value(&json, "(K1+1)"), Some(3.0));
    Ok(())
}

#[test]
fn exact_sloppy_boost_and_prefix_queries_use_configured_statistics() -> tantivy::Result<()> {
    let mut schema = Schema::builder();
    let text = schema.add_text_field("text", TEXT);
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    for value in ["alpha beta gamma", "alpha x beta gamma"] {
        writer.add_document(doc!(text => value))?;
    }
    writer.commit()?;
    let base = index.reader()?.searcher();
    let configured = base
        .clone()
        .with_field_bm25_parameters(text, parameters(0.0, 0.75))?;
    let terms = ["alpha", "beta"]
        .map(|value| Term::from_field_text(text, value))
        .to_vec();
    let exact = PhraseQuery::new(terms.clone());
    let mut sloppy = PhraseQuery::new(terms.clone());
    sloppy.set_slop(1);
    // k1=0 removes frequency/length effects for these nonempty matching fields.
    // Each term's native IDF is log(1 + .5/2.5), rounded before the phrase sum.
    let single_idf = 1.2f64.ln() as f32;
    let expected = single_idf + single_idf;
    let exact_hits = top(&configured, &exact)?;
    assert_eq!(exact_hits.len(), 1);
    assert_eq!(exact_hits[0].0.to_bits(), expected.to_bits());
    assert_eq!(exact_hits[0].1, DocAddress::new(0, 0));
    assert_ne!(
        exact_hits[0].0.to_bits(),
        top(&base, &exact)?[0].0.to_bits()
    );
    let sloppy_hits = top(&configured, &sloppy)?;
    assert_eq!(sloppy_hits.len(), 2);
    for (actual, address) in sloppy_hits {
        assert_eq!(actual.to_bits(), expected.to_bits());
        assert_eq!(
            sloppy.explain(&configured, address)?.value().to_bits(),
            expected.to_bits()
        );
    }
    let boosted = BoostQuery::new(Box::new(sloppy), 3.25);
    for (actual, address) in top(&configured, &boosted)? {
        assert_eq!(actual.to_bits(), (expected * 3.25).to_bits());
        assert_eq!(
            boosted.explain(&configured, address)?.value().to_bits(),
            actual.to_bits()
        );
    }

    let mut prefix_terms = terms;
    prefix_terms.push(Term::from_field_text(text, "gam"));
    let prefix = PhrasePrefixQuery::new(prefix_terms);
    // Existing prefix scores use fixed terms. This fixture has one suffix and
    // identical frequency to the exact fixed phrase, making that a control.
    assert_eq!(top(&configured, &prefix)?, exact_hits);
    assert_eq!(score(&configured, &prefix, 0)?, expected.to_bits());
    let collector = TopDocs::with_limit(3).order_by_score();
    assert_eq!(
        configured.search_with_statistics_provider(&prefix, &collector, &Historical)?,
        base.search_with_statistics_provider(&prefix, &collector, &Historical)?
    );
    assert_eq!(configured.search(&prefix, &Count)?, 1);
    Ok(())
}
