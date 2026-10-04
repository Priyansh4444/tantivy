use std::io::Write;
use tantivy::schema::Value;
use tantivy::{Index, TantivyDocument};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut output = std::io::BufWriter::new(stdout.lock());
    let path = std::env::args().nth(1).ok_or("expected index directory")?;
    let index = Index::open_in_dir(path)?;
    let schema = index.schema();
    let id = schema.get_field("id")?;
    let text = schema.get_field("text")?;
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let mut rows = Vec::with_capacity(searcher.num_docs() as usize);
    for segment in searcher.segment_readers() {
        if segment.num_docs() != segment.max_doc() {
            return Err("expected no deleted documents".into());
        }
        let norms = segment
            .fieldnorms_readers()
            .get_field(text)?
            .ok_or("missing text fieldnorms")?;
        let sort = segment.fast_fields().u64("sort_field")?;
        let store = segment.get_store_reader(8)?;
        for doc in 0..segment.max_doc() {
            let document: TantivyDocument = store.get(doc)?;
            let external_id = document
                .get_first(id)
                .and_then(|value| value.as_str())
                .ok_or("missing stored ID")?;
            if !external_id.is_ascii()
                || external_id
                    .bytes()
                    .any(|byte| matches!(byte, b'\t' | b'\r' | b'\n'))
            {
                return Err("ID is outside ASCII TSV comparison domain".into());
            }
            let mut values = sort.values_for_doc(doc);
            let value = values.next().ok_or("missing sort value")?;
            if values.next().is_some() {
                return Err("multiple sort values".into());
            }
            rows.push((external_id.to_owned(), value, norms.fieldnorm_id(doc)));
        }
    }
    rows.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    for pair in rows.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err("duplicate external ID".into());
        }
    }
    for (id, sort, norm) in rows {
        writeln!(output, "{id}\t{sort}\t{norm}")?;
    }
    output.flush()?;
    Ok(())
}
