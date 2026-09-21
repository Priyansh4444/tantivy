//! Registration of named, computed value sources.
//!
//! A [`ValueSourceProvider`] is a cross-segment *definition*: it is shared by every segment of a
//! search, so it must be `Send + Sync`. [`ValueSourceProvider::for_segment`] binds it to one
//! segment, producing the handle the aggregation actually reads through. That handle only needs
//! to outlive the segment collector, which is `'static` but neither `Send` nor `Sync`.

use std::collections::HashMap;
use std::sync::Arc;

use columnar::ColumnType;

use super::block_accessor::ValueSource;
use crate::SegmentReader;

/// Acts as a ValueSource object factory, producing  value source for a given Segment.
pub trait ValueSourceProvider: Send + Sync + 'static {
    /// The type of the values produced. The ColumnBlockAccessor only stores
    /// u64, so values are assumed to be encoded with the monotonic mapping.
    fn column_type(&self) -> ColumnType;
    /// Binds this definition to a single segment.
    fn for_segment(&self, reader: &SegmentReader) -> crate::Result<Arc<dyn ValueSource>>;
}

/// Named computed sources available to an aggregation request.
#[derive(Clone, Default)]
pub struct ValueSourceRegistry {
    providers: HashMap<String, Arc<dyn ValueSourceProvider>>,
}

impl ValueSourceRegistry {
    /// Registers `provider` under `name`, which aggregation requests then use as a field name.
    ///
    /// Inserting the same name several times results in an override.
    pub fn register(&mut self, name: &str, provider: Arc<dyn ValueSourceProvider>) {
        let name = name.to_string();
        self.providers.insert(name, provider);
    }

    #[inline]
    pub(crate) fn get(&self, name: &str) -> Option<&Arc<dyn ValueSourceProvider>> {
        self.providers.get(name)
    }
}

#[cfg(test)]
mod tests {
    use columnar::Cardinality;

    use super::*;
    use crate::aggregation::ColumnBlockAccessor;
    use crate::DocId;

    /// A stand-in source: every document has the value 1. Deliberately trivial — the point is to
    /// exercise registration and dispatch, not expression evaluation.
    #[derive(Debug)]
    pub(crate) struct Constant(u64);

    impl ValueSource for Constant {
        fn load_block(
            &self,
            docs: &[DocId],
            values: &mut Vec<u64>,
            _docids: &mut Vec<DocId>,
            _row_ids: &mut Vec<columnar::RowId>,
        ) -> Cardinality {
            values.clear();
            values.resize(docs.len(), self.0);
            Cardinality::Full
        }
    }

    pub(crate) struct ConstantProvider(u64);

    impl ValueSourceProvider for ConstantProvider {
        fn column_type(&self) -> ColumnType {
            ColumnType::U64
        }

        fn for_segment(&self, _reader: &SegmentReader) -> crate::Result<Arc<dyn ValueSource>> {
            Ok(Arc::new(Constant(self.0)))
        }
    }

    #[test]
    fn test_register_then_get() {
        let mut registry = ValueSourceRegistry::default();
        registry.register("computed", Arc::new(ConstantProvider(1)));
        assert!(registry.get("computed").is_some());
        assert!(registry.get("absent").is_none());
    }

    #[test]
    fn test_register_overrides() {
        let mut registry = ValueSourceRegistry::default();
        registry.register("computed", Arc::new(ConstantProvider(1)));
        registry.register("computed", Arc::new(ConstantProvider(2)));
        let index = index_with_scores(&[1u64]);
        let searcher = index.reader().unwrap().searcher();
        let value_source_provider = registry.get("computed").unwrap();
        let mut block_accessor = ColumnBlockAccessor::default();
        let value_source = value_source_provider
            .for_segment(searcher.segment_reader(0u32))
            .unwrap();
        let docs = &[1u32];
        block_accessor.fetch_block(docs, &*value_source);
        assert!(block_accessor.has_one_value_per_doc(docs));
        assert_eq!(block_accessor.values(), &[2u64]);
    }

    fn index_with_scores(scores: &[u64]) -> crate::Index {
        use crate::schema::{Schema, FAST};
        let mut builder = Schema::builder();
        let score = builder.add_u64_field("score", FAST);
        let index = crate::Index::create_in_ram(builder.build());
        let mut writer = index.writer_for_tests().unwrap();
        for &value in scores {
            writer.add_document(crate::doc!(score => value)).unwrap();
        }
        writer.commit().unwrap();
        index
    }

    fn run_agg(index: &crate::Index, aggs: serde_json::Value) -> serde_json::Value {
        let mut registry = ValueSourceRegistry::default();
        registry.register("computed", Arc::new(ConstantProvider(1u64)));
        run_agg_with_registry(index, aggs, registry)
    }

    fn run_agg_with_registry(
        index: &crate::Index,
        aggs: serde_json::Value,
        registry: ValueSourceRegistry,
    ) -> serde_json::Value {
        use crate::aggregation::agg_req::Aggregations;
        use crate::aggregation::{AggContextParams, AggregationCollector};
        use crate::query::AllQuery;

        let context = AggContextParams::default().with_value_sources(Arc::new(registry));
        let aggs: Aggregations = serde_json::from_value(aggs).unwrap();
        let collector = AggregationCollector::from_aggs(aggs, context);
        let searcher = index.reader().unwrap().searcher();
        let result = searcher.search(&AllQuery, &collector).unwrap();
        serde_json::to_value(result).unwrap()
    }

    #[test]
    fn test_metric_over_registered_source() {
        let index = index_with_scores(&[10, 20, 30, 40]);
        let result = run_agg(
            &index,
            serde_json::json!({ "s": { "stats": { "field": "computed" } } }),
        );
        // Every document contributes exactly 1.
        assert_eq!(result["s"]["count"], 4);
        assert_eq!(result["s"]["sum"], 4.0);
        assert_eq!(result["s"]["avg"], 1.0);
        assert_eq!(result["s"]["min"], 1.0);
        assert_eq!(result["s"]["max"], 1.0);
    }

    #[test]
    fn test_registered_source_as_sub_aggregation_of_terms() {
        // The sub-aggregation reads the computed source out of the per-bucket doc buffer, which
        // is the path that matters: the parent drains the shared block accessor before the child
        // fetches into it.
        let index = index_with_scores(&[7, 7, 7, 9]);
        let result = run_agg(
            &index,
            serde_json::json!({
                "by_score": {
                    "terms": { "field": "score" },
                    "aggs": { "s": { "sum": { "field": "computed" } } }
                }
            }),
        );
        let buckets = result["by_score"]["buckets"].as_array().unwrap();
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0]["key"], 7.0);
        assert_eq!(buckets[0]["doc_count"], 3);
        assert_eq!(buckets[0]["s"]["value"], 3.0);
        assert_eq!(buckets[1]["key"], 9.0);
        assert_eq!(buckets[1]["doc_count"], 1);
        assert_eq!(buckets[1]["s"]["value"], 1.0);
    }
}
