use std::collections::HashSet;
use std::path::Path;

use sha2::{Digest, Sha256};
use tantivy::schema::Value;
use tantivy::{Searcher, TantivyDocument};

#[derive(Debug, PartialEq, Eq)]
pub struct PhysicalRow {
    pub doc_id: u32,
    pub id: String,
    pub sort: u64,
    pub norm: u8,
}

impl PhysicalRow {
    pub fn wire(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\n",
            self.doc_id, self.id, self.sort, self.norm
        )
    }
}

fn valid_id(id: &str) -> bool {
    id.is_ascii() && !id.bytes().any(|byte| matches!(byte, b'\t' | b'\r' | b'\n'))
}

fn unsigned(raw: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let value = raw.parse::<u64>()?;
    if value.to_string() != raw {
        return Err("expected canonical unsigned decimal".into());
    }
    Ok(value)
}

pub fn read_map(path: &Path) -> Result<(Vec<PhysicalRow>, String), Box<dyn std::error::Error>> {
    let raw = std::fs::read(path)?;
    if raw.is_empty() || !raw.ends_with(b"\n") {
        return Err("physical map must contain LF-terminated rows".into());
    }
    let mut rows = Vec::new();
    let mut ids = HashSet::new();
    for line in std::str::from_utf8(&raw)?.split_terminator('\n') {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 4 || !valid_id(fields[1]) {
            return Err("physical map requires docID/ASCII ID/u64 sort/u8 norm".into());
        }
        let row = PhysicalRow {
            doc_id: unsigned(fields[0])?.try_into()?,
            id: fields[1].to_owned(),
            sort: unsigned(fields[2])?,
            norm: unsigned(fields[3])?.try_into()?,
        };
        if row.doc_id as usize != rows.len() || !ids.insert(row.id.clone()) {
            return Err("physical map has a docID gap/order violation or duplicate ID".into());
        }
        rows.push(row);
    }
    Ok((rows, format!("{:x}", Sha256::digest(&raw))))
}

pub fn visit(
    searcher: &Searcher,
    mut observe: impl FnMut(PhysicalRow) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if searcher.segment_readers().len() != 1 {
        return Err("physical inventory requires one segment".into());
    }
    let segment = searcher.segment_reader(0);
    if segment.max_doc() != segment.num_docs() {
        return Err("physical inventory requires no deletions".into());
    }
    let schema = searcher.schema();
    let id = schema.get_field("id")?;
    let text = schema.get_field("text")?;
    let norms = segment
        .fieldnorms_readers()
        .get_field(text)?
        .ok_or("missing norms")?;
    let sort = segment.fast_fields().u64("sort_field")?;
    let store = segment.get_store_reader(8)?;
    let mut ids = HashSet::new();
    for doc_id in 0..segment.max_doc() {
        let document: TantivyDocument = store.get(doc_id)?;
        let mut id_values = document.get_all(id);
        let value = id_values.next().ok_or("missing stored ID")?;
        let external_id = value.as_str().ok_or("stored ID must be a string")?;
        if id_values.next().is_some()
            || !valid_id(external_id)
            || !ids.insert(external_id.to_owned())
        {
            return Err("multiple/duplicate stored IDs or ID outside ASCII TSV domain".into());
        }
        let mut values = sort.values_for_doc(doc_id);
        let value = values.next().ok_or("missing sort value")?;
        if values.next().is_some() {
            return Err("multiple sort values".into());
        }
        observe(PhysicalRow {
            doc_id,
            id: external_id.to_owned(),
            sort: value,
            norm: norms.fieldnorm_id(doc_id),
        })?;
    }
    Ok(())
}

pub fn verify(
    searcher: &Searcher,
    expected: &[PhysicalRow],
) -> Result<String, Box<dyn std::error::Error>> {
    let mut observed = Sha256::new();
    let mut count = 0usize;
    visit(searcher, |row| {
        if expected.get(count) != Some(&row) {
            return Err(format!("actual physical tuple differs at docID {count}").into());
        }
        observed.update(row.wire().as_bytes());
        count += 1;
        Ok(())
    })?;
    if count != expected.len() {
        return Err("actual physical document count differs".into());
    }
    Ok(format!("{:x}", observed.finalize()))
}
