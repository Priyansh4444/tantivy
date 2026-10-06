use lucene_rs::analysis::{tokenize, StandardAnalyzer};
type Score = f32;
fn native_idf(doc_freq: u64, doc_count: u64) -> Score {
    assert!(doc_count >= doc_freq, "{doc_count} >= {doc_freq}");
    let x = ((doc_count - doc_freq) as f64 + 0.5) / (doc_freq as f64 + 0.5);
    (1.0 + x).ln() as Score
}
fn main() {
    let n = 54505;
    let idf = lucene_rs::sim::idf(n,n);
    let native = native_idf(n,n);
    let sim = lucene_rs::sim::Bm25::for_term(1.0,n,n,n);
    println!("idf={:08x} ours_native_idf={:08x} score={:08x}", idf.to_bits(), native.to_bits(), sim.score(1.0,1).to_bits());
    println!("tokens={:?}",tokenize(&StandardAnalyzer::new().max_token_length(3),"body","abcdef z 😀"));
}
