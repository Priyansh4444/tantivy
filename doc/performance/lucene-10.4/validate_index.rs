use std::io::{self, BufRead};
use tantivy::collector::TopDocs;
use tantivy::query::{EnableScoring, QueryParser};
use tantivy::schema::Value;
use tantivy::tokenizer::{LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer};
use tantivy::{DocAddress, Index, Score, TantivyDocument, TERMINATED};

fn main() -> tantivy::Result<()> {
    let index = Index::open_in_dir(std::env::args().nth(1).expect("index path"))?;
    let schema = index.schema();
    let text = schema.get_field("text")?;
    let id = schema.get_field("id")?;
    index.tokenizers().register(
        "wiki_ascii_lucene",
        TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(RemoveLongFilter::limit(256))
            .filter(LowerCaser)
            .build(),
    );
    let parser = QueryParser::for_index(&index, vec![text]);
    let reader = index.reader()?;
    let searcher = reader.searcher();
    for line in io::stdin().lock().lines() {
        let query_text = line?;
        let query = parser.parse_query(&query_text)?;
        let optimized = searcher.search(&query, &TopDocs::with_limit(10).order_by_score())?;
        let weight = query.weight(EnableScoring::enabled_from_searcher(&searcher))?;
        let mut exhaustive: Vec<(Score, DocAddress)> = Vec::new();
        for (segment_ord, segment) in searcher.segment_readers().iter().enumerate() {
            let mut scorer = weight.scorer(segment, 1.0)?;
            while scorer.doc() != TERMINATED {
                if segment.alive_bitset().is_none_or(|alive| alive.is_alive(scorer.doc())) {
                    exhaustive.push((scorer.score(), DocAddress::new(segment_ord as u32, scorer.doc())));
                }
                scorer.advance();
            }
        }
        exhaustive.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let count = query.count(&searcher)?;
        let count_matches = count == exhaustive.len();
        let ranking_matches = optimized.len() == exhaustive.len().min(10)
            && optimized.iter().zip(&exhaustive).all(|((score, addr), expected)| {
                *addr == expected.1
                    && (score - expected.0).abs() <= 1e-5 * expected.0.abs().max(1.0)
            });
        let mut top100 = Vec::new();
        for (score, addr) in exhaustive.iter().take(100) {
            let doc: TantivyDocument = searcher.doc(*addr)?;
            let external_id = doc.get_first(id).and_then(|value| value.as_str()).expect("stored id");
            top100.push(serde_json::json!({"id": external_id, "score": score}));
        }
        println!("{}", serde_json::json!({
            "query": query_text, "count": count,
            "count_matches_exhaustive": count_matches,
            "ranking_matches_exhaustive": ranking_matches,
            "top100": top100,
        }));
        assert!(count_matches && ranking_matches, "query correctness mismatch: {}", query_text);
    }
    Ok(())
}
