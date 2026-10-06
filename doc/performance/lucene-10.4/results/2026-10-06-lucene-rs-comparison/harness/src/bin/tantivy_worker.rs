use balanced_search::{
    Kind, Mode, QueryCase, Request, Result, emit, load_queries, validate_request,
};
use serde_json::json;
use std::io::BufRead;
use std::time::Instant;
use tantivy::collector::TopDocs;
use tantivy::query::{
    Bm25StatisticsProvider, BooleanQuery, EnableScoring, Occur, PhraseQuery, Query, TermQuery,
};
use tantivy::schema::{Field, IndexRecordOption};
use tantivy::{Index, ReloadPolicy, Searcher, TERMINATED, Term};

fn ast(case: &QueryCase, field: Field) -> Box<dyn Query> {
    let term = |text: &str| Term::from_field_text(field, text);
    match case.kind {
        Kind::Term => Box::new(TermQuery::new(
            term(&case.terms[0]),
            IndexRecordOption::WithFreqs,
        )),
        Kind::Phrase => Box::new(PhraseQuery::new(
            case.terms.iter().map(|t| term(t)).collect(),
        )),
        Kind::And | Kind::Or => {
            let occur = if matches!(case.kind, Kind::And) {
                Occur::Must
            } else {
                Occur::Should
            };
            Box::new(BooleanQuery::new(
                case.terms
                    .iter()
                    .map(|t| {
                        (
                            occur,
                            Box::new(TermQuery::new(term(t), IndexRecordOption::WithFreqs))
                                as Box<dyn Query>,
                        )
                    })
                    .collect(),
            ))
        }
    }
}

fn hits(searcher: &Searcher, query: &dyn Query) -> Result<Vec<(u32, u32)>> {
    Ok(searcher
        .search(query, &TopDocs::with_limit(10).order_by_score())?
        .into_iter()
        .map(|(score, doc)| {
            assert_eq!(doc.segment_ord, 0);
            (doc.doc_id, score.to_bits())
        })
        .collect())
}

fn exhaustive(searcher: &Searcher, query: &dyn Query) -> Result<serde_json::Value> {
    let weight = query.weight(EnableScoring::enabled_from_searcher(searcher))?;
    let mut scorer = weight.scorer(&searcher.segment_readers()[0], 1.0)?;
    let mut all = Vec::new();
    while scorer.doc() != TERMINATED {
        let score = scorer.score();
        if !score.is_finite() {
            return Err("nonfinite oracle score".into());
        }
        all.push((scorer.doc(), score));
        scorer.advance();
    }
    let count = all.len();
    all.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top: Vec<_> = all
        .into_iter()
        .take(10)
        .map(|(d, s)| (d, s.to_bits()))
        .collect();
    Ok(json!({"count":count,"hits":top}))
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: tantivy_worker INDEX QUERIES".into());
    }
    let cases = load_queries(std::path::Path::new(&args[1]))?;
    let index = Index::open_in_dir(&args[0])?;
    let field = index.schema().get_field("text")?;
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()?;
    let searcher = reader.searcher();
    if searcher.segment_readers().len() != 1
        || searcher.segment_readers()[0].num_deleted_docs() != 0
    {
        return Err("expected one segment without deletions".into());
    }
    let stats = searcher.field_statistics(field)?;
    let queries: Vec<_> = cases.iter().map(|c| ast(c, field)).collect();
    emit(
        &json!({"op":"ready","engine":"tantivy","queries":queries.len(),
        "max_doc":searcher.segment_readers()[0].max_doc(),"num_docs":searcher.num_docs(),
        "doc_count":stats.doc_count(),"sum_total_term_freq":stats.sum_total_term_freq(),
        "k1_bits":format!("{:08x}",stats.parameters().k1().to_bits()),
        "b_bits":format!("{:08x}",stats.parameters().b().to_bits()),
        "collector":"top10-no-totals","query_cache":"disabled"}),
    )?;
    for line in std::io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        validate_request(&request, queries.len())?;
        match request {
            Request::Dump { id } => {
                let query = std::hint::black_box(&*queries[id]);
                emit(
                    &json!({"id":id,"count":query.count(&searcher)?,"hits":hits(&searcher,query)?,
                    "reported":null,"oracle":exhaustive(&searcher,query)?}),
                )?;
            }
            Request::Run {
                id,
                mode,
                iterations,
            } => {
                let query = std::hint::black_box(&*queries[id]);
                let mut ns = Vec::with_capacity(iterations);
                let mut checksum = 0u64;
                for _ in 0..iterations {
                    match mode {
                        Mode::Count => {
                            let start = Instant::now();
                            let result = query.count(&searcher)?;
                            let elapsed = start.elapsed().as_nanos() as u64;
                            std::hint::black_box(&result);
                            checksum = checksum.wrapping_add(result as u64);
                            ns.push(elapsed);
                        }
                        Mode::Top10 => {
                            let start = Instant::now();
                            let result = searcher
                                .search(query, &TopDocs::with_limit(10).order_by_score())?;
                            let elapsed = start.elapsed().as_nanos() as u64;
                            std::hint::black_box(&result);
                            checksum = checksum.wrapping_add(
                                result
                                    .iter()
                                    .map(|(s, d)| u64::from(d.doc_id) ^ u64::from(s.to_bits()))
                                    .sum::<u64>(),
                            );
                            ns.push(elapsed);
                        }
                    }
                }
                emit(&json!({"id":id,"mode":mode,"ns":ns,"checksum":checksum}))?;
            }
        }
    }
    Ok(())
}
