//! Exercise real index readers, collectors, pruning and unscored position paths.
//! The reference scans scored postings without pruning, filters deletes, and
//! checks matches against literal tokens rather than another query optimizer.

use std::collections::BTreeSet;

use tantivy::collector::{Count, TopDocs};
use tantivy::merge_policy::NoMergePolicy;
use tantivy::query::{
    Bm25Parameters, BooleanQuery, BoostQuery, EnableScoring, Occur, PhrasePrefixQuery, PhraseQuery,
    Query, TermQuery,
};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, INDEXED, STORED,
};
use tantivy::{doc, DocAddress, Index, Score, Searcher, TantivyDocument, Term, TERMINATED};

const DOCS_PER_SEGMENT: usize = 320;

#[derive(Clone)]
enum Matches {
    Any(Vec<&'static str>),
    All(Vec<&'static str>),
    Phrase(Vec<&'static str>),
    PhrasePrefix(Vec<&'static str>),
}

impl Matches {
    fn matches(&self, tokens: &[String]) -> bool {
        match self {
            Self::Any(terms) => terms
                .iter()
                .any(|term| tokens.iter().any(|token| token == term)),
            Self::All(terms) => terms
                .iter()
                .all(|term| tokens.iter().any(|token| token == term)),
            Self::Phrase(terms) => tokens
                .windows(terms.len())
                .any(|window| window.iter().zip(terms).all(|(token, term)| token == term)),
            Self::PhrasePrefix(terms) => tokens.windows(terms.len()).any(|window| {
                window[..window.len() - 1]
                    .iter()
                    .zip(&terms[..terms.len() - 1])
                    .all(|(token, term)| token == term)
                    && window.last().unwrap().starts_with(terms.last().unwrap())
            }),
        }
    }
}

struct Case {
    name: String,
    query: Box<dyn Query>,
    matches: Matches,
}

fn term_query(field: Field, text: &str) -> Box<dyn Query> {
    Box::new(TermQuery::new(
        Term::from_field_text(field, text),
        IndexRecordOption::WithFreqsAndPositions,
    ))
}

fn boolean_query(
    field: Field,
    terms: &[&str],
    occur: Occur,
    boosts: Option<&[Score]>,
) -> Box<dyn Query> {
    Box::new(BooleanQuery::new(
        terms
            .iter()
            .enumerate()
            .map(|(i, term)| {
                let query = term_query(field, term);
                let query = if let Some(boosts) = boosts {
                    Box::new(BoostQuery::new(query, boosts[i])) as Box<dyn Query>
                } else {
                    query
                };
                (occur, query)
            })
            .collect(),
    ))
}

fn cases(field: Field) -> Vec<Case> {
    let mut cases = Vec::new();
    for text in ["common", "rare", "absent"] {
        cases.push(Case {
            name: format!("term {text}"),
            query: term_query(field, text),
            matches: Matches::Any(vec![text]),
        });
    }
    for terms in [
        vec!["common", "alpha"],
        vec!["common", "rare"],
        vec!["alpha", "beta"],
        vec!["alpha", "absent"],
        vec!["common", "alpha", "rare"],
        vec!["alpha", "beta", "gamma"],
        vec!["alpha", "alpha", "absent", "rare"],
    ] {
        for occur in [Occur::Should, Occur::Must] {
            cases.push(Case {
                name: format!("{occur:?} {terms:?}"),
                query: boolean_query(field, &terms, occur, None),
                matches: if occur == Occur::Should {
                    Matches::Any(terms.clone())
                } else {
                    Matches::All(terms.clone())
                },
            });
        }
    }
    for terms in [
        vec!["alpha", "beta"],
        vec!["alpha", "beta", "gamma"],
        vec!["alpha", "alpha"],
        vec!["alpha", "absent"],
    ] {
        cases.push(Case {
            name: format!("phrase {terms:?}"),
            query: Box::new(PhraseQuery::new(
                terms
                    .iter()
                    .map(|term| Term::from_field_text(field, term))
                    .collect(),
            )),
            matches: Matches::Phrase(terms),
        });
    }
    for terms in [
        vec!["alpha", "b"],
        vec!["alpha", "beta", "gam"],
        vec!["alpha", "beta", "absent"],
    ] {
        cases.push(Case {
            name: format!("phrase-prefix {terms:?}"),
            query: Box::new(PhrasePrefixQuery::new(
                terms
                    .iter()
                    .map(|term| Term::from_field_text(field, term))
                    .collect(),
            )),
            matches: Matches::PhrasePrefix(terms),
        });
    }
    // Outer boosts exercise query wrappers; per-clause boosts preserve the pure
    // TermScorer specialization and exercise its score-bound assumptions.
    let outer_cases = cases
        .iter()
        .map(|case| {
            (
                case.name.clone(),
                case.query.box_clone(),
                case.matches.clone(),
            )
        })
        .collect::<Vec<_>>();
    for (name, query, matches) in outer_cases {
        for boost in [0.0, 2.5, -1.0] {
            cases.push(Case {
                name: format!("boost {boost} ({name})"),
                query: Box::new(BoostQuery::new(query.box_clone(), boost)),
                matches: matches.clone(),
            });
        }
    }
    for terms in [vec!["common", "alpha"], vec!["common", "rare"]] {
        for occur in [Occur::Should, Occur::Must] {
            for boosts in [[0.0, 0.0], [2.5, 0.5], [-1.0, -0.5], [1.0, -2.0]] {
                cases.push(Case {
                    name: format!("clause boosts {boosts:?} {occur:?} {terms:?}"),
                    query: boolean_query(field, &terms, occur, Some(&boosts)),
                    matches: if occur == Occur::Should {
                        Matches::Any(terms.clone())
                    } else {
                        Matches::All(terms.clone())
                    },
                });
            }
        }
    }
    for boosts in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 2.5],
        [2.5, 0.5, 0.25],
        [-1.0, -0.5, -0.25],
        [1.0, -2.0, 0.5],
        [1.0e16, 1.0, 1.0e-16],
    ] {
        let terms = ["common", "alpha", "rare"];
        cases.push(Case {
            name: format!("three-term OR clause boosts {boosts:?}"),
            query: boolean_query(field, &terms, Occur::Should, Some(&boosts)),
            matches: Matches::Any(terms.to_vec()),
        });
    }
    cases
}

struct Fixture {
    index: Index,
    id: Field,
    text: Field,
    tokens: Vec<Vec<String>>,
}

fn fixture(segment_count: usize, fieldnorms: bool) -> tantivy::Result<Fixture> {
    let mut schema = Schema::builder();
    let id = schema.add_u64_field("id", INDEXED | STORED);
    let text = schema.add_text_field(
        "text",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions)
                .set_fieldnorms(fieldnorms),
        ),
    );
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    writer.set_merge_policy(Box::new(NoMergePolicy));
    let mut tokens = Vec::new();
    let patterns = [
        "",
        "",
        "common alpha beta gamma",
        "common alpha beta gamma",
        "common beta alpha gamma",
        "common alpha beta gamut",
        "common alpha alpha beta gamma",
        "common infrequent alpha beta gamma",
        "common alpha x beta gamma",
        "common alpha beta gamma common alpha beta gamma",
        "common",
        "alpha beta gamma",
        "common alpha beta delta",
        "common infrequent",
        "common alpha beta gamma",
        "common alpha beta gamma",
    ];
    for segment in 0..segment_count {
        for local_id in 0..DOCS_PER_SEGMENT {
            let external_id = tokens.len() as u64;
            let pattern = local_id % patterns.len();
            let mut words = patterns[pattern]
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if pattern == 3 {
                words = (0..160).flat_map(|_| words.iter().cloned()).collect(); // Positions cross
                                                                                // multiple 128-value
                                                                                // blocks.
            }
            if local_id == DOCS_PER_SEGMENT - 1 {
                words.push("rare".to_owned());
            }
            if pattern != 0 && pattern != 1 && pattern != 14 {
                let scale = [1, 10, 200][segment];
                words.extend(std::iter::repeat_n(
                    "filler".to_owned(),
                    (local_id % 7) * scale,
                ));
            }
            let mut document = doc!(id => external_id);
            if pattern != 1 {
                document.add_text(text, words.join(" "));
            }
            writer.add_document(document)?;
            tokens.push(words);
        }
        writer.commit()?;
    }
    Ok(Fixture {
        index,
        id,
        text,
        tokens,
    })
}

fn validate_case(
    fixture: &Fixture,
    searcher: &Searcher,
    deleted: &BTreeSet<u64>,
    case: &Case,
    context: &str,
) -> tantivy::Result<()> {
    let weight = case
        .query
        .weight(EnableScoring::enabled_from_searcher(searcher))?;
    let mut exhaustive: Vec<(Score, DocAddress)> = Vec::new();
    let mut observed_ids = BTreeSet::new();
    for (segment_ord, reader) in searcher.segment_readers().iter().enumerate() {
        let mut scorer = weight.scorer(reader, 1.0)?;
        while scorer.doc() != TERMINATED {
            if reader
                .alive_bitset()
                .is_none_or(|alive| alive.is_alive(scorer.doc()))
            {
                let address = DocAddress::new(segment_ord as u32, scorer.doc());
                let document: TantivyDocument = searcher.doc(address)?;
                let id = document.get_first(fixture.id).unwrap().as_u64().unwrap();
                assert!(
                    observed_ids.insert(id),
                    "duplicate document: {context} {}",
                    case.name
                );
                exhaustive.push((scorer.score(), address));
            }
            scorer.advance();
        }
    }
    let expected_ids = fixture
        .tokens
        .iter()
        .enumerate()
        .filter_map(|(id, tokens)| {
            let id = id as u64;
            (!deleted.contains(&id) && case.matches.matches(tokens)).then_some(id)
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        observed_ids, expected_ids,
        "matching docs: {context} {}",
        case.name
    );
    assert_eq!(
        case.query.count(searcher)?,
        exhaustive.len(),
        "query COUNT: {context} {}",
        case.name
    );
    assert_eq!(
        searcher.search(&*case.query, &Count)?,
        exhaustive.len(),
        "collector COUNT: {context} {}",
        case.name
    );
    exhaustive.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for k in [1, 3, 10, 100] {
        let optimized = searcher.search(&*case.query, &TopDocs::with_limit(k).order_by_score())?;
        let expected = &exhaustive[..k.min(exhaustive.len())];
        assert_eq!(
            optimized.len(),
            expected.len(),
            "top-{k} length: {context} {}",
            case.name
        );
        for ((score, address), (expected_score, expected_address)) in optimized.iter().zip(expected)
        {
            assert_eq!(
                address, expected_address,
                "top-{k} doc: {context} {} (score={score}, expected={expected_score})",
                case.name
            );
            assert_eq!(
                score.to_bits(),
                expected_score.to_bits(),
                "top-{k} raw score bits: {context} {}: {score} != {expected_score}",
                case.name
            );
        }
    }
    Ok(())
}

fn run_configuration(
    segment_count: usize,
    fieldnorms: bool,
    with_deletes: bool,
) -> tantivy::Result<()> {
    let fixture = fixture(segment_count, fieldnorms)?;
    let cases = cases(fixture.text);
    let mut deleted = BTreeSet::new();
    if with_deletes {
        let mut writer = fixture
            .index
            .writer_with_num_threads::<TantivyDocument>(1, 15_000_000)?;
        writer.set_merge_policy(Box::new(NoMergePolicy));
        for segment in 0..segment_count {
            for local_id in [0, 2, 3, 63, 127, 128, 255, 319] {
                let id = (segment * DOCS_PER_SEGMENT + local_id) as u64;
                writer.delete_term(Term::from_field_u64(fixture.id, id));
                deleted.insert(id);
            }
        }
        writer.commit()?;
    }
    let searcher = fixture.index.reader()?.searcher();
    assert_eq!(searcher.segment_readers().len(), segment_count);
    let context =
        format!("segments={segment_count} fieldnorms={fieldnorms} deletes={with_deletes}");
    for case in &cases {
        validate_case(&fixture, &searcher, &deleted, case, &context)?;
    }
    Ok(())
}

macro_rules! configuration_test {
    ($name:ident, $segments:expr, $fieldnorms:expr, $deletes:expr) => {
        #[test]
        fn $name() -> tantivy::Result<()> {
            run_configuration($segments, $fieldnorms, $deletes)
        }
    };
}

configuration_test!(one_segment, 1, true, false);
configuration_test!(one_segment_deleted, 1, true, true);
configuration_test!(three_segments, 3, true, false);
configuration_test!(three_segments_deleted, 3, true, true);
configuration_test!(one_segment_without_fieldnorms, 1, false, false);
configuration_test!(one_segment_deleted_without_fieldnorms, 1, false, true);
configuration_test!(three_segments_without_fieldnorms, 3, false, false);
configuration_test!(three_segments_deleted_without_fieldnorms, 3, false, true);

#[test]
fn configured_three_term_or_keeps_exact_bits_with_deletes() -> tantivy::Result<()> {
    let fixture = fixture(3, true)?;
    let mut writer = fixture
        .index
        .writer_with_num_threads::<TantivyDocument>(1, 15_000_000)?;
    writer.set_merge_policy(Box::new(NoMergePolicy));
    let mut deleted = BTreeSet::new();
    for segment in 0..3 {
        for local in [0, 127, 128, 255, 319] {
            let id = (segment * DOCS_PER_SEGMENT + local) as u64;
            writer.delete_term(Term::from_field_u64(fixture.id, id));
            deleted.insert(id);
        }
    }
    writer.commit()?;
    let reader = fixture.index.reader()?;
    for (k1, b) in [(0.9, 0.4), (2.0, 1.0)] {
        let searcher = reader
            .searcher()
            .with_bm25_parameters(Bm25Parameters::new(k1, b)?);
        for terms in [
            vec!["common", "alpha", "rare"],
            vec!["alpha", "beta", "gamma", "absent"],
            vec!["alpha", "alpha", "rare"],
        ] {
            let case = Case {
                name: format!("configured OR {terms:?}"),
                query: boolean_query(fixture.text, &terms, Occur::Should, None),
                matches: Matches::Any(terms),
            };
            validate_case(
                &fixture,
                &searcher,
                &deleted,
                &case,
                &format!("k1={k1} b={b}"),
            )?;
        }
    }
    Ok(())
}
