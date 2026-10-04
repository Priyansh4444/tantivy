use super::*;

const JAVA: &str = include_str!(
    "../../doc/performance/lucene-10.4/parity/bm25-parameters-reference/reference.csv"
);

fn bits(hex: &str) -> u32 {
    u32::from_str_radix(hex, 16).unwrap()
}
fn float(hex: &str) -> f32 {
    f32::from_bits(bits(hex))
}
fn reference_bits(value: f32) -> u32 {
    // Java Float.floatToIntBits canonicalizes NaN. Finite values, infinities and
    // signed zeros retain their exact bits; NaN sign/payload are not a score contract.
    if value.is_nan() {
        0x7fc00000
    } else {
        value.to_bits()
    }
}

#[test]
fn parameter_validation_matches_java_and_preserves_getter_bits() {
    for line in JAVA.lines() {
        let row: Vec<_> = line.split(',').collect();
        let value = float(row[1]);
        let actual = match row[0] {
            "k1" => Bm25Parameters::new(value, 0.75),
            "b" => Bm25Parameters::new(1.2, value),
            _ => continue,
        };
        assert_eq!(actual.is_ok(), row[2] == "true", "{line}");
        if let Ok(parameters) = actual {
            let getter = if row[0] == "k1" {
                parameters.k1()
            } else {
                parameters.b()
            };
            assert_eq!(getter.to_bits(), value.to_bits());
        }
    }
}

#[test]
fn parameter_scalar_matrix_matches_java_all_norms_and_extremes() {
    let frequencies = [0.0, 0.5, 1.0, 7.0, 254.0, 255.0, 256.0, u32::MAX as f32];
    for line in JAVA.lines().filter(|line| line.starts_with("matrix,")) {
        let row: Vec<_> = line.split(',').collect();
        let parameters = Bm25Parameters::new(float(row[1]), float(row[2])).unwrap();
        let base = Bm25Weight::new_native_with_parameters(
            native_idf_explanation(53, 347),
            float(row[3]),
            parameters,
        );
        let mut hash = 0xcbf29ce484222325u64;
        for boost in [0.0, -0.0, 1.0, 3.25, -2.0, f32::MAX] {
            let weight = base.boost_by(boost);
            for norm in 0..=255 {
                for frequency in frequencies {
                    let score = weight.score_with_frequency(norm, frequency);
                    hash ^= u64::from(reference_bits(score));
                    hash = hash.wrapping_mul(0x100000001b3);
                    if frequency > 0.0 && score.is_finite() {
                        assert!(
                            weight.max_score() >= score,
                            "{line} norm={norm} f={frequency} boost={boost}"
                        );
                    }
                }
            }
        }
        assert_eq!(hash, u64::from_str_radix(row[4], 16).unwrap(), "{line}");
        assert_eq!(
            base.supports_frequency_ceiling(),
            parameters.is_default_profile()
        );
        for selection in [
            BlockMaxSelection::LegacyTfFactor,
            BlockMaxSelection::NativeSaturationInput,
        ] {
            assert_eq!(
                base.can_use_stored_block_max_with_selection(float(row[3]), selection),
                parameters.is_default_profile()
                    && selection == BlockMaxSelection::NativeSaturationInput
            );
        }
    }
}

#[test]
fn parameter_exceptional_cache_matches_java_without_finite_bound() {
    for line in JAVA.lines().filter(|line| line.starts_with("exceptional,")) {
        let row: Vec<_> = line.split(',').collect();
        let parameters = Bm25Parameters::new(float(row[1]), 1.0).unwrap();
        let weight = Bm25Weight::new_native_with_parameters(
            native_idf_explanation(53, 347),
            float(row[2]),
            parameters,
        );
        let norm: u8 = row[3].parse().unwrap();
        assert_eq!(
            reference_bits(weight.score(norm, 1)),
            bits(row[4]),
            "{line}"
        );
        if weight.cache.iter().any(|value| value.is_nan()) {
            assert!(!weight.has_safe_score_bounds());
            assert_eq!(weight.max_score(), f32::INFINITY);
        }
        assert!(!weight.supports_frequency_ceiling());
        assert!(!weight.can_use_stored_block_max_with_selection(
            float(row[2]),
            BlockMaxSelection::NativeSaturationInput
        ));
    }
}

#[test]
fn parameter_profile_is_bit_exact_and_classic_defaults_remain_eligible() {
    assert!(Bm25Parameters::DEFAULT.is_default_profile());
    for parameters in [
        Bm25Parameters::new(1.2f32.next_up(), 0.75).unwrap(),
        Bm25Parameters::new(1.2, 0.75f32.next_down()).unwrap(),
        Bm25Parameters::new(-0.0, 0.75).unwrap(),
    ] {
        assert!(!parameters.is_default_profile());
        let classic =
            Bm25Weight::new_with_parameters(Explanation::new("idf", 1.0), 1.0, parameters);
        assert!(!classic.can_use_stored_block_max(1.0));
    }
    let default = Bm25Weight::new(Explanation::new("idf", 1.0), 1.0);
    assert!(default.can_use_stored_block_max(1.0));
}

#[test]
fn parameter_serialized_bounds_and_optimized_queries_match_exhaustive() -> crate::Result<()> {
    use crate::collector::TopDocs;
    use crate::query::{BooleanQuery, BoostQuery, EnableScoring, Occur, Query, Scorer, TermQuery};
    use crate::schema::{Schema, TEXT};
    use crate::{DocAddress, DocSet, Index, TERMINATED};

    let mut schema = Schema::builder();
    let field = schema.add_text_field("text", TEXT);
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    for doc_id in 0..401 {
        let frequency = if doc_id == 17 || doc_id == 400 {
            300
        } else {
            doc_id % 7 + 1
        };
        writer.add_document(
            doc!(field => format!("{}beta {}", "alpha ".repeat(frequency),
            if doc_id % 2 == 0 { "gamma" } else { "" })),
        )?;
    }
    writer.commit()?;
    let reader = index.reader()?;
    let segment = reader.searcher().segment_reader(0).clone();
    let inverted = segment.inverted_index(field)?;
    assert_eq!(
        inverted.stored_block_max_selection(),
        BlockMaxSelection::NativeSaturationInput
    );
    let selection_average = inverted.stored_selection_average_fieldnorm();
    let alpha = Term::from_field_text(field, "alpha");
    let term = TermQuery::new(alpha.clone(), crate::schema::IndexRecordOption::WithFreqs);

    for line in JAVA.lines().filter(|line| line.starts_with("matrix,")) {
        let row: Vec<_> = line.split(',').collect();
        // Each profile once; the real index determines its own physical average.
        if row[3] != "3f800000" {
            continue;
        }
        let parameters = Bm25Parameters::new(float(row[1]), float(row[2])).unwrap();
        let searcher = reader.searcher().with_bm25_parameters(parameters);
        let bm25 = Bm25Weight::for_terms(&searcher, std::slice::from_ref(&alpha))?;
        assert_eq!(
            bm25.average_fieldnorm.to_bits(),
            selection_average.to_bits()
        );
        assert_eq!(
            bm25.can_use_stored_block_max_with_selection(
                selection_average,
                inverted.stored_block_max_selection()
            ),
            parameters.is_default_profile()
        );
        let term_weight =
            term.specialized_weight(EnableScoring::enabled_from_searcher(&searcher))?;
        let mut scorer = term_weight.term_scorer_for_test(&segment, 1.0)?.unwrap();
        while scorer.doc() != TERMINATED {
            let bound = scorer.block_max_score();
            assert!(bound >= scorer.score(), "{line} doc={}", scorer.doc());
            if !parameters.is_default_profile() && scorer.doc() < 384 {
                assert_eq!(
                    bound.to_bits(),
                    bm25.max_score().to_bits(),
                    "complete block must distrust DEFAULT pairs"
                );
            }
            scorer.advance();
        }
        for boost in [0.0, -0.0, 1.0, 3.25, -2.0] {
            let union = BooleanQuery::new(
                ["alpha", "beta", "gamma"]
                    .map(|value| {
                        (
                            Occur::Should,
                            Box::new(TermQuery::new(
                                Term::from_field_text(field, value),
                                crate::schema::IndexRecordOption::WithFreqs,
                            )) as Box<dyn Query>,
                        )
                    })
                    .into_iter()
                    .collect(),
            );
            for query in [Box::new(term.clone()) as Box<dyn Query>, Box::new(union)] {
                let query = BoostQuery::new(query, boost);
                let weight = query.weight(EnableScoring::enabled_from_searcher(&searcher))?;
                let mut exhaustive = Vec::new();
                weight.for_each(&segment, &mut |doc, score| {
                    exhaustive.push((score, DocAddress::new(0, doc)))
                })?;
                exhaustive.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                let pivot = exhaustive[0].0;
                for threshold in [pivot.next_down(), pivot, pivot.next_up()] {
                    let mut pruned = Vec::new();
                    weight.for_each_pruning(threshold, &segment, &mut |doc, score| {
                        pruned.push((score, DocAddress::new(0, doc)));
                        threshold
                    })?;
                    pruned.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                    let expected: Vec<_> = exhaustive
                        .iter()
                        .copied()
                        .filter(|(score, _)| *score > threshold)
                        .collect();
                    assert_eq!(
                        pruned, expected,
                        "{line} boost={boost} threshold={threshold:?}"
                    );
                }
                for limit in [1, 7, 40] {
                    let actual =
                        searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?;
                    assert_eq!(
                        actual,
                        exhaustive[..limit],
                        "{line} boost={boost} limit={limit}"
                    );
                }
            }
        }
    }

    let norms = segment.get_fieldnorms_reader(field)?;
    let mut postings = inverted
        .read_postings(&alpha, crate::schema::IndexRecordOption::WithFreqs)?
        .unwrap();
    let safe = Bm25Weight::for_native_block_bounds(selection_average);
    assert!(postings
        .block_cursor
        .block_max_score_with_stored_max(&norms, &safe, true)
        .is_finite());
    let unsafe_cache = Bm25Weight::new_native_with_parameters(
        Explanation::new("idf", 1.0),
        f32::from_bits(1),
        Bm25Parameters::new(0.0, 1.0).unwrap(),
    );
    let nonfinite_weight = safe.boost_by(f32::INFINITY);
    for weight in [&unsafe_cache, &nonfinite_weight] {
        // The exceptional guard precedes a previously populated internal cache.
        assert_eq!(
            postings
                .block_cursor
                .block_max_score_with_stored_max(&norms, weight, true),
            f32::INFINITY
        );
        assert_eq!(
            postings.block_cursor.block_max_score(&norms, weight),
            f32::INFINITY
        );
    }
    postings.block_cursor.seek_block(400);
    postings.block_cursor.load_block();
    for weight in [&unsafe_cache, &nonfinite_weight] {
        assert_eq!(
            postings.block_cursor.block_max_score(&norms, weight),
            f32::INFINITY,
            "loaded tail must not discard exceptional scores with f32::max"
        );
    }
    Ok(())
}
