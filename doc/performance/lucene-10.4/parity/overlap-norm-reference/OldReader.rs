fn main() {
    assert_eq!(tantivy::INDEX_FORMAT_VERSION, 11);
    let path = std::env::args().nth(1).expect("index directory");
    match tantivy::Index::open_in_dir(path) {
        Ok(index) => { println!("OPEN format={} segments={}", tantivy::INDEX_FORMAT_VERSION, index.searchable_segment_ids().unwrap().len()); }
        Err(error) => { eprintln!("REJECT format=11 {error}"); std::process::exit(2); }
    }
}
