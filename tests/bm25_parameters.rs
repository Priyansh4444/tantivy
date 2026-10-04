use tantivy::query::{Query, TermQuery};
use tantivy::schema::{IndexRecordOption, Schema, TEXT};
use tantivy::{doc, DocAddress, Index, Searcher, Term};

const JAVA_REFERENCE: &str =
    include_str!("../doc/performance/lucene-10.4/parity/bm25-parameters-reference/reference.csv");

fn configured_searcher(searcher: Searcher, k1: f32, b: f32) -> tantivy::Result<Searcher> {
    // Red baseline: production has no query-parameter API and always uses DEFAULT.
    // The fix replaces this route with its public validated builder; Java oracle
    // values and fixture inputs remain unchanged.
    let _ = (k1, b);
    Ok(searcher)
}

#[test]
fn native_parameter_profiles_match_actual_lucene_query_scores() -> tantivy::Result<()> {
    let mut schema = Schema::builder();
    let text = schema.add_text_field("text", TEXT);
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    for value in ["alpha alpha beta", "beta", "alpha beta"] {
        writer.add_document(doc!(text => value))?;
    }
    writer.commit()?;
    let reader = index.reader()?;
    let query = TermQuery::new(
        Term::from_field_text(text, "alpha"),
        IndexRecordOption::WithFreqs,
    );
    for line in JAVA_REFERENCE
        .lines()
        .filter(|line| line.starts_with("term,"))
    {
        let parts: Vec<_> = line.split(',').collect();
        let k1 = f32::from_bits(u32::from_str_radix(parts[1], 16).unwrap());
        let b = f32::from_bits(u32::from_str_radix(parts[2], 16).unwrap());
        let doc: u32 = parts[3].parse().unwrap();
        let expected = u32::from_str_radix(parts[4], 16).unwrap();
        let searcher = configured_searcher(reader.searcher(), k1, b)?;
        let score = query.explain(&searcher, DocAddress::new(0, doc))?.value();
        assert_eq!(
            score.to_bits(),
            expected,
            "Java profile k1={k1:?}({:08x}) b={b:?}({:08x}) doc={doc} actual={score}",
            k1.to_bits(),
            b.to_bits()
        );
    }
    Ok(())
}
