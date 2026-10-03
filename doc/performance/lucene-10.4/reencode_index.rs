use tantivy::directory::MmapDirectory;
use tantivy::indexer::merge_indices;
use tantivy::Index;
fn main() -> tantivy::Result<()> {
    let mut args = std::env::args().skip(1);
    let source = args.next().expect("source index");
    let target = args.next().expect("new empty output directory");
    std::fs::create_dir(&target)?;
    let index = Index::open_in_dir(source)?;
    let output = merge_indices(&[index], MmapDirectory::open(target)?)?;
    let reader = output.reader()?;
    println!(
        "segments={} docs={}",
        reader.searcher().segment_readers().len(),
        reader.searcher().num_docs()
    );
    Ok(())
}
