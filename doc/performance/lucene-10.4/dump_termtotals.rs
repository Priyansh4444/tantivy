use std::io::Write;
use tantivy::postings::Postings;
use tantivy::schema::IndexRecordOption;
use tantivy::{DocSet, Index, TERMINATED};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut output = std::io::BufWriter::new(stdout.lock());
    let path = std::env::args().nth(1).ok_or("expected index directory")?;
    let with_positions = std::env::args().nth(2).as_deref() == Some("--positions");
    let index = Index::open_in_dir(path)?;
    let field = index.schema().get_field("text")?;
    let reader = index.reader()?;
    let searcher = reader.searcher();
    if searcher.segment_readers().len() != 1 {
        return Err("expected one segment".into());
    }
    let segment = searcher.segment_reader(0);
    if segment.max_doc() != segment.num_docs() {
        return Err("expected no deletions".into());
    }
    let inverted = segment.inverted_index(field)?;
    let mut terms = inverted.terms().stream()?;
    let mut present = vec![false; segment.max_doc() as usize];
    let mut term_count = 0u64;
    let mut token_count = 0u64;
    while terms.advance() {
        let term = std::str::from_utf8(terms.key())?;
        if term.is_empty() || !term.bytes().all(|byte| byte.is_ascii_lowercase()) {
            return Err("term is outside lowercase ASCII workload".into());
        }
        let info = terms.value();
        let mut postings = inverted.read_postings_from_terminfo(
            info,
            if with_positions {
                IndexRecordOption::WithFreqsAndPositions
            } else {
                IndexRecordOption::WithFreqs
            },
        )?;
        let mut df = 0u32;
        let mut ttf = 0u64;
        let mut posting_positions = Vec::new();
        while postings.doc() != TERMINATED {
            present[postings.doc() as usize] = true;
            if with_positions {
                let mut positions = Vec::new();
                postings.positions(&mut positions);
                posting_positions.push((postings.doc(), positions));
            }
            df += 1;
            ttf += u64::from(postings.term_freq());
            postings.advance();
        }
        if df != info.doc_freq {
            return Err("term doc-frequency metadata differs from postings".into());
        }
        if with_positions {
            writeln!(
                output,
                "{term}\t{df}\t{ttf}\t{}",
                serde_json::to_string(&posting_positions)?
            )?;
        } else {
            writeln!(output, "{term}\t{df}\t{ttf}")?;
        }
        term_count += 1;
        token_count += ttf;
    }
    eprintln!(
        "{}",
        serde_json::json!({"documents":segment.num_docs(),"field_doc_count":present.iter().filter(|&&yes|yes).count(),
        "terms":term_count,"derived_total_tokens":token_count,"serialized_total_tokens":inverted.total_num_tokens()})
    );
    output.flush()?;
    Ok(())
}
