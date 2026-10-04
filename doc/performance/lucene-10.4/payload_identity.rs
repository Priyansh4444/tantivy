use sha2::{Digest, Sha256};
use tantivy::postings::Postings;
use tantivy::schema::{Document, FieldType};
use tantivy::{DocSet, Index, TantivyDocument, TERMINATED};

fn bytes(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

fn finish(hash: Sha256) -> String {
    format!("{:x}", hash.finalize())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected an index directory")?;
    let index = Index::open_in_dir(&path)?;
    let schema = index.schema();
    let reader = index.reader()?;
    let searcher = reader.searcher();
    if searcher.segment_readers().len() != 1 {
        return Err("payload verifier requires the frozen single-segment fixture".into());
    }
    let segment = searcher.segment_reader(0);
    if segment.max_doc() != segment.num_docs() {
        return Err("payload verifier requires a deletion-free fixture; doc-ID remapping needs a separate oracle".into());
    }
    let mut schema_hash = Sha256::new();
    bytes(&mut schema_hash, &serde_json::to_vec(&schema)?);
    let mut fields = Vec::new();
    let mut metadata = Vec::new();
    let mut positions = Vec::new();
    for (field, entry) in schema.fields() {
        if matches!(entry.field_type(), FieldType::JsonObject(_)) {
            return Err(format!("unsupported mixed JSON field {}", entry.name()).into());
        }
        let mut field_report = serde_json::json!({"name": entry.name()});
        if let Some(option) = entry.field_type().index_record_option() {
            let inverted = segment.inverted_index(field)?;
            let mut terms = inverted.terms().stream()?;
            let mut hash = Sha256::new();
            bytes(&mut hash, b"canonical-postings-v1");
            bytes(&mut hash, entry.name().as_bytes());
            let mut term_count = 0u64;
            let mut postings_count = 0u64;
            let mut token_count = 0u64;
            let mut position_count = 0u64;
            let mut field_docs = vec![false; segment.max_doc() as usize];
            while terms.advance() {
                term_count += 1;
                bytes(&mut hash, terms.key());
                let info = terms.value();
                hash.update(info.doc_freq.to_le_bytes());
                let mut postings = inverted.read_postings_from_terminfo(info, option)?;
                let mut observed_doc_freq = 0u32;
                while postings.doc() != TERMINATED {
                    let doc = postings.doc();
                    let frequency = postings.term_freq();
                    field_docs[doc as usize] = true;
                    observed_doc_freq += 1;
                    postings_count += 1;
                    token_count += u64::from(frequency);
                    hash.update(doc.to_le_bytes());
                    hash.update(frequency.to_le_bytes());
                    positions.clear();
                    if option.has_positions() {
                        postings.positions(&mut positions);
                    }
                    hash.update((positions.len() as u64).to_le_bytes());
                    position_count += positions.len() as u64;
                    for &position in &positions {
                        hash.update(position.to_le_bytes());
                    }
                    postings.advance();
                }
                if observed_doc_freq != info.doc_freq {
                    return Err(format!("{} term doc-frequency mismatch", entry.name()).into());
                }
            }
            hash.update(term_count.to_le_bytes());
            field_report["postings"] = serde_json::json!({
                "sha256": finish(hash), "terms": term_count, "postings": postings_count,
                "derived_tokens": token_count, "positions": position_count,
                "derived_field_docs": field_docs.iter().filter(|&&present| present).count(),
            });
            metadata.push(serde_json::json!({"name":entry.name(), "serialized_token_total":inverted.total_num_tokens()}));
        }
        if let Some(norms) = segment.fieldnorms_readers().get_field(field)? {
            let mut hash = Sha256::new();
            bytes(&mut hash, b"canonical-fieldnorms-v1");
            for doc in 0..segment.max_doc() {
                hash.update([norms.fieldnorm_id(doc)]);
            }
            field_report["fieldnorms_sha256"] = serde_json::json!(finish(hash));
        }
        if entry.field_type().is_fast() {
            // Fail closed for other types: the frozen wiki fixture has only a
            // u64 sort_field. Do not silently omit an unknown fast-field payload.
            if !matches!(entry.field_type(), FieldType::U64(_)) {
                return Err(format!("unsupported fast-field type for {}", entry.name()).into());
            }
            let column = segment.fast_fields().u64(entry.name())?;
            let mut hash = Sha256::new();
            bytes(&mut hash, b"canonical-fast-u64-v1");
            for doc in 0..segment.max_doc() {
                let values: Vec<_> = column.values_for_doc(doc).collect();
                hash.update((values.len() as u64).to_le_bytes());
                for value in values {
                    hash.update(value.to_le_bytes());
                }
            }
            field_report["fast_u64_sha256"] = serde_json::json!(finish(hash));
        }
        fields.push(field_report);
    }
    let store = segment.get_store_reader(8)?;
    let mut stored_hash = Sha256::new();
    bytes(&mut stored_hash, b"canonical-stored-json-v1");
    for doc_id in 0..segment.max_doc() {
        let document: TantivyDocument = store.get(doc_id)?;
        let value: serde_json::Value = serde_json::from_str(&document.to_json(&schema))?;
        bytes(&mut stored_hash, &serde_json::to_vec(&value)?);
    }
    println!(
        "{}",
        serde_json::json!({
            "index": path,
            "logical": {"schema_sha256":finish(schema_hash), "max_doc":segment.max_doc(),
                        "live_docs":segment.num_docs(), "fields":fields,
                        "stored_documents_sha256":finish(stored_hash)},
            "metadata_headers":metadata,
        })
    );
    Ok(())
}
