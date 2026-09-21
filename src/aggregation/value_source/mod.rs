mod block_accessor;
mod value_source_registry;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use block_accessor::ColumnBlockAccessor;
use columnar::{Cardinality, Column, ColumnValues, RowId};
pub use value_source_registry::{ValueSourceProvider, ValueSourceRegistry};

use crate::DocId;

/// A source of values for a block of documents.
pub trait ValueSource: std::fmt::Debug {
    /// Loads the values for `docs` into `values`, and for a non-full source the matching document
    /// ids into `docids`, returning how the two are aligned.
    ///
    /// `row_ids` is scratch the implementation may use freely. `docs` is sorted ascending with no
    /// duplicates.
    fn load_block(
        &self,
        docs: &[DocId],
        values: &mut Vec<u64>,
        docids: &mut Vec<DocId>,
        row_ids: &mut Vec<RowId>,
    ) -> Cardinality;

    /// Downcast the BlockValueSource to a Column.
    fn as_column(&self) -> Option<&Column<u64>> {
        None
    }

    /// Global value bounds, for fast paths that need to size or clamp something up front.
    fn bounds(&self) -> Option<(u64, u64)> {
        let column = self.as_column()?;
        Some((column.min_value(), column.max_value()))
    }
}

impl ValueSource for Column<u64> {
    #[inline]
    fn load_block(
        &self,
        docs: &[DocId],
        values: &mut Vec<u64>,
        docids: &mut Vec<DocId>,
        row_ids: &mut Vec<RowId>,
    ) -> Cardinality {
        let cardinality = self.index.get_cardinality();
        if cardinality.is_full() {
            load_full_column_values(docs, &*self.values, values);
        } else {
            docids.clear();
            row_ids.clear();
            self.row_ids_for_docs(docs, docids, row_ids);
            values.resize(row_ids.len(), 0u64);
            self.values.get_vals(row_ids, values);
        }
        cardinality
    }

    #[inline]
    fn as_column(&self) -> Option<&Column<u64>> {
        Some(self)
    }
}

#[inline]
fn load_full_column_values(
    docs: &[DocId],
    column_values: &dyn ColumnValues<u64>,
    values: &mut Vec<u64>,
) {
    // Skip the resize when already the right length (common case: fixed-size blocks).
    if values.len() != docs.len() {
        values.resize(docs.len(), 0u64);
    }
    // When the docs form a contiguous ascending run we can fetch the values as a single range.
    // This lets codecs (e.g. bitpacked) bulk-decode the slice instead of gathering value-by-value.
    if is_contiguous(docs) {
        column_values.get_range(docs[0] as u64, values);
    } else {
        column_values.get_vals(docs, values);
    }
}

/// Returns true if `docs` is a contiguous ascending run `[d, d + 1, ..., d + n - 1]`.
///
/// Assumes `docs` is sorted ascending and free of duplicates (the invariant for the
/// doc blocks passed to `fetch_block`), so comparing the endpoints is sufficient.
#[inline]
fn is_contiguous(docs: &[u32]) -> bool {
    let (Some(&first), Some(&last)) = (docs.first(), docs.last()) else {
        return false;
    };
    debug_assert!(
        docs.windows(2).all(|w| w[0] < w[1]),
        "fetch_block requires docs sorted ascending without duplicates"
    );
    (last - first) as usize + 1 == docs.len()
}
