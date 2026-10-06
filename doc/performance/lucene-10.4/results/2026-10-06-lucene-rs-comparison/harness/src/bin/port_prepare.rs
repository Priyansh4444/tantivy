use balanced_search::{Result, emit};
use lucene_rs::analysis::WhitespaceAnalyzer;
use lucene_rs::{
    Analyzer, DirectoryReader, Document, Field, IndexWriter, IndexWriterConfig, OpenMode, Store,
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::Path;

const DOCS: usize = 1_000_000;
const FIELD_DOCS: u64 = 917_578;
const TOKENS: u64 = 294_827_020;
const TERMS: u64 = 1_642_896;
const POSTINGS: u64 = 126_703_012;
const MAP_SHA: &str = "6f7082e3ff479c7be7fe6f819857716630fc4b6b64acc2d6005f1107127029c7";
const CORPUS_SHA: &str = "5a04d4c5f6e418e8d0cf035f27dda8f9781e852ef41adb133237359fca263961";
const PAYLOAD_SHA: &str = "893a75958e6d15c997d414d5fb5fc47c633aa18c68fcd33bc0f8e13abfa8dbb8";

#[derive(Debug)]
struct Row {
    id: String,
    sort: u64,
    norm: u8,
}

fn unsigned(text: &str) -> Result<u64> {
    let value: u64 = text.parse()?;
    if value.to_string() != text {
        return Err("noncanonical unsigned decimal".into());
    }
    Ok(value)
}

fn load_map(path: &Path) -> Result<(Vec<Row>, String)> {
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err("map needs LF termination".into());
    }
    let mut rows = Vec::new();
    let mut ids = HashSet::new();
    for line in std::str::from_utf8(&bytes)?.split_terminator('\n') {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 4
            || !fields[1].is_ascii()
            || fields[1]
                .bytes()
                .any(|b| matches!(b, b'\r' | b'\n' | b'\t'))
        {
            return Err("invalid physical map row".into());
        }
        if usize::try_from(unsigned(fields[0])?)? != rows.len() || !ids.insert(fields[1].to_owned())
        {
            return Err("map order or duplicate ID violation".into());
        }
        rows.push(Row {
            id: fields[1].to_owned(),
            sort: unsigned(fields[2])?,
            norm: unsigned(fields[3])?.try_into()?,
        });
    }
    Ok((rows, format!("{:x}", Sha256::digest(bytes))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusRow {
    id: String,
    text: String,
    sort_field: u64,
}

fn text_length(text: &str) -> Result<u32> {
    if !text.bytes().all(|b| b == b' ' || b.is_ascii_lowercase()) {
        return Err("text outside [a-z ] domain".into());
    }
    let words: Vec<_> = text.split(' ').filter(|word| !word.is_empty()).collect();
    if words.iter().any(|word| word.len() > 255) {
        return Err("token exceeds 255 bytes".into());
    }
    let mut cursor = 0usize;
    let mut valid = true;
    WhitespaceAnalyzer.analyze("text", text, &mut |term, increment| {
        if increment != 1 || words.get(cursor).copied() != Some(term) {
            valid = false;
        }
        cursor += 1;
    });
    if !valid || cursor != words.len() {
        return Err("analyzer token/position mismatch".into());
    }
    Ok(words.len().try_into()?)
}

fn index(corpus: &Path, output: &Path, map: &Path) -> Result<()> {
    let (rows, map_hash) = load_map(map)?;
    if rows.len() != DOCS || map_hash != MAP_SHA {
        return Err("frozen map identity mismatch".into());
    }
    if output.exists() {
        return Err("index output must not exist".into());
    }
    let config = IndexWriterConfig::new(WhitespaceAnalyzer)
        .ram_buffer_size_mb(500)
        .max_buffered_docs(25_000)
        .open_mode(OpenMode::Create);
    let mut writer = IndexWriter::open(output, config)?;
    let mut reader = BufReader::new(std::fs::File::open(corpus)?);
    let mut raw_hash = Sha256::new();
    let mut line = String::new();
    let mut count = 0usize;
    let mut tokens = 0u64;
    let mut field_docs = 0u64;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if !line.ends_with('\n') {
            return Err("corpus row missing LF".into());
        }
        raw_hash.update(line.as_bytes());
        let row: CorpusRow = serde_json::from_str(&line)?;
        let expected = rows.get(count).ok_or("corpus exceeds map")?;
        let length = text_length(&row.text)?;
        if row.id != expected.id
            || row.sort_field != expected.sort
            || lucene_rs::sim::int_to_byte4(length) != expected.norm
        {
            return Err(format!("corpus/map identity or norm mismatch at {count}").into());
        }
        tokens += u64::from(length);
        field_docs += u64::from(length > 0);
        let doc = Document::new()
            .with(Field::stored("id", row.id))
            .with(Field::stored("sort_field", row.sort_field.to_string()))
            .with(Field::text("text", row.text, Store::No));
        writer.add_document(&doc)?;
        count += 1;
        if count % 25_000 == 0 {
            writer.flush()?;
        }
    }
    let corpus_hash = format!("{:x}", raw_hash.finalize());
    if count != DOCS || tokens != TOKENS || field_docs != FIELD_DOCS || corpus_hash != CORPUS_SHA {
        return Err("frozen corpus population or hash mismatch".into());
    }
    writer.force_merge(1)?;
    writer.commit()?;
    writer.close()?;
    emit(
        &json!({"op":"index","documents":count,"field_docs":field_docs,"tokens":tokens,"corpus_sha256":corpus_hash,"map_sha256":map_hash,"batch_docs":25000,"ram_mb":500,"force_merge":1}),
    )
}

fn framed(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn audit(index: &Path, map: &Path, terms: &Path) -> Result<()> {
    let (rows, map_hash) = load_map(map)?;
    if rows.len() != DOCS || map_hash != MAP_SHA {
        return Err("frozen map identity mismatch".into());
    }
    let reader = DirectoryReader::open(index)?;
    reader.check_integrity()?;
    if reader.segments().len() != 1
        || reader.max_doc() as usize != DOCS
        || reader.num_deleted_docs() != 0
    {
        return Err("requires one deletion-free million-document segment".into());
    }
    let seg = &reader.segments()[0];
    let field = seg.field_infos().get("text").ok_or("missing text field")?;
    let norms = seg.norms(field.number).ok_or("missing text norms")?;
    let stats = seg.field_stats("text").ok_or("missing field statistics")?;
    if stats.doc_count != FIELD_DOCS
        || stats.sum_total_term_freq != TOKENS
        || stats.sum_doc_freq != POSTINGS
        || stats.num_terms != TERMS
    {
        return Err("field statistics mismatch".into());
    }
    let mut physical = Sha256::new();
    for (doc, row) in rows.iter().enumerate() {
        let stored = seg.document(doc.try_into()?)?;
        if stored.len() != 2
            || stored.get_all("id").count() != 1
            || stored.get_all("sort_field").count() != 1
            || stored.get_str("id") != Some(row.id.as_str())
            || stored.get_str("sort_field") != Some(row.sort.to_string().as_str())
            || norms.get(doc).copied() != Some(row.norm)
        {
            return Err(format!("physical tuple mismatch at {doc}").into());
        }
        physical.update(format!("{doc}\t{}\t{}\t{}\n", row.id, row.sort, row.norm).as_bytes());
    }
    let physical_hash = format!("{:x}", physical.finalize());
    if physical_hash != MAP_SHA {
        return Err("physical tuple hash mismatch".into());
    }
    let mut hash = Sha256::new();
    framed(&mut hash, b"canonical-postings-v1");
    framed(&mut hash, b"text");
    let mut previous_term = String::new();
    let mut term_count = 0u64;
    let mut postings_count = 0u64;
    let mut position_count = 0u64;
    let mut present = vec![false; DOCS];
    let mut term_list_hash = Sha256::new();
    let mut input = BufReader::new(std::fs::File::open(terms)?);
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        if !line.ends_with('\n') {
            return Err("term list needs LF termination".into());
        }
        term_list_hash.update(line.as_bytes());
        let parts: Vec<_> = line.trim_end_matches('\n').split('\t').collect();
        if parts.len() != 3
            || parts[0].is_empty()
            || parts[0].len() > 255
            || !parts[0].bytes().all(|b| b.is_ascii_lowercase())
            || parts[0] <= previous_term.as_str()
        {
            return Err("invalid sorted canonical term list".into());
        }
        let df: u32 = unsigned(parts[1])?.try_into()?;
        let ttf = unsigned(parts[2])?;
        if df == 0 || ttf < u64::from(df) {
            return Err("invalid expected term statistics".into());
        }
        let (info, meta) = seg
            .term_meta("text", parts[0].as_bytes())
            .ok_or("expected term absent")?;
        if meta.doc_freq != df || meta.total_term_freq != ttf {
            return Err("term statistics mismatch".into());
        }
        framed(&mut hash, parts[0].as_bytes());
        hash.update(df.to_le_bytes());
        let mut posting = seg.postings(&info, &meta, true);
        let mut seen = 0u32;
        let mut seen_tokens = 0u64;
        let mut previous_doc = None;
        loop {
            let raw_doc = posting.next_doc();
            if raw_doc == i32::MAX {
                break;
            }
            let doc = u32::try_from(raw_doc)?;
            if doc as usize >= DOCS || previous_doc.is_some_and(|prev| doc <= prev) {
                return Err("decoded doc order/domain violation".into());
            }
            let freq = posting.freq();
            if freq == 0 {
                return Err("zero posting frequency".into());
            }
            hash.update(doc.to_le_bytes());
            hash.update(freq.to_le_bytes());
            hash.update(u64::from(freq).to_le_bytes());
            let mut last_position = None;
            for _ in 0..freq {
                let position = posting.next_position();
                if last_position.is_some_and(|last| position <= last) {
                    return Err("decoded position order violation".into());
                }
                hash.update(position.to_le_bytes());
                last_position = Some(position);
            }
            seen += 1;
            seen_tokens += u64::from(freq);
            present[doc as usize] = true;
            previous_doc = Some(doc);
        }
        if seen != df || seen_tokens != ttf {
            return Err("decoded term population mismatch".into());
        }
        term_count += 1;
        postings_count += u64::from(seen);
        position_count += seen_tokens;
        previous_term.clear();
        previous_term.push_str(parts[0]);
    }
    hash.update(term_count.to_le_bytes());
    let payload_hash = format!("{:x}", hash.finalize());
    if term_count != TERMS
        || postings_count != POSTINGS
        || position_count != TOKENS
        || present.iter().filter(|&&p| p).count() as u64 != FIELD_DOCS
        || payload_hash != PAYLOAD_SHA
    {
        return Err("canonical payload population/hash mismatch".into());
    }
    reader.check_integrity()?;
    emit(
        &json!({"op":"audit","documents":DOCS,"field_docs":FIELD_DOCS,"terms":term_count,"postings":postings_count,"positions":position_count,"physical_sha256":physical_hash,"payload_sha256":payload_hash,"term_list_sha256":format!("{:x}",term_list_hash.finalize()),"integrity":"pass"}),
    )
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, corpus, output, map] if mode == "index" => {
            index(Path::new(corpus), Path::new(output), Path::new(map))
        }
        [mode, index, map, terms] if mode == "audit" => {
            audit(Path::new(index), Path::new(map), Path::new(terms))
        }
        _ => Err("usage: port_prepare index CORPUS OUTPUT MAP | audit INDEX MAP TERMS".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restricted_replay_tokens_are_exact_and_empty_is_preserved() {
        assert_eq!(text_length("  a abc a  ").unwrap(), 3);
        assert_eq!(text_length("   ").unwrap(), 0);
        assert!(text_length("A").is_err());
        assert!(text_length("a\tb").is_err());
        assert!(text_length(&"a".repeat(256)).is_err());
        assert_eq!(text_length(&"a".repeat(255)).unwrap(), 1);
        assert!(unsigned("01").is_err());
    }
}
