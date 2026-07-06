//! Offset maps for translating byte positions from a transformed input back
//! to the coordinates of the string that transform was applied to.
//!
//! The markdown parser rewrites its input in up to two span-local passes
//! (escaped-newline decoding, math shielding). Each pass records the spans it
//! replaced as [`OffsetMap`] edits; provenance and diagnostic byte ranges are
//! translated back through the maps so the produced `Document` always refers
//! to **original-source** coordinates.

use crate::ir::ByteRange;

#[derive(Debug, Clone, Copy)]
struct Edit {
    /// Start of the replacement span in transformed coordinates.
    transformed_pos: usize,
    /// Length of the replacement text in the transformed string.
    transformed_len: usize,
    /// Length of the replaced text in the pre-transform string.
    original_len: usize,
}

/// Maps positions in a transformed string back to the pre-transform string.
/// Edits must be recorded in ascending, non-overlapping order (the scanners
/// that produce them work left to right).
#[derive(Debug, Clone, Default)]
pub(super) struct OffsetMap {
    edits: Vec<Edit>,
}

impl OffsetMap {
    pub(super) fn push_edit(
        &mut self,
        transformed_pos: usize,
        transformed_len: usize,
        original_len: usize,
    ) {
        debug_assert!(
            self.edits
                .last()
                .map(|e| e.transformed_pos + e.transformed_len <= transformed_pos)
                .unwrap_or(true),
            "offset-map edits must be recorded in ascending order"
        );
        self.edits.push(Edit {
            transformed_pos,
            transformed_len,
            original_len,
        });
    }

    pub(super) fn is_identity(&self) -> bool {
        self.edits.is_empty()
    }

    /// Map a range start. Positions strictly inside a replaced span snap to
    /// the span's start in pre-transform coordinates.
    pub(super) fn to_original_start(&self, pos: usize) -> usize {
        let mut delta: isize = 0;
        for e in &self.edits {
            if pos >= e.transformed_pos + e.transformed_len {
                delta += e.original_len as isize - e.transformed_len as isize;
            } else if pos > e.transformed_pos {
                return (e.transformed_pos as isize + delta) as usize;
            } else {
                break;
            }
        }
        (pos as isize + delta) as usize
    }

    /// Map a range end (exclusive). Positions strictly inside a replaced span
    /// snap to the span's end in pre-transform coordinates so mapped ranges
    /// always cover the full replaced span.
    pub(super) fn to_original_end(&self, pos: usize) -> usize {
        let mut delta: isize = 0;
        for e in &self.edits {
            if pos >= e.transformed_pos + e.transformed_len {
                delta += e.original_len as isize - e.transformed_len as isize;
            } else if pos > e.transformed_pos {
                return (e.transformed_pos as isize + delta) as usize + e.original_len;
            } else {
                break;
            }
        }
        (pos as isize + delta) as usize
    }

    pub(super) fn map_range(&self, range: ByteRange) -> ByteRange {
        ByteRange::new(
            self.to_original_start(range.start),
            self.to_original_end(range.end),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_map_is_noop() {
        let map = OffsetMap::default();
        assert!(map.is_identity());
        assert_eq!(map.map_range(ByteRange::new(3, 9)), ByteRange::new(3, 9));
    }

    // Transform replaced 5 original bytes at original pos 4 with 2 bytes.
    // transformed: 0..4 same, [4..6) placeholder, 6.. shifted left by 3.
    fn shrinking_map() -> OffsetMap {
        let mut map = OffsetMap::default();
        map.push_edit(4, 2, 5);
        map
    }

    #[test]
    fn positions_before_edit_are_unchanged() {
        let map = shrinking_map();
        assert_eq!(map.map_range(ByteRange::new(0, 4)), ByteRange::new(0, 4));
    }

    #[test]
    fn positions_after_edit_are_shifted() {
        let map = shrinking_map();
        assert_eq!(map.map_range(ByteRange::new(6, 10)), ByteRange::new(9, 13));
    }

    #[test]
    fn range_covering_edit_expands_to_original_span() {
        let map = shrinking_map();
        // Placeholder span [4,6) maps to original [4,9).
        assert_eq!(map.map_range(ByteRange::new(4, 6)), ByteRange::new(4, 9));
        // Positions inside the placeholder snap outward.
        assert_eq!(map.map_range(ByteRange::new(5, 5)), ByteRange::new(4, 9));
    }

    #[test]
    fn multiple_edits_accumulate_delta() {
        let mut map = OffsetMap::default();
        map.push_edit(2, 1, 2); // -1
        map.push_edit(10, 1, 4); // -3
        assert_eq!(map.to_original_start(0), 0);
        assert_eq!(map.to_original_start(5), 6);
        assert_eq!(map.to_original_start(12), 16);
    }
}
