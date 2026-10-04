//! The inverted index as a [`SegmentPlugin`] implementation.
//!
//! Field norms, the term dictionary, postings, and positions form a single subsystem:
//! field norms are produced by the same tokenization pass that feeds the postings, and
//! postings serialization reads the field norms back. This plugin therefore owns all four
//! files (`fieldnorm`, `term`, `idx`, `pos`) and drives them together, both while indexing
//! (`add_document`) and while merging.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use columnar::MonotonicallyMappableToU64;
use common::JsonPathWriter;
use itertools::Itertools;
use measure_time::debug_time;
use tokenizer_api::BoxTokenStream;

use crate::directory::{CompositeFile, Directory};
use crate::docset::{DocSet, TERMINATED};
use crate::error::DataCorruption;
use crate::fieldnorm::{FieldNormReader, FieldNormReaders, FieldNormsSerializer, FieldNormsWriter};
use crate::index::field_statistics::derive_retained;
use crate::index::{Segment, SegmentComponent, SegmentReader};
use crate::indexer::doc_id_mapping::{DocIdMapping, SegmentDocIdMapping};
use crate::indexer::indexing_term::IndexingTerm;
use crate::json_utils::{index_json_value, IndexingPositionsPerPath};
use crate::plugin::{PluginMergeContext, PluginWriter, PluginWriterContext, SegmentPlugin};
use crate::postings::{
    compute_table_memory_size, serialize_postings, IndexingContext, IndexingPosition,
    InvertedIndexSerializer, PerFieldPostingsWriter, Postings, PostingsWriter, SegmentPostings,
};
use crate::schema::document::{Document, Value};
use crate::schema::{Field, FieldType, IndexRecordOption, Schema, DATE_TIME_PRECISION_INDEXED};
use crate::space_usage::{ComponentSpaceUsage, FIELDNORMS, POSITIONS, POSTINGS, TERMDICT};
use crate::termdict::{TermMerger, TermOrdinal};
use crate::tokenizer::{FacetTokenizer, PreTokenizedStream, TextAnalyzer, Tokenizer};
use crate::{DocId, InvertedIndexReader, TantivyError};

/// Built-in segment plugin that stores and merges the inverted index.
pub struct InvertedIndexPlugin;

/// Computes the initial size of the term hash table.
///
/// Returns the recommended initial table size as a power of 2.
///
/// Note this is a very dumb way to compute log2, but it is easier to proofread that way.
fn compute_initial_table_size(per_thread_memory_budget: usize) -> crate::Result<usize> {
    let table_memory_upper_bound = per_thread_memory_budget / 3;
    (10..20) // We cap it at 2^19 = 512K capacity.
        // TODO: There are cases where this limit causes a
        // reallocation in the hashmap. Check if this affects performance.
        .map(|power| 1 << power)
        .take_while(|capacity| compute_table_memory_size(*capacity) < table_memory_upper_bound)
        .last()
        .ok_or_else(|| {
            crate::TantivyError::InvalidArgument(format!(
                "per thread memory budget (={per_thread_memory_budget}) is too small. Raise the \
                 memory budget or lower the number of threads."
            ))
        })
}

impl SegmentPlugin for InvertedIndexPlugin {
    fn extensions(&self) -> &[&str] {
        &["fieldnorm", "term", "idx", "pos"]
    }

    fn create_writer(&self, _ctx: &PluginWriterContext) -> crate::Result<Box<dyn PluginWriter>> {
        unimplemented!("InvertedIndexPlugin is a built-in; use InvertedIndexPluginWriter::new")
    }

    fn merge(&self, ctx: PluginMergeContext) -> crate::Result<()> {
        // Field norms first: postings merge reads them back from the target segment.
        merge_fieldnorms(&ctx)?;

        debug_time!("write-postings");
        debug!("write-postings");
        let target_segment = ctx.target_segment;
        let mut serializer = InvertedIndexSerializer::open(target_segment)?;
        let fieldnorm_data = target_segment.open_read(SegmentComponent::FieldNorms)?;
        let fieldnorm_readers = FieldNormReaders::open(fieldnorm_data)?;
        write_postings_merge(
            ctx.readers,
            ctx.schema,
            &mut serializer,
            fieldnorm_readers,
            ctx.doc_id_mapping,
        )?;
        serializer.close()?;
        Ok(())
    }

    fn space_usage(
        &self,
        segment_reader: &SegmentReader,
    ) -> crate::Result<BTreeMap<String, ComponentSpaceUsage>> {
        let schema = segment_reader.schema();

        let fieldnorms =
            FieldNormReaders::open(segment_reader.open_read(SegmentComponent::FieldNorms)?)?
                .space_usage(schema);
        let termdict = CompositeFile::open(&segment_reader.open_read(SegmentComponent::Terms)?)?
            .space_usage(schema);
        let postings = CompositeFile::open(&segment_reader.open_read(SegmentComponent::Postings)?)?
            .space_usage(schema);
        let positions = match segment_reader.open_read(SegmentComponent::Positions) {
            Ok(file) => CompositeFile::open(&file)?.space_usage(schema),
            Err(_) => CompositeFile::empty().space_usage(schema),
        };
        Ok(BTreeMap::from([
            (
                FIELDNORMS.to_string(),
                ComponentSpaceUsage::PerField(fieldnorms),
            ),
            (
                TERMDICT.to_string(),
                ComponentSpaceUsage::PerField(termdict),
            ),
            (
                POSTINGS.to_string(),
                ComponentSpaceUsage::PerField(postings),
            ),
            (
                POSITIONS.to_string(),
                ComponentSpaceUsage::PerField(positions),
            ),
        ]))
    }
}

// --- Plugin writer ---

/// Accumulates and serializes inverted-index data for a segment.
pub struct InvertedIndexPluginWriter {
    schema: Schema,
    per_field_postings_writers: PerFieldPostingsWriter,
    field_doc_counts: Vec<u32>,
    per_field_text_analyzers: Vec<TextAnalyzer>,
    fieldnorms_writer: FieldNormsWriter,
    term_buffer: IndexingTerm,
    json_path_writer: JsonPathWriter,
    json_positions_per_path: IndexingPositionsPerPath,
    ctx: IndexingContext,
    postings_serializer: InvertedIndexSerializer,
    fieldnorm_serializer: FieldNormsSerializer,
    max_doc: DocId,
}

impl InvertedIndexPluginWriter {
    pub(crate) fn new(ctx: &PluginWriterContext) -> crate::Result<Self> {
        let segment = ctx.segment;
        let schema = segment.schema();
        let tokenizer_manager = segment.index().tokenizers().clone();

        let per_field_text_analyzers = schema
            .fields()
            .map(|(_, field_entry)| {
                let text_options = match field_entry.field_type() {
                    FieldType::Str(ref text_options) => text_options.get_indexing_options(),
                    FieldType::JsonObject(ref json_object_options) => {
                        json_object_options.get_text_indexing_options()
                    }
                    _ => None,
                };
                let tokenizer_name = text_options
                    .map(|text_index_option| text_index_option.tokenizer())
                    .unwrap_or("default");
                tokenizer_manager.get(tokenizer_name).ok_or_else(|| {
                    TantivyError::SchemaError(format!(
                        "Error getting tokenizer for field: {}",
                        field_entry.name()
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let fieldnorm_path = segment.relative_path(SegmentComponent::FieldNorms);
        let fieldnorm_write = segment.index().directory().open_write(&fieldnorm_path)?;

        let table_size = compute_initial_table_size(ctx.memory_budget_in_bytes)?;
        Ok(InvertedIndexPluginWriter {
            per_field_postings_writers: PerFieldPostingsWriter::for_schema(&schema),
            field_doc_counts: vec![0; schema.num_fields()],
            per_field_text_analyzers,
            fieldnorms_writer: FieldNormsWriter::for_schema(&schema),
            term_buffer: IndexingTerm::with_capacity(16),
            json_path_writer: JsonPathWriter::default(),
            json_positions_per_path: IndexingPositionsPerPath::default(),
            ctx: IndexingContext::new(table_size),
            postings_serializer: InvertedIndexSerializer::open(segment)?,
            fieldnorm_serializer: FieldNormsSerializer::from_write(fieldnorm_write)?,
            schema,
            max_doc: 0,
        })
    }

    /// Generic, zero-copy document ingestion. The `SegmentWriter` calls this directly on the
    /// built-in inverted-index writer, so a custom `Document` is tokenized and indexed in
    /// place — no `TantivyDocument` is materialized.
    pub(crate) fn index_document<D: Document>(
        &mut self,
        doc_id: DocId,
        doc: &D,
    ) -> crate::Result<()> {
        let vals_grouped_by_field = doc
            .iter_fields_and_values()
            .sorted_by_key(|(field, _)| *field)
            .chunk_by(|(field, _)| *field);

        for (field, field_values) in &vals_grouped_by_field {
            let values = field_values.map(|el| el.1);

            let field_entry = self.schema.get_field_entry(field);
            let make_schema_error = || {
                TantivyError::SchemaError(format!(
                    "Expected a {:?} for field {:?}",
                    field_entry.field_type().value_type(),
                    field_entry.name()
                ))
            };
            if !field_entry.is_indexed() {
                continue;
            }

            let (term_buffer, ctx) = (&mut self.term_buffer, &mut self.ctx);
            let postings_writer: &mut dyn PostingsWriter =
                self.per_field_postings_writers.get_for_field_mut(field);
            let tokens_before = postings_writer.total_num_tokens();
            term_buffer.clear_with_field(field);

            match field_entry.field_type() {
                FieldType::Facet(_) => {
                    let mut facet_tokenizer = FacetTokenizer::default(); // this can be global
                    for value in values {
                        let value = value.as_value();

                        let facet_str = value.as_facet().ok_or_else(make_schema_error)?;
                        let mut facet_tokenizer = facet_tokenizer.token_stream(facet_str);
                        let mut indexing_position = IndexingPosition::default();
                        postings_writer.index_text(
                            doc_id,
                            &mut facet_tokenizer,
                            term_buffer,
                            ctx,
                            &mut indexing_position,
                        );
                    }
                }
                FieldType::Str(text_options) => {
                    let mut indexing_position = IndexingPosition::default();
                    for value in values {
                        let value = value.as_value();

                        let mut token_stream = if let Some(text) = value.as_str() {
                            let text_analyzer =
                                &mut self.per_field_text_analyzers[field.field_id() as usize];
                            text_analyzer.token_stream(text)
                        } else if let Some(tok_str) = value.into_pre_tokenized_text() {
                            BoxTokenStream::new(PreTokenizedStream::from(*tok_str.clone()))
                        } else {
                            continue;
                        };

                        assert!(term_buffer.is_empty());
                        postings_writer.index_text(
                            doc_id,
                            &mut *token_stream,
                            term_buffer,
                            ctx,
                            &mut indexing_position,
                        );
                    }
                    if field_entry.has_fieldnorms()
                        && field_entry.field_type().index_record_option()
                            != Some(IndexRecordOption::Basic)
                    {
                        let policy = text_options
                            .get_indexing_options()
                            .expect("indexed text field has indexing options")
                            .fieldnorm_policy();
                        let length = match policy {
                            crate::schema::FieldNormPolicy::CountAllTokens => {
                                indexing_position.num_tokens
                            }
                            crate::schema::FieldNormPolicy::DiscountOverlaps => {
                                indexing_position.num_tokens - indexing_position.num_overlaps
                            }
                        };
                        self.fieldnorms_writer.record(doc_id, field, length);
                    }
                }
                FieldType::U64(_) => {
                    for value in values {
                        let value = value.as_value();

                        let u64_val = value.as_u64().ok_or_else(make_schema_error)?;
                        term_buffer.set_u64(u64_val);
                        postings_writer.subscribe(doc_id, 0u32, term_buffer, ctx);
                    }
                }
                FieldType::Date(_) => {
                    for value in values {
                        let value = value.as_value();

                        let date_val = value.as_datetime().ok_or_else(make_schema_error)?;
                        term_buffer
                            .set_u64(date_val.truncate(DATE_TIME_PRECISION_INDEXED).to_u64());
                        postings_writer.subscribe(doc_id, 0u32, term_buffer, ctx);
                    }
                }
                FieldType::I64(_) => {
                    for value in values {
                        let value = value.as_value();

                        let i64_val = value.as_i64().ok_or_else(make_schema_error)?;
                        term_buffer.set_i64(i64_val);
                        postings_writer.subscribe(doc_id, 0u32, term_buffer, ctx);
                    }
                }
                FieldType::F64(_) => {
                    for value in values {
                        let value = value.as_value();
                        let f64_val = value.as_f64().ok_or_else(make_schema_error)?;
                        term_buffer.set_f64(f64_val);
                        postings_writer.subscribe(doc_id, 0u32, term_buffer, ctx);
                    }
                }
                FieldType::Bool(_) => {
                    for value in values {
                        let value = value.as_value();
                        let bool_val = value.as_bool().ok_or_else(make_schema_error)?;
                        term_buffer.set_bool(bool_val);
                        postings_writer.subscribe(doc_id, 0u32, term_buffer, ctx);
                    }
                }
                FieldType::Bytes(_) => {
                    for value in values {
                        let value = value.as_value();
                        let bytes = value.as_bytes().ok_or_else(make_schema_error)?;
                        term_buffer.set_bytes(bytes);
                        postings_writer.subscribe(doc_id, 0u32, term_buffer, ctx);
                    }
                }
                FieldType::JsonObject(json_options) => {
                    let text_analyzer =
                        &mut self.per_field_text_analyzers[field.field_id() as usize];

                    self.json_positions_per_path.clear();
                    self.json_path_writer
                        .set_expand_dots(json_options.is_expand_dots_enabled());
                    for json_value in values {
                        self.json_path_writer.clear();

                        index_json_value(
                            doc_id,
                            json_value,
                            text_analyzer,
                            term_buffer,
                            &mut self.json_path_writer,
                            postings_writer,
                            ctx,
                            &mut self.json_positions_per_path,
                        );
                    }
                }
                FieldType::IpAddr(_) => {
                    for value in values {
                        let value = value.as_value();

                        let ip_addr = value.as_ip_addr().ok_or_else(make_schema_error)?;
                        term_buffer.set_ip_addr(ip_addr);
                        postings_writer.subscribe(doc_id, 0u32, term_buffer, ctx);
                    }
                }
                // Custom fields are not indexed; the `is_indexed()` guard above skips them.
                FieldType::Custom(_) => {
                    unreachable!("the inverted index does not support custom field types")
                }
            }
            let tokens_added = postings_writer.total_num_tokens() - tokens_before;
            if field_entry.has_fieldnorms()
                && field_entry.field_type().index_record_option() == Some(IndexRecordOption::Basic)
            {
                // Basic postings count unique encoded term/document relations, just
                // like Lucene DOCS uniqueTermCount, including encoded value aliases.
                self.fieldnorms_writer
                    .record(doc_id, field, tokens_added as u32);
            }
            if tokens_added > 0 {
                self.field_doc_counts[field.field_id() as usize] += 1;
            }
        }
        self.max_doc = doc_id + 1;
        Ok(())
    }
}

impl PluginWriter for InvertedIndexPluginWriter {
    fn serialize(
        mut self: Box<Self>,
        segment: &Segment,
        doc_id_map: Option<&DocIdMapping>,
    ) -> crate::Result<()> {
        // Fresh serialization remaps permutations only; its statistics cover every doc.
        if doc_id_map.is_some_and(|map| map.len() != self.max_doc as usize) {
            return Err(TantivyError::InvalidArgument(
                "Fresh postings mapping must retain every document".into(),
            ));
        }
        // Field norms first: postings serialization reads them back below.
        self.fieldnorms_writer.fill_up_to_max_doc(self.max_doc);
        self.fieldnorms_writer
            .serialize(self.fieldnorm_serializer, doc_id_map)
            .map_err(|e| crate::TantivyError::InternalError(e.to_string()))?;

        let fieldnorm_data = segment.open_read(SegmentComponent::FieldNorms)?;
        let fieldnorm_readers = FieldNormReaders::open(fieldnorm_data)?;
        serialize_postings(
            self.ctx,
            self.schema,
            &self.per_field_postings_writers,
            &self.field_doc_counts,
            fieldnorm_readers,
            doc_id_map,
            &mut self.postings_serializer,
        )?;
        self.postings_serializer.close()?;
        Ok(())
    }

    fn mem_usage(&self) -> usize {
        self.ctx.mem_usage() + self.fieldnorms_writer.mem_usage()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// --- Field norm merge ---

fn merge_fieldnorms(ctx: &PluginMergeContext) -> crate::Result<()> {
    let path = ctx
        .target_segment
        .relative_path(SegmentComponent::FieldNorms);
    let write = ctx.target_segment.index().directory().open_write(&path)?;
    let mut serializer = FieldNormsSerializer::from_write(write)?;

    let schema = ctx.schema;
    let fields = FieldNormsWriter::fields_with_fieldnorm(schema);
    let max_doc: usize = ctx
        .readers
        .iter()
        .map(|reader| reader.num_docs() as usize)
        .sum();
    let mut fieldnorms_data = Vec::with_capacity(max_doc);

    for field in fields {
        fieldnorms_data.clear();
        let fieldnorms_readers: Vec<FieldNormReader> = ctx
            .readers
            .iter()
            .map(|reader| reader.get_fieldnorms_reader(field))
            .collect::<Result<_, _>>()?;
        for old_doc_addr in ctx.doc_id_mapping.iter_old_doc_addrs() {
            let reader = &fieldnorms_readers[old_doc_addr.segment_ord as usize];
            let fieldnorm_id = reader.fieldnorm_id(old_doc_addr.doc_id);
            fieldnorms_data.push(fieldnorm_id);
        }
        serializer.serialize_field(field, &fieldnorms_data)?;
    }
    serializer.close()?;
    Ok(())
}

// --- Postings merge helpers (moved from IndexMerger) ---

struct DeltaComputer {
    buffer: Vec<u32>,
}

impl DeltaComputer {
    fn new() -> DeltaComputer {
        DeltaComputer {
            buffer: vec![0u32; 512],
        }
    }

    fn compute_delta(&mut self, positions: &[u32]) -> &[u32] {
        if positions.len() > self.buffer.len() {
            self.buffer.resize(positions.len(), 0u32);
        }
        let mut last_pos = 0u32;
        for (cur_pos, dest) in positions.iter().cloned().zip(self.buffer.iter_mut()) {
            *dest = cur_pos - last_pos;
            last_pos = cur_pos;
        }
        &self.buffer[..positions.len()]
    }
}

fn write_postings_for_field(
    readers: &[SegmentReader],
    schema: &Schema,
    indexed_field: Field,
    serializer: &mut InvertedIndexSerializer,
    fieldnorm_reader: Option<FieldNormReader>,
    doc_id_mapping: &SegmentDocIdMapping,
) -> crate::Result<()> {
    debug_time!("write-postings-for-field");
    let mut positions_buffer: Vec<u32> = Vec::with_capacity(1_000);
    let mut delta_computer = DeltaComputer::new();

    let mut max_term_ords: Vec<TermOrdinal> = Vec::new();

    let field_readers: Vec<Arc<InvertedIndexReader>> = readers
        .iter()
        .map(|reader| reader.inverted_index(indexed_field))
        .collect::<crate::Result<Vec<_>>>()?;

    let mut field_term_streams = Vec::new();
    for field_reader in &field_readers {
        let terms = field_reader.terms();
        field_term_streams.push(terms.stream()?);
        max_term_ords.push(terms.num_terms() as u64);
    }

    let mut merged_terms = TermMerger::new(field_term_streams);

    let mut merged_doc_id_map: Vec<Vec<Option<DocId>>> = readers
        .iter()
        .map(|reader| {
            let mut segment_local_map = vec![];
            segment_local_map.resize(reader.max_doc() as usize, None);
            segment_local_map
        })
        .collect();
    for (new_doc_id, old_doc_addr) in doc_id_mapping.iter_old_doc_addrs().enumerate() {
        let segment_map = &mut merged_doc_id_map[old_doc_addr.segment_ord as usize];
        let entry = segment_map
            .get_mut(old_doc_addr.doc_id as usize)
            .ok_or_else(|| {
                TantivyError::InvalidArgument("Merge mapping document exceeds maxDoc".into())
            })?;
        if entry.replace(new_doc_id as DocId).is_some() {
            return Err(TantivyError::InvalidArgument(
                "Merge mapping repeats a source document".into(),
            ));
        }
    }

    let statistics = derive_retained(
        &field_readers,
        &merged_doc_id_map,
        doc_id_mapping.iter_old_doc_addrs().count() as u32,
    )?;
    let fully_retained: Vec<bool> = merged_doc_id_map
        .iter()
        .map(|map| map.iter().all(Option::is_some))
        .collect();
    let mut field_serializer =
        serializer.new_field_with_statistics(indexed_field, statistics, fieldnorm_reader)?;

    let field_entry = schema.get_field_entry(indexed_field);

    let segment_postings_option = field_entry.field_type().get_index_record_option().expect(
        "Encountered a field that is not supposed to be
                     indexed. Have you modified the schema?",
    );

    let mut segment_postings_containing_the_term: Vec<(usize, SegmentPostings)> = vec![];
    let mut doc_id_and_positions = vec![];

    while merged_terms.advance() {
        segment_postings_containing_the_term.clear();
        let term_bytes: &[u8] = merged_terms.key();

        let mut total_doc_freq = 0;

        for (segment_ord, term_info) in merged_terms.current_segment_ords_and_term_infos() {
            let inverted_index: &InvertedIndexReader = &field_readers[segment_ord];
            let segment_postings =
                inverted_index.read_postings_from_terminfo(&term_info, segment_postings_option)?;
            // The actual mapping may omit documents independently of alive masks.
            let doc_freq = if fully_retained[segment_ord] {
                segment_postings.doc_freq()
            } else {
                let mut retained = segment_postings.clone();
                let mut doc_freq = 0;
                while retained.doc() != TERMINATED {
                    if merged_doc_id_map[segment_ord][retained.doc() as usize].is_some() {
                        doc_freq += 1;
                    }
                    retained.advance();
                }
                doc_freq
            };
            if doc_freq > 0u32 {
                total_doc_freq += doc_freq;
                segment_postings_containing_the_term.push((segment_ord, segment_postings));
            }
        }

        if total_doc_freq == 0u32 {
            continue;
        }

        assert!(!segment_postings_containing_the_term.is_empty());

        let has_term_freq = {
            let has_term_freq = !segment_postings_containing_the_term[0]
                .1
                .block_cursor
                .freqs()
                .is_empty();
            for (_, postings) in &segment_postings_containing_the_term[1..] {
                if has_term_freq == postings.block_cursor.freqs().is_empty() {
                    return Err(DataCorruption::comment_only(
                        "Term freqs are inconsistent across segments",
                    )
                    .into());
                }
            }
            has_term_freq
        };

        field_serializer.new_term(term_bytes, total_doc_freq, has_term_freq)?;

        for (segment_ord, mut segment_postings) in segment_postings_containing_the_term.drain(..) {
            let old_to_new_doc_id = &merged_doc_id_map[segment_ord];

            let mut doc = segment_postings.doc();
            while doc != TERMINATED {
                if let Some(remapped_doc_id) = old_to_new_doc_id[doc as usize] {
                    let term_freq = if has_term_freq {
                        segment_postings.positions(&mut positions_buffer);
                        segment_postings.term_freq()
                    } else {
                        positions_buffer.clear();
                        0u32
                    };

                    if !doc_id_mapping.is_trivial() {
                        doc_id_and_positions.push((
                            remapped_doc_id,
                            term_freq,
                            positions_buffer.to_vec(),
                        ));
                    } else {
                        let delta_positions = delta_computer.compute_delta(&positions_buffer);
                        field_serializer.write_doc(remapped_doc_id, term_freq, delta_positions);
                    }
                }

                doc = segment_postings.advance();
            }
        }
        if !doc_id_mapping.is_trivial() {
            doc_id_and_positions.sort_unstable_by_key(|&(doc_id, _, _)| doc_id);

            for (doc_id, term_freq, positions) in &doc_id_and_positions {
                let delta_positions = delta_computer.compute_delta(positions);
                field_serializer.write_doc(*doc_id, *term_freq, delta_positions);
            }
            doc_id_and_positions.clear();
        }
        field_serializer.close_term()?;
    }
    field_serializer.close()?;
    Ok(())
}

fn write_postings_merge(
    readers: &[SegmentReader],
    schema: &Schema,
    serializer: &mut InvertedIndexSerializer,
    fieldnorm_readers: FieldNormReaders,
    doc_id_mapping: &SegmentDocIdMapping,
) -> crate::Result<()> {
    for (field, field_entry) in schema.fields() {
        let fieldnorm_reader = fieldnorm_readers.get_field(field)?;
        if field_entry.is_indexed() {
            write_postings_for_field(
                readers,
                schema,
                field,
                serializer,
                fieldnorm_reader,
                doc_id_mapping,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compute_initial_table_size;

    #[test]
    fn field_statistics_manual_mapping_serializes_exact_totals_and_term_df() -> crate::Result<()> {
        use std::sync::Arc;

        use super::write_postings_for_field;
        use crate::directory::CompositeFile;
        use crate::fieldnorm::FieldNormReader;
        use crate::index::field_statistics::FieldStatistics;
        use crate::index::SegmentComponent;
        use crate::indexer::doc_id_mapping::{MappingType, SegmentDocIdMapping};
        use crate::postings::{InvertedIndexSerializer, Postings};
        use crate::schema::{Schema, TEXT};
        use crate::termdict::TermDictionary;
        use crate::{DocAddress, DocSet, Index, InvertedIndexReader, Term, TERMINATED};

        let mut schema = Schema::builder();
        let text = schema.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        writer.add_document(doc!(text => "a a"))?;
        writer.add_document(doc!(text => "b b b"))?;
        writer.add_document(doc!(text => "c"))?;
        writer.commit()?;
        let searcher = index.reader()?.searcher();
        assert!(!searcher.segment_reader(0).has_deletes());
        for (order, expected_tokens) in [(vec![2, 0], 3), (vec![2, 0, 1], 6)] {
            let mapping = SegmentDocIdMapping::new(
                order.iter().map(|&doc| DocAddress::new(0, doc)).collect(),
                MappingType::Shuffled,
                vec![None],
            );
            let target = index.new_segment().with_max_doc(order.len() as u32);
            let mut serializer = InvertedIndexSerializer::open(&target)?;
            let norms: Vec<u32> = order.iter().map(|&doc| [2, 3, 1][doc as usize]).collect();
            write_postings_for_field(
                searcher.segment_readers(),
                &index.schema(),
                text,
                &mut serializer,
                Some(FieldNormReader::for_test(&norms)),
                &mapping,
            )?;
            serializer.close()?;
            let terms = CompositeFile::open(&target.open_read(SegmentComponent::Terms)?)?;
            let postings = CompositeFile::open(&target.open_read(SegmentComponent::Postings)?)?;
            let positions = CompositeFile::open(&target.open_read(SegmentComponent::Positions)?)?;
            let count = postings.open_read_with_idx(text, 1).unwrap().read_bytes()?;
            let reader = Arc::new(InvertedIndexReader::new(
                TermDictionary::open(terms.open_read(text).unwrap())?,
                postings.open_read(text).unwrap(),
                positions.open_read(text).unwrap(),
                crate::schema::IndexRecordOption::WithFreqsAndPositions,
                order.len() as u32,
                Some(&count),
            )?);
            assert_eq!(
                reader.field_statistics()?,
                FieldStatistics {
                    doc_count: order.len() as u32,
                    sum_total_term_freq: expected_tokens
                }
            );
            assert_eq!(
                reader.doc_freq(&Term::from_field_text(text, "b"))?,
                if order.len() == 2 { 0 } else { 1 }
            );
            let mut a = reader
                .read_postings(
                    &Term::from_field_text(text, "a"),
                    crate::schema::IndexRecordOption::WithFreqs,
                )?
                .unwrap();
            assert_eq!(a.doc(), 1);
            assert_eq!(a.term_freq(), 2);
            assert_eq!(a.doc_freq(), 1);
            assert_eq!(a.advance(), TERMINATED);
        }
        Ok(())
    }

    #[test]
    #[cfg(not(feature = "compare_hash_only"))]
    fn test_hashmap_size() {
        assert_eq!(compute_initial_table_size(100_000).unwrap(), 1 << 12);
        assert_eq!(compute_initial_table_size(1_000_000).unwrap(), 1 << 15);
        assert_eq!(compute_initial_table_size(15_000_000).unwrap(), 1 << 19);
        assert_eq!(compute_initial_table_size(1_000_000_000).unwrap(), 1 << 19);
        assert_eq!(compute_initial_table_size(4_000_000_000).unwrap(), 1 << 19);
    }
}
