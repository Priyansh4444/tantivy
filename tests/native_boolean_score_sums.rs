//! Native Boolean sums must retain ties when heap/cost order changes.
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, EnableScoring, Occur, Query, TermQuery};
use tantivy::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions};
use tantivy::{doc, Index, Term, TERMINATED};

#[test]
fn native_boolean_identical_documents_keep_ties_across_clause_orders() -> tantivy::Result<()> {
    let mut schema = Schema::builder();
    let text = schema.add_text_field(
        "text",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqs)
                .set_fieldnorms(false),
        ),
    );
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_with_num_threads(1, 15_000_000)?;
    // A smaller corpus with the exact failing case's native statistics:
    // N=147, tokens=1583, df(alpha)=df(beta)=108, df(gamma)=53.
    for id in 0..147 {
        let mut tokens = Vec::new();
        if id < 108 {
            tokens.extend(["alpha", "beta"]);
        }
        if id < 53 {
            tokens.extend(["gamma", "gamma"]);
        }
        tokens.extend(std::iter::repeat_n("padding", 8 + usize::from(id < 85)));
        writer.add_document(doc!(text => tokens.join(" ")))?;
    }
    writer.commit()?;
    let searcher = index.reader()?.searcher();
    let scoring = EnableScoring::enabled_from_searcher(&searcher);
    let terms = ["alpha", "beta", "gamma"];
    let mut components = Vec::new();
    for term in terms {
        let query = TermQuery::new(
            Term::from_field_text(text, term),
            IndexRecordOption::WithFreqs,
        );
        let mut scorer = query
            .weight(scoring)?
            .scorer(searcher.segment_reader(0), 1.0)?;
        assert_eq!(scorer.doc(), 0);
        components.push(scorer.score());
    }
    assert_eq!(
        components.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        [0x3e65c6cb, 0x3e65c6cb, 0x3f5a91a8]
    );
    // Lucene 10.4 DisjunctionSumScorer/ConjunctionScorer accumulate double,
    // then cast once. This is 1.3025673627853394, not its lower f32 neighbor.
    let expected = components.iter().map(|&x| f64::from(x)).sum::<f64>() as f32;
    for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
        for (occur, minimum) in [(Occur::Should, 2), (Occur::Should, 1), (Occur::Must, 0)] {
            let query = BooleanQuery::with_minimum_required_clauses(
                order
                    .iter()
                    .map(|&i| {
                        (
                            occur,
                            Box::new(TermQuery::new(
                                Term::from_field_text(text, terms[i]),
                                IndexRecordOption::WithFreqs,
                            )) as Box<dyn Query>,
                        )
                    })
                    .collect(),
                minimum,
            );
            let mut scorer = query
                .weight(scoring)?
                .scorer(searcher.segment_reader(0), 1.0)?;
            while scorer.doc() != TERMINATED {
                if scorer.doc() < 53 {
                    assert_eq!(
                        scorer.score().to_bits(),
                        expected.to_bits(),
                        "doc={} order={order:?} occur={occur:?} minimum={minimum}",
                        scorer.doc()
                    );
                }
                scorer.advance();
            }
            let top = searcher.search(&query, &TopDocs::with_limit(10).order_by_score())?;
            assert!(
                top.iter()
                    .all(|&(score, _)| score.to_bits() == expected.to_bits()),
                "pruned top order={order:?} occur={occur:?} minimum={minimum}"
            );
            let weight = query.weight(scoring)?;
            for threshold in [expected.next_down(), expected, expected.next_up()] {
                let mut accepted = Vec::new();
                weight.for_each_pruning(
                    threshold,
                    searcher.segment_reader(0),
                    &mut |doc, score| {
                        accepted.push((doc, score.to_bits()));
                        threshold
                    },
                )?;
                let wanted: Vec<_> = if threshold < expected {
                    (0..53).map(|doc| (doc, expected.to_bits())).collect()
                } else {
                    Vec::new()
                };
                assert_eq!(
                    accepted, wanted,
                    "threshold={threshold:?} order={order:?} occur={occur:?}"
                );
            }
        }
    }

    // An existing custom combiner opting into sum pruning may still perform
    // recursive f32 addition. Keep its arithmetic through the supplied combiner.
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tantivy::query::{BooleanWeight, ScoreCombiner, Scorer, Weight};
    static UPDATES: AtomicUsize = AtomicUsize::new(0);
    #[derive(Clone, Copy, Default)]
    struct FloatSum(f32);
    impl ScoreCombiner for FloatSum {
        const SUPPORTS_BLOCK_WAND: bool = true;
        fn update<S: Scorer>(&mut self, scorer: &mut S) {
            UPDATES.fetch_add(1, Ordering::Relaxed);
            self.0 += scorer.score();
        }
        fn clear(&mut self) {
            self.0 = 0.0;
        }
        fn score(&self) -> f32 {
            self.0
        }
    }
    let weights = ["gamma", "alpha", "beta"]
        .iter()
        .map(|term| {
            Ok((
                Occur::Should,
                TermQuery::new(
                    Term::from_field_text(text, term),
                    IndexRecordOption::WithFreqs,
                )
                .weight(scoring)?,
            ))
        })
        .collect::<tantivy::Result<_>>()?;
    let custom = BooleanWeight::with_minimum_number_should_match(
        weights,
        1,
        true,
        Box::new(FloatSum::default),
    );
    let mut exhaustive = Vec::new();
    custom.for_each(searcher.segment_reader(0), &mut |doc, score| {
        exhaustive.push((doc, score.to_bits()))
    })?;
    assert_eq!(
        exhaustive[0].1, 1067891334,
        "retain the custom combiner's lower f32 result"
    );
    UPDATES.store(0, Ordering::Relaxed);
    let mut pruned = Vec::new();
    let custom_threshold = f32::from_bits(1067891334).next_down();
    custom.for_each_pruning(
        custom_threshold,
        searcher.segment_reader(0),
        &mut |doc, score| {
            pruned.push((doc, score.to_bits()));
            custom_threshold
        },
    )?;
    assert!(
        UPDATES.load(Ordering::Relaxed) > 0,
        "custom sum must use its own update arithmetic"
    );
    assert_eq!(
        pruned,
        exhaustive
            .into_iter()
            .filter(|&(_, bits)| f32::from_bits(bits) > custom_threshold)
            .collect::<Vec<_>>()
    );
    Ok(())
}
