use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::path::Path;
use tantivy::merge_policy::NoMergePolicy;
use tantivy::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions, FAST, STORED};
use tantivy::tokenizer::{LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer};
use tantivy::{Index, TantivyDocument};

const ANALYZER: &str = "wiki_ascii_lucene";
const WIKI_DOCS: u64 = 1_000_000;
const WIKI_TOKENS: u64 = 294_827_020;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .ok_or("expected a new absent output directory")?;
    let expected_docs = match args.next().as_deref() {
        None => WIKI_DOCS,
        Some("--fixture-docs") => args
            .next()
            .ok_or("missing fixture document count")?
            .parse::<u64>()?,
        Some(_) => return Err("unknown option".into()),
    };
    if expected_docs == 0 || args.next().is_some() {
        return Err("invalid arguments".into());
    }
    let fixture = expected_docs != WIKI_DOCS;
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
    let mut builder = Schema::builder();
    let id = builder.add_text_field("id", STORED);
    let text = builder.add_text_field(
        "text",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(ANALYZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions)
                .set_fieldnorms(true),
        ),
    );
    let sort = builder.add_u64_field("sort_field", FAST);
    let index = Index::create_in_dir(Path::new(&output), builder.build())?;
    index.tokenizers().register(
        ANALYZER,
        TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(RemoveLongFilter::limit(256))
            .filter(LowerCaser)
            .build(),
    );
    let mut writer = index.writer_with_num_threads(1, 500_000_000)?;
    writer.set_merge_policy(Box::new(NoMergePolicy));
    let mut count = 0u64;
    let mut tokens = 0u64;
    let mut corpus_hash = Sha256::new();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
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
    }
    if count != expected_docs {
        return Err(format!("expected {expected_docs} documents, observed {count}").into());
    }
    if !fixture && tokens != WIKI_TOKENS {
        return Err(format!("expected {WIKI_TOKENS} tokens, observed {tokens}").into());
    }
    writer.commit()?;
    writer.wait_merging_threads()?;
    let segments = index.searchable_segment_ids()?;
    if segments.len() > 1 {
        let mut merger = index.writer_with_num_threads::<TantivyDocument>(1, 500_000_000)?;
        merger.merge(&segments).wait()?;
        merger.garbage_collect_files().wait()?;
        merger.wait_merging_threads()?;
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
    let completion = serde_json::json!({"analyzer":ANALYZER,"documents":count,"tokens":tokens,
        "segments":1,"fixture_mode":fixture,"corpus_canonical_lines_sha256":format!("{:x}",corpus_hash.finalize())});
    let mut receipt_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(receipt)?;
    receipt_file.write_all(&serde_json::to_vec_pretty(&completion)?)?;
    println!("{completion}");
    Ok(())
}
