//! Exact physical field statistics and the provenance of serialized block bounds.
use std::io;

use common::BitSet;
use once_cell::sync::OnceCell;

use crate::postings::Postings;
use crate::schema::IndexRecordOption;
use crate::{DocId, DocSet, InvertedIndexReader, Score, TERMINATED};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FieldStatistics {
    pub doc_count: u32,
    pub sum_total_term_freq: u64,
}

pub(crate) fn native_average(tokens: u64, docs: u64) -> Score {
    if docs == 0 {
        0.0
    } else {
        (tokens as f64 / docs as f64) as Score
    }
}

pub(crate) fn legacy_average(tokens: u64, docs: u32) -> Score {
    tokens as Score / docs as Score
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl FieldStatistics {
    pub(crate) fn validate(self, max_doc: u32) -> io::Result<Self> {
        if self.doc_count > max_doc
            || (self.doc_count == 0 && self.sum_total_term_freq != 0)
            || self.sum_total_term_freq < u64::from(self.doc_count)
        {
            return Err(invalid("Inconsistent exact field statistics"));
        }
        Ok(self)
    }

    pub(crate) fn average(self) -> Score {
        native_average(self.sum_total_term_freq, u64::from(self.doc_count))
    }

    fn add(&mut self, other: Self) -> io::Result<()> {
        self.doc_count = self
            .doc_count
            .checked_add(other.doc_count)
            .ok_or_else(|| invalid("Field population overflow"))?;
        self.sum_total_term_freq = self
            .sum_total_term_freq
            .checked_add(other.sum_total_term_freq)
            .ok_or_else(|| invalid("Field token total overflow"))?;
        Ok(())
    }
}

/// The comparator that selected a serialized (fieldnorm, frequency) bound.
/// Statistics alone do not certify compatibility between scoring expressions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BlockMaxSelection {
    LegacyTfFactor,
    NativeSaturationInput,
}

impl BlockMaxSelection {
    pub(crate) const NATIVE_TAG: u8 = 1;
}

pub(crate) enum StatisticsSource {
    Native(FieldStatistics, BlockMaxSelection),
    Legacy {
        header_tokens: u64,
        max_doc: u32,
        exact: OnceCell<FieldStatistics>,
    },
    Empty,
}

impl StatisticsSource {
    pub(crate) fn open(header_tokens: u64, max_doc: u32, count: Option<&[u8]>) -> io::Result<Self> {
        match count {
            Some(bytes) => {
                let selection = match bytes.len() {
                    4 => BlockMaxSelection::LegacyTfFactor,
                    5 if bytes[4] == BlockMaxSelection::NATIVE_TAG => {
                        BlockMaxSelection::NativeSaturationInput
                    }
                    _ => {
                        return Err(invalid(
                            "Invalid field population or bound-selection metadata",
                        ))
                    }
                };
                let bytes: [u8; 4] = bytes[..4].try_into().unwrap();
                let statistics = FieldStatistics {
                    doc_count: u32::from_le_bytes(bytes),
                    sum_total_term_freq: header_tokens,
                }
                .validate(max_doc)?;
                Ok(Self::Native(statistics, selection))
            }
            None => Ok(Self::Legacy {
                header_tokens,
                max_doc,
                exact: OnceCell::new(),
            }),
        }
    }

    pub(crate) fn header_tokens(&self) -> u64 {
        match self {
            Self::Native(stats, _) => stats.sum_total_term_freq,
            Self::Legacy { header_tokens, .. } => *header_tokens,
            Self::Empty => 0,
        }
    }

    pub(crate) fn selection_average(&self) -> Score {
        match self {
            Self::Native(stats, _) => stats.average(),
            Self::Legacy {
                header_tokens,
                max_doc,
                ..
            } => legacy_average(*header_tokens, *max_doc),
            Self::Empty => 0.0,
        }
    }

    pub(crate) fn selection(&self) -> BlockMaxSelection {
        match self {
            Self::Native(_, selection) => *selection,
            Self::Legacy { .. } | Self::Empty => BlockMaxSelection::LegacyTfFactor,
        }
    }

    pub(crate) fn exact(&self, reader: &InvertedIndexReader) -> io::Result<FieldStatistics> {
        match self {
            Self::Native(stats, _) => Ok(*stats),
            Self::Legacy { max_doc, exact, .. } => exact
                .get_or_try_init(|| derive_physical(reader, *max_doc))
                .copied(),
            Self::Empty => Ok(FieldStatistics::default()),
        }
    }
}

// Both reducers use actual decoded frequencies. No-frequency postings return one,
// including JSON numerical terms with short VInt tails and full compressed blocks.
fn reduce(
    reader: &InvertedIndexReader,
    source_max_doc: u32,
    mapping: Option<&[Option<DocId>]>,
    docs: &mut BitSet,
    tokens: &mut u64,
) -> io::Result<()> {
    let mut terms = reader.terms().stream()?;
    while let Some((_, info)) = terms.next() {
        let mut postings =
            reader.read_postings_from_terminfo(info, IndexRecordOption::WithFreqs)?;
        while postings.doc() != TERMINATED {
            let doc = postings.doc();
            if doc >= source_max_doc {
                return Err(invalid("Posting exceeds segment maxDoc"));
            }
            let retained = match mapping {
                Some(map) => map[doc as usize],
                None => Some(doc),
            };
            if let Some(target) = retained {
                docs.insert(target);
                *tokens = tokens
                    .checked_add(u64::from(postings.term_freq()))
                    .ok_or_else(|| invalid("Field token total overflow"))?;
            }
            postings.advance();
        }
    }
    Ok(())
}

pub(crate) fn derive_physical(
    reader: &InvertedIndexReader,
    max_doc: u32,
) -> io::Result<FieldStatistics> {
    let mut docs = BitSet::with_max_value(max_doc);
    let mut tokens = 0;
    reduce(reader, max_doc, None, &mut docs, &mut tokens)?;
    FieldStatistics {
        doc_count: docs.len() as u32,
        sum_total_term_freq: tokens,
    }
    .validate(max_doc)
}

pub(crate) fn derive_retained(
    readers: &[std::sync::Arc<InvertedIndexReader>],
    mappings: &[Vec<Option<DocId>>],
    target_max_doc: u32,
) -> io::Result<FieldStatistics> {
    // Proof is the actual complete map, independent of source alive masks or mapping kind.
    if mappings.iter().all(|map| map.iter().all(Option::is_some)) {
        let mut statistics = FieldStatistics::default();
        for reader in readers {
            statistics.add(reader.field_statistics()?)?;
        }
        return statistics.validate(target_max_doc);
    }
    let mut docs = BitSet::with_max_value(target_max_doc);
    let mut tokens = 0;
    for (reader, mapping) in readers.iter().zip(mappings) {
        reduce(
            reader,
            mapping.len() as u32,
            Some(mapping),
            &mut docs,
            &mut tokens,
        )?;
    }
    FieldStatistics {
        doc_count: docs.len() as u32,
        sum_total_term_freq: tokens,
    }
    .validate(target_max_doc)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    use common::HasLen;

    use super::*;
    use crate::directory::{CompositeFile, FileHandle, FileSlice, OwnedBytes};
    use crate::fieldnorm::FieldNormReader;
    use crate::index::SegmentComponent;
    use crate::postings::InvertedIndexSerializer;
    use crate::schema::{Facet, Schema, TextFieldIndexing, TextOptions, INDEXED, TEXT};
    use crate::termdict::TermDictionary;
    use crate::Index;

    fn legacy_parts() -> crate::Result<(FileSlice, FileSlice, FileSlice)> {
        let mut schema = Schema::builder();
        let options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default().set_index_option(IndexRecordOption::WithFreqs),
        );
        let field = schema.add_text_field("text", options);
        let index = Index::create_in_ram(schema.build());
        let segment = index.new_segment().with_max_doc(4);
        let mut serializer = InvertedIndexSerializer::open(&segment)?;
        let mut writer = serializer.new_field(field, 100, Some(FieldNormReader::constant(4, 1)))?;
        writer.new_term(b"a", 2, true)?;
        writer.write_doc(0, 3, &[]);
        writer.write_doc(2, 2, &[]);
        writer.close_term()?;
        writer.new_term(b"b", 1, true)?;
        writer.write_doc(2, 2, &[]);
        writer.close_term()?;
        writer.close()?;
        serializer.close()?;
        let terms = CompositeFile::open(&segment.open_read(SegmentComponent::Terms)?)?
            .open_read(field)
            .unwrap();
        let postings = CompositeFile::open(&segment.open_read(SegmentComponent::Postings)?)?;
        assert!(
            postings.open_read_with_idx(field, 1).is_none(),
            "public legacy writer must not certify native bounds"
        );
        let postings = postings.open_read(field).unwrap();
        let positions = CompositeFile::open(&segment.open_read(SegmentComponent::Positions)?)?
            .open_read(field)
            .unwrap();
        Ok((terms, postings, positions))
    }

    #[test]
    fn field_statistics_legacy_exact_tokens_preserve_header_provenance() -> crate::Result<()> {
        let (terms, postings, positions) = legacy_parts()?;
        let reader = InvertedIndexReader::new(
            TermDictionary::open(terms)?,
            postings,
            positions,
            IndexRecordOption::WithFreqs,
            4,
            None,
        )?;
        assert_eq!(reader.total_num_tokens(), 100);
        assert_eq!(
            reader.field_statistics()?,
            FieldStatistics {
                doc_count: 2,
                sum_total_term_freq: 7
            }
        );
        assert_eq!(reader.stored_selection_average_fieldnorm(), 25.0);
        let native = crate::query::Bm25Weight::for_one_term(2, 2, 3.5);
        assert!(!native.can_use_stored_block_max(reader.stored_selection_average_fieldnorm()));
        let legacy = crate::query::Bm25Weight::for_one_term(2, 4, 25.0);
        assert!(legacy.can_use_stored_block_max(reader.stored_selection_average_fieldnorm()));
        Ok(())
    }

    #[test]
    fn native_bound_metadata_round_trip_and_symmetric_reader_policy() -> crate::Result<()> {
        use crate::query::{Bm25Weight, EnableScoring, TermQuery};
        let mut schema = Schema::builder();
        let text = schema.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        for _ in 0..128 {
            writer.add_document(doc!(text => "a"))?;
        }
        writer.commit()?;
        let searcher = index.reader()?.searcher();
        let segment = searcher.segment_reader(0);
        let inverted = segment.inverted_index(text)?;
        assert_eq!(
            inverted.stored_block_max_selection(),
            BlockMaxSelection::NativeSaturationInput
        );
        let stored = index.segment(index.searchable_segment_metas()?.remove(0));
        let composite = CompositeFile::open(&stored.open_read(SegmentComponent::Postings)?)?;
        assert_eq!(
            composite
                .open_read_with_idx(text, 1)
                .unwrap()
                .read_bytes()?
                .as_slice(),
            &[128, 0, 0, 0, 1]
        );
        let term = crate::Term::from_field_text(text, "a");
        let native = Bm25Weight::for_terms(&searcher, &[term.clone()])?;
        let legacy = Bm25Weight::for_one_term(128, 128, 1.0);
        assert!(native
            .can_use_stored_block_max_with_selection(1.0, inverted.stored_block_max_selection()));
        assert!(!legacy
            .can_use_stored_block_max_with_selection(1.0, inverted.stored_block_max_selection()));
        // Actual native Searcher -> TermWeight -> reader provenance -> TermScorer route.
        let query = TermQuery::new(term, IndexRecordOption::WithFreqs);
        let mut scorer = query
            .specialized_weight(EnableScoring::enabled_from_searcher(&searcher))?
            .term_scorer_for_test(segment, 1.0)?
            .unwrap();
        let bound = scorer.block_max_score();
        assert_eq!(bound, native.score(1, 1));
        assert!(bound < native.max_score());
        Ok(())
    }

    #[test]
    fn field_statistics_metadata_validation_and_rounding() {
        for bytes in [&[][..], &[0, 0, 0][..], &[0; 5][..]] {
            assert!(StatisticsSource::open(7, 4, Some(bytes)).is_err());
        }
        for (tokens, count) in [(7, 5u32), (7, 0), (1, 2)] {
            assert!(StatisticsSource::open(tokens, 4, Some(&count.to_le_bytes())).is_err());
        }
        let empty = StatisticsSource::open(0, 4, Some(&0u32.to_le_bytes())).unwrap();
        assert_eq!(empty.selection_average(), 0.0);
        let native = StatisticsSource::open(7, 4, Some(&2u32.to_le_bytes())).unwrap();
        assert_eq!(native.selection_average(), 3.5);
        assert_eq!(native.selection(), BlockMaxSelection::LegacyTfFactor);
        let tagged = StatisticsSource::open(7, 4, Some(&[2, 0, 0, 0, 1])).unwrap();
        assert_eq!(tagged.selection_average(), 3.5);
        assert_eq!(tagged.selection(), BlockMaxSelection::NativeSaturationInput);
        for bytes in [
            &[2, 0, 0, 0, 0][..],
            &[2, 0, 0, 0, 2][..],
            &[2, 0, 0, 0, 1, 0][..],
        ] {
            assert!(StatisticsSource::open(7, 4, Some(bytes)).is_err());
        }
        // Explicitly exercise inputs where the two historical rounding policies differ.
        let (tokens, count) = (16_777_216u64..16_778_000)
            .flat_map(|tokens| (3..30u32).map(move |count| (tokens, count)))
            .find(|&(tokens, count)| {
                native_average(tokens, u64::from(count)) != legacy_average(tokens, count)
            })
            .expect("large integer averages distinguish native and legacy rounding");
        let native = StatisticsSource::open(tokens, count, Some(&count.to_le_bytes())).unwrap();
        let legacy = StatisticsSource::open(tokens, count, None).unwrap();
        assert_eq!(
            native.selection_average(),
            (tokens as f64 / f64::from(count)) as f32
        );
        assert_eq!(legacy.selection_average(), tokens as f32 / count as f32);
        assert_ne!(native.selection_average(), legacy.selection_average());
    }

    #[test]
    fn field_statistics_native_matches_physical_mixed_json_blocks_and_tails() -> crate::Result<()> {
        let mut schema = Schema::builder();
        let json = schema.add_json_field("json", TEXT);
        let text = schema.add_text_field("text", TEXT);
        let basic = schema.add_text_field(
            "basic",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::Basic),
            ),
        );
        let no_norms = schema.add_text_field(
            "no_norms",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqs)
                    .set_fieldnorms(false),
            ),
        );
        let empty = schema.add_text_field("empty", TEXT);
        let number = schema.add_u64_field("number", INDEXED);
        let facet = schema.add_facet_field("facet", crate::schema::FacetOptions::default());
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        for i in 0..140 {
            let value = if i < 3 {
                serde_json::json!({"s": "a a b", "n": [1, 1], "short": 7})
            } else {
                serde_json::json!({"s": "a a b", "n": [1, 1]})
            };
            writer.add_document(
                doc!(json => value, text => "a a b", basic => "a a b", no_norms => "a a b",
                    number => 1u64, number => 1u64, facet => Facet::from("/a/b"), facet => Facet::from("/a/b")),
            )?;
        }
        writer.add_document(
            doc!(json => serde_json::json!({}), text => "", basic => "", no_norms => ""),
        )?;
        writer.add_document(doc!())?;
        writer.commit()?;
        let searcher = index.reader()?.searcher();
        let segment = searcher.segment_reader(0);
        for (field, tokens) in [
            (json, 563),
            (text, 420),
            (basic, 280),
            (no_norms, 420),
            (number, 140),
            (facet, 420),
        ] {
            let reader = segment.inverted_index(field)?;
            let expected = FieldStatistics {
                doc_count: 140,
                sum_total_term_freq: tokens,
            };
            assert_eq!(reader.field_statistics()?, expected);
            assert_eq!(derive_physical(&reader, segment.max_doc())?, expected);
            assert_eq!(
                reader.stored_selection_average_fieldnorm(),
                expected.average()
            );
        }
        assert_eq!(
            segment.inverted_index(empty)?.field_statistics()?,
            FieldStatistics::default()
        );
        Ok(())
    }

    #[test]
    fn field_statistics_retained_mapping_filters_independent_of_alive_masks() -> crate::Result<()> {
        let (terms, postings, positions) = legacy_parts()?;
        let reader = Arc::new(InvertedIndexReader::new(
            TermDictionary::open(terms)?,
            postings,
            positions,
            IndexRecordOption::WithFreqs,
            4,
            None,
        )?);
        assert_eq!(
            derive_retained(
                &[Arc::clone(&reader)],
                &[vec![None, Some(0), Some(1), None]],
                2
            )?,
            FieldStatistics {
                doc_count: 1,
                sum_total_term_freq: 4
            }
        );
        assert_eq!(
            derive_retained(&[reader], &[vec![Some(3), Some(2), Some(1), Some(0)]], 4)?,
            FieldStatistics {
                doc_count: 2,
                sum_total_term_freq: 7
            }
        );
        Ok(())
    }

    #[derive(Debug)]
    struct RetryFile {
        bytes: OwnedBytes,
        fail: AtomicBool,
        body_reads: AtomicUsize,
    }
    impl HasLen for RetryFile {
        fn len(&self) -> usize {
            self.bytes.len()
        }
    }
    impl FileHandle for RetryFile {
        fn read_bytes(&self, range: std::ops::Range<usize>) -> io::Result<OwnedBytes> {
            if range.start >= 8 {
                self.body_reads.fetch_add(1, Ordering::SeqCst);
                if self.fail.swap(false, Ordering::SeqCst) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "injected transient read failure",
                    ));
                }
            }
            Ok(self.bytes.slice(range))
        }
    }

    #[test]
    fn field_statistics_legacy_cache_shares_success_and_retries_failure() -> crate::Result<()> {
        let (terms, postings, positions) = legacy_parts()?;
        let file = Arc::new(RetryFile {
            bytes: postings.read_bytes()?,
            fail: AtomicBool::new(true),
            body_reads: AtomicUsize::new(0),
        });
        let reader = Arc::new(InvertedIndexReader::new(
            TermDictionary::open(terms)?,
            FileSlice::new(file.clone()),
            positions,
            IndexRecordOption::WithFreqs,
            4,
            None,
        )?);
        assert!(reader.field_statistics().is_err());
        let barrier = Arc::new(Barrier::new(8));
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let reader = Arc::clone(&reader);
                    let barrier = Arc::clone(&barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        reader.field_statistics().unwrap()
                    })
                })
                .collect();
            for handle in handles {
                assert_eq!(
                    handle.join().unwrap(),
                    FieldStatistics {
                        doc_count: 2,
                        sum_total_term_freq: 7
                    }
                );
            }
        });
        assert_eq!(
            file.body_reads.load(Ordering::SeqCst),
            3,
            "one failed term read plus two successful term reads"
        );
        reader.field_statistics()?;
        assert_eq!(file.body_reads.load(Ordering::SeqCst), 3);
        Ok(())
    }

    #[test]
    fn field_statistics_concurrent_segment_opens_return_canonical_arc() -> crate::Result<()> {
        let mut schema = Schema::builder();
        let text = schema.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        writer.add_document(doc!(text => "a"))?;
        writer.commit()?;
        let searcher = index.reader()?.searcher();
        let segment = searcher.segment_reader(0);
        let barrier = Barrier::new(8);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        segment.inverted_index(text).unwrap()
                    })
                })
                .collect();
            let readers: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            for reader in &readers[1..] {
                assert!(Arc::ptr_eq(&readers[0], reader));
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod pruning_tests {
    use std::io::Write;

    use crate::directory::TerminatingWrite;
    use crate::index::SegmentComponent;
    use crate::postings::{InvertedIndexSerializer, Postings};
    use crate::query::{Bm25StatisticsProvider, EnableScoring, TermQuery, Weight};
    use crate::schema::{Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions};
    use crate::{DocSet, Index, Score, SegmentReader, Term, TERMINATED};

    #[test]
    fn field_statistics_new_legacy_mixed_pruning_agrees_with_exhaustive() -> crate::Result<()> {
        let mut schema = Schema::builder();
        let text = schema.add_text_field(
            "text",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqs),
            ),
        );
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        for i in 0..384 {
            if i % 4 == 0 {
                writer.add_document(doc!())?;
            } else {
                writer.add_document(doc!(text => format!("{} b", "a ".repeat(i % 7 + 1))))?;
            }
        }
        writer.commit()?;
        let searcher = index.reader()?.searcher();
        let native = searcher.segment_reader(0).clone();
        let source = index.segment(index.searchable_segment_metas()?.remove(0));
        let target = index.new_segment().with_max_doc(native.max_doc());
        for component in [
            SegmentComponent::Store,
            SegmentComponent::FastFields,
            SegmentComponent::FieldNorms,
        ] {
            let mut output = target.open_write(component.clone())?;
            output.write_all(&source.open_read(component)?.read_bytes()?)?;
            output.terminate()?;
        }
        let original = native.inverted_index(text)?;
        let exact = original.field_statistics()?;
        let historical_tokens = exact.sum_total_term_freq + 111;
        let mut serializer = InvertedIndexSerializer::open(&target)?;
        let mut output = serializer.new_field(
            text,
            historical_tokens,
            Some(native.get_fieldnorms_reader(text)?),
        )?;
        let mut terms = original.terms().stream()?;
        while let Some((term, info)) = terms.next() {
            output.new_term(term, info.doc_freq, true)?;
            let mut postings =
                original.read_postings_from_terminfo(info, IndexRecordOption::WithFreqs)?;
            while postings.doc() != TERMINATED {
                output.write_doc(postings.doc(), postings.term_freq(), &[]);
                postings.advance();
            }
            output.close_term()?;
        }
        output.close()?;
        serializer.close()?;
        let legacy = SegmentReader::open(&target)?;
        assert_eq!(legacy.inverted_index(text)?.field_statistics()?, exact);
        assert_eq!(
            legacy.inverted_index(text)?.total_num_tokens(),
            historical_tokens
        );

        struct Statistics<'a> {
            readers: &'a [SegmentReader],
            docs: u64,
            tokens: u64,
        }
        impl Bm25StatisticsProvider for Statistics<'_> {
            fn total_num_docs(&self) -> crate::Result<u64> {
                Ok(self.docs)
            }
            fn total_num_tokens(&self, _: Field) -> crate::Result<u64> {
                Ok(self.tokens)
            }
            fn doc_freq(&self, term: &Term) -> crate::Result<u64> {
                self.readers
                    .iter()
                    .map(|reader| {
                        Ok(u64::from(
                            reader.inverted_index(term.field())?.doc_freq(term)?,
                        ))
                    })
                    .sum()
            }
        }
        let query = TermQuery::new(
            Term::from_field_text(text, "a"),
            IndexRecordOption::WithFreqs,
        );
        for readers in [
            vec![native.clone()],
            vec![legacy.clone()],
            vec![native, legacy],
        ] {
            // Mode 0 uses the actual Searcher's native policy. Modes 1/2 retain
            // historical custom providers with exact/overridden statistics.
            for mode in 0..3 {
                let custom = mode == 2;
                let docs = if custom {
                    readers.iter().map(|r| u64::from(r.max_doc())).sum()
                } else {
                    u64::from(exact.doc_count) * readers.len() as u64
                };
                let tokens = if custom {
                    historical_tokens * readers.len() as u64
                } else {
                    exact.sum_total_term_freq * readers.len() as u64
                };
                let statistics = Statistics {
                    readers: &readers,
                    docs,
                    tokens,
                };
                let scoring = if mode == 0 {
                    EnableScoring::enabled_from_searcher(&searcher)
                } else {
                    EnableScoring::enabled_from_statistics_provider(&statistics, &searcher)
                };
                let weight = query.specialized_weight(scoring)?;
                let mut exhaustive = Vec::new();
                for (segment, reader) in readers.iter().enumerate() {
                    weight.for_each(reader, &mut |doc, score| {
                        exhaustive.push((score, segment, doc))
                    })?;
                }
                let sort = |values: &mut Vec<(Score, usize, u32)>| {
                    values.sort_by(|a, b| {
                        b.0.total_cmp(&a.0)
                            .then_with(|| (a.1, a.2).cmp(&(b.1, b.2)))
                    })
                };
                sort(&mut exhaustive);
                for k in [1, 3, 10, 100] {
                    let mut top = Vec::new();
                    let mut threshold = 0.0;
                    for (segment, reader) in readers.iter().enumerate() {
                        weight.for_each_pruning(threshold, reader, &mut |doc, score| {
                            top.push((score, segment, doc));
                            sort(&mut top);
                            top.truncate(k);
                            if top.len() == k {
                                // Keep tied scores eligible so the oracle's doc-address order
                                // holds.
                                threshold =
                                    f32::from_bits(top[k - 1].0.to_bits().saturating_sub(1));
                            }
                            threshold
                        })?;
                    }
                    assert_eq!(
                        top,
                        exhaustive[..k],
                        "segments={} mode={mode} k={k}",
                        readers.len()
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use crate::indexer::NoMergePolicy;
    use crate::schema::{Schema, INDEXED, TEXT};
    use crate::{Index, Term};

    #[test]
    fn field_statistics_json_merge_keeps_effective_modes_across_block_sizes() -> crate::Result<()> {
        let mut schema = Schema::builder();
        let json = schema.add_json_field("json", TEXT);
        let id = schema.add_u64_field("id", INDEXED);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        writer.set_merge_policy(Box::new(NoMergePolicy));
        for count in [140, 3] {
            for i in 0..count {
                writer.add_document(
                    doc!(json => serde_json::json!({"s": "a a b", "n": [1, 1]}), id => i as u64),
                )?;
            }
            writer.commit()?;
        }
        let reader = index.reader()?;
        let segments = index.searchable_segment_ids()?;
        writer.merge(&segments).wait()?;
        reader.reload()?;
        let searcher = reader.searcher();
        let merged = searcher.segment_reader(0);
        let expected = FieldStatistics {
            doc_count: 143,
            sum_total_term_freq: 572,
        };
        assert_eq!(merged.inverted_index(json)?.field_statistics()?, expected);
        assert_eq!(
            derive_physical(merged.inverted_index(json)?.as_ref(), merged.max_doc())?,
            expected
        );
        for i in 0..70 {
            writer.delete_term(Term::from_field_u64(id, i));
        }
        writer.commit()?;
        reader.reload()?;
        assert_eq!(
            reader
                .searcher()
                .segment_reader(0)
                .inverted_index(json)?
                .field_statistics()?,
            expected,
            "pending deletions retain physical collection statistics"
        );
        writer.merge(&index.searchable_segment_ids()?).wait()?;
        reader.reload()?;
        let searcher = reader.searcher();
        let merged = searcher.segment_reader(0);
        let expected = FieldStatistics {
            doc_count: 70,
            sum_total_term_freq: 280,
        };
        assert_eq!(merged.inverted_index(json)?.field_statistics()?, expected);
        assert_eq!(
            derive_physical(merged.inverted_index(json)?.as_ref(), merged.max_doc())?,
            expected
        );
        Ok(())
    }
}
