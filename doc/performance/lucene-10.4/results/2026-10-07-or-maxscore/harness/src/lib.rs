use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::Path;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Kind {
    Term,
    And,
    Or,
    Phrase,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueryCase {
    pub id: usize,
    pub kind: Kind,
    pub terms: Vec<String>,
    pub tags: Vec<String>,
}

pub fn load_queries(path: &Path) -> Result<Vec<QueryCase>> {
    let mut cases = Vec::new();
    for line in std::io::BufReader::new(std::fs::File::open(path)?).lines() {
        let case: QueryCase = serde_json::from_str(&line?)?;
        if case.id != cases.len()
            || case.terms.is_empty()
            || (matches!(case.kind, Kind::Term) && case.terms.len() != 1)
            || (!matches!(case.kind, Kind::Term) && case.terms.len() < 2)
            || case.terms.iter().any(|t| {
                t.is_empty()
                    || t.len() > 255
                    || !t
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
        {
            return Err("invalid query manifest".into());
        }
        cases.push(case);
    }
    if cases.is_empty() {
        return Err("empty query manifest".into());
    }
    Ok(cases)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum Request {
    Dump {
        id: usize,
    },
    Run {
        id: usize,
        mode: Mode,
        iterations: usize,
    },
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Count,
    Top10,
}

pub fn emit(value: &serde_json::Value) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, value)?;
    writeln!(out)?;
    out.flush()?;
    Ok(())
}

pub fn validate_request(request: &Request, queries: usize) -> Result<()> {
    let id = match request {
        Request::Dump { id } | Request::Run { id, .. } => *id,
    };
    if id >= queries {
        return Err("unknown query id".into());
    }
    if let Request::Run { iterations, .. } = request {
        if !(1..=256).contains(iterations) {
            return Err("invalid iteration budget".into());
        }
    }
    Ok(())
}
