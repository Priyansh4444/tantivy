//! Registration of named, computed value sources.
//!
//! A [`ValueSourceProvider`] is a cross-segment *definition*: it is shared by every segment of a
//! search, so it must be `Send + Sync`. [`ValueSourceProvider::for_segment`] binds it to one
//! segment, producing the handle the aggregation actually reads through. That handle only needs
//! to outlive the segment collector, which is `'static` but neither `Send` nor `Sync`.

use std::collections::HashMap;
use std::sync::Arc;

use columnar::ColumnType;

use super::ValueSource;
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
    use super::*;
    use crate::aggregation::value_source::tests::ConstantProvider;
    use crate::schema::Schema;

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
        let index = crate::Index::create_in_ram(Schema::builder().build());
        let mut writer = index.writer_for_tests().unwrap();
        writer.add_document(crate::doc!()).unwrap();
        writer.commit().unwrap();
        let searcher = index.reader().unwrap().searcher();
        let value_source_provider = registry.get("computed").unwrap();
        let value_source = value_source_provider
            .for_segment(searcher.segment_reader(0u32))
            .unwrap();
        let mut values = Vec::new();
        let mut doc_ids = Vec::new();
        let mut row_ids = Vec::new();
        let docs = &[1u32];
        value_source.load_block(docs, &mut values, &mut doc_ids, &mut row_ids);
        assert!(doc_ids.is_empty());
        assert_eq!(&values, &[2u64]);
    }
}
