//! Raw norms/scores from pinned Lucene 10.4 OverlapNormReference.java.
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{Bm25StatisticsProvider, Query, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions};
use tantivy::tokenizer::{PreTokenizedString, Token};
use tantivy::{DocAddress, Index, TantivyDocument, Term};

fn tokens(values: &[(&str, usize, usize)]) -> PreTokenizedString {
    PreTokenizedString {
        text: values
            .iter()
            .map(|(term, _, _)| *term)
            .collect::<Vec<_>>()
            .join(" "),
        tokens: values
            .iter()
            .enumerate()
            .map(|(offset, (term, position, length))| Token {
                text: (*term).to_owned(),
                position: *position,
                position_length: *length,
                offset_from: offset,
                offset_to: offset + 1,
            })
            .collect(),
    }
}

fn standard() -> Vec<Vec<PreTokenizedString>> {
    vec![
        vec![tokens(&[
            ("alpha", 0, 1),
            ("synonym", 0, 1),
            ("beta", 1, 1),
        ])],
        vec![tokens(&[("alpha", 0, 1), ("beta", 1, 1)])],
    ]
}

fn indexing() -> TextFieldIndexing {
    TextFieldIndexing::default().set_index_option(IndexRecordOption::WithFreqsAndPositions)
}

fn fixture(
    options: TextFieldIndexing,
    values: Vec<Vec<PreTokenizedString>>,
) -> tantivy::Result<(Index, Field)> {
    let mut schema = Schema::builder();
    let field = schema.add_text_field("text", TextOptions::default().set_indexing_options(options));
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    for values in values {
        let mut doc = TantivyDocument::default();
        for value in values {
            doc.add_pre_tokenized_text(field, value);
        }
        writer.add_document(doc)?;
    }
    writer.commit()?;
    Ok((index, field))
}

fn check(index: &Index, field: Field, norms: &[u32], scores: &[u32]) -> tantivy::Result<()> {
    let searcher = index.reader()?.searcher();
    let statistics = searcher.field_statistics(field)?;
    assert_eq!(
        (statistics.doc_count(), statistics.sum_total_term_freq()),
        (2, 5)
    );
    let term = Term::from_field_text(field, "alpha");
    assert_eq!(searcher.doc_freq(&term)?, 2);
    let query = TermQuery::new(term, IndexRecordOption::WithFreqs);
    assert_eq!(searcher.search(&query, &Count)?, 2);
    let fieldnorms = searcher.segment_reader(0).get_fieldnorms_reader(field)?;
    for (doc, expected) in norms.iter().enumerate() {
        assert_eq!(
            fieldnorms.fieldnorm(doc as u32),
            *expected,
            "physical norm doc {doc}"
        );
        assert_eq!(fieldnorms.fieldnorm_id(doc as u32), *expected as u8);
    }
    let top = searcher.search(&query, &TopDocs::with_limit(2).order_by_score())?;
    for (score, address) in top {
        assert_eq!(score.to_bits(), scores[address.doc_id as usize]);
        assert_eq!(
            query.explain(&searcher, address)?.value().to_bits(),
            score.to_bits()
        );
    }
    Ok(())
}

#[test]
fn native_default_overlap_norms_match_lucene() -> tantivy::Result<()> {
    let (index, field) = fixture(indexing(), standard())?;
    check(&index, field, &[2, 2], &[1035524425, 1035524425])
}

#[test]
fn native_default_overlap_scores_match_lucene() -> tantivy::Result<()> {
    let (index, field) = fixture(indexing(), standard())?;
    let searcher = index.reader()?.searcher();
    let query = TermQuery::new(
        Term::from_field_text(field, "alpha"),
        IndexRecordOption::WithFreqs,
    );
    assert_eq!(
        query
            .explain(&searcher, DocAddress::new(0, 0))?
            .value()
            .to_bits(),
        1035524425
    );
    Ok(())
}

#[test]
fn legacy_boolean_schema_overlap_norms_and_basic_control() -> tantivy::Result<()> {
    let legacy: TextFieldIndexing =
        serde_json::from_str(r#"{"record":"position","fieldnorms":true,"tokenizer":"default"}"#)?;
    let (index, field) = fixture(legacy, standard())?;
    check(&index, field, &[3, 2], &[1033692019, 1035524425])?;
    let (basic, field) = fixture(
        indexing().set_index_option(IndexRecordOption::Basic),
        standard(),
    )?;
    check(&basic, field, &[3, 2], &[1033692019, 1035524425])
}
