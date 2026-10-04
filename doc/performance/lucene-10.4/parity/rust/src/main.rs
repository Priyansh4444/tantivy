use std::io::{self, BufRead};

use serde_json::{json, Value as Json};
use tantivy::collector::{Count, TopDocs};
use tantivy::merge_policy::NoMergePolicy;
use tantivy::query::{
    BooleanQuery, BoostQuery, ConstScoreQuery, EnableScoring, Occur, PhraseQuery, Query, TermQuery,
};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, INDEXED, STORED,
};
use tantivy::tokenizer::{PreTokenizedString, Token};
use tantivy::{DocAddress, Index, TantivyDocument, Term, TERMINATED};

fn query_from_ast(ast: &Json, field: Field) -> Box<dyn Query> {
    match ast["type"].as_str().unwrap() {
        "term" => Box::new(TermQuery::new(
            Term::from_field_text(field, ast["term"].as_str().unwrap()),
            IndexRecordOption::WithFreqsAndPositions,
        )),
        "boost" => Box::new(BoostQuery::new(
            query_from_ast(&ast["query"], field),
            ast["boost"].as_f64().unwrap() as f32,
        )),
        "phrase" => {
            let terms = ast["terms"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(position, term)| {
                    (
                        position,
                        Term::from_field_text(field, term.as_str().unwrap()),
                    )
                })
                .collect();
            Box::new(PhraseQuery::new_with_offset_and_slop(
                terms,
                ast["slop"].as_u64().unwrap_or(0) as u32,
            ))
        }
        "bool" => {
            let mut clauses = Vec::new();
            for clause in ast["clauses"].as_array().unwrap() {
                let mut query = query_from_ast(&clause["query"], field);
                let occur = match clause["occur"].as_str().unwrap() {
                    "must" => Occur::Must,
                    "should" => Occur::Should,
                    "must_not" => Occur::MustNot,
                    "filter" => {
                        query = Box::new(ConstScoreQuery::new(query, 0.0));
                        Occur::Must
                    }
                    unknown => panic!("unsupported occurrence {unknown}"),
                };
                clauses.push((occur, query));
            }
            let mut query = BooleanQuery::new(clauses);
            if let Some(minimum) = ast["minimum_should_match"].as_u64() {
                query.set_minimum_number_should_match(minimum as usize);
            }
            Box::new(query)
        }
        unknown => panic!("unsupported query type {unknown}"),
    }
}

fn run(case: &Json) -> Result<Json, Box<dyn std::error::Error>> {
    let mut builder = Schema::builder();
    let id_field = builder.add_u64_field("id", INDEXED | STORED);
    let text_field = builder.add_text_field(
        "text",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_index_option(IndexRecordOption::WithFreqsAndPositions)
                .set_fieldnorms(case["fieldnorms"].as_bool().unwrap_or(true)),
        ),
    );
    let index = Index::create_in_ram(builder.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    writer.set_merge_policy(Box::new(NoMergePolicy));
    let mut segment = None;
    for document in case["documents"].as_array().unwrap() {
        let next_segment = document["segment"].as_u64().unwrap_or(0);
        if segment.is_some_and(|previous| previous != next_segment) {
            writer.commit()?;
        }
        segment = Some(next_segment);
        let mut output = TantivyDocument::default();
        output.add_u64(id_field, document["id"].as_u64().unwrap());
        if let Some(tokens) = document["tokens"].as_array() {
            let mut offset = 0;
            let tokens: Vec<Token> = tokens
                .iter()
                .enumerate()
                .map(|(position, word)| {
                    let text = word.as_str().unwrap().to_owned();
                    let token = Token {
                        offset_from: offset,
                        offset_to: offset + text.len(),
                        position,
                        text,
                        position_length: 1,
                    };
                    offset = token.offset_to + 1;
                    token
                })
                .collect();
            output.add_pre_tokenized_text(
                text_field,
                PreTokenizedString {
                    text: tokens
                        .iter()
                        .map(|token| token.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                    tokens,
                },
            );
        }
        writer.add_document(output)?;
    }
    writer.commit()?;
    for document in case["documents"].as_array().unwrap() {
        if document["deleted"].as_bool().unwrap_or(false) {
            writer.delete_term(Term::from_field_u64(
                id_field,
                document["id"].as_u64().unwrap(),
            ));
        }
    }
    writer.commit()?;
    if case["merge"].as_bool().unwrap_or(false) {
        writer.merge(&index.searchable_segment_ids()?).wait()?;
    }
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let total_tokens: u64 = searcher
        .segment_readers()
        .iter()
        .map(|segment| {
            segment
                .inverted_index(text_field)
                .unwrap()
                .total_num_tokens()
        })
        .sum();
    let max_doc: u64 = searcher
        .segment_readers()
        .iter()
        .map(|segment| u64::from(segment.max_doc()))
        .sum();
    let id_at = |address: DocAddress| -> Result<u64, tantivy::TantivyError> {
        let document: TantivyDocument = searcher.doc(address)?;
        Ok(document.get_first(id_field).unwrap().as_u64().unwrap())
    };
    let mut outputs = Vec::new();
    for definition in case["queries"].as_array().unwrap() {
        let ast = &definition["query"];
        let query = query_from_ast(ast, text_field);
        let weight = query.weight(EnableScoring::enabled_from_searcher(&searcher))?;
        let mut exhaustive = Vec::new();
        for (segment_ord, segment) in searcher.segment_readers().iter().enumerate() {
            let mut scorer = weight.scorer(segment, 1.0)?;
            while scorer.doc() != TERMINATED {
                if !segment.is_deleted(scorer.doc()) {
                    exhaustive.push(json!({"id": id_at(DocAddress::new(segment_ord as u32, scorer.doc()))?, "score": scorer.score()}));
                }
                scorer.advance();
            }
        }
        exhaustive.sort_by(|a, b| {
            b["score"]
                .as_f64()
                .unwrap()
                .total_cmp(&a["score"].as_f64().unwrap())
                .then_with(|| a["id"].as_u64().cmp(&b["id"].as_u64()))
        });
        let mut top = searcher
            .search(&*query, &TopDocs::with_limit(10).order_by_score())?
            .into_iter()
            .map(|(score, address)| Ok(json!({"id":id_at(address)?,"score":score})))
            .collect::<tantivy::Result<Vec<_>>>()?;
        top.sort_by(|a, b| {
            b["score"]
                .as_f64()
                .unwrap()
                .total_cmp(&a["score"].as_f64().unwrap())
                .then_with(|| a["id"].as_u64().cmp(&b["id"].as_u64()))
        });
        outputs.push(json!({"name":definition["name"],"count":searcher.search(&*query,&Count)?,"exhaustive":exhaustive,"top":top}));
    }
    Ok(
        json!({"name":case["name"],"statistics":{"max_doc":max_doc,"live_docs":searcher.num_docs(),"total_tokens":total_tokens,"segments":searcher.segment_readers().len()},"queries":outputs}),
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        println!("{}", run(&serde_json::from_str::<Json>(&line?)?)?);
    }
    Ok(())
}
