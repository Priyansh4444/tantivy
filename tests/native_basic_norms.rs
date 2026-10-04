//! Lucene 10.4 DOCS term-field reference lives in
//! doc/performance/lucene-10.4/parity/basic-norm-reference/BasicNormReference.java.
//! These are term fields with norms, not Lucene point fields.
use std::net::Ipv6Addr;

use tantivy::collector::TopDocs;
use tantivy::merge_policy::NoMergePolicy;
use tantivy::query::{Bm25StatisticsProvider, Query, TermQuery};
use tantivy::schema::{
    BytesOptions, DateOptions, Field, IndexRecordOption, IpAddrOptions, NumericOptions, Schema,
    TextFieldIndexing, TextOptions,
};
use tantivy::{DateTime, DocAddress, Index, TantivyDocument, Term};

struct Fixture {
    index: Index,
    fields: [Field; 7],
    basic_text: Field,
    frequency_text: Field,
    without_norms: Field,
    id: Field,
}

impl Fixture {
    fn new() -> Self {
        let mut schema = Schema::builder();
        let numeric = NumericOptions::default().set_indexed().set_fieldnorm();
        let fields = [
            schema.add_u64_field("u64", numeric.clone()),
            schema.add_i64_field("i64", numeric.clone()),
            schema.add_f64_field("f64", numeric.clone()),
            schema.add_bool_field("bool", numeric),
            schema.add_date_field("date", DateOptions::default().set_indexed().set_fieldnorm()),
            schema.add_bytes_field(
                "bytes",
                BytesOptions::default().set_indexed().set_fieldnorms(),
            ),
            schema.add_ip_addr_field(
                "ip",
                IpAddrOptions::default().set_indexed().set_fieldnorms(),
            ),
        ];
        let text_options = |record| {
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_index_option(record)
                    .set_tokenizer("default"),
            )
        };
        let basic_text =
            schema.add_text_field("basic_text", text_options(IndexRecordOption::Basic));
        let frequency_text =
            schema.add_text_field("frequency_text", text_options(IndexRecordOption::WithFreqs));
        let without_norms =
            schema.add_u64_field("without_norms", NumericOptions::default().set_indexed());
        let id = schema.add_u64_field("id", NumericOptions::default().set_indexed().set_stored());
        Self {
            index: Index::create_in_ram(schema.build()),
            fields,
            basic_text,
            frequency_text,
            without_norms,
            id,
        }
    }

    fn document(&self, row: u64) -> TantivyDocument {
        let mut document = TantivyDocument::default();
        document.add_u64(self.id, row);
        let values: &[u64] = match row {
            0 => &[1, 1, 2],
            1 => &[1, 2],
            2 => &[1],
            _ => &[],
        };
        for (position, &value) in values.iter().enumerate() {
            document.add_u64(self.fields[0], value);
            document.add_i64(self.fields[1], value as i64 - 2);
            document.add_f64(self.fields[2], value as f64 / 2.0);
            document.add_bool(self.fields[3], value == 1);
            // Distinct input timestamps share a serialized term at indexed second precision.
            let millis = value as i64 * 1_000 + if row == 0 { position as i64 * 100 } else { 0 };
            document.add_date(self.fields[4], DateTime::from_timestamp_millis(millis));
            document.add_bytes(self.fields[5], &[value as u8]);
            document.add_ip_addr(self.fields[6], Ipv6Addr::from(value as u128));
            document.add_u64(self.without_norms, value);
        }
        if !values.is_empty() {
            let text = match row {
                0 => "alpha alpha beta",
                1 => "alpha beta",
                _ => "alpha",
            };
            document.add_text(self.basic_text, text);
            document.add_text(self.frequency_text, text);
        }
        document
    }

    fn terms(&self) -> [(Term, Term); 7] {
        [
            (
                Term::from_field_u64(self.fields[0], 1),
                Term::from_field_u64(self.fields[0], 2),
            ),
            (
                Term::from_field_i64(self.fields[1], -1),
                Term::from_field_i64(self.fields[1], 0),
            ),
            (
                Term::from_field_f64(self.fields[2], 0.5),
                Term::from_field_f64(self.fields[2], 1.0),
            ),
            (
                Term::from_field_bool(self.fields[3], true),
                Term::from_field_bool(self.fields[3], false),
            ),
            (
                Term::from_field_date_for_search(
                    self.fields[4],
                    DateTime::from_timestamp_millis(1_100),
                ),
                Term::from_field_date_for_search(
                    self.fields[4],
                    DateTime::from_timestamp_millis(2_200),
                ),
            ),
            (
                Term::from_field_bytes(self.fields[5], &[1]),
                Term::from_field_bytes(self.fields[5], &[2]),
            ),
            (
                Term::from_field_ip_addr(self.fields[6], Ipv6Addr::from(1u128)),
                Term::from_field_ip_addr(self.fields[6], Ipv6Addr::from(2u128)),
            ),
        ]
    }

    fn write(&self) -> tantivy::Result<()> {
        let mut writer = self.index.writer_with_num_threads(1, 15_000_000)?;
        for row in 0..4 {
            writer.add_document(self.document(row))?;
        }
        writer.commit()?;
        Ok(())
    }
}

#[test]
fn native_basic_norms_count_distinct_encoded_terms() -> tantivy::Result<()> {
    let fixture = Fixture::new();
    fixture.write()?;
    let searcher = fixture.index.reader()?.searcher();
    for field in fixture.fields.into_iter().chain([fixture.basic_text]) {
        let stats = searcher.field_statistics(field)?;
        assert_eq!((stats.doc_count(), stats.sum_total_term_freq()), (3, 5));
        let norms = searcher.segment_reader(0).get_fieldnorms_reader(field)?;
        let actual: Vec<_> = (0..4).map(|doc| norms.fieldnorm(doc)).collect();
        assert_eq!(
            actual,
            [2, 2, 1, 0],
            "field {} must use encoded term identity",
            field.field_id()
        );
    }
    let norms = searcher
        .segment_reader(0)
        .get_fieldnorms_reader(fixture.frequency_text)?;
    assert_eq!(
        (0..4).map(|doc| norms.fieldnorm(doc)).collect::<Vec<_>>(),
        [3, 2, 1, 0]
    );
    let stats = searcher.field_statistics(fixture.without_norms)?;
    assert_eq!((stats.doc_count(), stats.sum_total_term_freq()), (3, 5));
    Ok(())
}

#[test]
fn native_basic_norms_scores_match_lucene_docs_term_fields() -> tantivy::Result<()> {
    let fixture = Fixture::new();
    fixture.write()?;
    let searcher = fixture.index.reader()?.searcher();
    for (a, b) in fixture.terms() {
        assert_eq!(searcher.doc_freq(&a)?, 3);
        assert_eq!(searcher.doc_freq(&b)?, 2);
        assert_ne!(a.serialized_value_bytes(), b.serialized_value_bytes());
        let query = TermQuery::new(a, IndexRecordOption::Basic);
        for (doc, expected_bits) in [(0, 0x3d65cf02), (1, 0x3d65cf02), (2, 0x3d94a050)] {
            let score = query.explain(&searcher, DocAddress::new(0, doc))?.value();
            assert_eq!(score.to_bits(), expected_bits, "doc {doc} score={score}");
        }
        let top = searcher.search(&query, &TopDocs::with_limit(3).order_by_score())?;
        assert_eq!(
            top.iter()
                .map(|(_, address)| address.doc_id)
                .collect::<Vec<_>>(),
            [2, 0, 1]
        );
    }
    Ok(())
}

#[test]
fn native_basic_norms_survive_deletion_and_merge() -> tantivy::Result<()> {
    let fixture = Fixture::new();
    let mut writer = fixture.index.writer_with_num_threads(1, 15_000_000)?;
    writer.set_merge_policy(Box::new(NoMergePolicy));
    for row in 0..4 {
        writer.add_document(fixture.document(row))?;
        if row == 1 {
            writer.commit()?;
        }
    }
    writer.commit()?;
    writer.delete_term(Term::from_field_u64(fixture.id, 1));
    writer.commit()?;
    let reader = fixture.index.reader()?;
    for field in fixture.fields {
        let stats = reader.searcher().field_statistics(field)?;
        assert_eq!(
            (stats.doc_count(), stats.sum_total_term_freq()),
            (3, 5),
            "pending deletes remain physical"
        );
    }
    writer
        .merge(&fixture.index.searchable_segment_ids()?)
        .wait()?;
    reader.reload()?;
    let searcher = reader.searcher();
    assert_eq!(searcher.segment_readers().len(), 1);
    for ((a, b), field) in fixture.terms().into_iter().zip(fixture.fields) {
        let stats = searcher.field_statistics(field)?;
        assert_eq!((stats.doc_count(), stats.sum_total_term_freq()), (2, 3));
        assert_eq!(searcher.doc_freq(&a)?, 2);
        assert_eq!(searcher.doc_freq(&b)?, 1);
        let norms = searcher.segment_reader(0).get_fieldnorms_reader(field)?;
        let mut actual = (0..3).map(|doc| norms.fieldnorm(doc)).collect::<Vec<_>>();
        actual.sort_unstable();
        assert_eq!(actual, [0, 1, 2]);
    }
    Ok(())
}

#[test]
fn native_basic_norms_use_float_term_identity_instead_of_numeric_equality() -> tantivy::Result<()> {
    let mut schema = Schema::builder();
    let field = schema.add_f64_field(
        "float",
        NumericOptions::default().set_indexed().set_fieldnorm(),
    );
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    let mut document = TantivyDocument::default();
    for value in [-0.0, 0.0, -0.0, 0.0] {
        document.add_f64(field, value);
    }
    writer.add_document(document)?;
    writer.commit()?;
    let searcher = index.reader()?.searcher();
    let negative = Term::from_field_f64(field, -0.0);
    let positive = Term::from_field_f64(field, 0.0);
    assert_ne!(
        negative.serialized_value_bytes(),
        positive.serialized_value_bytes()
    );
    assert_eq!(searcher.doc_freq(&negative)?, 1);
    assert_eq!(searcher.doc_freq(&positive)?, 1);
    assert_eq!(searcher.field_statistics(field)?.sum_total_term_freq(), 2);
    assert_eq!(
        searcher
            .segment_reader(0)
            .get_fieldnorms_reader(field)?
            .fieldnorm(0),
        2
    );
    Ok(())
}
