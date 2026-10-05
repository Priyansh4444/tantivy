use super::*;
use crate::directory::FileSlice;
use crate::postings::{BlockInfo, BlockSegmentPostings, SegmentPostings};
use crate::query::{Bm25Parameters, TermScorer};
use crate::schema::{Schema, TEXT};
use crate::{DocSet, Index, Term, TERMINATED};

// Query scoring provenance comes from an actual native Searcher, including its
// installed parameters. Arbitrary synthetic norms/TFs below go through the real
// native serializer; they do not assume the string writer excludes norm0.
fn query_weight(parameters: Bm25Parameters) -> crate::Result<Bm25Weight> {
    let mut schema = Schema::builder();
    let field = schema.add_text_field("text", TEXT);
    let index = Index::create_in_ram(schema.build());
    let mut writer = index.writer_for_tests()?;
    writer.add_document(doc!(field => "alpha ".repeat(321)))?;
    writer.add_document(doc!(field => "beta ".repeat(321)))?;
    writer.commit()?;
    let searcher = index.reader()?.searcher().with_bm25_parameters(parameters);
    Bm25Weight::for_terms(&searcher, &[Term::from_field_text(field, "alpha")])
}

fn serialized(
    average: Score,
    selection: BlockMaxSelection,
    dense: bool,
    frequencies: &[u32],
    norms_by_posting: &[u8],
    metadata: bool,
) -> (BlockSegmentPostings, FieldNormReader, Vec<(DocId, u32)>) {
    let docs: Vec<_> = frequencies
        .iter()
        .enumerate()
        .map(|(i, &tf)| {
            let doc = if dense {
                (i / 128 * 256 + i % 128 + if i % 128 >= 64 { 40 } else { 0 }) as u32
            } else {
                i as u32
            };
            (doc, tf)
        })
        .collect();
    let mut norms = vec![0; docs.last().unwrap().0 as usize + 1];
    for (&(doc, _), &norm) in docs.iter().zip(norms_by_posting) {
        norms[doc as usize] = FieldNormReader::id_to_fieldnorm(norm);
    }
    let reader = FieldNormReader::for_test(&norms);
    let mut serializer = PostingsSerializer::new_with_selection(
        average,
        IndexRecordOption::WithFreqs,
        if metadata { Some(reader.clone()) } else { None },
        selection,
    );
    serializer.new_term(docs.len() as u32, true);
    for &(doc, tf) in &docs {
        serializer.write_doc(doc, tf);
    }
    let mut bytes = Vec::new();
    serializer
        .close_term(docs.len() as u32, &mut bytes)
        .unwrap();
    let cursor = BlockSegmentPostings::open(
        docs.len() as u32,
        FileSlice::from(bytes),
        IndexRecordOption::WithFreqs,
        IndexRecordOption::WithFreqs,
    )
    .unwrap();
    assert_eq!(
        matches!(cursor.skip_reader().block_info(), BlockInfo::Dense { .. }),
        dense
    );
    (cursor, reader, docs)
}

fn own_weight(
    cursor: BlockSegmentPostings,
    reader: &FieldNormReader,
    weight: Bm25Weight,
    average: Score,
    selection: BlockMaxSelection,
) -> TermScorer {
    TermScorer::new(
        SegmentPostings::from_block_postings(cursor, None),
        reader.clone(),
        weight,
    )
    .with_stored_block_max_selection(average, selection)
}

#[test]
fn native_transform_serialized_dense_for_all_norms_and_frequency_extremes() -> crate::Result<()> {
    let query = query_weight(Bm25Parameters::new(0.9, 0.4)?)?;
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
    for dense in [false, true] {
        for tf in frequencies {
            let tfs = vec![tf; 257];
            let norms: Vec<_> = (0..257).map(|i| (i % 256) as u8).collect();
            let (fixture, reader, docs) = serialized(
                3.0,
                BlockMaxSelection::NativeSaturationInput,
                dense,
                &tfs,
                &norms,
                true,
            );
            let mut certificate_cursor = fixture.clone();
            let selection_weight = Bm25Weight::for_native_block_bounds(3.0);
            for block in 0..2 {
                if block != 0 {
                    certificate_cursor.seek_block(docs[block * 128].0);
                }
                let pair = certificate_cursor
                    .skip_reader()
                    .selected_input_pair()
                    .expect("native complete block must actually carry its input certificate");
                assert_eq!(
                    pair.selected_tf_ceiling.get(),
                    if tf < 255 { tf } else { u32::MAX }
                );
                let block_docs = &docs[block * 128..(block + 1) * 128];
                assert!(block_docs
                    .iter()
                    .any(|&(doc, _)| reader.fieldnorm_id(doc) == pair.norm));
                let stored_input = selection_weight
                    .native_saturation_input(pair.norm, pair.selected_tf_ceiling.get());
                for &(doc, frequency) in block_docs {
                    assert!(
                        selection_weight
                            .native_saturation_input(reader.fieldnorm_id(doc), frequency)
                            <= stored_input
                    );
                }
            }
            for boost in [
                f32::from_bits(1),
                1.0,
                100.0,
                -1.0,
                0.0,
                -0.0,
                f32::INFINITY,
                f32::NAN,
            ] {
                let weight = query.boost_by(boost);
                let mut scorer = own_weight(
                    fixture.clone(),
                    &reader,
                    weight.clone(),
                    3.0,
                    BlockMaxSelection::NativeSaturationInput,
                );
                for block in 0..2 {
                    if block != 0 {
                        scorer.seek_block(docs[block * 128].0);
                        assert!(!scorer.block_cursor().block_is_loaded());
                    }
                    let bound = scorer.block_max_score();
                    for &(doc, frequency) in &docs[block * 128..(block + 1) * 128] {
                        let score = weight.score(reader.fieldnorm_id(doc), frequency);
                        assert!(
                            score.is_nan() || score <= bound,
                            "dense={dense} tf={tf} boost={boost} doc={doc} score={score} \
                             bound={bound}"
                        );
                    }
                    if tf == 1 && boost == 1.0 {
                        assert!(
                            bound < weight.max_score(),
                            "finite B1 must tighten this real serialized block"
                        );
                    }
                    scorer.seek(docs[block * 128].0);
                    assert!(scorer.block_cursor().block_is_loaded());
                    assert_eq!(bound.to_bits(), scorer.block_max_score().to_bits());
                    let mut cloned = scorer.clone();
                    let public = cloned
                        .block_cursor()
                        .block_max_score(&reader, &weight.boost_by(0.01));
                    assert_eq!(
                        public.to_bits(),
                        weight.boost_by(0.01).max_score().to_bits()
                    );
                    assert_eq!(cloned.block_max_score().to_bits(), bound.to_bits());
                }
                scorer.seek_block(docs[256].0);
                assert_eq!(scorer.last_doc_in_block(), TERMINATED);
                assert_eq!(
                    scorer.block_max_score().to_bits(),
                    weight.max_score().to_bits()
                );
                scorer.seek(docs[256].0);
                let tail_expected = if weight.has_safe_score_bounds() {
                    weight.score(reader.fieldnorm_id(docs[256].0), tf).max(0.0)
                } else {
                    Score::INFINITY
                };
                assert_eq!(scorer.block_max_score().to_bits(), tail_expected.to_bits());
            }
        }
    }
    Ok(())
}

#[test]
fn native_transform_keeps_legacy_absent_metadata_and_b1_global() -> crate::Result<()> {
    let parameters = Bm25Parameters::new(0.9, 0.4)?;
    let query = query_weight(parameters)?;
    let b1 = query_weight(Bm25Parameters::new(2.5, 1.0)?)?;
    for dense in [false, true] {
        for (selection, metadata) in [
            (BlockMaxSelection::LegacyTfFactor, true),
            (BlockMaxSelection::NativeSaturationInput, false),
            (BlockMaxSelection::NativeSaturationInput, true),
        ] {
            let tfs = vec![1; 256];
            let norms = vec![0; 256];
            let (fixture, reader, docs) = serialized(3.0, selection, dense, &tfs, &norms, metadata);
            let weight = if selection == BlockMaxSelection::NativeSaturationInput && metadata {
                &b1
            } else {
                &query
            };
            let mut scorer = own_weight(fixture, &reader, weight.clone(), 3.0, selection);
            assert_eq!(
                scorer.block_max_score().to_bits(),
                weight.max_score().to_bits()
            );
            scorer.seek_block(docs[128].0);
            assert!(!scorer.block_cursor().block_is_loaded());
            assert_eq!(
                scorer.block_max_score().to_bits(),
                weight.max_score().to_bits()
            );
            scorer.seek(docs[128].0);
            assert_eq!(
                scorer.block_max_score().to_bits(),
                weight.max_score().to_bits()
            );
        }
    }
    Ok(())
}

#[test]
fn native_transform_serialized_mismatched_averages_cover_changed_maximizer() -> crate::Result<()> {
    let query = query_weight(Bm25Parameters::new(0.9, 0.4)?)?;
    let competing_norm = FieldNormReader::fieldnorm_to_id(1000);
    for dense in [false, true] {
        let norms: Vec<_> = (0..256)
            .map(|i| if i % 2 == 0 { 0 } else { competing_norm })
            .collect();
        let frequencies: Vec<_> = (0..256).map(|i| if i % 2 == 0 { 1 } else { 100 }).collect();
        let (fixture, reader, docs) = serialized(
            3.0,
            BlockMaxSelection::NativeSaturationInput,
            dense,
            &frequencies,
            &norms,
            true,
        );
        let old = Bm25Weight::for_native_block_bounds(3.0);
        assert!(
            old.native_saturation_input(0, 1) > old.native_saturation_input(competing_norm, 100)
        );
        assert!(
            query.native_saturation_input(0, 1)
                < query.native_saturation_input(competing_norm, 100)
        );
        let mut scorer = own_weight(
            fixture,
            &reader,
            query.clone(),
            3.0,
            BlockMaxSelection::NativeSaturationInput,
        );
        let bound = scorer.block_max_score();
        for &(doc, tf) in &docs[..128] {
            assert!(query.score(reader.fieldnorm_id(doc), tf) <= bound);
        }
        let selected = scorer
            .block_cursor()
            .skip_reader()
            .selected_input_pair()
            .unwrap();
        assert_eq!(selected.norm, 0);
        assert_eq!(selected.selected_tf_ceiling.get(), 1);
    }
    Ok(())
}
