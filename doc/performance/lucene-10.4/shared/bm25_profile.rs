use std::path::PathBuf;

use tantivy::query::{Bm25Parameters, Bm25StatisticsProvider};
use tantivy::schema::Field;
use tantivy::{Searcher, TantivyError};

pub struct ProfileArgs {
    pub index: PathBuf,
    parameters: Option<Bm25Parameters>,
}

fn invalid(message: &str) -> TantivyError {
    TantivyError::InvalidArgument(message.to_owned())
}

fn scalar(bits: &str) -> tantivy::Result<f32> {
    if bits.len() != 8
        || !bits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid(
            "Expected eight lowercase hex digits for BM25 parameter bits",
        ));
    }
    Ok(f32::from_bits(
        u32::from_str_radix(bits, 16).map_err(|_| invalid("Invalid BM25 bits"))?,
    ))
}

impl ProfileArgs {
    // Validate the complete boundary before opening an index. No extra argument
    // may silently turn a requested configured run into DEFAULT.
    pub fn parse(args: &[String]) -> tantivy::Result<Self> {
        let parameters = match args.len() {
            1 => None,
            3 => Some(Bm25Parameters::new(scalar(&args[1])?, scalar(&args[2])?)?),
            _ => return Err(invalid("Usage: INDEX [K1_BITS B_BITS]")),
        };
        Ok(Self {
            index: PathBuf::from(&args[0]),
            parameters,
        })
    }

    pub fn configure(&self, searcher: Searcher) -> Searcher {
        match self.parameters {
            Some(parameters) => searcher.with_bm25_parameters(parameters),
            None => searcher,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.parameters.is_some()
    }

    pub fn receipt(&self, searcher: &Searcher, text: Field) -> tantivy::Result<serde_json::Value> {
        let statistics = searcher.field_statistics(text)?;
        let parameters = statistics.parameters();
        Ok(serde_json::json!({
            "protocol": "bm25-profile-v1", "engine": "tantivy", "field": "text",
            "k1_bits": format!("{:08x}", parameters.k1().to_bits()),
            "b_bits": format!("{:08x}", parameters.b().to_bits()),
            "scale_bits": "3f800000", "collection_statistics": "physical",
            "query_cache": "disabled", "doc_count": statistics.doc_count(),
            "sum_total_term_freq": statistics.sum_total_term_freq(),
        }))
    }
}
