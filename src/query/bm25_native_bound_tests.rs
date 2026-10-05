use super::*;

fn native(parameters: Bm25Parameters, average: Score, weight: Score) -> Bm25Weight {
    Bm25Weight::new_native_with_parameters(
        Explanation::new("test weight", weight),
        average,
        parameters,
    )
}

fn context(weight: &Bm25Weight, average: Score) -> Option<NativeSelectionContext> {
    weight.native_selection_context(average, BlockMaxSelection::NativeSaturationInput)
}

#[test]
fn finite_native_enclosure_covers_literal_inputs_and_scores() {
    let frequencies = [
        1,
        254,
        255,
        256,
        (1 << 24) - 1,
        1 << 24,
        (1 << 24) + 1,
        u32::MAX,
    ];
    let profiles = [
        (0.9, 0.4),
        (1.2, 0.0),
        (0.9, 1.0f32.next_down()),
        (f32::MIN_POSITIVE, 0.0),
    ];
    let mut comparisons = 0;
    for (k1, b) in profiles {
        let parameters = Bm25Parameters::new(k1, b).unwrap();
        for selection_average in [3.0, f32::from_bits(0x43a0a7af), 2.0e9, 1.0e30] {
            let old = Bm25Weight::for_native_block_bounds(selection_average);
            for query_average in [3.0, f32::from_bits(0x43a0a7af), 2.0e9, 1.0e30] {
                let query = native(parameters, query_average, 1.0);
                let envelope =
                    query.native_input_envelope(context(&query, selection_average).unwrap());
                // Tiny normalization may legitimately overflow a query inverse.
                let Some(envelope) = envelope else {
                    assert!(query.cache.iter().any(|value| !value.is_finite()));
                    continue;
                };
                for shift in [0usize, 73, 128] {
                    for frequency in frequencies {
                        let postings: Vec<_> = (0..128)
                            .map(|i| {
                                let norm = ((i + shift) % 256) as u8;
                                let tf = if i % 3 == 0 {
                                    frequency
                                } else {
                                    1 + i as u32 % 7
                                };
                                (norm, tf)
                            })
                            .collect();
                        let &(selected_norm, selected_tf) = postings
                            .iter()
                            .max_by(|&&(a, t), &&(b, u)| {
                                old.native_saturation_input(a, t)
                                    .total_cmp(&old.native_saturation_input(b, u))
                            })
                            .unwrap();
                        let selected_ceiling = NonZeroU32::new(if selected_tf >= 255 {
                            u32::MAX
                        } else {
                            selected_tf
                        })
                        .unwrap();
                        let upper = envelope
                            .input_upper(selected_norm, selected_ceiling)
                            .unwrap_or(Score::INFINITY);
                        for &(norm, tf) in &postings {
                            assert!(
                                query.native_saturation_input(norm, tf) <= upper,
                                "input k1={k1} b={b} old={selection_average} \
                                 query={query_average} norm={norm} tf={tf}"
                            );
                            for boost in
                                [f32::from_bits(1), f32::MIN_POSITIVE, 1.0, 100.0, f32::MAX]
                            {
                                let boosted = query.boost_by(boost);
                                let bound = boosted.score_from_native_input_upper(upper);
                                assert!(
                                    boosted.score(norm, tf) <= bound,
                                    "score k1={k1} b={b} norm={norm} tf={tf} boost={boost}"
                                );
                                comparisons += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(comparisons > 500_000);
}

#[test]
fn native_context_rejects_untrusted_profiles_statistics_and_actual_boosts() {
    let parameters = Bm25Parameters::new(0.9, 0.4).unwrap();
    let query = native(parameters, 321.0, 1.0);
    assert!(context(&query, 3.0).is_some()); // Query/selection averages may differ.
    assert!(query
        .native_selection_context(321.0, BlockMaxSelection::LegacyTfFactor)
        .is_none());
    let classic =
        Bm25Weight::new_with_parameters(Explanation::new("custom", 1.0), 321.0, parameters);
    assert!(context(&classic, 321.0).is_none());
    for average in [
        0.0,
        -0.0,
        -1.0,
        Score::INFINITY,
        Score::NEG_INFINITY,
        Score::NAN,
    ] {
        assert!(context(&query, average).is_none());
        assert!(context(&native(parameters, average, 1.0), 321.0).is_none());
    }
    for boost in [
        0.0,
        -0.0,
        -1.0,
        Score::INFINITY,
        Score::NEG_INFINITY,
        Score::NAN,
    ] {
        assert!(context(&query.boost_by(boost), 321.0).is_none());
    }
    assert!(context(&query.boost_by(f32::from_bits(1)), 321.0).is_some());
    assert!(context(&query.boost_by(Score::MAX), 321.0).is_some());
    assert!(context(&native(parameters, 321.0, 2.0).boost_by(Score::MAX), 321.0).is_none());
    for k1 in [0.0, -0.0] {
        assert!(context(
            &native(Bm25Parameters::new(k1, 0.4).unwrap(), 321.0, 1.0),
            321.0
        )
        .is_none());
    }
    let default = native(Bm25Parameters::DEFAULT, 321.0, 1.0);
    assert!(default
        .can_use_stored_block_max_with_selection(321.0, BlockMaxSelection::NativeSaturationInput));
    assert!(!default
        .can_use_stored_block_max_with_selection(3.0, BlockMaxSelection::NativeSaturationInput));
    assert!(context(&default, 321.0).is_none());
    assert!(context(&default, 3.0).is_none()); // No DEFAULT mismatch transform.
}

#[test]
fn finite_native_envelope_classifies_every_actual_cache_entry() {
    let parameters = Bm25Parameters::new(0.9, 0.4).unwrap();
    for norm in 0..256 {
        for invalid in [Score::NAN, Score::INFINITY, Score::NEG_INFINITY, -1.0, -0.0] {
            let mut query = native(parameters, 321.0, 1.0);
            Arc::make_mut(&mut query.cache)[norm] = invalid;
            assert!(
                query
                    .native_input_envelope(context(&query, 321.0).unwrap())
                    .is_none(),
                "norm={norm} invalid={invalid}"
            );
        }
    }
    for parameters in [
        Bm25Parameters::new(2.5, 1.0).unwrap(),
        Bm25Parameters::new(f32::from_bits(1), 0.4).unwrap(),
        Bm25Parameters::new(f32::from_bits(1), 1.0).unwrap(),
    ] {
        let query = native(parameters, 321.0, 1.0);
        assert!(query.cache.iter().any(|value| !value.is_finite()));
        assert!(query
            .native_input_envelope(context(&query, 321.0).unwrap())
            .is_none());
    }
    // A finite positive average can still produce zero old inverses because
    // the literal normalization overflows at large norm IDs.
    let query = native(parameters, 321.0, 1.0);
    let tiny_selection = f32::from_bits(1);
    assert!(context(&query, tiny_selection).is_some());
    assert!(query
        .native_input_envelope(context(&query, tiny_selection).unwrap())
        .is_none());
}

#[test]
fn finite_native_envelope_preserves_zero_and_subnormal_query_inverses() {
    let mut query = native(Bm25Parameters::new(0.9, 0.4).unwrap(), 321.0, 1.0);
    query.cache = Arc::new([0.0; 256]);
    let all_zero = query
        .native_input_envelope(context(&query, 321.0).unwrap())
        .unwrap();
    assert_eq!(all_zero.ratio_up, 0.0);
    assert_eq!(
        all_zero.input_upper(0, NonZeroU32::new(1).unwrap()),
        Some(0.0)
    );
    assert_eq!(query.score_from_native_input_upper(0.0), 0.0);
    Arc::make_mut(&mut query.cache)[255] = f32::from_bits(1);
    let tiny = query
        .native_input_envelope(context(&query, 321.0).unwrap())
        .unwrap();
    let upper = tiny.input_upper(255, NonZeroU32::new(1).unwrap()).unwrap();
    assert!(upper >= query.native_saturation_input(255, 1));
}

#[test]
fn native_envelope_uses_selected_frequency_ceiling_and_outward_rounding() {
    // The stored product rounds down. Its real product exceeds M; the output
    // must include an outward old endpoint rather than treating M as real.
    let envelope = NativeInputEnvelope {
        selection_inverse: Arc::new([f32::from_bits(0x3f800001); 256]),
        ratio_up: 1.0f64.next_up(),
    };
    assert_eq!((5.0 * envelope.selection_inverse[0]).to_bits(), 0x40a00001);
    let upper = envelope
        .input_upper(0, NonZeroU32::new(5).unwrap())
        .unwrap();
    assert_eq!(upper.to_bits(), 0x40a00003);
    assert!(f64::from(upper) >= 5.0 * f64::from(envelope.selection_inverse[0]));
    assert_eq!(u32::MAX as f32, 4294967296.0);
    let saturated = envelope
        .input_upper(0, NonZeroU32::new(u32::MAX).unwrap())
        .unwrap();
    assert!(saturated >= u32::MAX as f32 * envelope.selection_inverse[0]);
    for inverse in [0.0, Score::MAX, Score::INFINITY] {
        let unsupported = NativeInputEnvelope {
            selection_inverse: Arc::new([inverse; 256]),
            ratio_up: 1.0,
        };
        assert!(unsupported
            .input_upper(0, NonZeroU32::new(1).unwrap())
            .is_none());
    }
    let cast_up = NativeInputEnvelope {
        selection_inverse: Arc::new([1.0; 256]),
        ratio_up: 1.0 + 2.0f64.powi(-25),
    };
    assert!(cast_up.input_upper(0, NonZeroU32::new(1).unwrap()).unwrap() > 1.0);
}

#[test]
fn b1_rounded_norm_zero_tie_retains_global_fallback() {
    let old = Bm25Weight::for_native_block_bounds(3.0);
    assert_eq!(old.native_saturation_input(0, 1).to_bits(), 0x40555555);
    assert_eq!(old.native_saturation_input(1, 2).to_bits(), 0x40555555);
    let query = native(Bm25Parameters::new(2.5, 1.0).unwrap(), 3.0, 7.0);
    assert_eq!(query.score(0, 1), query.max_score());
    assert!(query
        .native_input_envelope(context(&query, 3.0).unwrap())
        .is_none());
}

#[test]
fn native_envelope_reconstructs_actual_selection_average_not_query_average() {
    let query = native(
        Bm25Parameters::new(0.9, 0.4).unwrap(),
        f32::from_bits(0x4f000000),
        1.0,
    );
    let old = Bm25Weight::for_native_block_bounds(3.0);
    assert!(old.native_saturation_input(0, 1) > old.native_saturation_input(255, 100));
    let actual = query
        .native_input_envelope(context(&query, 3.0).unwrap())
        .unwrap();
    let correct_upper = actual.input_upper(0, NonZeroU32::new(1).unwrap()).unwrap();
    assert!(query.score_from_native_input_upper(correct_upper) >= query.score(255, 100));
    // This plausible but wrong reconstruction understates the competing score.
    let wrong = query
        .native_input_envelope(context(&query, query.average_fieldnorm).unwrap())
        .unwrap();
    let wrong_upper = wrong.input_upper(0, NonZeroU32::new(1).unwrap()).unwrap();
    assert_eq!(
        query.score_from_native_input_upper(wrong_upper).to_bits(),
        0x3f501a34
    );
    assert_eq!(query.score(255, 100).to_bits(), 0x3f7dc5ed);
    assert!(query.score_from_native_input_upper(wrong_upper) < query.score(255, 100));
}
