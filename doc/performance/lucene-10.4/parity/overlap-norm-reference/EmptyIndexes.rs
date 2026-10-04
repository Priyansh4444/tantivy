use tantivy::schema::{FieldNormPolicy, Schema, TextFieldIndexing, TextOptions};
fn main() -> tantivy::Result<()> {
    let root=std::env::args().nth(1).expect("root directory");
    for (name,policy,enabled) in [
        ("legacy_enabled",FieldNormPolicy::CountAllTokens,true),
        ("legacy_disabled",FieldNormPolicy::CountAllTokens,false),
        ("discount_enabled",FieldNormPolicy::DiscountOverlaps,true),
        ("discount_disabled",FieldNormPolicy::DiscountOverlaps,false),
    ] {
        let path=std::path::Path::new(&root).join(name);
        std::fs::create_dir_all(&path)?;
        let mut schema=Schema::builder();
        schema.add_text_field("text",TextOptions::default().set_indexing_options(TextFieldIndexing::default().set_fieldnorm_policy(policy).set_fieldnorms(enabled)));
        let index=tantivy::Index::create_in_dir(&path,schema.build())?;
        assert!(index.searchable_segment_ids()?.is_empty());
        assert!(tantivy::Index::open_in_dir(&path)?.searchable_segment_ids()?.is_empty());
        println!("NEW OPEN {name} format={}",tantivy::INDEX_FORMAT_VERSION);
    }
    Ok(())
}
