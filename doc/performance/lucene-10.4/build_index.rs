#[path = "shared/wiki_physical.rs"]
mod wiki_physical;

use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tantivy::index::SegmentId;
use tantivy::merge_policy::NoMergePolicy;
use tantivy::query::Bm25StatisticsProvider;
use tantivy::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions, FAST, STORED};
use tantivy::tokenizer::{LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer};
use tantivy::{Index, IndexWriter, TantivyDocument};

const ANALYZER: &str = "wiki_ascii_lucene";
const WIKI_DOCS: u64 = 1_000_000;
const WIKI_TOKENS: u64 = 294_827_020;

struct OrderedReplay {
    rows: Vec<wiki_physical::PhysicalRow>,
    map_path: PathBuf,
    map_hash: String,
    receipt_path: PathBuf,
    receipt_hash: String,
    raw_hash: String,
    tokens: u64,
    field_docs: u64,
    batch_size: usize,
    memory: usize,
}

fn commit_batch(
    writer: &mut IndexWriter<TantivyDocument>,
    index: &Index,
    previous: &mut HashSet<SegmentId>,
    ordered: &mut Vec<SegmentId>,
    batches: &mut Vec<serde_json::Value>,
    start: u64,
    count: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    writer.commit()?;
    let metas = index.searchable_segment_metas()?;
    let current: HashSet<_> = metas.iter().map(|meta| meta.id()).collect();
    let new: Vec<_> = metas
        .iter()
        .filter(|meta| !previous.contains(&meta.id()))
        .collect();
    if !previous.is_subset(&current) || new.len() != 1 {
        return Err(
            "ordered batch must expose exactly one new segment and retain all prior segments"
                .into(),
        );
    }
    let meta = new[0];
    if u64::from(meta.max_doc()) != count || meta.num_docs() != meta.max_doc() {
        return Err("ordered batch segment count/deletions differ".into());
    }
    ordered.push(meta.id());
    let mut before: Vec<_> = previous.iter().map(|id| id.uuid_string()).collect();
    let mut after: Vec<_> = current.iter().map(|id| id.uuid_string()).collect();
    before.sort();
    after.sort();
    batches.push(serde_json::json!({"segment_id":meta.id().uuid_string(),"start_ordinal":start,"documents":count,
        "before_segment_ids":before,"after_segment_ids":after}));
    *previous = current;
    Ok(())
}

fn wiki_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field("id", STORED);
    builder.add_text_field(
        "text",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(ANALYZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions)
                .set_fieldnorms(true),
        ),
    );
    builder.add_u64_field("sort_field", FAST);
    builder.build()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .ok_or("expected a new absent output directory")?;
    let mut expected_docs = WIKI_DOCS;
    let mut fixture_flag = false;
    let mut batch_size = None;
    let mut map_path = None;
    let mut replay_receipt = None;
    let mut memory = None;
    let mut seen = HashSet::new();
    while let Some(option) = args.next() {
        if !seen.insert(option.clone()) {
            return Err("duplicate option".into());
        }
        match option.as_str() {
            "--fixture-docs" => {
                expected_docs = args.next().ok_or("missing fixture count")?.parse()?;
                fixture_flag = true;
            }
            "--ordered-batches" => {
                batch_size = Some(args.next().ok_or("missing batch size")?.parse::<usize>()?)
            }
            "--physical-map" => {
                map_path = Some(PathBuf::from(args.next().ok_or("missing physical map")?))
            }
            "--replay-receipt" => {
                replay_receipt = Some(PathBuf::from(args.next().ok_or("missing replay receipt")?))
            }
            "--memory-bytes" => {
                memory = Some(
                    args.next()
                        .ok_or("missing memory budget")?
                        .parse::<usize>()?,
                )
            }
            _ => return Err("unknown option".into()),
        }
    }
    if expected_docs == 0 || expected_docs > u64::from(u32::MAX) {
        return Err("invalid document count".into());
    }
    let ordered_options = (batch_size, map_path, replay_receipt);
    let ordered = match ordered_options {
        (None, None, None) if memory.is_none() => None,
        (Some(batch_size), Some(map_path), Some(receipt_path)) => {
            let memory = memory.unwrap_or(500_000_000);
            if batch_size == 0 || !(15_000_000..(u32::MAX as usize - 1_000_000)).contains(&memory) {
                return Err("invalid ordered batch size or memory budget".into());
            }
            let (rows, map_hash) = wiki_physical::read_map(&map_path)?;
            let receipt_bytes = std::fs::read(&receipt_path)?;
            let replay: serde_json::Value = serde_json::from_slice(&receipt_bytes)?;
            if rows.len() as u64 != expected_docs
                || replay["protocol"].as_str() != Some("wiki-physical-replay-v1")
                || replay["fixture_mode"].as_bool() != Some(fixture_flag)
                || replay["documents"].as_u64() != Some(expected_docs)
                || replay["physical_map_sha256"].as_str() != Some(map_hash.as_str())
                || replay["exact_id_bijection"].as_bool() != Some(true)
                || replay["source_id_lines_sha256"] != replay["replay_id_lines_sha256"]
            {
                return Err("physical map/replay receipt mismatch".into());
            }
            let raw_hash = replay["replay_raw_sha256"]
                .as_str()
                .ok_or("missing actual replay hash")?
                .to_owned();
            for key in [
                "replay_raw_sha256",
                "corpus_raw_sha256",
                "source_id_lines_sha256",
                "replay_id_lines_sha256",
            ] {
                let hash = replay[key].as_str().ok_or("missing replay hash")?;
                if hash.len() != 64
                    || !hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err("invalid replay hash".into());
                }
            }
            let tokens = replay["tokens"].as_u64().ok_or("missing replay tokens")?;
            let field_docs = replay["field_doc_count"]
                .as_u64()
                .ok_or("missing physical field count")?;
            if field_docs > expected_docs || tokens < field_docs {
                return Err("invalid replay physical population/tokens".into());
            }
            if !fixture_flag
                && (tokens != WIKI_TOKENS
                    || field_docs != 917_578
                    || replay["corpus_raw_sha256"].as_str()
                        != Some("2b630549676f1c58a579017b6cd949e25115fe63989f9e75cadadc5c1a1a8238"))
            {
                return Err("ordered full replay differs from frozen corpus boundary".into());
            }
            Some(OrderedReplay {
                rows,
                map_path,
                map_hash,
                receipt_path,
                receipt_hash: format!("{:x}", Sha256::digest(&receipt_bytes)),
                raw_hash,
                tokens,
                field_docs,
                batch_size,
                memory,
            })
        }
        _ => {
            return Err(
                "--ordered-batches/--physical-map/--replay-receipt are required together; memory \
                 is ordered-only"
                    .into(),
            )
        }
    };
    let fixture = if ordered.is_some() {
        fixture_flag
    } else {
        expected_docs != WIKI_DOCS
    };
    let output_path = Path::new(&output);
    let receipt = output_path.with_file_name(format!(
        "{}.build.json",
        output_path
            .file_name()
            .ok_or("invalid output directory")?
            .to_string_lossy()
    ));
    if receipt.exists() {
        return Err("build receipt already exists".into());
    }
    std::fs::create_dir(&output)?;
    let schema = wiki_schema();
    let id = schema.get_field("id")?;
    let text = schema.get_field("text")?;
    let sort = schema.get_field("sort_field")?;
    let index = Index::create_in_dir(Path::new(&output), schema)?;
    index.tokenizers().register(
        ANALYZER,
        TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(RemoveLongFilter::limit(256))
            .filter(LowerCaser)
            .build(),
    );
    if index.settings().sort_by_field.is_some() || index.settings().manual_doc_id_mapping {
        return Err("ordered replay cannot use index sorting/manual remapping".into());
    }
    let mut writer = index
        .writer_with_num_threads(1, ordered.as_ref().map_or(500_000_000, |mode| mode.memory))?;
    writer.set_merge_policy(Box::new(NoMergePolicy));
    let mut count = 0u64;
    let mut tokens = 0u64;
    let mut corpus_hash = Sha256::new();
    let mut raw_hash = Sha256::new();
    let mut committed = HashSet::new();
    let mut ordered_ids = Vec::new();
    let mut batches = Vec::new();
    let mut input = std::io::stdin().lock();
    let mut raw = Vec::new();
    loop {
        raw.clear();
        if input.read_until(b'\n', &mut raw)? == 0 {
            break;
        }
        raw_hash.update(&raw);
        let line = std::str::from_utf8(&raw)?;
        let line = if let Some(line) = line.strip_suffix('\n') {
            line.strip_suffix('\r').unwrap_or(line)
        } else {
            line
        };
        corpus_hash.update(line.as_bytes());
        corpus_hash.update(b"\n");
        let row: serde_json::Value = serde_json::from_str(&line)?;
        let row = row.as_object().ok_or("JSON row must be an object")?;
        if row.len() != 3
            || !["id", "text", "sort_field"]
                .iter()
                .all(|key| row.contains_key(*key))
        {
            return Err(format!("row {} must have exactly id/text/sort_field", count + 1).into());
        }
        let external_id = row["id"].as_str().ok_or("id must be a string")?;
        let content = row["text"].as_str().ok_or("text must be a string")?;
        let sort_value = row["sort_field"]
            .as_u64()
            .ok_or("sort_field must be a u64")?;
        if let Some(mode) = &ordered {
            let expected = mode
                .rows
                .get(count as usize)
                .ok_or("ordered input exceeds map")?;
            if expected.id != external_id || expected.sort != sort_value {
                return Err("ordered input ID/sort differs from physical map".into());
            }
        }
        if !content
            .bytes()
            .all(|byte| byte == b' ' || byte.is_ascii_lowercase())
        {
            return Err(format!("row {} text is outside [a-z ]", count + 1).into());
        }
        let mut row_tokens = 0u64;
        for word in content.split(' ').filter(|word| !word.is_empty()) {
            if word.len() > 255 {
                return Err(
                    format!("row {} contains a word longer than 255 bytes", count + 1).into(),
                );
            }
            row_tokens += 1;
        }
        count += 1;
        if count > expected_docs {
            return Err("input exceeds expected document count".into());
        }
        tokens += row_tokens;
        let mut document = TantivyDocument::default();
        document.add_text(id, external_id);
        document.add_text(text, content);
        document.add_u64(sort, sort_value);
        writer.add_document(document)?;
        if let Some(mode) = &ordered {
            if count % mode.batch_size as u64 == 0 {
                commit_batch(
                    &mut writer,
                    &index,
                    &mut committed,
                    &mut ordered_ids,
                    &mut batches,
                    count - mode.batch_size as u64,
                    mode.batch_size as u64,
                )?;
            }
        }
    }
    if count != expected_docs {
        return Err(format!("expected {expected_docs} documents, observed {count}").into());
    }
    if !fixture && tokens != WIKI_TOKENS {
        return Err(format!("expected {WIKI_TOKENS} tokens, observed {tokens}").into());
    }
    let actual_raw_hash = format!("{:x}", raw_hash.finalize());
    if let Some(mode) = &ordered {
        if actual_raw_hash != mode.raw_hash || tokens != mode.tokens {
            return Err("actual input raw bytes/tokens differ from replay receipt".into());
        }
        let last = count % mode.batch_size as u64;
        if last != 0 {
            commit_batch(
                &mut writer,
                &index,
                &mut committed,
                &mut ordered_ids,
                &mut batches,
                count - last,
                last,
            )?;
        }
        let current: HashSet<_> = index.searchable_segment_ids()?.into_iter().collect();
        if current != committed
            || ordered_ids.iter().copied().collect::<HashSet<_>>() != current
            || ordered_ids.len() != current.len()
        {
            return Err("ordered merge vector does not cover actual segment set exactly".into());
        }
        if ordered_ids.len() > 1 {
            writer.merge(&ordered_ids).wait()?;
        }
        writer.garbage_collect_files().wait()?;
        writer.wait_merging_threads()?;
    } else {
        writer.commit()?;
        writer.wait_merging_threads()?;
        let segments = index.searchable_segment_ids()?;
        if segments.len() > 1 {
            let mut merger = index.writer_with_num_threads::<TantivyDocument>(1, 500_000_000)?;
            merger.merge(&segments).wait()?;
            merger.garbage_collect_files().wait()?;
            merger.wait_merging_threads()?;
        }
    }
    let reader = index.reader()?;
    let searcher = reader.searcher();
    if searcher.segment_readers().len() != 1 || u64::from(searcher.num_docs()) != expected_docs {
        return Err("final replay must have one segment and the expected document count".into());
    }
    let serialized_tokens = searcher
        .segment_reader(0)
        .inverted_index(text)?
        .total_num_tokens();
    if serialized_tokens != tokens {
        return Err("final serialized token total differs from validated input".into());
    }
    let mut completion = serde_json::json!({"analyzer":ANALYZER,"documents":count,"tokens":tokens,
        "segments":1,"fixture_mode":fixture,"corpus_canonical_lines_sha256":format!("{:x}",corpus_hash.finalize())});
    if let Some(mode) = &ordered {
        let physical = wiki_physical::verify(&searcher, &mode.rows)?;
        let (_, current_map_hash) = wiki_physical::read_map(&mode.map_path)?;
        if current_map_hash != mode.map_hash
            || format!("{:x}", Sha256::digest(std::fs::read(&mode.receipt_path)?)) != mode.receipt_hash
            || searcher.field_statistics(text)?.doc_count() != mode.field_docs
        {
            return Err("physical map changed or actual physical field population differs".into());
        }
        completion["ordered_replay"] = serde_json::json!({"protocol":"wiki-ordered-build-v1",
            "actual_raw_input_sha256":actual_raw_hash,"physical_map_sha256":mode.map_hash,
            "replay_receipt_sha256":mode.receipt_hash,"actual_physical_sha256":physical,
            "physical_verified":true,"field_doc_count":mode.field_docs,"memory_bytes":mode.memory,
            "batch_size":mode.batch_size,"batches":batches,
            "merge_order":ordered_ids.iter().map(|id| id.uuid_string()).collect::<Vec<_>>()});
    }
    let mut receipt_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(receipt)?;
    receipt_file.write_all(&serde_json::to_vec_pretty(&completion)?)?;
    println!("{completion}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reversed_merge_changes_physical_order_but_preserves_id_sorted_payload(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::var_os("NATIVE_DOCORDER_TEST_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "wiki-docorder-reverse-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ))
            });
        std::fs::create_dir(&root)?;
        let mut maps = Vec::new();
        for reverse in [false, true] {
            let path = root.join(if reverse {
                "reversed.idx"
            } else {
                "ordered.idx"
            });
            std::fs::create_dir(&path)?;
            let schema = wiki_schema();
            let id = schema.get_field("id")?;
            let text = schema.get_field("text")?;
            let sort = schema.get_field("sort_field")?;
            let index = Index::create_in_dir(&path, schema)?;
            index.tokenizers().register(
                ANALYZER,
                TextAnalyzer::builder(SimpleTokenizer::default())
                    .filter(RemoveLongFilter::limit(256))
                    .filter(LowerCaser)
                    .build(),
            );
            let mut writer = index.writer_with_num_threads::<TantivyDocument>(1, 50_000_000)?;
            writer.set_merge_policy(Box::new(NoMergePolicy));
            let mut previous = HashSet::new();
            let mut ordered = Vec::new();
            let mut batches = Vec::new();
            for (start, ids) in [(0u64, vec!["b", "c"]), (2u64, vec!["a"])] {
                for value in &ids {
                    let mut doc = TantivyDocument::default();
                    doc.add_text(id, value);
                    doc.add_text(text, "alpha beta");
                    doc.add_u64(sort, u64::MAX);
                    writer.add_document(doc)?;
                }
                commit_batch(
                    &mut writer,
                    &index,
                    &mut previous,
                    &mut ordered,
                    &mut batches,
                    start,
                    ids.len() as u64,
                )?;
            }
            if reverse {
                ordered.reverse();
            }
            writer.merge(&ordered).wait()?;
            writer.wait_merging_threads()?;
            let searcher = index.reader()?.searcher();
            let mut actual = Vec::new();
            wiki_physical::visit(&searcher, |row| {
                actual.push(row);
                Ok(())
            })?;
            maps.push(actual);
        }
        assert!(wiki_physical::verify(
            &Index::open_in_dir(root.join("reversed.idx"))?
                .reader()?
                .searcher(),
            &maps[0]
        )
        .is_err());
        let mut left: Vec<_> = maps[0]
            .iter()
            .map(|row| (&row.id, row.sort, row.norm))
            .collect();
        let mut right: Vec<_> = maps[1]
            .iter()
            .map(|row| (&row.id, row.sort, row.norm))
            .collect();
        left.sort();
        right.sort();
        assert_eq!(left, right);
        assert_eq!(
            maps[0]
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["b", "c", "a"]
        );
        assert_eq!(
            maps[1]
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        println!(
            "WITNESS\t{}",
            serde_json::json!({"root":root,"physical_rejected":true,"id_sorted_equal":true,
            "ordered_ids":["b","c","a"],"reversed_ids":["a","b","c"]})
        );
        Ok(())
    }
}
