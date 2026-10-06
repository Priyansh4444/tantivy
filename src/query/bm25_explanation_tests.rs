use serde_json::{json, Value};

use super::*;
use crate::query::{AllWeight, EnableScoring, PhraseQuery, Query, TermQuery, Weight};
use crate::schema::{IndexRecordOption, Schema, TEXT};

const BASELINE: &str = include_str!("bm25_explanation_baseline.json");

struct Statistics {
    docs: u64,
    tokens: u64,
    freqs: Vec<u64>,
    native: bool,
    parameters: Bm25Parameters,
}
impl Bm25StatisticsProvider for Statistics {
    fn total_num_tokens(&self, _: Field) -> crate::Result<u64> {
        Ok(self.tokens)
    }
    fn total_num_docs(&self) -> crate::Result<u64> {
        Ok(self.docs)
    }
    fn doc_freq(&self, term: &Term) -> crate::Result<u64> {
        Ok(self.freqs[term.serialized_value_bytes()[0] as usize - b'a' as usize])
    }
    fn field_statistics(&self, _: Field) -> crate::Result<Bm25FieldStatistics> {
        Ok(if self.native {
            Bm25FieldStatistics::native(self.docs, self.tokens)
        } else {
            Bm25FieldStatistics::new(self.docs, self.tokens)
        }
        .with_parameters(self.parameters))
    }
}
fn terms(words: &[&str]) -> Vec<Term> {
    words
        .iter()
        .map(|word| Term::from_field_text(Field::from_field_id(0), word))
        .collect()
}
fn tree(explanation: Explanation) -> Value {
    json!({"bits": explanation.bit_snapshot(),
           "finite_json": explanation.is_finite_tree().then(|| explanation.to_pretty_json())})
}
fn capture(name: String, weight: Bm25Weight, norm: u8, freq: Score) -> Value {
    json!({
        "name": name,
        "explanation": tree(weight.explain_with_frequency(norm, freq)),
        "weight": format!("{:08x}", weight.weight.to_bits()),
        "average": format!("{:08x}", weight.average_fieldnorm.to_bits()),
        "max_score": format!("{:08x}", weight.max_score().to_bits()),
        "cache": weight.cache.iter().map(|value| format!("{:08x}", value.to_bits())).collect::<Vec<_>>(),
        "safe": weight.has_safe_score_bounds(),
        "ceiling": weight.supports_frequency_ceiling(),
        "legacy_selection": weight.can_use_stored_block_max_with_selection(weight.average_fieldnorm, BlockMaxSelection::LegacyTfFactor),
        "native_selection": weight.can_use_stored_block_max_with_selection(weight.average_fieldnorm, BlockMaxSelection::NativeSaturationInput),
        "scores": ([0.0,0.5,1.0,7.0,u32::MAX as Score].iter().flat_map(|frequency| [0,91,255].map(|norm| format!("{:08x}",weight.score_with_frequency(norm,*frequency).to_bits()))).collect::<Vec<_>>()),
    })
}
fn fixtures() -> crate::Result<Value> {
    let mut cases = Vec::new();
    for native in [false, true] {
        for parameters in [
            Bm25Parameters::DEFAULT,
            Bm25Parameters::new(0.9, 0.4).unwrap(),
        ] {
            let stats = Statistics {
                docs: 347,
                tokens: 1403,
                freqs: vec![1, 53, 346],
                native,
                parameters,
            };
            for (label, words, norm, freq, boost) in [
                ("single", vec!["b"], 91, 7.0, 1.0),
                ("single-boost", vec!["a"], 255, 1.0, 3.25),
                ("phrase-ordered", vec!["a", "c", "b", "a"], 91, 0.5, 1.0),
                ("phrase-zero", vec!["b", "a", "b"], 0, 7.0, 0.0),
                ("phrase-negative", vec!["c", "b"], 255, 1.0, -2.0),
                ("signed-zero", vec!["b"], 91, 0.5, -0.0),
            ] {
                let name = format!("{native}-{:08x}-{label}", parameters.k1().to_bits());
                cases.push(capture(
                    name,
                    Bm25Weight::for_terms(&stats, &terms(&words))?.boost_by(boost),
                    norm,
                    freq,
                ));
            }
        }
    }
    let witness = Statistics {
        docs: 347,
        tokens: 1403,
        freqs: vec![1, 346],
        native: true,
        parameters: Bm25Parameters::DEFAULT,
    };
    cases.push(capture(
        "native-f64-sum-witness".into(),
        Bm25Weight::for_terms(&witness, &terms(&["a", "b", "a"]))?,
        91,
        0.5,
    ));
    for (native, docs, tokens, freqs, label) in [
        (
            false,
            16_777_229,
            16_777_231,
            vec![16_777_217, 3],
            "classic-large",
        ),
        (
            true,
            16_777_229,
            16_777_231,
            vec![16_777_217, 3],
            "native-large",
        ),
        (
            false,
            u64::MAX,
            u64::MAX,
            vec![u64::MAX - 1, 1],
            "classic-u64",
        ),
        (
            true,
            u64::MAX,
            u64::MAX,
            vec![u64::MAX - 1, 1],
            "native-u64",
        ),
        (false, 0, 0, vec![0, 0], "classic-zero"),
        (true, 0, 0, vec![0, 0], "native-zero"),
    ] {
        let stats = Statistics {
            native,
            docs,
            tokens,
            freqs,
            parameters: Bm25Parameters::DEFAULT,
        };
        cases.push(capture(
            label.into(),
            Bm25Weight::for_terms(&stats, &terms(&["a", "b", "a"]))?,
            91,
            0.5,
        ));
    }
    cases.push(capture(
        "public-one".into(),
        Bm25Weight::for_one_term(53, 347, 4.0),
        91,
        7.0,
    ));
    cases.push(capture(
        "public-no-explain".into(),
        Bm25Weight::for_one_term_without_explain(53, 347, 4.0),
        91,
        7.0,
    ));
    cases.push(capture(
        "internal-no-explain".into(),
        Bm25Weight::new_without_explain(-0.0, 4.0),
        91,
        7.0,
    ));
    cases.push(capture(
        "native-no-explain".into(),
        Bm25Weight::for_native_block_bounds(4.0),
        91,
        7.0,
    ));
    for native in [false, true] {
        let mut explicit = Explanation::new_with_string("owned idf".into(), -0.0);
        explicit.add_context("root context".into());
        let mut child = Explanation::new_with_string("owned child".into(), f32::INFINITY);
        child.add_const("nan child", f32::from_bits(0x7fc01234));
        child.add_context("nested context".into());
        explicit.add_detail(child);
        let weight = if native {
            Bm25Weight::new_native(explicit, 4.0)
        } else {
            Bm25Weight::new(explicit, 4.0)
        };
        cases.push(capture(
            format!("explicit-{native}"),
            weight.boost_by(3.25).boost_by(-2.0),
            91,
            0.5,
        ));
    }
    let mut schema = Schema::builder();
    let field = schema.add_text_field("text", TEXT);
    let index = crate::Index::create_in_ram(schema.build());
    let mut writer = index.writer_for_tests()?;
    writer.add_document(doc!(field=>"a b a b"))?;
    writer.add_document(doc!(field=>"a c"))?;
    writer.commit()?;
    let searcher = index.reader()?.searcher();
    let segment = searcher.segment_reader(0);
    for enabled in [false, true] {
        let scoring = if enabled {
            EnableScoring::enabled_from_searcher(&searcher)
        } else {
            EnableScoring::disabled_from_searcher(&searcher)
        };
        let term = TermQuery::new(
            Term::from_field_text(field, "a"),
            IndexRecordOption::WithFreqs,
        );
        cases.push(json!({"name":format!("term-scoring-{enabled}"),"explanation":tree(term.weight(scoring)?.explain(segment,0)?)}));
        let phrase = PhraseQuery::new(vec![
            Term::from_field_text(field, "a"),
            Term::from_field_text(field, "b"),
        ]);
        cases.push(json!({"name":format!("phrase-scoring-{enabled}"),"explanation":tree(phrase.weight(scoring)?.explain(segment,0)?)}));
    }
    cases.push(json!({"name":"all-no-score","explanation":tree(AllWeight.explain(segment,0)?)}));
    Ok(Value::Array(cases))
}

#[test]
fn frozen_baseline_whole_trees_and_scalars() -> crate::Result<()> {
    let actual = fixtures()?;
    let expected: Value = serde_json::from_str(BASELINE).unwrap();
    assert_eq!(actual, expected);
    Ok(())
}
#[test]
fn explanation_layout_receipt() {
    eprintln!(
        "weight={} weight_align={} explanation={} term_scorer={}",
        std::mem::size_of::<Bm25Weight>(),
        std::mem::align_of::<Bm25Weight>(),
        std::mem::size_of::<Explanation>(),
        std::mem::size_of::<crate::query::term_query::TermScorer>()
    );
}
#[test]
fn native_phrase_sum_rounding_witness() {
    let values = [native_idf(1, 347), native_idf(346, 347), native_idf(1, 347)];
    let native_sum = values
        .iter()
        .fold(0.0f64, |sum, &value| sum + f64::from(value)) as Score;
    let classic_sum = values.iter().fold(0.0f32, |sum, &value| sum + value);
    assert_eq!(native_sum.to_bits(), 1093557597);
    assert_eq!(classic_sum.to_bits(), 1093557598);
    let stats = Statistics {
        docs: 347,
        tokens: 1403,
        freqs: vec![1, 346],
        native: true,
        parameters: Bm25Parameters::DEFAULT,
    };
    assert_eq!(
        Bm25Weight::for_terms(&stats, &terms(&["a", "b", "a"]))
            .unwrap()
            .weight
            .to_bits(),
        1093557597
    );
    eprintln!(
        "frozen native sum witness docs347 freqs1/346 [a,b,a]: f64={:08x} f32={:08x}",
        native_sum.to_bits(),
        classic_sum.to_bits()
    );
}

use std::cell::RefCell;
struct Spy {
    calls: RefCell<Vec<String>>,
    fail: Option<&'static str>,
}
impl Spy {
    fn new(fail: Option<&'static str>) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            fail,
        }
    }
    fn record(&self, call: String, value: u64) -> crate::Result<u64> {
        self.calls.borrow_mut().push(call.clone());
        if self.fail == Some(call.as_str()) {
            Err(crate::TantivyError::InvalidArgument(call))
        } else {
            Ok(value)
        }
    }
    fn freq(&self, term: &Term) -> crate::Result<u64> {
        self.record(
            format!(
                "freq:{}",
                std::str::from_utf8(term.serialized_value_bytes()).unwrap()
            ),
            53,
        )
    }
}
impl Bm25StatisticsProvider for Spy {
    fn total_num_tokens(&self, _: Field) -> crate::Result<u64> {
        self.record("tokens".into(), 1403)
    }
    fn total_num_docs(&self) -> crate::Result<u64> {
        self.record("docs".into(), 347)
    }
    fn doc_freq(&self, term: &Term) -> crate::Result<u64> {
        self.freq(term)
    }
}
struct NativeSpy(Spy);
impl Bm25StatisticsProvider for NativeSpy {
    fn total_num_tokens(&self, _: Field) -> crate::Result<u64> {
        self.0.record("tokens".into(), 1403)
    }
    fn total_num_docs(&self) -> crate::Result<u64> {
        self.0.record("docs".into(), 347)
    }
    fn doc_freq(&self, term: &Term) -> crate::Result<u64> {
        self.0.freq(term)
    }
    fn field_statistics(&self, _: Field) -> crate::Result<Bm25FieldStatistics> {
        self.0.record("field".into(), 0)?;
        Ok(Bm25FieldStatistics::native(347, 1403))
    }
}
#[test]
fn provider_order_duplicates_failures_and_cold_operations() -> crate::Result<()> {
    let query = terms(&["a", "b", "a"]);
    let classic = Spy::new(None);
    let weight = Bm25Weight::for_terms(&classic, &query)?;
    let expected = ["tokens", "docs", "freq:a", "freq:b", "freq:a"];
    assert_eq!(classic.calls.borrow().as_slice(), expected);
    let clone = weight.clone();
    let boosted = clone.boost_by(3.25).boost_by(1.0);
    drop(weight);
    drop(clone);
    drop(boosted.explain(91, 7));
    assert_eq!(classic.calls.borrow().as_slice(), expected);
    let native = NativeSpy(Spy::new(None));
    let weight = Bm25Weight::for_terms(&native, &query)?;
    let expected = ["field", "freq:a", "freq:b", "freq:a"];
    assert_eq!(native.0.calls.borrow().as_slice(), expected);
    drop(
        weight
            .clone()
            .boost_by(3.25)
            .boost_by(1.0)
            .explain_with_frequency(91, 0.5),
    );
    assert_eq!(native.0.calls.borrow().as_slice(), expected);
    for (failure, expected) in [
        ("tokens", vec!["tokens"]),
        ("docs", vec!["tokens", "docs"]),
        ("freq:b", vec!["tokens", "docs", "freq:a", "freq:b"]),
    ] {
        let spy = Spy::new(Some(failure));
        assert!(Bm25Weight::for_terms(&spy, &query).is_err());
        assert_eq!(spy.calls.borrow().as_slice(), expected);
    }
    for (failure, expected) in [
        ("field", vec!["field"]),
        ("freq:b", vec!["field", "freq:a", "freq:b"]),
    ] {
        let spy = NativeSpy(Spy::new(Some(failure)));
        assert!(Bm25Weight::for_terms(&spy, &query).is_err());
        assert_eq!(spy.0.calls.borrow().as_slice(), expected);
    }
    let single = NativeSpy(Spy::new(None));
    drop(Bm25Weight::for_terms(&single, &query[..1])?);
    assert_eq!(single.0.calls.borrow().as_slice(), ["field", "freq:a"]);
    let assertions = Spy::new(None);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Bm25Weight::for_terms(
            &assertions,
            &[]
        )))
        .is_err()
    );
    let mixed = [
        query[0].clone(),
        Term::from_field_text(Field::from_field_id(1), "a"),
    ];
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Bm25Weight::for_terms(
            &assertions,
            &mixed
        )))
        .is_err()
    );
    assert!(assertions.calls.borrow().is_empty());
    Ok(())
}

#[test]
fn metadata_sharing_drop_and_independent_owned_outputs() -> crate::Result<()> {
    let stats = Statistics {
        docs: 347,
        tokens: 1403,
        freqs: vec![1, 346],
        native: true,
        parameters: Bm25Parameters::DEFAULT,
    };
    let base = Bm25Weight::for_terms(&stats, &terms(&["a", "b", "a"]))?;
    let clone = base.boost_by(1.0);
    assert!(Arc::ptr_eq(&base.cache, &clone.cache));
    let IdfExplanation::NativeSum { terms: records, .. } = &base.idf_explain else {
        panic!("native phrase metadata")
    };
    let IdfExplanation::NativeSum { terms: copied, .. } = &clone.idf_explain else {
        panic!("cloned native phrase metadata")
    };
    assert!(Arc::ptr_eq(records, copied));
    assert_eq!(records.len(), 3);
    let boosted = base.boost_by(3.25);
    let IdfExplanation::NativeSum {
        terms: boosted_records,
        ..
    } = &boosted.idf_explain
    else {
        panic!("boosted native phrase metadata")
    };
    assert!(Arc::ptr_eq(records, boosted_records));
    let weak = Arc::downgrade(records);
    let mut first = base.explain_with_frequency(91, 0.5);
    let frozen: Value = serde_json::from_str(BASELINE).unwrap();
    let expected = frozen
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "native-f64-sum-witness")
        .unwrap()["explanation"]["bits"]
        .clone();
    drop(base);
    drop(boosted);
    assert!(weak.upgrade().is_some());
    let second = clone.explain_with_frequency(91, 0.5);
    assert_eq!(second.bit_snapshot(), expected);
    assert_eq!(
        format!("{:08x}", clone.score_with_frequency(91, 0.5).to_bits()),
        expected["value_bits"]
    );
    drop(clone);
    assert!(weak.upgrade().is_none());
    first.add_context("independent output mutation".into());
    first.add_const("extra", 7.0);
    assert_ne!(first.bit_snapshot(), expected);
    assert_eq!(second.bit_snapshot(), expected);

    let mut supplied = Explanation::new_with_string("custom owned idf".into(), 1.25);
    supplied.add_context("owned context".into());
    supplied.add_detail(Explanation::new_with_string("owned child".into(), -0.0));
    let weight = Bm25Weight::new(supplied, 4.0);
    let boosted = weight.boost_by(3.25);
    let IdfExplanation::Explicit(shared) = &weight.idf_explain else {
        panic!("explicit metadata")
    };
    let IdfExplanation::Explicit(copied) = &boosted.idf_explain else {
        panic!("cloned explicit metadata")
    };
    assert!(Arc::ptr_eq(shared, copied));
    assert!(Arc::ptr_eq(&weight.cache, &boosted.cache));
    let weak = Arc::downgrade(shared);
    let mut output = boosted.explain(91, 7);
    let expected = output.bit_snapshot();
    drop(weight);
    drop(boosted);
    assert!(weak.upgrade().is_none());
    output.add_context("after owners dropped".into());
    assert_ne!(output.bit_snapshot(), expected);

    let single = Bm25Weight::for_terms(&stats, &terms(&["a"]))?;
    assert!(matches!(single.idf_explain, IdfExplanation::Single(_)));
    let classic = Statistics {
        native: false,
        ..stats
    };
    assert!(matches!(
        Bm25Weight::for_terms(&classic, &terms(&["a", "b"]))?.idf_explain,
        IdfExplanation::ClassicScalar(_)
    ));
    assert!(matches!(
        Bm25Weight::for_one_term_without_explain(53, 347, 4.0).idf_explain,
        IdfExplanation::None
    ));
    Ok(())
}
