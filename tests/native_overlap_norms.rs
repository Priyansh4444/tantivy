//! Raw norms/scores from pinned Lucene 10.4 OverlapNormReference.java.
use tantivy::collector::{Count, TopDocs};
use tantivy::merge_policy::NoMergePolicy;
use tantivy::query::{
    Bm25StatisticsProvider, Bm25Weight, BoostQuery, EnableScoring, Query, TermQuery,
};
use tantivy::schema::{
    Field, FieldNormPolicy, IndexRecordOption, NumericOptions, Schema, TextFieldIndexing,
    TextOptions,
};
use tantivy::tokenizer::{NgramTokenizer, PreTokenizedString, Token};
use tantivy::{
    DocAddress, DocSet, Index, IndexSettings, IndexSortByField, Order, TantivyDocument, Term,
    TERMINATED,
};

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

#[test]
fn policy_serialization_is_fail_closed_and_preserves_disabled_configuration() {
    #[derive(serde::Deserialize)]
    struct OldIndexing {
        fieldnorms: bool,
    }
    for policy in [
        FieldNormPolicy::CountAllTokens,
        FieldNormPolicy::DiscountOverlaps,
    ] {
        for enabled in [false, true] {
            let options = indexing()
                .set_fieldnorm_policy(policy)
                .set_fieldnorms(enabled);
            let json = serde_json::to_value(&options).unwrap();
            let reopened: TextFieldIndexing = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(reopened, options);
            assert_eq!(
                reopened
                    .clone()
                    .set_fieldnorms(!enabled)
                    .set_fieldnorms(enabled),
                options
            );
            let old = serde_json::from_value::<OldIndexing>(json.clone());
            match policy {
                FieldNormPolicy::CountAllTokens => {
                    assert_eq!(json["fieldnorms"], serde_json::json!(enabled));
                    assert_eq!(old.unwrap().fieldnorms, enabled);
                }
                FieldNormPolicy::DiscountOverlaps => {
                    assert_eq!(
                        json["fieldnorms"],
                        serde_json::json!({"enabled":enabled,"policy":"discount_overlaps"})
                    );
                    assert!(old.is_err());
                }
            }
        }
    }
    let missing: TextFieldIndexing = serde_json::from_str("{}").unwrap();
    assert!(missing.fieldnorms());
    assert_eq!(missing.fieldnorm_policy(), FieldNormPolicy::CountAllTokens);
    assert_eq!(
        TextFieldIndexing::default().fieldnorm_policy(),
        FieldNormPolicy::DiscountOverlaps
    );
    for invalid in [
        serde_json::json!({"enabled":true,"policy":"count_all_tokens"}),
        serde_json::json!({"enabled":true,"policy":"unknown"}),
        serde_json::json!({"enabled":true,"policy":"discount_overlaps","extra":1}),
        serde_json::json!({"policy":"discount_overlaps"}),
        serde_json::json!({"enabled":true}),
    ] {
        assert!(serde_json::from_value::<TextFieldIndexing>(
            serde_json::json!({"fieldnorms":invalid})
        )
        .is_err());
    }
}

#[test]
fn overlap_boundaries_match_pinned_lucene() -> tantivy::Result<()> {
    for first in [
        vec![tokens(&[("alpha", 0, 1), ("alpha", 0, 1), ("beta", 1, 1)])],
        vec![
            tokens(&[("alpha", 0, 1), ("synonym", 0, 1)]),
            tokens(&[("beta", 0, 1)]),
        ],
        vec![tokens(&[
            ("alpha", 0, 3),
            ("synonym", 0, 1),
            ("beta", 1, 1),
        ])],
        vec![tokens(&[
            ("alpha", 5, 1),
            ("synonym", 5, 1),
            ("beta", 8, 1),
        ])],
    ] {
        let same_term = first[0].tokens[1].text == "alpha";
        let mut docs = standard();
        docs[0] = first;
        let (index, field) = fixture(indexing(), docs)?;
        check(
            &index,
            field,
            &[2, 2],
            &[if same_term { 1039615994 } else { 1035524425 }, 1035524425],
        )?;
    }
    // Frequency-only postings have the same index-time norm policy.
    let mut docs = standard();
    docs[0][0].tokens[0].position_length = 3;
    let (index, field) = fixture(
        indexing().set_index_option(IndexRecordOption::WithFreqs),
        docs,
    )?;
    check(&index, field, &[2, 2], &[1035524425, 1035524425])?;
    Ok(())
}

#[test]
fn empty_values_reset_overlap_comparison_and_empty_docs_do_not_populate_stats(
) -> tantivy::Result<()> {
    let docs = vec![
        vec![
            tokens(&[]),
            tokens(&[("alpha", 0, 1), ("synonym", 0, 1)]),
            tokens(&[]),
            tokens(&[("beta", 0, 1)]),
            tokens(&[]),
        ],
        standard().remove(1),
        vec![],
        vec![tokens(&[])],
    ];
    let (index, field) = fixture(indexing(), docs)?;
    let searcher = index.reader()?.searcher();
    assert_eq!(searcher.num_docs(), 4);
    let stats = searcher.field_statistics(field)?;
    assert_eq!((stats.doc_count(), stats.sum_total_term_freq()), (2, 5));
    let norms = searcher.segment_reader(0).get_fieldnorms_reader(field)?;
    assert_eq!(
        (0..4)
            .map(|doc| norms.fieldnorm_id(doc))
            .collect::<Vec<_>>(),
        [2, 2, 0, 0]
    );
    let query = TermQuery::new(
        Term::from_field_text(field, "alpha"),
        IndexRecordOption::WithFreqs,
    );
    assert_eq!(searcher.search(&query, &Count)?, 2);
    for (score, address) in searcher.search(&query, &TopDocs::with_limit(4).order_by_score())? {
        assert_eq!(score.to_bits(), 1035524425);
        assert!(address.doc_id < 2);
    }
    Ok(())
}

#[test]
fn nonoverlap_norms_statistics_and_raw_scores_are_policy_invariant() -> tantivy::Result<()> {
    for policy in [
        FieldNormPolicy::CountAllTokens,
        FieldNormPolicy::DiscountOverlaps,
    ] {
        let value = tokens(&[("alpha", 0, 1), ("beta", 1, 1)]);
        let (index, field) = fixture(
            indexing().set_fieldnorm_policy(policy),
            vec![vec![value.clone()], vec![value]],
        )?;
        let searcher = index.reader()?.searcher();
        let stats = searcher.field_statistics(field)?;
        assert_eq!((stats.doc_count(), stats.sum_total_term_freq()), (2, 4));
        let norms = searcher.segment_reader(0).get_fieldnorms_reader(field)?;
        assert_eq!([norms.fieldnorm_id(0), norms.fieldnorm_id(1)], [2, 2]);
        let query = TermQuery::new(
            Term::from_field_text(field, "alpha"),
            IndexRecordOption::WithFreqs,
        );
        for (score, _) in searcher.search(&query, &TopDocs::with_limit(2).order_by_score())? {
            assert_eq!(score.to_bits(), 1034533260);
        }
    }
    Ok(())
}

#[test]
fn same_term_basic_is_unique_and_ngram_native_length_is_one() -> tantivy::Result<()> {
    let mut docs = standard();
    docs[0][0].tokens[1].text = "alpha".into();
    let (index, field) = fixture(indexing().set_index_option(IndexRecordOption::Basic), docs)?;
    let searcher = index.reader()?.searcher();
    let stats = searcher.field_statistics(field)?;
    assert_eq!((stats.doc_count(), stats.sum_total_term_freq()), (2, 4));
    let norms = searcher.segment_reader(0).get_fieldnorms_reader(field)?;
    assert_eq!([norms.fieldnorm_id(0), norms.fieldnorm_id(1)], [2, 2]);

    let mut schema = Schema::builder();
    let field = schema.add_text_field(
        "text",
        TextOptions::default().set_indexing_options(indexing().set_tokenizer("grams")),
    );
    let index = Index::create_in_ram(schema.build());
    index
        .tokenizers()
        .register("grams", NgramTokenizer::all_ngrams(2, 3)?);
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    writer.add_document(tantivy::doc!(field=>"abcd"))?;
    writer.commit()?;
    let searcher = index.reader()?.searcher();
    assert_eq!(searcher.field_statistics(field)?.sum_total_term_freq(), 5);
    assert_eq!(
        searcher
            .segment_reader(0)
            .get_fieldnorms_reader(field)?
            .fieldnorm_id(0),
        1
    );
    Ok(())
}

#[test]
fn disabled_norms_and_historical_provider_keep_their_score_formula() -> tantivy::Result<()> {
    struct Historical;
    impl Bm25StatisticsProvider for Historical {
        fn total_num_tokens(&self, _: Field) -> tantivy::Result<u64> {
            Ok(5)
        }
        fn total_num_docs(&self) -> tantivy::Result<u64> {
            Ok(2)
        }
        fn doc_freq(&self, _: &Term) -> tantivy::Result<u64> {
            Ok(2)
        }
    }
    for enabled in [false, true] {
        let (index, field) = fixture(indexing().set_fieldnorms(enabled), standard())?;
        let searcher = index.reader()?.searcher();
        assert_eq!(
            searcher
                .segment_reader(0)
                .fieldnorms_readers()
                .get_field(field)?
                .is_some(),
            enabled
        );
        let term = Term::from_field_text(field, "alpha");
        let query = TermQuery::new(term.clone(), IndexRecordOption::WithFreqs);
        let weight = query.weight(EnableScoring::enabled_from_statistics_provider(
            &Historical,
            &searcher,
        ))?;
        let expected = Bm25Weight::for_one_term(2, 2, 2.5).score(if enabled { 2 } else { 1 }, 1);
        let mut scorer = weight.scorer(searcher.segment_reader(0), 1.0)?;
        while scorer.doc() != TERMINATED {
            assert_eq!(scorer.score().to_bits(), expected.to_bits());
            assert_eq!(
                weight
                    .explain(searcher.segment_reader(0), scorer.doc())?
                    .value()
                    .to_bits(),
                expected.to_bits()
            );
            scorer.advance();
        }
        assert_eq!(searcher.search(&query, &Count)?, 2);
        if !enabled {
            let native = Bm25Weight::for_terms(&searcher, &[term])?;
            assert_eq!(
                query
                    .explain(&searcher, DocAddress::new(0, 0))?
                    .value()
                    .to_bits(),
                native.score(1, 1).to_bits()
            );
        }
    }
    Ok(())
}

#[test]
fn overlap_norm_bytes_survive_reopen_sort_deletion_merge_and_legacy_append() -> tantivy::Result<()>
{
    for policy in [
        FieldNormPolicy::CountAllTokens,
        FieldNormPolicy::DiscountOverlaps,
    ] {
        let mut schema = Schema::builder();
        let options = if policy == FieldNormPolicy::CountAllTokens {
            serde_json::from_str::<TextFieldIndexing>(r#"{"record":"position","fieldnorms":true}"#)?
        } else {
            indexing()
        };
        let field =
            schema.add_text_field("text", TextOptions::default().set_indexing_options(options));
        let id = schema.add_u64_field("id", NumericOptions::default().set_fast().set_indexed());
        let settings = IndexSettings {
            sort_by_field: Some(IndexSortByField {
                field: "id".into(),
                order: Order::Asc,
            }),
            ..IndexSettings::default()
        };
        let directory = tempfile::tempdir()?;
        Index::builder()
            .schema(schema.build())
            .settings(settings)
            .create_in_dir(directory.path())?;
        // Empty schema reopen must already preserve the policy before any footer exists.
        let index = Index::open_in_dir(directory.path())?;
        let schema = index.schema();
        let tantivy::schema::FieldType::Str(options) = schema.get_field_entry(field).field_type()
        else {
            panic!("fixture is a text field");
        };
        let actual = options.get_indexing_options().unwrap().fieldnorm_policy();
        assert_eq!(actual, policy);
        let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
        writer.set_merge_policy(Box::new(NoMergePolicy));
        for (row, values) in standard().into_iter().enumerate() {
            let mut doc = TantivyDocument::default();
            doc.add_u64(id, 1 - row as u64);
            for value in values {
                doc.add_pre_tokenized_text(field, value);
            }
            writer.add_document(doc)?;
        }
        writer.commit()?;
        drop(writer);
        let index = Index::open_in_dir(directory.path())?;
        let reader = index.reader()?;
        let norms = reader
            .searcher()
            .segment_reader(0)
            .get_fieldnorms_reader(field)?;
        assert_eq!(
            [norms.fieldnorm_id(0), norms.fieldnorm_id(1)],
            [
                2,
                if policy == FieldNormPolicy::CountAllTokens {
                    3
                } else {
                    2
                }
            ]
        );
        let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
        writer.set_merge_policy(Box::new(NoMergePolicy));
        let mut doc = TantivyDocument::default();
        doc.add_u64(id, 2);
        doc.add_pre_tokenized_text(
            field,
            tokens(&[("alpha", 0, 1), ("synonym", 0, 1), ("beta", 1, 1)]),
        );
        writer.add_document(doc)?;
        writer.commit()?;
        writer.delete_term(Term::from_field_u64(id, 0));
        writer.commit()?;
        reader.reload()?;
        assert_eq!(
            reader
                .searcher()
                .field_statistics(field)?
                .sum_total_term_freq(),
            8
        );
        writer.merge(&index.searchable_segment_ids()?).wait()?;
        reader.reload()?;
        let searcher = reader.searcher();
        assert_eq!(searcher.segment_readers().len(), 1);
        let stats = searcher.field_statistics(field)?;
        assert_eq!((stats.doc_count(), stats.sum_total_term_freq()), (2, 6));
        let norms = searcher.segment_reader(0).get_fieldnorms_reader(field)?;
        assert_eq!(
            [norms.fieldnorm_id(0), norms.fieldnorm_id(1)],
            if policy == FieldNormPolicy::CountAllTokens {
                [3, 3]
            } else {
                [2, 2]
            }
        );
    }
    Ok(())
}

#[test]
fn native_overlap_block_pruning_matches_exhaustive_scores() -> tantivy::Result<()> {
    let docs = (0..257)
        .map(|doc| {
            let mut values = vec![("alpha", 0, 1); doc % 5 + 1];
            for position in 1..=doc % 4 + 1 {
                values.push(("padding", position, 1));
            }
            vec![tokens(&values)]
        })
        .collect();
    let (index, field) = fixture(indexing(), docs)?;
    let searcher = index.reader()?.searcher();
    for boost in [0.0, 0.25, 1.0, 3.25] {
        let query = BoostQuery::new(
            Box::new(TermQuery::new(
                Term::from_field_text(field, "alpha"),
                IndexRecordOption::WithFreqs,
            )),
            boost,
        );
        let weight = query.weight(EnableScoring::enabled_from_searcher(&searcher))?;
        let mut scorer = weight.scorer(searcher.segment_reader(0), 1.0)?;
        let mut exhaustive = vec![];
        while scorer.doc() != TERMINATED {
            exhaustive.push((scorer.score(), DocAddress::new(0, scorer.doc())));
            scorer.advance();
        }
        exhaustive.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let top = searcher.search(&query, &TopDocs::with_limit(40).order_by_score())?;
        assert_eq!(top, exhaustive[..40]);
        assert_eq!(searcher.search(&query, &Count)?, 257);
    }
    Ok(())
}
