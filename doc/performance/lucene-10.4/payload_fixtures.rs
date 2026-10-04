use std::path::Path;
use tantivy::directory::{Directory, ManagedDirectory, MmapDirectory};
use tantivy::fieldnorm::FieldNormsSerializer;
use tantivy::merge_policy::NoMergePolicy;
use tantivy::schema::{
    IndexRecordOption, JsonObjectOptions, Schema, TextFieldIndexing, TextOptions, FAST, STORED,
    TEXT,
};
use tantivy::tokenizer::{PreTokenizedString, Token};
use tantivy::{Index, TantivyDocument};

fn create(path: &Path, variant: &str) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir(path)?;
    let mut builder = Schema::builder();
    let id = builder.add_text_field("id", STORED);
    let text = builder.add_text_field("text", TEXT);
    let string_fast = variant == "unsupported_fast_string";
    let sort = if string_fast {
        builder.add_text_field("sort_field", TextOptions::default().set_fast("raw"))
    } else {
        builder.add_u64_field("sort_field", FAST)
    };
    if variant == "unsupported_mixed_json" {
        builder.add_json_field(
            "payload",
            JsonObjectOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            ),
        );
    }
    let schema = builder.build();
    let index = Index::create_in_dir(path, schema.clone())?;
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    writer.set_merge_policy(Box::new(NoMergePolicy));
    for (doc_id, original) in [
        vec!["alpha", "beta", "alpha"],
        vec!["beta", "gamma"],
        vec!["alpha", "gamma", "beta"],
    ]
    .into_iter()
    .enumerate()
    {
        if variant == "unsupported_mixed_json" {
            writer.add_document(TantivyDocument::parse_json(&schema,
                &serde_json::json!({"id":doc_id.to_string(),"text":original.join(" "),"sort_field":doc_id,
                                   "payload":{"words":"alpha beta","number":7,"mixed":[1,"gamma"]}}).to_string())?)?;
            continue;
        }
        let mut words = original;
        if variant == "postings" && doc_id == 1 {
            words[1] = "delta";
        }
        if variant == "frequency" && doc_id == 0 {
            words[2] = "beta";
        }
        let mut offset = 0;
        let tokens = words
            .iter()
            .enumerate()
            .map(|(position, word)| {
                let token = Token {
                    offset_from: offset,
                    offset_to: offset + word.len(),
                    position: if variant == "positions" && doc_id == 0 && position == 2 {
                        6
                    } else {
                        position
                    },
                    text: (*word).to_owned(),
                    position_length: 1,
                };
                offset = token.offset_to + 1;
                token
            })
            .collect();
        let mut document = TantivyDocument::default();
        document.add_text(
            id,
            if variant == "stored" && doc_id == 1 {
                "changed-id".to_owned()
            } else {
                doc_id.to_string()
            },
        );
        document.add_pre_tokenized_text(
            text,
            PreTokenizedString {
                text: words.join(" "),
                tokens,
            },
        );
        if string_fast {
            document.add_text(sort, doc_id.to_string());
        } else {
            document.add_u64(
                sort,
                if variant == "fast" && doc_id == 1 {
                    99
                } else {
                    doc_id as u64 * 10
                },
            );
        }
        writer.add_document(document)?;
    }
    writer.commit()?;
    drop(writer);
    drop(index);
    if variant == "fieldnorm" {
        // Rewrite only the actual synthetic fieldnorm component through the
        // public serializer. ManagedDirectory supplies a valid CRC/version footer.
        let filename = std::fs::read_dir(path)?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .find(|name| name.to_string_lossy().ends_with(".fieldnorm"))
            .ok_or("missing fixture fieldnorm component")?;
        std::fs::remove_file(path.join(&filename))?;
        let directory = ManagedDirectory::wrap(Box::new(MmapDirectory::open(path)?))?;
        let mut serializer =
            FieldNormsSerializer::from_write(directory.open_write(Path::new(&filename))?)?;
        serializer.serialize_field(text, &[4, 2, 3])?;
        serializer.close()?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("expected a new absent fixture directory")?;
    let root = Path::new(&root);
    std::fs::create_dir(root)?;
    for variant in [
        "baseline",
        "postings",
        "frequency",
        "positions",
        "fieldnorm",
        "stored",
        "fast",
        "unsupported_mixed_json",
        "unsupported_fast_string",
    ] {
        create(&root.join(variant), variant)?;
    }
    println!("fixtures={}", root.display());
    Ok(())
}
