use balanced_search::{
    Kind, Mode, QueryCase, Request, Result, emit, load_queries, validate_request,
};
use lucene_rs::sim::{Bm25, avg_field_length, idf};
use lucene_rs::{BooleanQuery, DirectoryReader, IndexSearcher, PhraseQuery, Query};
use serde_json::json;
use std::collections::HashSet;
use std::hint::black_box;
use std::io::BufRead;
use std::path::Path;
use std::time::Instant;

fn query(case: &QueryCase) -> Query {
    match case.kind {
        Kind::Term => Query::term("text", &case.terms[0]),
        Kind::And | Kind::Or => {
            let mut boolean = BooleanQuery::new();
            for term in &case.terms {
                let leaf = Query::term("text", term);
                boolean = if matches!(case.kind, Kind::And) {
                    boolean.must(leaf)
                } else {
                    boolean.should(leaf)
                };
            }
            boolean.build()
        }
        Kind::Phrase => {
            let mut phrase = PhraseQuery::new("text");
            for term in &case.terms {
                phrase = phrase.term(term);
            }
            phrase.build()
        }
    }
}

#[derive(Debug)]
struct Oracle {
    count: u64,
    hits: Vec<(u32, u32)>,
}

fn top(mut scores: Vec<(u32, f32)>) -> Result<Oracle> {
    if scores.iter().any(|(_, s)| !s.is_finite()) {
        return Err("oracle nonfinite score".into());
    }
    let count = scores.len() as u64;
    scores.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scores.truncate(10);
    Ok(Oracle {
        count,
        hits: scores.into_iter().map(|(d, s)| (d, s.to_bits())).collect(),
    })
}

fn phrase_frequency(positions: &[&[u32]]) -> Result<u32> {
    let Some(first) = positions.first() else {
        return Ok(0);
    };
    let mut count = 0u32;
    for &start in *first {
        let mut matched = true;
        for (offset, other) in positions.iter().enumerate().skip(1) {
            let Some(expected) = start.checked_add(offset.try_into()?) else {
                matched = false;
                break;
            };
            if other.binary_search(&expected).is_err() {
                matched = false;
                break;
            }
        }
        if matched {
            count = count.checked_add(1).ok_or("phrase frequency overflow")?;
        }
    }
    Ok(count)
}

fn oracle(reader: &DirectoryReader, case: &QueryCase) -> Result<Oracle> {
    let empty = || Oracle {
        count: 0,
        hits: Vec::new(),
    };
    let Some(collection) = reader.collection_stats("text") else {
        return Ok(empty());
    };
    let seg = &reader.segments()[0];
    let field = seg
        .field_infos()
        .get("text")
        .ok_or("missing oracle field")?;
    let norms = seg.norms(field.number).ok_or("missing oracle norms")?;
    let avg = avg_field_length(collection.sum_total_term_freq, collection.doc_count);
    let mut unique: Vec<(&str, usize)> = Vec::new();
    for term in &case.terms {
        if let Some((_, boost)) = unique.iter_mut().find(|(t, _)| *t == term.as_str()) {
            *boost += 1;
        } else {
            unique.push((term.as_str(), 1));
        }
    }
    if matches!(case.kind, Kind::Phrase) {
        let mut postings: Vec<Vec<(u32, Vec<u32>)>> = Vec::new();
        for &(term, _) in &unique {
            let Some((info, meta)) = seg.term_meta("text", term.as_bytes()) else {
                return Ok(empty());
            };
            let mut cursor = seg.postings(&info, &meta, true);
            let mut docs = Vec::with_capacity(meta.doc_freq as usize);
            let mut previous_doc = None;
            loop {
                let raw = cursor.next_doc();
                if raw == i32::MAX {
                    break;
                }
                let doc: u32 = raw.try_into()?;
                if doc >= reader.max_doc() || previous_doc.is_some_and(|p| doc <= p) {
                    return Err("oracle postings order".into());
                }
                let freq = cursor.freq();
                if freq == 0 {
                    return Err("oracle zero frequency".into());
                }
                let mut positions = Vec::with_capacity(freq as usize);
                for _ in 0..freq {
                    let position = cursor.next_position();
                    if positions.last().is_some_and(|&p| position <= p) {
                        return Err("oracle position order".into());
                    }
                    positions.push(position);
                }
                docs.push((doc, positions));
                previous_doc = Some(doc);
            }
            if docs.len() != meta.doc_freq as usize {
                return Err("oracle term DF mismatch".into());
            }
            postings.push(docs);
        }
        let offsets: Vec<_> = case
            .terms
            .iter()
            .map(|term| {
                unique
                    .iter()
                    .position(|(t, _)| *t == term.as_str())
                    .ok_or("missing phrase term")
            })
            .collect::<std::result::Result<_, _>>()?;
        let mut idf_sum = 0f64;
        for term in &case.terms {
            let stats = reader
                .term_stats("text", term.as_bytes())
                .ok_or("missing phrase statistics")?;
            idf_sum += f64::from(idf(stats.doc_freq, collection.doc_count));
        }
        let sim = Bm25::new(1.0, idf_sum as f32, avg);
        let mut scores = Vec::new();
        for (doc, _) in &postings[offsets[0]] {
            let mut selected = Vec::with_capacity(offsets.len());
            for &ordinal in &offsets {
                if let Ok(i) = postings[ordinal].binary_search_by_key(doc, |(d, _)| *d) {
                    selected.push(postings[ordinal][i].1.as_slice());
                } else {
                    break;
                }
            }
            if selected.len() != offsets.len() {
                continue;
            }
            let freq = phrase_frequency(&selected)?;
            if freq > 0 {
                scores.push((*doc, sim.score(freq as f32, norms[*doc as usize])));
            }
        }
        return top(scores);
    }
    // The public rewrite coalesces identical MUST/SHOULD terms into one boosted
    // leaf. Apply that semantic rule independently, rather than summing repeated
    // unboosted leaf scores (which can round differently).
    let mut sums = vec![0f64; reader.max_doc() as usize];
    let mut matches = vec![0usize; reader.max_doc() as usize];
    for &(term, multiplicity) in &unique {
        let Some((info, meta)) = seg.term_meta("text", term.as_bytes()) else {
            if matches!(case.kind, Kind::And | Kind::Term) {
                return Ok(empty());
            }
            continue;
        };
        let stats = reader
            .term_stats("text", term.as_bytes())
            .ok_or("missing term statistics")?;
        let sim = Bm25::new(
            multiplicity as f32,
            idf(stats.doc_freq, collection.doc_count),
            avg,
        );
        let mut cursor = seg.postings(&info, &meta, false);
        let mut previous = None;
        let mut seen = 0u32;
        loop {
            let raw = cursor.next_doc();
            if raw == i32::MAX {
                break;
            }
            let doc: u32 = raw.try_into()?;
            if doc >= reader.max_doc() || previous.is_some_and(|p| doc <= p) {
                return Err("oracle postings order".into());
            }
            let freq = cursor.freq();
            if freq == 0 {
                return Err("oracle zero frequency".into());
            }
            sums[doc as usize] += f64::from(sim.score(freq as f32, norms[doc as usize]));
            matches[doc as usize] += 1;
            previous = Some(doc);
            seen += 1;
        }
        if seen != meta.doc_freq {
            return Err("oracle term DF mismatch".into());
        }
    }
    let mut scores = Vec::new();
    for (doc, (sum, n)) in sums.into_iter().zip(matches).enumerate() {
        let matched = if matches!(case.kind, Kind::And) {
            n == unique.len()
        } else {
            n > 0
        };
        if matched {
            scores.push((doc.try_into()?, sum as f32));
        }
    }
    top(scores)
}

fn checked_hits(top: &lucene_rs::TopDocs) -> Result<Vec<(u32, u32)>> {
    let mut ids = HashSet::new();
    let mut previous: Option<(u32, f32)> = None;
    for hit in &top.score_docs {
        if !hit.score.is_finite() || !ids.insert(hit.doc) {
            return Err("invalid optimized hit".into());
        }
        if let Some((doc, score)) = previous {
            if hit.score > score || (hit.score == score && hit.doc <= doc) {
                return Err("optimized score/tie order violation".into());
            }
        }
        previous = Some((hit.doc, hit.score));
    }
    Ok(top
        .score_docs
        .iter()
        .map(|h| (h.doc, h.score.to_bits()))
        .collect())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: port_worker INDEX QUERIES".into());
    }
    let cases = load_queries(Path::new(&args[1]))?;
    let queries: Vec<_> = cases.iter().map(query).collect();
    let reader = DirectoryReader::open(&args[0])?;
    if reader.segments().len() != 1 || reader.num_deleted_docs() != 0 {
        return Err("worker requires single deletion-free segment".into());
    }
    let stats = reader
        .collection_stats("text")
        .ok_or("missing text stats")?;
    let searcher = IndexSearcher::new(reader);
    emit(
        &json!({"op":"ready","engine":"lucene-rs","queries":cases.len(),
            "max_doc":searcher.reader().max_doc(),"num_docs":searcher.reader().num_docs(),
            "doc_count":stats.doc_count,"sum_total_term_freq":stats.sum_total_term_freq,
            "k1_bits":format!("{:08x}",1.2f32.to_bits()),"b_bits":format!("{:08x}",0.75f32.to_bits()),
            "total_hits_threshold":1000,"collector":"top10-totals-threshold1000","query_cache":"disabled",
            "duplicate_boolean_terms":"coalesced_into_summed_boost"}),
    )?;
    for line in std::io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        validate_request(&request, cases.len())?;
        match request {
            Request::Dump { id } => {
                let result = searcher.search(&queries[id], 10)?;
                let count = searcher.count(&queries[id])?;
                let hits = checked_hits(&result)?;
                let reference = oracle(searcher.reader(), &cases[id])?;
                let relation = match result.total_hits.relation {
                    lucene_rs::search::TotalHitsRelation::EqualTo => "eq",
                    lucene_rs::search::TotalHitsRelation::GreaterThanOrEqualTo => "gte",
                };
                emit(
                    &json!({"id":id,"count":count,"hits":hits,"reported":{"value":result.total_hits.value,"relation":relation},"oracle":{"count":reference.count,"hits":reference.hits}}),
                )?;
            }
            Request::Run {
                id,
                mode,
                iterations,
            } => {
                let mut ns = Vec::with_capacity(iterations);
                let mut checksum = 0u64;
                let query = black_box(&queries[id]);
                for _ in 0..iterations {
                    match mode {
                        Mode::Count => {
                            let start = Instant::now();
                            let result = searcher.count(query)?;
                            let elapsed: u64 = start.elapsed().as_nanos().try_into()?;
                            black_box(&result);
                            checksum = checksum.wrapping_add(result);
                            ns.push(elapsed);
                        }
                        Mode::Top10 => {
                            let start = Instant::now();
                            let result = searcher.search(query, 10)?;
                            let elapsed: u64 = start.elapsed().as_nanos().try_into()?;
                            black_box(&result);
                            for hit in &result.score_docs {
                                checksum = checksum.wrapping_add(
                                    u64::from(hit.doc) ^ u64::from(hit.score.to_bits()),
                                );
                            }
                            ns.push(elapsed);
                            drop(result);
                        }
                    }
                }
                emit(&json!({"id":id,"mode":mode,"ns":ns,"checksum":checksum}))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucene_rs::analysis::WhitespaceAnalyzer;
    use lucene_rs::{Document, Field, IndexWriter, IndexWriterConfig, Store};
    #[test]
    fn overlapping_and_repeated_phrase_starts_are_counted() {
        assert_eq!(phrase_frequency(&[&[0, 1, 2], &[0, 1, 2]]).unwrap(), 2);
        assert_eq!(phrase_frequency(&[&[0, 2], &[1, 3], &[0, 2]]).unwrap(), 1);
        assert_eq!(phrase_frequency(&[&[0], &[2]]).unwrap(), 0);
    }
    #[test]
    fn public_results_match_exhaustive_fixture_with_empty_and_duplicates() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "balanced-port-fixture-{}-{unique}",
            std::process::id()
        ));
        let mut writer =
            IndexWriter::open(&dir, IndexWriterConfig::new(WhitespaceAnalyzer)).unwrap();
        for text in ["a a a", "a b a b", "b", "", "a b"] {
            writer
                .add_document(&Document::new().with(Field::text("text", text, Store::No)))
                .unwrap();
        }
        writer.commit().unwrap();
        writer.close().unwrap();
        let searcher = IndexSearcher::new(DirectoryReader::open(&dir).unwrap());
        let norm = searcher.reader().segments()[0]
            .norms(
                searcher.reader().segments()[0]
                    .field_infos()
                    .get("text")
                    .unwrap()
                    .number,
            )
            .unwrap();
        assert_eq!(norm[3], 0);
        for (kind, terms, count) in [
            (Kind::Term, vec!["a"], 3),
            (Kind::And, vec!["a", "b"], 2),
            (Kind::Or, vec!["a", "b"], 4),
            (Kind::Phrase, vec!["a", "a"], 1),
            (Kind::Phrase, vec!["a", "b"], 2),
            (Kind::And, vec!["a", "a", "b"], 2),
            (Kind::Or, vec!["a", "a", "b"], 4),
            (Kind::And, vec!["a", "missing"], 0),
            (Kind::Or, vec!["a", "missing"], 3),
        ] {
            let case = QueryCase {
                id: 0,
                kind,
                terms: terms.into_iter().map(str::to_owned).collect(),
                tags: Vec::new(),
            };
            let ast = query(&case);
            let reference = oracle(searcher.reader(), &case).unwrap();
            assert_eq!(reference.count, count);
            assert_eq!(searcher.count(&ast).unwrap(), count);
            assert_eq!(
                checked_hits(&searcher.search(&ast, 10).unwrap()).unwrap(),
                reference.hits
            );
        }
        drop(searcher);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
